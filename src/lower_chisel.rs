use std::collections::{BTreeSet, HashMap, HashSet};

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

        // Type definitions (Bundle classes, ChiselEnum objects)
        self.emit_type_declarations();

        // Module
        self.line(&format!("class {pipe_name} extends Module {{"));
        self.indent();

        self.emit_io_bundle();
        self.blank();
        self.emit_constants();
        self.emit_cell_declarations();
        self.emit_queue_instances();
        self.emit_rule_enables();

        // Default assignments (mirrors SV always_comb defaults)
        self.emit_defaults();

        // Rule bodies inside when(will_fire) blocks
        self.emit_rule_logic();

        // deq_ready wiring (assign-style)
        self.emit_deq_ready_wiring();

        self.dedent();
        self.line("}");

        self.out.clone()
    }

    // -----------------------------------------------------------------------
    // Type declarations
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
                    // Emit as integer constants (matching SV typedef enum).
                    // ChiselEnum is type-incompatible with our packed-bit-vector
                    // records where enum fields are extracted as UInt slices.
                    self.line(&format!("object {name} {{"));
                    self.indent();
                    for (i, (vname, _)) in variants.iter().enumerate() {
                        let escaped = escape_scala_keyword(vname);
                        self.line(&format!("val {escaped} = {i}.U"));
                    }
                    self.dedent();
                    self.line("}");
                    self.blank();
                }
                _ => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // IO bundle
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
                ports.push(format!("val q_{sname} = Decoupled({chisel_data_type})"));
            } else {
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
        let mut any = false;
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if let QueueEdgeKind::Cell { init, .. } = &edge.kind {
                let sname = sanitize(&edge.name);
                let w = bit_width(&edge.elem_ty);
                let init_val = init.map_or("0".to_string(), |v| format!("{v}"));
                self.line(&format!("val c_{sname} = RegInit({init_val}.U({w}.W))"));
                any = true;
            }
        }
        if any {
            self.blank();
        }
    }

    // -----------------------------------------------------------------------
    // Queue instances
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
                self.line(&format!("// AsyncQueue: {}", edge.name));
                self.line(&format!(
                    "val q_{sname} = Module(new TbnAsyncFifo({w}, {}))",
                    edge.depth
                ));
            } else {
                let init_count = match &edge.kind {
                    QueueEdgeKind::Queue { init_tokens, .. } if *init_tokens > 0 => {
                        Some(*init_tokens)
                    }
                    _ => None,
                };
                self.line(&format!("// Queue: {}", edge.name));
                if init_count.is_some() {
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

                    let mut conditions = vec![];

                    for take_port in &blocking_takes[rule_idx] {
                        if let Some(&edge_idx) = port_edges.get(take_port) {
                            let edge = &self.net.network.graph[edge_idx];
                            if is_queue_like(&edge.kind) {
                                let sname = sanitize(&edge.name);
                                conditions.push(self.queue_deq_valid(&sname, edge_idx));
                            }
                        }
                    }

                    for put_port in &unconditional_puts[rule_idx] {
                        if let Some(&edge_idx) = port_edges.get(put_port) {
                            let edge = &self.net.network.graph[edge_idx];
                            if is_queue_like(&edge.kind) {
                                let sname = sanitize(&edge.name);
                                conditions.push(self.queue_enq_ready(&sname, edge_idx));
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
    // Default assignments (mirrors SV always_comb defaults)
    // -----------------------------------------------------------------------

    fn emit_defaults(&mut self) {
        // Cell defaults: c_NAME := c_NAME (identity — overridden in when blocks)
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if matches!(edge.kind, QueueEdgeKind::Cell { .. }) {
                let sname = sanitize(&edge.name);
                self.line(&format!("c_{sname} := c_{sname}"));
            }
        }

        // Queue enq defaults
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if is_queue_like(&edge.kind) {
                if let Some(&cpu_is_writer) = self.memory_edges.get(&edge_idx)
                    && !cpu_is_writer
                {
                    continue;
                }
                let sname = sanitize(&edge.name);
                let (enq_valid, enq_bits) = self.queue_enq_signals(&sname, edge_idx);
                self.line(&format!("{enq_valid} := false.B"));
                self.line(&format!("{enq_bits} := 0.U"));
            }
        }

        // try_take-only deq_ready defaults
        for &ei in &self.try_take_only_edges.clone() {
            let sname = sanitize(&self.net.network.graph[ei].name);
            let deq_ready = self.queue_deq_ready_signal(&sname, ei);
            self.line(&format!("{deq_ready} := false.B"));
        }

        self.blank();
    }

    // -----------------------------------------------------------------------
    // Rule body logic — when(will_fire) blocks
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
                    self.line(&format!("when (r_{inst}_{rule_name}_will_fire) {{"));
                    self.indent();

                    for stmt in &rule.body {
                        self.emit_stmt(&stmt.node, &mut ctx);
                    }

                    self.dedent();
                    self.line("}");
                }
            }
        }
        self.blank();
    }

    // -----------------------------------------------------------------------
    // deq_ready wiring
    // -----------------------------------------------------------------------

    fn emit_deq_ready_wiring(&mut self) {
        let mut deq_ready_drivers: HashMap<EdgeIndex, Vec<String>> = HashMap::new();

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
                let blocking = collect_blocking_takes(rule);

                for port_name in &blocking {
                    if let Some(&edge_idx) = port_edges.get(port_name) {
                        let edge = &self.net.network.graph[edge_idx];
                        if is_queue_like(&edge.kind) {
                            deq_ready_drivers
                                .entry(edge_idx)
                                .or_default()
                                .push(format!("r_{inst}_{rule_name}_will_fire"));
                        }
                    }
                }
            }
        }

        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if !is_queue_like(&edge.kind) {
                continue;
            }
            if let Some(&cpu_is_writer) = self.memory_edges.get(&edge_idx)
                && cpu_is_writer
            {
                continue;
            }
            if self.try_take_only_edges.contains(&edge_idx) {
                continue;
            }
            let sname = sanitize(&edge.name);
            let deq_ready = self.queue_deq_ready_signal(&sname, edge_idx);
            if let Some(drivers) = deq_ready_drivers.get(&edge_idx) {
                let expr = drivers.join(" || ");
                self.line(&format!("{deq_ready} := {expr}"));
            } else {
                self.line(&format!("{deq_ready} := false.B"));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Statement emission (inside when blocks)
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

                    match &edge.kind {
                        QueueEdgeKind::Cell { .. } => {
                            self.line(&format!("c_{sname} := {val_chisel}"));
                        }
                        QueueEdgeKind::Queue { .. } | QueueEdgeKind::AsyncQueue => {
                            let (enq_valid, enq_bits) = self.queue_enq_signals(&sname, edge_idx);
                            self.line(&format!("{enq_bits} := {val_chisel}"));
                            self.line(&format!("{enq_valid} := true.B"));
                        }
                    }
                }
            }
            Stmt::Expr(expr) => {
                if let Expr::Call { func, args } = &expr.node {
                    let arg_strs: Vec<String> =
                        args.iter().map(|a| self.emit_expr(&a.node, ctx)).collect();
                    self.line(&format!("// DPI: {func}({})", arg_strs.join(", ")));
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                let cond_chisel = self.emit_expr(&cond.node, ctx);
                self.line(&format!("when ({cond_chisel}) {{"));
                self.indent();
                for s in then_body {
                    self.emit_stmt(&s.node, ctx);
                }
                self.dedent();
                if !else_body.is_empty() {
                    self.line("} .otherwise {");
                    self.indent();
                    for s in else_body {
                        self.emit_stmt(&s.node, ctx);
                    }
                    self.dedent();
                }
                self.line("}");
            }
            Stmt::Match { scrutinee, arms } => {
                let scrut_ty = self.infer_expr_type(&scrutinee.node, ctx);

                // Option type from try_take
                if let Ty::Option(inner_ty) = &scrut_ty
                    && let Expr::TryTake { queue } = &scrutinee.node
                    && let Some(&edge_idx) = ctx.port_edges.get(queue)
                {
                    let edge = &self.net.network.graph[edge_idx];
                    let sname = sanitize(&edge.name);
                    let valid_signal = self.queue_deq_valid_signal(&sname, edge_idx);
                    let bits_signal = self.queue_deq_bits_signal(&sname, edge_idx);
                    let deq_ready = self.queue_deq_ready_signal(&sname, edge_idx);

                    for (i, arm) in arms.iter().enumerate() {
                        match &arm.pattern.node {
                            Pattern::Variant { name, fields } if name == "Some" => {
                                if i == 0 {
                                    self.line(&format!("when ({valid_signal}) {{"));
                                } else {
                                    self.line(&format!("}} .elsewhen ({valid_signal}) {{"));
                                }
                                self.indent();
                                // Assert deq_ready
                                self.line(&format!("{deq_ready} := true.B"));
                                // Bind inner value
                                if fields.len() == 1 {
                                    self.bind_pattern(&fields[0].node, &bits_signal, inner_ty, ctx);
                                }
                                for s in &arm.body {
                                    self.emit_stmt(&s.node, ctx);
                                }
                                self.dedent();
                            }
                            Pattern::Variant { name, .. } if name == "None" => {
                                self.line("} .otherwise {");
                                self.indent();
                                for s in &arm.body {
                                    self.emit_stmt(&s.node, ctx);
                                }
                                self.dedent();
                            }
                            _ => {
                                self.line("} .otherwise {");
                                self.indent();
                                for s in &arm.body {
                                    self.emit_stmt(&s.node, ctx);
                                }
                                self.dedent();
                            }
                        }
                    }
                    if !arms.is_empty() {
                        self.line("}");
                    }
                    return;
                }

                // Enum match
                if let Ty::Enum { variants, .. } = &scrut_ty {
                    let scrut_chisel = self.emit_expr(&scrutinee.node, ctx);
                    let num_variants = variants.len() as u64;
                    let tag_bits = if num_variants <= 1 {
                        1
                    } else {
                        (num_variants as f64).log2().ceil() as u64
                    };
                    let max_payload: u64 = variants
                        .iter()
                        .map(|(_, fs)| fs.iter().map(bit_width).sum::<u64>())
                        .max()
                        .unwrap_or(0);

                    for (i, arm) in arms.iter().enumerate() {
                        let cond = self.emit_match_condition(
                            &arm.pattern.node,
                            &scrut_chisel,
                            &scrut_ty,
                            tag_bits,
                            max_payload,
                            ctx,
                        );
                        if i == 0 {
                            self.line(&format!("when ({cond}) {{"));
                        } else {
                            self.line(&format!("}} .elsewhen ({cond}) {{"));
                        }
                        self.indent();
                        for s in &arm.body {
                            self.emit_stmt(&s.node, ctx);
                        }
                        self.dedent();
                    }
                    if !arms.is_empty() {
                        self.line("}");
                    }
                    return;
                }

                // Non-Option, non-Enum match — handle Some/None on plain values
                // (e.g. Cell peek wrapped in match Some(x) => ...)
                let scrut_chisel = self.emit_expr(&scrutinee.node, ctx);
                for (i, arm) in arms.iter().enumerate() {
                    let cond = match &arm.pattern.node {
                        Pattern::Literal(Literal::Int(n)) => {
                            format!("{scrut_chisel} === {n}.U")
                        }
                        Pattern::Wildcard => "true.B".to_string(),
                        Pattern::Bind(name) => {
                            ctx.vars.insert(name.clone(), scrut_chisel.clone());
                            ctx.var_types.insert(name.clone(), scrut_ty.clone());
                            "true.B".to_string()
                        }
                        Pattern::Variant { name, fields } if name == "Some" => {
                            // Non-Option Some pattern: bind inner value to scrutinee
                            if fields.len() == 1 {
                                self.bind_pattern(&fields[0].node, &scrut_chisel, &scrut_ty, ctx);
                            }
                            "true.B".to_string()
                        }
                        Pattern::Variant { name, .. } if name == "None" => "false.B".to_string(),
                        _ => "true.B".to_string(),
                    };
                    if i == 0 {
                        self.line(&format!("when ({cond}) {{"));
                    } else {
                        self.line(&format!("}} .elsewhen ({cond}) {{"));
                    }
                    self.indent();
                    for s in &arm.body {
                        self.emit_stmt(&s.node, ctx);
                    }
                    self.dedent();
                }
                if !arms.is_empty() {
                    self.line("}");
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Enum match condition
    // -----------------------------------------------------------------------

    fn emit_match_condition(
        &self,
        pattern: &Pattern,
        scrut_chisel: &str,
        scrut_ty: &Ty,
        tag_bits: u64,
        max_payload: u64,
        ctx: &mut RuleCtx,
    ) -> String {
        match pattern {
            Pattern::Variant { name, fields } => {
                if let Ty::Enum {
                    name: enum_name,
                    variants,
                    ..
                } = scrut_ty
                {
                    let qualified = format!("{enum_name}.{}", escape_scala_keyword(name));
                    if max_payload == 0 {
                        // Pure enum
                        return format!("{scrut_chisel} === {qualified}");
                    }
                    let total_w = tag_bits + max_payload;
                    let tag_sv = format!("{scrut_chisel}({}, {})", total_w - 1, max_payload);
                    // Bind payload fields
                    if let Some((_, variant_fields)) = variants.iter().find(|(vn, _)| vn == name)
                        && !variant_fields.is_empty()
                        && !fields.is_empty()
                    {
                        let mut offset = 0u64;
                        for (pat, vty) in fields.iter().zip(variant_fields.iter()).rev() {
                            let w = bit_width(vty);
                            let chisel_slice = if w == 1 {
                                format!("{scrut_chisel}({offset})")
                            } else {
                                format!("{scrut_chisel}({}, {})", offset + w - 1, offset)
                            };
                            self.bind_pattern(&pat.node, &chisel_slice, vty, ctx);
                            offset += w;
                        }
                    }
                    format!("{tag_sv} === {qualified}")
                } else {
                    format!("{scrut_chisel} === {name}")
                }
            }
            Pattern::Literal(Literal::Int(n)) => format!("{scrut_chisel} === {n}.U"),
            Pattern::Wildcard => "true.B".to_string(),
            Pattern::Bind(name) => {
                ctx.vars.insert(name.clone(), scrut_chisel.to_string());
                ctx.var_types.insert(name.clone(), scrut_ty.clone());
                "true.B".to_string()
            }
            _ => "true.B".to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // Expression emission
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
                            self.queue_deq_bits_signal(&sname, edge_idx)
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
                // try_take is handled in match statement emission
                if let Some(&edge_idx) = ctx.port_edges.get(queue) {
                    let edge = &self.net.network.graph[edge_idx];
                    let sname = sanitize(&edge.name);
                    self.queue_deq_valid_signal(&sname, edge_idx)
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
                // Records are packed bit vectors. Emit Cat(field_values) in
                // declaration order (MSB first, same as SV struct packed).
                if let Some(Ty::Record {
                    fields: ty_fields, ..
                }) = self.net.network.type_defs.get(name)
                {
                    // Build a map of field name -> expression
                    let field_map: HashMap<&str, &Spanned<Expr>> = fields
                        .iter()
                        .map(|(fname, val)| (fname.node.as_str(), val))
                        .collect();
                    // Emit in declaration order (MSB first for Cat)
                    let parts: Vec<String> = ty_fields
                        .iter()
                        .map(|(fname, _)| {
                            if let Some(val) = field_map.get(fname.as_str()) {
                                self.emit_expr(&val.node, ctx)
                            } else {
                                "0.U".to_string()
                            }
                        })
                        .collect();
                    format!("Cat({})", parts.join(", "))
                } else {
                    let parts: Vec<String> = fields
                        .iter()
                        .map(|(_, val)| self.emit_expr(&val.node, ctx))
                        .collect();
                    format!("Cat({})", parts.join(", "))
                }
            }
            Expr::FieldAccess { expr, field } => {
                // Field access on a packed bit vector: extract the right bits
                let parent_ty = self.infer_expr_type(&expr.node, ctx);
                let e = self.emit_expr(&expr.node, ctx);
                if let Ty::Record { fields, .. } = &parent_ty {
                    // Calculate bit offset and width for the field
                    // Fields are packed MSB-first (first field = highest bits)
                    let total_w: u64 = fields.iter().map(|(_, t)| bit_width(t)).sum();
                    let mut bit_pos = total_w;
                    for (fname, fty) in fields {
                        let fw = bit_width(fty);
                        bit_pos -= fw;
                        if fname == &field.node {
                            return if fw == 1 {
                                format!("{e}({bit_pos})")
                            } else {
                                format!("{e}({}, {})", bit_pos + fw - 1, bit_pos)
                            };
                        }
                    }
                }
                format!("{e} /* .{} unknown */", field.node)
            }
            Expr::Index { expr: e, index } => {
                let base = self.emit_expr(&e.node, ctx);
                let idx = self.emit_expr(&index.node, ctx);
                let base_ty = self.infer_expr_type(&e.node, ctx);
                if let Ty::Array { elem, .. } = &base_ty {
                    let elem_w = bit_width(elem);
                    // Dynamic array indexing: shift right by (idx * elem_w) and mask
                    // Chisel's (hi, lo) requires Int, not UInt, so we use >> and mask
                    format!("({base} >> ({idx} * {elem_w}.U))({}, 0)", elem_w - 1)
                } else {
                    format!("{base}({idx})")
                }
            }
            Expr::Call { func, args } => {
                // DPI calls have no Chisel equivalent. Emit 0.U as a placeholder.
                // The return type width is unknown here, so we use 0.U which
                // Chisel will infer. A comment on the preceding line documents the call.
                let _arg_strs: Vec<String> =
                    args.iter().map(|a| self.emit_expr(&a.node, ctx)).collect();
                let _ = func;
                "0.U".to_string()
            }
            Expr::Update {
                expr: e,
                index: _,
                value: _,
            } => {
                // Functional array update — no direct Chisel equivalent.
                // Emit the base value as a placeholder; the update is lost.
                // TODO: implement via VecInit + index assignment
                self.emit_expr(&e.node, ctx)
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
            Expr::Record { name, .. } => self
                .net
                .network
                .type_defs
                .get(name)
                .cloned()
                .unwrap_or(Ty::Error),
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
    // Queue signal helpers — abstract over internal vs external (IO port)
    // -----------------------------------------------------------------------

    fn queue_deq_valid(&self, sname: &str, edge_idx: EdgeIndex) -> String {
        self.queue_deq_valid_signal(sname, edge_idx)
    }

    fn queue_deq_valid_signal(&self, sname: &str, edge_idx: EdgeIndex) -> String {
        if self.memory_edges.contains_key(&edge_idx) {
            format!("io.q_{sname}.valid")
        } else {
            format!("q_{sname}.io.deq.valid")
        }
    }

    fn queue_deq_bits_signal(&self, sname: &str, edge_idx: EdgeIndex) -> String {
        if self.memory_edges.contains_key(&edge_idx) {
            format!("io.q_{sname}.bits")
        } else {
            format!("q_{sname}.io.deq.bits")
        }
    }

    fn queue_deq_ready_signal(&self, sname: &str, edge_idx: EdgeIndex) -> String {
        if self.memory_edges.contains_key(&edge_idx) {
            format!("io.q_{sname}.ready")
        } else {
            format!("q_{sname}.io.deq.ready")
        }
    }

    fn queue_enq_ready(&self, sname: &str, edge_idx: EdgeIndex) -> String {
        if self.memory_edges.contains_key(&edge_idx) {
            format!("io.q_{sname}.ready")
        } else {
            format!("q_{sname}.io.enq.ready")
        }
    }

    fn queue_enq_signals(&self, sname: &str, edge_idx: EdgeIndex) -> (String, String) {
        if self.memory_edges.contains_key(&edge_idx) {
            (format!("io.q_{sname}.valid"), format!("io.q_{sname}.bits"))
        } else {
            (
                format!("q_{sname}.io.enq.valid"),
                format!("q_{sname}.io.enq.bits"),
            )
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn chisel_type(&self, ty: &Ty) -> String {
        match ty {
            Ty::Bits(n) => format!("UInt({n}.W)"),
            Ty::Bool => "Bool()".to_string(),
            Ty::Tuple(ts) => {
                let total: u64 = ts.iter().map(bit_width).sum();
                format!("UInt({total}.W)")
            }
            Ty::Record { name, .. } => format!("new {name}"),
            Ty::Enum { variants, .. } => {
                let n = variants.len() as u64;
                let bits = if n <= 1 {
                    1
                } else {
                    (n as f64).log2().ceil() as u64
                };
                format!("UInt({bits}.W)")
            }
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

/// Backtick-escape Scala/Chisel reserved words that may appear as enum variant names.
fn escape_scala_keyword(name: &str) -> String {
    match name {
        "None" | "Some" | "type" | "val" | "var" | "def" | "class" | "object" | "trait"
        | "import" | "true" | "false" | "null" | "new" | "match" | "case" | "if" | "else"
        | "for" | "while" | "do" | "return" | "throw" | "try" | "catch" | "finally" | "yield"
        | "abstract" | "extends" | "with" | "override" | "lazy" | "sealed" | "implicit"
        | "private" | "protected" => format!("`{name}`"),
        _ => name.to_string(),
    }
}

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
        assert!(
            scala.contains("when (r_Counter_tick_will_fire)"),
            "when block"
        );
        assert!(scala.contains("c_Counter_count :="), "cell assignment");
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
        assert!(scala.contains(".io.enq.valid := true.B"), "enq valid");
        assert!(scala.contains(".io.enq.bits := 42.U"), "enq bits");
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
        // Both rules write the cell inside when blocks — last connect wins
        assert!(
            scala.contains("when (r_Counter_inc_will_fire)"),
            "inc when block"
        );
        assert!(
            scala.contains("when (r_Counter_dec_will_fire)"),
            "dec when block"
        );
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

        assert!(scala.contains("when ("), "conditional routing uses when");
        assert!(
            scala.contains("new Queue(UInt(32.W), 2)"),
            "Queue instances"
        );
    }

    #[test]
    fn record_construction_emits_cat() {
        let src = r#"
record Cmd { op : Bits 8, addr : Bits 32 }

process Sender {
    produces: out : Queue(Cmd)
    rule go {
        out.put(Cmd { op = 1, addr = 42 })
    }
}

process Receiver {
    consumes: inp : Queue(Cmd)
    rule go {
        let c = inp.take()
    }
}

pipe Top {
    let q = Queue(Cmd, depth = 2)
    Sender { out = q }
    Receiver { inp = q }
}
"#;
        let files = crate::build_chisel(src, "test.tbn").unwrap();
        let scala = &files[0].content;

        assert!(
            scala.contains("Cat(1.U, 42.U)"),
            "record construction uses Cat"
        );
        assert!(scala.contains("class Cmd extends Bundle"), "Bundle class");
    }

    #[test]
    fn field_access_emits_bit_slice() {
        let src = r#"
record Cmd { op : Bits 8, addr : Bits 32 }

process Reader {
    consumes: inp : Queue(Cmd)
    state: last_op : Cell(Bits 8, init = 0)
    rule go {
        let c = inp.take()
        let _ = last_op.take()
        last_op.put(c.op)
    }
}

process Writer {
    produces: out : Queue(Cmd)
    rule go {
        out.put(Cmd { op = 1, addr = 0 })
    }
}

pipe Top {
    let q = Queue(Cmd, depth = 2)
    Writer { out = q }
    Reader { inp = q }
}
"#;
        let files = crate::build_chisel(src, "test.tbn").unwrap();
        let scala = &files[0].content;

        // op field is bits [39:32] (MSB in a 40-bit packed record)
        assert!(scala.contains("(39, 32)"), "field access bit slice for op");
    }

    #[test]
    fn spi_v2_compiles() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let tbn_path = manifest_dir.join("examples/spi2tlul_v2.tbn");
        let src = std::fs::read_to_string(&tbn_path).unwrap();

        let files = crate::build_chisel(&src, "spi2tlul_v2.tbn").unwrap();
        assert_eq!(files.len(), 1);
        let scala = &files[0].content;

        // Record types
        assert!(scala.contains("class DmaDesc extends Bundle"), "DmaDesc");
        assert!(scala.contains("class TlA extends Bundle"), "TlA");
        assert!(scala.contains("class TlD extends Bundle"), "TlD");

        // External queues
        assert!(scala.contains("Decoupled("), "Decoupled IO");
        assert!(scala.contains("Flipped(Decoupled("), "Flipped IO");

        // AsyncQueues
        assert!(scala.contains("TbnAsyncFifo"), "AsyncQueue BlackBox");

        // Constants
        assert!(scala.contains("val TL_GET = 4.U"), "TL_GET constant");

        // when blocks
        assert!(scala.contains("when ("), "when blocks for rules");

        // Record construction (Cat)
        assert!(scala.contains("Cat("), "record construction uses Cat");

        // No SV constructs
        assert!(!scala.contains("always_ff"), "no SV");
        assert!(!scala.contains("always_comb"), "no SV");
    }
}
