use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write;

use petgraph::graph::{EdgeIndex, NodeIndex};

use crate::ast::*;
use crate::ir::*;
use crate::lower::{
    bit_width, collect_blocking_takes, collect_try_takes, collect_unconditional_puts,
    is_queue_like, sanitize,
};
use crate::schedule::*;
use crate::types::ty::Ty;

/// A generated Chisel/Scala file.
pub struct ChiselFile {
    pub name: String,
    pub content: String,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Lower scheduled networks to Chisel (Scala) files.
pub fn lower_chisel(scheduled: &[ScheduledNetwork]) -> Vec<ChiselFile> {
    let mut files = vec![];

    for sn in scheduled {
        let mut emitter = ChiselEmitter::new(sn);
        let content = emitter.emit();
        files.push(ChiselFile {
            name: format!("{}.scala", sn.network.name),
            content,
        });
    }

    files
}

// ---------------------------------------------------------------------------
// Chisel emitter
// ---------------------------------------------------------------------------

struct ChiselEmitter<'a> {
    net: &'a ScheduledNetwork,
    out: String,
    indent: usize,
    /// Edge indices for memory-exposed edges. Value is true if CPU is the writer (enq side).
    memory_edges: HashMap<EdgeIndex, bool>,
    /// Node indices for memory stub process nodes.
    memory_stub_nodes: HashSet<NodeIndex>,
    /// Queue edges that are ONLY accessed via try_take (not blocking take).
    try_take_only_edges: HashSet<EdgeIndex>,
}

/// Per-rule context for expression emission.
#[allow(dead_code)]
struct RuleCtx<'a> {
    instance_name: String,
    rule_name: String,
    port_edges: &'a HashMap<String, EdgeIndex>,
    /// Variable name -> inlined Chisel expression.
    vars: HashMap<String, String>,
    /// Variable name -> resolved type.
    var_types: HashMap<String, Ty>,
    /// Counter for unique temp names.
    temp_counter: usize,
    /// deq_ready signals to assert inside match Some arms.
    pending_deq_readys: Vec<String>,
}

impl<'a> ChiselEmitter<'a> {
    fn new(net: &'a ScheduledNetwork) -> Self {
        let mut memory_edges = HashMap::new();
        let mut memory_stub_nodes = HashSet::new();

        for node_idx in net.network.graph.node_indices() {
            if net.network.graph[node_idx].is_memory_stub {
                memory_stub_nodes.insert(node_idx);
            }
        }

        for edge_idx in net.network.graph.edge_indices() {
            let edge = &net.network.graph[edge_idx];

            if let Some((src, dst)) = net.network.graph.edge_endpoints(edge_idx)
                && (memory_stub_nodes.contains(&src) || memory_stub_nodes.contains(&dst))
            {
                let cpu_is_writer = memory_stub_nodes.contains(&dst);
                memory_edges.insert(edge_idx, cpu_is_writer);
                continue;
            }

            if let QueueEdgeKind::Queue {
                is_external: true, ..
            } = &edge.kind
            {
                let mut has_writer = false;
                for node_idx in net.network.graph.node_indices() {
                    let node = &net.network.graph[node_idx];
                    for port in &node.ports {
                        if port.bound_to == Some(edge_idx)
                            && matches!(port.kind, PortKind::Produces)
                        {
                            has_writer = true;
                        }
                    }
                }
                memory_edges.insert(edge_idx, has_writer);
            }
        }

        // Compute try_take-only edges
        let mut all_blocking: HashSet<EdgeIndex> = HashSet::new();
        let mut all_try_take: HashSet<EdgeIndex> = HashSet::new();
        for node_idx in net.network.graph.node_indices() {
            if memory_stub_nodes.contains(&node_idx) {
                continue;
            }
            let node = &net.network.graph[node_idx];
            let port_edges: HashMap<String, EdgeIndex> = node
                .ports
                .iter()
                .filter_map(|p| p.bound_to.map(|e| (p.name.clone(), e)))
                .collect();
            for rule in &node.rules {
                for pn in collect_blocking_takes(rule) {
                    if let Some(&ei) = port_edges.get(&pn) {
                        all_blocking.insert(ei);
                    }
                }
                for pn in collect_try_takes(rule) {
                    if let Some(&ei) = port_edges.get(&pn) {
                        all_try_take.insert(ei);
                    }
                }
            }
        }
        let try_take_only_edges: HashSet<EdgeIndex> =
            all_try_take.difference(&all_blocking).copied().collect();

        Self {
            net,
            out: String::new(),
            indent: 0,
            memory_edges,
            memory_stub_nodes,
            try_take_only_edges,
        }
    }

    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn blank(&mut self) {
        self.out.push('\n');
    }

    fn indent(&mut self) {
        self.indent += 1;
    }

    fn dedent(&mut self) {
        self.indent -= 1;
    }

    // -----------------------------------------------------------------------
    // Top-level orchestration
    // -----------------------------------------------------------------------

    fn emit(&mut self) -> String {
        let pipe_name = self.net.network.name.clone();

        self.line("package tbn");
        self.blank();
        self.line("import chisel3._");
        self.line("import chisel3.util._");
        self.blank();

        // Emit type definitions (Bundle classes, ChiselEnum objects)
        self.emit_type_declarations();

        // Module
        self.line(&format!("class {pipe_name} extends Module {{"));
        self.indent();

        // IO bundle
        self.emit_io_bundle();
        self.blank();

        // Constants
        self.emit_constants();

        // Cell declarations (RegInit)
        self.emit_cell_declarations();

        // Queue instances
        self.emit_queue_instances();

        // Rule enables (can_fire / will_fire)
        self.emit_rule_enables();

        // Rule body logic — compute next values
        self.emit_rule_logic();

        // Cell writebacks (single := with Mux)
        self.emit_cell_writebacks();

        // Queue wiring (enq/deq)
        self.emit_queue_wiring();

        self.dedent();
        self.line("}");

        self.out.clone()
    }

    // -----------------------------------------------------------------------
    // Type declarations (Bundle, ChiselEnum)
    // -----------------------------------------------------------------------

    fn emit_type_declarations(&mut self) {
        let type_defs = &self.net.network.type_defs;
        if type_defs.is_empty() {
            return;
        }

        let mut sorted: Vec<(&String, &Ty)> = type_defs.iter().collect();
        sorted.sort_by_key(|(name, _)| (*name).clone());

        for (name, ty) in &sorted {
            match ty {
                Ty::Record { fields, .. } => {
                    self.line(&format!("class {name} extends Bundle {{"));
                    self.indent();
                    for (fname, fty) in fields {
                        let chisel_ty = self.chisel_type(fty);
                        self.line(&format!("val {fname} = {chisel_ty}"));
                    }
                    self.dedent();
                    self.line("}");
                    self.blank();
                }
                Ty::Enum { variants, .. } => {
                    self.line(&format!("object {name} extends ChiselEnum {{"));
                    self.indent();
                    let variant_names: Vec<&str> =
                        variants.iter().map(|(vn, _)| vn.as_str()).collect();
                    self.line(&format!("val {} = Value", variant_names.join(", ")));
                    self.dedent();
                    self.line("}");
                    self.blank();
                }
                _ => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // IO bundle (external queues, memory ports, domain clocks)
    // -----------------------------------------------------------------------

    fn emit_io_bundle(&mut self) {
        let mem_ports = self.collect_io_ports();
        if mem_ports.is_empty() {
            self.line("val io = IO(new Bundle {})");
        } else {
            self.line("val io = IO(new Bundle {");
            self.indent();
            for port in &mem_ports {
                self.line(port);
            }
            self.dedent();
            self.line("})");
        }
    }

    fn collect_io_ports(&self) -> Vec<String> {
        let mut ports = vec![];
        let mut sorted: Vec<(EdgeIndex, bool)> =
            self.memory_edges.iter().map(|(&k, &v)| (k, v)).collect();
        sorted.sort_by_key(|(idx, _)| idx.index());

        for (edge_idx, cpu_is_writer) in &sorted {
            let edge = &self.net.network.graph[*edge_idx];
            let sname = sanitize(&edge.name);
            let w = bit_width(&edge.elem_ty);
            let chisel_data_type = chisel_uint(w);

            if *cpu_is_writer {
                // CPU writes -> expose as Decoupled output
                ports.push(format!("val q_{sname} = Decoupled({chisel_data_type})"));
            } else {
                // CPU reads -> expose as Flipped(Decoupled) input
                ports.push(format!(
                    "val q_{sname} = Flipped(Decoupled({chisel_data_type}))"
                ));
            }
        }
        ports
    }

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    fn emit_constants(&mut self) {
        let constants = &self.net.network.constants;
        if constants.is_empty() {
            return;
        }
        let mut sorted: Vec<(&String, &u64)> = constants.iter().collect();
        sorted.sort_by_key(|(name, _)| (*name).clone());
        for (name, value) in sorted {
            self.line(&format!("val {name} = {value}.U"));
        }
        self.blank();
    }

    // -----------------------------------------------------------------------
    // Cell declarations (RegInit)
    // -----------------------------------------------------------------------

    fn emit_cell_declarations(&mut self) {
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if let QueueEdgeKind::Cell { init, .. } = &edge.kind {
                let sname = sanitize(&edge.name);
                let w = bit_width(&edge.elem_ty);
                let init_val = init.map_or("0".to_string(), |v| format!("{v}"));
                self.line(&format!("val c_{sname} = RegInit({init_val}.U({w}.W))"));
            }
        }
        let has_cells = self
            .net
            .network
            .graph
            .edge_weights()
            .any(|e| matches!(e.kind, QueueEdgeKind::Cell { .. }));
        if has_cells {
            self.blank();
        }
    }

    // -----------------------------------------------------------------------
    // Queue instances (Module(new Queue(...)))
    // -----------------------------------------------------------------------

    fn emit_queue_instances(&mut self) {
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if !is_queue_like(&edge.kind) {
                continue;
            }
            if self.memory_edges.contains_key(&edge_idx) {
                continue;
            }
            let sname = sanitize(&edge.name);
            let w = bit_width(&edge.elem_ty);

            if matches!(edge.kind, QueueEdgeKind::AsyncQueue) {
                // AsyncQueue -> BlackBox wrapping tbn_async_fifo
                self.line(&format!("// AsyncQueue: {}", edge.name));
                self.line(&format!(
                    "val aq_{sname} = Module(new TbnAsyncFifo({w}, {}))",
                    edge.depth
                ));
            } else {
                let kind_label = "Queue";
                let init_count = match &edge.kind {
                    QueueEdgeKind::Queue { init_tokens, .. } if *init_tokens > 0 => {
                        Some(*init_tokens)
                    }
                    _ => None,
                };
                self.line(&format!("// {kind_label}: {}", edge.name));
                if let Some(_count) = init_count {
                    // Chisel Queue doesn't support pre-loaded tokens.
                    // Emit a standard Queue with a TODO comment.
                    self.line("// TODO: init tokens not directly supported in Chisel Queue");
                }
                self.line(&format!(
                    "val q_{sname} = Module(new Queue(UInt({w}.W), {}))",
                    edge.depth
                ));
            }
            self.blank();
        }
    }

    // -----------------------------------------------------------------------
    // Rule enables (can_fire / will_fire)
    // -----------------------------------------------------------------------

    fn emit_rule_enables(&mut self) {
        for node_idx in self.net.network.graph.node_indices() {
            if self.memory_stub_nodes.contains(&node_idx) {
                continue;
            }
            let node = &self.net.network.graph[node_idx];
            let schedule = &self.net.schedules[&node_idx];
            let inst = &node.instance_name;

            let port_edges: HashMap<String, EdgeIndex> = node
                .ports
                .iter()
                .filter_map(|p| p.bound_to.map(|e| (p.name.clone(), e)))
                .collect();

            let blocking_takes: Vec<BTreeSet<String>> =
                node.rules.iter().map(collect_blocking_takes).collect();
            let unconditional_puts: Vec<BTreeSet<String>> =
                node.rules.iter().map(collect_unconditional_puts).collect();

            let mut higher_will_fires: Vec<String> = vec![];

            for slot in &schedule.rule_order {
                let rule_indices = match slot {
                    RuleSlot::Exclusive(i) => vec![*i],
                    RuleSlot::Concurrent(v) => v.clone(),
                };

                for &rule_idx in &rule_indices {
                    let rule_name = &node.rules[rule_idx].name.node;

                    // can_fire = conjunction of queue readiness conditions
                    let mut conditions = vec![];

                    for take_port in &blocking_takes[rule_idx] {
                        if let Some(&edge_idx) = port_edges.get(take_port) {
                            let edge = &self.net.network.graph[edge_idx];
                            if is_queue_like(&edge.kind) {
                                let sname = sanitize(&edge.name);
                                if self.memory_edges.contains_key(&edge_idx) {
                                    conditions.push(format!("io.q_{sname}.valid"));
                                } else {
                                    conditions.push(format!("q_{sname}.io.deq.valid"));
                                }
                            }
                        }
                    }

                    for put_port in &unconditional_puts[rule_idx] {
                        if let Some(&edge_idx) = port_edges.get(put_port) {
                            let edge = &self.net.network.graph[edge_idx];
                            if is_queue_like(&edge.kind) {
                                let sname = sanitize(&edge.name);
                                if self.memory_edges.contains_key(&edge_idx) {
                                    conditions.push(format!("io.q_{sname}.ready"));
                                } else {
                                    conditions.push(format!("q_{sname}.io.enq.ready"));
                                }
                            }
                        }
                    }

                    let can_fire_expr = if conditions.is_empty() {
                        "true.B".to_string()
                    } else {
                        conditions.join(" && ")
                    };

                    self.line(&format!(
                        "val r_{inst}_{rule_name}_can_fire = {can_fire_expr}"
                    ));

                    let will_fire_expr = if higher_will_fires.is_empty() {
                        format!("r_{inst}_{rule_name}_can_fire")
                    } else {
                        let suppress = if higher_will_fires.len() == 1 {
                            higher_will_fires[0].clone()
                        } else {
                            format!("({})", higher_will_fires.join(" || "))
                        };
                        format!("r_{inst}_{rule_name}_can_fire && !{suppress}")
                    };

                    self.line(&format!(
                        "val r_{inst}_{rule_name}_will_fire = {will_fire_expr}"
                    ));
                }

                for &rule_idx in &rule_indices {
                    let rule_name = &node.rules[rule_idx].name.node;
                    higher_will_fires.push(format!("r_{inst}_{rule_name}_will_fire"));
                }
            }
            self.blank();
        }
    }

    // -----------------------------------------------------------------------
    // Rule body logic — compute next values as expression trees
    // -----------------------------------------------------------------------

    fn emit_rule_logic(&mut self) {
        for node_idx in self.net.network.graph.node_indices() {
            if self.memory_stub_nodes.contains(&node_idx) {
                continue;
            }
            let node = &self.net.network.graph[node_idx];
            let schedule = &self.net.schedules[&node_idx];
            let inst = node.instance_name.clone();

            let port_edges: HashMap<String, EdgeIndex> = node
                .ports
                .iter()
                .filter_map(|p| p.bound_to.map(|e| (p.name.clone(), e)))
                .collect();

            for slot in &schedule.rule_order {
                let rule_indices = match slot {
                    RuleSlot::Exclusive(i) => vec![*i],
                    RuleSlot::Concurrent(v) => v.clone(),
                };

                for &rule_idx in &rule_indices {
                    let rule = &node.rules[rule_idx];
                    let rule_name = rule.name.node.clone();

                    let mut ctx = RuleCtx {
                        instance_name: inst.clone(),
                        rule_name: rule_name.clone(),
                        port_edges: &port_edges,
                        vars: HashMap::new(),
                        var_types: HashMap::new(),
                        temp_counter: 0,
                        pending_deq_readys: vec![],
                    };

                    self.line(&format!("// Rule: {inst}.{rule_name}"));
                    for stmt in &rule.body {
                        self.emit_stmt(&stmt.node, &mut ctx);
                    }
                }
            }
        }
        self.blank();
    }

    // -----------------------------------------------------------------------
    // Cell writebacks (single Mux assignment per cell)
    // -----------------------------------------------------------------------

    fn emit_cell_writebacks(&mut self) {
        // Collect all rules that write to each cell, with their will_fire signals
        // and the computed next-value expression names.
        let mut cell_writers: HashMap<EdgeIndex, Vec<(String, String)>> = HashMap::new();

        for node_idx in self.net.network.graph.node_indices() {
            if self.memory_stub_nodes.contains(&node_idx) {
                continue;
            }
            let node = &self.net.network.graph[node_idx];
            let inst = &node.instance_name;

            let port_edges: HashMap<String, EdgeIndex> = node
                .ports
                .iter()
                .filter_map(|p| p.bound_to.map(|e| (p.name.clone(), e)))
                .collect();

            for rule in &node.rules {
                let rule_name = &rule.name.node;
                let will_fire = format!("r_{inst}_{rule_name}_will_fire");

                // Check which cell ports this rule puts to
                self.collect_rule_cell_puts(rule, &port_edges, &will_fire, &mut cell_writers);
            }
        }

        // Emit single := per cell
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if !matches!(edge.kind, QueueEdgeKind::Cell { .. }) {
                continue;
            }
            let sname = sanitize(&edge.name);

            if let Some(writers) = cell_writers.get(&edge_idx) {
                if writers.len() == 1 {
                    let (wf, val_name) = &writers[0];
                    self.line(&format!("c_{sname} := Mux({wf}, {val_name}, c_{sname})"));
                } else {
                    // Multiple writers -> MuxCase
                    let mut cases = String::new();
                    for (i, (wf, val_name)) in writers.iter().enumerate() {
                        if i > 0 {
                            cases.push_str(", ");
                        }
                        write!(cases, "{wf} -> {val_name}").unwrap();
                    }
                    self.line(&format!("c_{sname} := MuxCase(c_{sname}, Seq({cases}))"));
                }
            }
        }

        let has_cells = self
            .net
            .network
            .graph
            .edge_weights()
            .any(|e| matches!(e.kind, QueueEdgeKind::Cell { .. }));
        if has_cells {
            self.blank();
        }
    }

    /// Collect which cell edges a rule writes to (from Put statements).
    fn collect_rule_cell_puts(
        &self,
        rule: &Rule,
        port_edges: &HashMap<String, EdgeIndex>,
        will_fire: &str,
        cell_writers: &mut HashMap<EdgeIndex, Vec<(String, String)>>,
    ) {
        let rule_name = &rule.name.node;
        for stmt in &rule.body {
            self.collect_puts_from_stmt(&stmt.node, port_edges, will_fire, rule_name, cell_writers);
        }
    }

    fn collect_puts_from_stmt(
        &self,
        stmt: &Stmt,
        port_edges: &HashMap<String, EdgeIndex>,
        will_fire: &str,
        rule_name: &str,
        cell_writers: &mut HashMap<EdgeIndex, Vec<(String, String)>>,
    ) {
        match stmt {
            Stmt::Put { target, .. } => {
                if let Some(&edge_idx) = port_edges.get(&target.node) {
                    let edge = &self.net.network.graph[edge_idx];
                    if matches!(edge.kind, QueueEdgeKind::Cell { .. }) {
                        let sname = sanitize(&edge.name);
                        let val_name = format!("c_{sname}_{rule_name}_next");
                        cell_writers
                            .entry(edge_idx)
                            .or_default()
                            .push((will_fire.to_string(), val_name));
                    }
                }
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                for s in then_body {
                    self.collect_puts_from_stmt(
                        &s.node,
                        port_edges,
                        will_fire,
                        rule_name,
                        cell_writers,
                    );
                }
                for s in else_body {
                    self.collect_puts_from_stmt(
                        &s.node,
                        port_edges,
                        will_fire,
                        rule_name,
                        cell_writers,
                    );
                }
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    for s in &arm.body {
                        self.collect_puts_from_stmt(
                            &s.node,
                            port_edges,
                            will_fire,
                            rule_name,
                            cell_writers,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Queue wiring (enq/deq assignments)
    // -----------------------------------------------------------------------

    fn emit_queue_wiring(&mut self) {
        // Collect deq_ready drivers per queue
        let mut deq_ready_drivers: HashMap<EdgeIndex, Vec<String>> = HashMap::new();
        // Collect enq drivers per queue
        let mut enq_drivers: HashMap<EdgeIndex, Vec<(String, String)>> = HashMap::new();

        for node_idx in self.net.network.graph.node_indices() {
            if self.memory_stub_nodes.contains(&node_idx) {
                continue;
            }
            let node = &self.net.network.graph[node_idx];
            let inst = &node.instance_name;

            let port_edges: HashMap<String, EdgeIndex> = node
                .ports
                .iter()
                .filter_map(|p| p.bound_to.map(|e| (p.name.clone(), e)))
                .collect();

            for rule in &node.rules {
                let rule_name = &rule.name.node;
                let will_fire = format!("r_{inst}_{rule_name}_will_fire");

                // Blocking takes -> deq_ready
                let blocking = collect_blocking_takes(rule);
                for port_name in &blocking {
                    if let Some(&edge_idx) = port_edges.get(port_name) {
                        let edge = &self.net.network.graph[edge_idx];
                        if is_queue_like(&edge.kind) {
                            deq_ready_drivers
                                .entry(edge_idx)
                                .or_default()
                                .push(will_fire.clone());
                        }
                    }
                }

                // Puts -> enq
                self.collect_rule_queue_puts(rule, &port_edges, &will_fire, &mut enq_drivers);
            }
        }

        // Emit queue wiring
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if !is_queue_like(&edge.kind) {
                continue;
            }
            if self.memory_edges.contains_key(&edge_idx) {
                continue;
            }
            let sname = sanitize(&edge.name);

            // deq.ready
            if self.try_take_only_edges.contains(&edge_idx) {
                // try_take-only: default false, overridden in match arms
                self.line(&format!("q_{sname}.io.deq.ready := false.B"));
            } else if let Some(drivers) = deq_ready_drivers.get(&edge_idx) {
                let expr = drivers.join(" || ");
                self.line(&format!("q_{sname}.io.deq.ready := {expr}"));
            } else {
                self.line(&format!("q_{sname}.io.deq.ready := false.B"));
            }

            // enq.valid and enq.bits
            if let Some(writers) = enq_drivers.get(&edge_idx) {
                if writers.len() == 1 {
                    let (wf, val_name) = &writers[0];
                    self.line(&format!("q_{sname}.io.enq.valid := {wf}"));
                    self.line(&format!(
                        "q_{sname}.io.enq.bits := Mux({wf}, {val_name}, 0.U)"
                    ));
                } else {
                    let valid_expr: Vec<&str> = writers.iter().map(|(wf, _)| wf.as_str()).collect();
                    self.line(&format!(
                        "q_{sname}.io.enq.valid := {}",
                        valid_expr.join(" || ")
                    ));
                    let mut cases = String::new();
                    for (i, (wf, val_name)) in writers.iter().enumerate() {
                        if i > 0 {
                            cases.push_str(", ");
                        }
                        write!(cases, "{wf} -> {val_name}").unwrap();
                    }
                    self.line(&format!(
                        "q_{sname}.io.enq.bits := MuxCase(0.U, Seq({cases}))"
                    ));
                }
            } else {
                self.line(&format!("q_{sname}.io.enq.valid := false.B"));
                self.line(&format!("q_{sname}.io.enq.bits := 0.U"));
            }
            self.blank();
        }
    }

    /// Collect which queue edges a rule writes to (from Put statements).
    fn collect_rule_queue_puts(
        &self,
        rule: &Rule,
        port_edges: &HashMap<String, EdgeIndex>,
        will_fire: &str,
        enq_drivers: &mut HashMap<EdgeIndex, Vec<(String, String)>>,
    ) {
        let rule_name = &rule.name.node;
        for stmt in &rule.body {
            self.collect_queue_puts_from_stmt(
                &stmt.node,
                port_edges,
                will_fire,
                rule_name,
                enq_drivers,
            );
        }
    }

    fn collect_queue_puts_from_stmt(
        &self,
        stmt: &Stmt,
        port_edges: &HashMap<String, EdgeIndex>,
        will_fire: &str,
        rule_name: &str,
        enq_drivers: &mut HashMap<EdgeIndex, Vec<(String, String)>>,
    ) {
        match stmt {
            Stmt::Put { target, .. } => {
                if let Some(&edge_idx) = port_edges.get(&target.node) {
                    let edge = &self.net.network.graph[edge_idx];
                    if is_queue_like(&edge.kind) {
                        let sname = sanitize(&edge.name);
                        let val_name = format!("q_{sname}_{rule_name}_enq_val");
                        enq_drivers
                            .entry(edge_idx)
                            .or_default()
                            .push((will_fire.to_string(), val_name));
                    }
                }
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                for s in then_body {
                    self.collect_queue_puts_from_stmt(
                        &s.node,
                        port_edges,
                        will_fire,
                        rule_name,
                        enq_drivers,
                    );
                }
                for s in else_body {
                    self.collect_queue_puts_from_stmt(
                        &s.node,
                        port_edges,
                        will_fire,
                        rule_name,
                        enq_drivers,
                    );
                }
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    for s in &arm.body {
                        self.collect_queue_puts_from_stmt(
                            &s.node,
                            port_edges,
                            will_fire,
                            rule_name,
                            enq_drivers,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Statement emission
    // -----------------------------------------------------------------------

    fn emit_stmt(&mut self, stmt: &Stmt, ctx: &mut RuleCtx) {
        match stmt {
            Stmt::Let { pattern, value } => {
                let val_chisel = self.emit_expr(&value.node, ctx);
                let val_ty = self.infer_expr_type(&value.node, ctx);
                self.bind_pattern(&pattern.node, &val_chisel, &val_ty, ctx);
            }
            Stmt::Put { target, value } => {
                if let Some(&edge_idx) = ctx.port_edges.get(&target.node) {
                    let edge = &self.net.network.graph[edge_idx];
                    let sname = sanitize(&edge.name);
                    let val_chisel = self.emit_expr(&value.node, ctx);
                    let rn = &ctx.rule_name;

                    match &edge.kind {
                        QueueEdgeKind::Cell { .. } => {
                            // Include rule name to disambiguate multi-rule writers
                            self.line(&format!("val c_{sname}_{rn}_next = {val_chisel}"));
                        }
                        QueueEdgeKind::Queue { .. } | QueueEdgeKind::AsyncQueue => {
                            self.line(&format!("val q_{sname}_{rn}_enq_val = {val_chisel}"));
                        }
                    }
                }
            }
            Stmt::Expr(expr) => {
                let chisel = self.emit_expr(&expr.node, ctx);
                if matches!(&expr.node, Expr::Call { .. }) {
                    self.line(&format!("// DPI call: {chisel}"));
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                // For if/else that contains puts, we need to emit Mux expressions
                // For now, emit as nested val definitions with Mux
                let cond_chisel = self.emit_expr(&cond.node, ctx);

                // Check if this if/else contains puts — if so, we need to produce
                // conditional next-values. For statements like let bindings inside
                // branches, we need scoped emission.
                //
                // Strategy: collect all puts from both branches, compute the
                // conditional next-value for each target as Mux(cond, then_val, else_val)

                // First, check for simple cases where branches only contain puts
                let then_puts = collect_puts_from_body(then_body);
                let else_puts = collect_puts_from_body(else_body);

                if !then_puts.is_empty() || !else_puts.is_empty() {
                    // Emit let bindings from both branches (they may compute values)
                    for s in then_body {
                        if let Stmt::Let { .. } = &s.node {
                            self.emit_stmt(&s.node, ctx);
                        }
                    }
                    for s in else_body {
                        if let Stmt::Let { .. } = &s.node {
                            self.emit_stmt(&s.node, ctx);
                        }
                    }

                    // For each put target, emit Mux
                    let all_targets: BTreeSet<String> =
                        then_puts.keys().chain(else_puts.keys()).cloned().collect();

                    let rn = &ctx.rule_name.clone();
                    for target in &all_targets {
                        if let Some(&edge_idx) = ctx.port_edges.get(target) {
                            let edge = &self.net.network.graph[edge_idx];
                            let sname = sanitize(&edge.name);

                            let then_val = if let Some(val_expr) = then_puts.get(target) {
                                self.emit_expr(val_expr, ctx)
                            } else {
                                match &edge.kind {
                                    QueueEdgeKind::Cell { .. } => format!("c_{sname}"),
                                    _ => "0.U".to_string(),
                                }
                            };

                            let else_val = if let Some(val_expr) = else_puts.get(target) {
                                self.emit_expr(val_expr, ctx)
                            } else {
                                match &edge.kind {
                                    QueueEdgeKind::Cell { .. } => format!("c_{sname}"),
                                    _ => "0.U".to_string(),
                                }
                            };

                            match &edge.kind {
                                QueueEdgeKind::Cell { .. } => {
                                    self.line(&format!(
                                        "val c_{sname}_{rn}_next = Mux({cond_chisel}, {then_val}, {else_val})"
                                    ));
                                }
                                _ => {
                                    self.line(&format!(
                                        "val q_{sname}_{rn}_enq_val = Mux({cond_chisel}, {then_val}, {else_val})"
                                    ));
                                }
                            }
                        }
                    }
                } else {
                    // No puts — just emit the let bindings conditionally
                    for s in then_body {
                        self.emit_stmt(&s.node, ctx);
                    }
                    for s in else_body {
                        self.emit_stmt(&s.node, ctx);
                    }
                }
            }
            Stmt::Match { scrutinee, arms } => {
                let scrut_chisel = self.emit_expr(&scrutinee.node, ctx);
                let scrut_ty = self.infer_expr_type(&scrutinee.node, ctx);

                // For try_take match patterns (Option type), emit valid/data extraction
                if let Ty::Option(inner_ty) = &scrut_ty {
                    // Extract valid and data from the try_take tuple
                    // In Chisel, try_take maps to checking queue.io.deq.valid
                    for arm in arms {
                        match &arm.pattern.node {
                            Pattern::Variant { name, fields } if name == "Some" => {
                                ctx.pending_deq_readys.clear();
                                // Extract the queue name from the scrutinee
                                if let Expr::TryTake { queue } = &scrutinee.node
                                    && let Some(&edge_idx) = ctx.port_edges.get(queue)
                                {
                                    let edge = &self.net.network.graph[edge_idx];
                                    let sname = sanitize(&edge.name);
                                    if fields.len() == 1 {
                                        let data_expr = format!("q_{sname}.io.deq.bits");
                                        self.bind_pattern(
                                            &fields[0].node,
                                            &data_expr,
                                            inner_ty,
                                            ctx,
                                        );
                                    }
                                }
                                for s in &arm.body {
                                    self.emit_stmt(&s.node, ctx);
                                }
                            }
                            Pattern::Variant { name, .. } if name == "None" => {
                                // Nothing to emit for None arm (no-op when queue empty)
                                for s in &arm.body {
                                    self.emit_stmt(&s.node, ctx);
                                }
                            }
                            _ => {
                                for s in &arm.body {
                                    self.emit_stmt(&s.node, ctx);
                                }
                            }
                        }
                    }
                } else {
                    // Regular match — emit as MuxLookup or chained Mux
                    let _ = scrut_chisel;
                    for arm in arms {
                        // Bind patterns and emit body
                        self.bind_pattern(
                            &arm.pattern.node,
                            &self.emit_expr(&scrutinee.node, ctx),
                            &scrut_ty,
                            ctx,
                        );
                        for s in &arm.body {
                            self.emit_stmt(&s.node, ctx);
                        }
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Expression emission (returns Chisel expression string)
    // -----------------------------------------------------------------------

    fn emit_expr(&self, expr: &Expr, ctx: &RuleCtx) -> String {
        match expr {
            Expr::Lit(Literal::Int(n)) => format!("{n}.U"),
            Expr::Lit(Literal::Bool(true)) => "true.B".to_string(),
            Expr::Lit(Literal::Bool(false)) => "false.B".to_string(),
            Expr::Var(name) => {
                if let Some(chisel) = ctx.vars.get(name) {
                    chisel.clone()
                } else if let Some(&val) = self.net.network.constants.get(name) {
                    format!("{val}.U")
                } else {
                    name.clone()
                }
            }
            Expr::Take { queue } => {
                if let Some(&edge_idx) = ctx.port_edges.get(queue) {
                    let edge = &self.net.network.graph[edge_idx];
                    let sname = sanitize(&edge.name);
                    match &edge.kind {
                        QueueEdgeKind::Cell { .. } => format!("c_{sname}"),
                        QueueEdgeKind::Queue { .. } | QueueEdgeKind::AsyncQueue => {
                            if self.memory_edges.contains_key(&edge_idx) {
                                format!("io.q_{sname}.bits")
                            } else {
                                format!("q_{sname}.io.deq.bits")
                            }
                        }
                    }
                } else {
                    format!("0.U /* unknown port {queue} */")
                }
            }
            Expr::Peek { queue } => {
                if let Some(&edge_idx) = ctx.port_edges.get(queue) {
                    let edge = &self.net.network.graph[edge_idx];
                    let sname = sanitize(&edge.name);
                    format!("c_{sname}")
                } else {
                    format!("0.U /* unknown port {queue} */")
                }
            }
            Expr::TryTake { queue } => {
                if let Some(&edge_idx) = ctx.port_edges.get(queue) {
                    let edge = &self.net.network.graph[edge_idx];
                    let sname = sanitize(&edge.name);
                    // Returns a tuple-like (valid, data) in the match context
                    // The actual destructuring happens in the match arm handling
                    format!("q_{sname}.io.deq")
                } else {
                    format!("0.U /* unknown port {queue} */")
                }
            }
            Expr::BinOp { op, lhs, rhs } => {
                let l = self.emit_expr(&lhs.node, ctx);
                let r = self.emit_expr(&rhs.node, ctx);
                let op_chisel = binop_chisel(op);
                format!("({l} {op_chisel} {r})")
            }
            Expr::UnaryOp { op, expr } => {
                let e = self.emit_expr(&expr.node, ctx);
                let op_chisel = unaryop_chisel(op);
                format!("({op_chisel}{e})")
            }
            Expr::Tuple(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|item| self.emit_expr(&item.node, ctx))
                    .collect();
                format!("Cat({})", parts.join(", "))
            }
            Expr::Record { name, fields } => {
                // Bundle construction: Wire(new Name) with field assignments
                // For expression contexts, emit as a Bundle literal comment
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(fname, val)| {
                        let v = self.emit_expr(&val.node, ctx);
                        format!("{}: {v}", fname.node)
                    })
                    .collect();
                format!("/* {name}{{{}}}) */", parts.join(", "))
            }
            Expr::FieldAccess { expr, field } => {
                let e = self.emit_expr(&expr.node, ctx);
                format!("{e}.{}", field.node)
            }
            Expr::Index { expr: e, index } => {
                let base = self.emit_expr(&e.node, ctx);
                let idx = self.emit_expr(&index.node, ctx);
                format!("{base}({idx})")
            }
            Expr::Call { func, args } => {
                let arg_strs: Vec<String> =
                    args.iter().map(|a| self.emit_expr(&a.node, ctx)).collect();
                format!("/* DPI: {func}({}) */", arg_strs.join(", "))
            }
            Expr::Update {
                expr: e,
                index,
                value,
            } => {
                let base = self.emit_expr(&e.node, ctx);
                let idx = self.emit_expr(&index.node, ctx);
                let val = self.emit_expr(&value.node, ctx);
                format!("/* update({base}, {idx}, {val}) */")
            }
            Expr::BitSlice { expr, hi, lo } => {
                let e = self.emit_expr(&expr.node, ctx);
                format!("{e}({hi}, {lo})")
            }
            Expr::MethodCall { .. } => "0.U /* unsupported */".to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // Pattern binding
    // -----------------------------------------------------------------------

    fn bind_pattern(&self, pattern: &Pattern, val_chisel: &str, val_ty: &Ty, ctx: &mut RuleCtx) {
        match pattern {
            Pattern::Bind(name) => {
                ctx.vars.insert(name.clone(), val_chisel.to_string());
                ctx.var_types.insert(name.clone(), val_ty.clone());
            }
            Pattern::Tuple(sub_pats) => {
                if let Ty::Tuple(elem_tys) = val_ty {
                    let total_width: u64 = elem_tys.iter().map(bit_width).sum();
                    let mut bit_offset = 0u64;
                    let elems: Vec<_> = sub_pats.iter().zip(elem_tys.iter()).collect();
                    let mut slices: Vec<(usize, String, Ty)> = vec![];
                    for (i, (_pat, ety)) in elems.iter().enumerate().rev() {
                        let w = bit_width(ety);
                        let chisel_slice = if total_width == w && elems.len() == 1 {
                            val_chisel.to_string()
                        } else if w == 1 {
                            format!("{val_chisel}({bit_offset})")
                        } else {
                            format!("{val_chisel}({}, {})", bit_offset + w - 1, bit_offset)
                        };
                        slices.push((i, chisel_slice, (*ety).clone()));
                        bit_offset += w;
                    }
                    slices.sort_by_key(|(i, _, _)| *i);
                    for (i, chisel_slice, ety) in slices {
                        self.bind_pattern(&sub_pats[i].node, &chisel_slice, &ety, ctx);
                    }
                }
            }
            Pattern::Wildcard => {}
            Pattern::Variant { .. } | Pattern::Literal(_) => {}
        }
    }

    // -----------------------------------------------------------------------
    // Lightweight type inference
    // -----------------------------------------------------------------------

    fn infer_expr_type(&self, expr: &Expr, ctx: &RuleCtx) -> Ty {
        match expr {
            Expr::Lit(Literal::Int(_)) => Ty::Bits(32),
            Expr::Lit(Literal::Bool(_)) => Ty::Bool,
            Expr::Var(name) => ctx.var_types.get(name).cloned().unwrap_or(Ty::Error),
            Expr::Take { queue } | Expr::Peek { queue } => {
                if let Some(&edge_idx) = ctx.port_edges.get(queue) {
                    self.net.network.graph[edge_idx].elem_ty.clone()
                } else {
                    Ty::Error
                }
            }
            Expr::TryTake { queue } => {
                if let Some(&edge_idx) = ctx.port_edges.get(queue) {
                    Ty::Option(Box::new(self.net.network.graph[edge_idx].elem_ty.clone()))
                } else {
                    Ty::Error
                }
            }
            Expr::Tuple(items) => {
                let tys: Vec<Ty> = items
                    .iter()
                    .map(|item| self.infer_expr_type(&item.node, ctx))
                    .collect();
                Ty::Tuple(tys)
            }
            Expr::Record { name, .. } => {
                if let Some(ty) = self.net.network.type_defs.get(name) {
                    ty.clone()
                } else {
                    Ty::Error
                }
            }
            Expr::FieldAccess { expr: e, field } => {
                let parent_ty = self.infer_expr_type(&e.node, ctx);
                if let Ty::Record { fields, .. } = &parent_ty {
                    fields
                        .iter()
                        .find(|(n, _)| n == &field.node)
                        .map(|(_, t)| t.clone())
                        .unwrap_or(Ty::Error)
                } else {
                    Ty::Error
                }
            }
            Expr::Index { expr: e, .. } => {
                let parent_ty = self.infer_expr_type(&e.node, ctx);
                if let Ty::Array { elem, .. } = &parent_ty {
                    *elem.clone()
                } else {
                    Ty::Error
                }
            }
            Expr::BitSlice { hi, lo, .. } => Ty::Bits(hi - lo + 1),
            Expr::BinOp { op, lhs, rhs } => {
                if matches!(
                    op,
                    BinOp::Eq
                        | BinOp::Neq
                        | BinOp::Lt
                        | BinOp::Gt
                        | BinOp::Le
                        | BinOp::Ge
                        | BinOp::LogicalAnd
                        | BinOp::LogicalOr
                ) {
                    return Ty::Bool;
                }
                let lt = self.infer_expr_type(&lhs.node, ctx);
                let lw = bit_width(&lt);
                let rw = bit_width(&self.infer_expr_type(&rhs.node, ctx));
                if matches!(op, BinOp::Or | BinOp::And | BinOp::Xor | BinOp::Shl)
                    && rw > lw
                    && rw > 0
                {
                    Ty::Bits(rw)
                } else {
                    lt
                }
            }
            Expr::UnaryOp { expr: e, .. } => self.infer_expr_type(&e.node, ctx),
            _ => Ty::Error,
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Map a Ty to a Chisel type string.
    fn chisel_type(&self, ty: &Ty) -> String {
        match ty {
            Ty::Bits(n) => format!("UInt({n}.W)"),
            Ty::Bool => "Bool()".to_string(),
            Ty::Tuple(ts) => {
                let total: u64 = ts.iter().map(bit_width).sum();
                format!("UInt({total}.W)")
            }
            Ty::Record { name, .. } => format!("new {name}"),
            Ty::Enum { name, .. } => format!("{name}()"),
            Ty::Array { elem, size } => {
                let total = bit_width(elem) * size;
                format!("UInt({total}.W)")
            }
            Ty::Option(inner) => {
                let total = 1 + bit_width(inner);
                format!("UInt({total}.W)")
            }
            _ => "UInt(1.W)".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn chisel_uint(width: u64) -> String {
    format!("UInt({width}.W)")
}

fn binop_chisel(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::And => "&",
        BinOp::Or => "|",
        BinOp::Xor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Eq => "===",
        BinOp::Neq => "=/=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Le => "<=",
        BinOp::Ge => ">=",
        BinOp::LogicalAnd => "&&",
        BinOp::LogicalOr => "||",
    }
}

fn unaryop_chisel(op: &UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "~",
        UnaryOp::Neg => "-",
    }
}

/// Collect put targets from a statement body (only top-level puts).
fn collect_puts_from_body(body: &[Spanned<Stmt>]) -> HashMap<String, Expr> {
    let mut puts = HashMap::new();
    for stmt in body {
        if let Stmt::Put { target, value } = &stmt.node {
            puts.insert(target.node.clone(), value.node.clone());
        }
    }
    puts
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binop_chisel_ops() {
        assert_eq!(binop_chisel(&BinOp::Add), "+");
        assert_eq!(binop_chisel(&BinOp::Eq), "===");
        assert_eq!(binop_chisel(&BinOp::Neq), "=/=");
        assert_eq!(binop_chisel(&BinOp::LogicalAnd), "&&");
    }

    #[test]
    fn unaryop_chisel_ops() {
        assert_eq!(unaryop_chisel(&UnaryOp::Not), "~");
        assert_eq!(unaryop_chisel(&UnaryOp::Neg), "-");
    }

    #[test]
    fn counter_chisel_structure() {
        let src = r#"
process Counter {
    state: count : Cell(Bits 32, init = 0)
    rule tick {
        let c = count.take()
        count.put(c + 1)
    }
}

pipe Top {
    Counter {}
}
"#;
        let files = crate::build_chisel(src, "test.tbn").unwrap();
        assert_eq!(files.len(), 1);
        let scala = &files[0].content;

        assert!(
            scala.contains("class Top extends Module"),
            "module declaration"
        );
        assert!(scala.contains("RegInit"), "cell register");
        assert!(scala.contains("c_Counter_count"), "cell signal name");
        assert!(scala.contains("r_Counter_tick_can_fire"), "can_fire");
        assert!(scala.contains("r_Counter_tick_will_fire"), "will_fire");
        assert!(scala.contains("Mux("), "Mux for cell writeback");
        assert!(scala.contains("import chisel3._"), "chisel3 import");
    }

    #[test]
    fn producer_consumer_chisel_structure() {
        let src = r#"
process Producer {
    produces: out : Queue(Bits 32)
    rule go {
        out.put(42)
    }
}

process Consumer {
    consumes: input : Queue(Bits 32)
    rule go {
        let x = input.take()
    }
}

pipe Top {
    let q = Queue(Bits 32, depth = 4)
    Producer { out = q }
    Consumer { input = q }
}
"#;
        let files = crate::build_chisel(src, "test.tbn").unwrap();
        assert_eq!(files.len(), 1);
        let scala = &files[0].content;

        assert!(scala.contains("new Queue(UInt(32.W), 4)"), "Queue instance");
        assert!(scala.contains(".io.deq.valid"), "deq valid");
        assert!(scala.contains(".io.enq.valid"), "enq valid");
        assert!(scala.contains(".io.enq.bits"), "enq bits");
    }

    #[test]
    fn peek_chisel_structure() {
        let src = r#"
process Writer {
    state: reg : Cell(Bits 32, init = 0)
    rule go {
        let v = reg.take()
        reg.put(v + 1)
    }
}

process Reader {
    peeks: reg : Cell(Bits 32)
    rule go {
        let v = reg.peek()
    }
}

pipe Top {
    Writer {}
    Reader { reg = Writer.reg }
}
"#;
        let files = crate::build_chisel(src, "test.tbn").unwrap();
        assert_eq!(files.len(), 1);
        let scala = &files[0].content;

        assert!(scala.contains("RegInit"), "cell register");
        assert!(scala.contains("c_Writer_reg"), "cell name");
    }

    #[test]
    fn priority_chisel_structure() {
        let src = r#"
process Counter {
    state: count : Cell(Bits 32, init = 0)
    rule inc {
        let c = count.take()
        count.put(c + 1)
    }
    rule dec {
        let c = count.take()
        count.put(c - 1)
    }
}

pipe Top {
    Counter {}
}
"#;
        let files = crate::build_chisel(src, "test.tbn").unwrap();
        assert_eq!(files.len(), 1);
        let scala = &files[0].content;

        assert!(scala.contains("r_Counter_inc_will_fire"), "inc will_fire");
        assert!(scala.contains("r_Counter_dec_will_fire"), "dec will_fire");
        assert!(
            scala.contains("r_Counter_dec_can_fire && !r_Counter_inc_will_fire"),
            "priority suppression"
        );
        assert!(scala.contains("MuxCase"), "MuxCase for multi-writer cell");
    }

    #[test]
    fn branch_chisel_structure() {
        let src = r#"
process Router {
    consumes: input : Queue(Bits 32)
    produces: high  : Queue(Bits 32)
    produces: low   : Queue(Bits 32)
    rule go {
        let x = input.take()
        if x > 100 {
            high.put(x)
        } else {
            low.put(x)
        }
    }
}

process Source {
    produces: out : Queue(Bits 32)
    rule go {
        out.put(50)
    }
}

process SinkHigh {
    consumes: input : Queue(Bits 32)
    rule go {
        let x = input.take()
    }
}

process SinkLow {
    consumes: input : Queue(Bits 32)
    rule go {
        let x = input.take()
    }
}

pipe Top {
    let data = Queue(Bits 32, depth = 2)
    let hi   = Queue(Bits 32, depth = 2)
    let lo   = Queue(Bits 32, depth = 2)
    Source   { out = data }
    Router   { input = data, high = hi, low = lo }
    SinkHigh { input = hi }
    SinkLow  { input = lo }
}
"#;
        let files = crate::build_chisel(src, "test.tbn").unwrap();
        assert_eq!(files.len(), 1);
        let scala = &files[0].content;

        assert!(scala.contains("Mux("), "conditional routing uses Mux");
        assert!(
            scala.contains("new Queue(UInt(32.W), 2)"),
            "Queue instances"
        );
    }
}
