use std::collections::{BTreeSet, HashMap};

use petgraph::graph::EdgeIndex;

use crate::ast::*;
use crate::ir::*;
use crate::schedule::*;
use crate::types::ty::Ty;

/// A generated SystemVerilog file.
pub struct SvFile {
    pub name: String,
    pub content: String,
}

// ---------------------------------------------------------------------------
// Bit-width calculation
// ---------------------------------------------------------------------------

/// Calculate the bit width of a resolved type.
pub fn bit_width(ty: &Ty) -> u64 {
    match ty {
        Ty::Bits(n) => *n,
        Ty::Bool => 1,
        Ty::Tuple(ts) => ts.iter().map(bit_width).sum(),
        Ty::Record { fields, .. } => fields.iter().map(|(_, t)| bit_width(t)).sum(),
        Ty::Enum { variants, .. } => {
            let num_variants = variants.len() as u64;
            let tag_bits = if num_variants <= 1 {
                0
            } else {
                (num_variants as f64).log2().ceil() as u64
            };
            let max_payload: u64 = variants
                .iter()
                .map(|(_, fields)| fields.iter().map(bit_width).sum::<u64>())
                .max()
                .unwrap_or(0);
            tag_bits + max_payload
        }
        Ty::Array { elem, size } => bit_width(elem) * size,
        Ty::Option(inner) => 1 + bit_width(inner),
        // These shouldn't appear at lowering level
        Ty::Queue { .. } | Ty::Cell { .. } | Ty::Named(_) | Ty::Error => 0,
    }
}

// ---------------------------------------------------------------------------
// FIFO module
// ---------------------------------------------------------------------------

fn fifo_module_sv() -> &'static str {
    r#"module tbn_fifo #(
    parameter WIDTH = 8,
    parameter DEPTH = 2
)(
    input  wire              clk,
    input  wire              rst_n,
    input  wire              enq_valid,
    output wire              enq_ready,
    input  wire [WIDTH-1:0]  enq_data,
    output wire              deq_valid,
    input  wire              deq_ready,
    output wire [WIDTH-1:0]  deq_data
);
    localparam AWIDTH = (DEPTH < 2) ? 1 : $clog2(DEPTH);

    reg [WIDTH-1:0]  storage [0:DEPTH-1];
    reg [AWIDTH-1:0] rd_ptr;
    reg [AWIDTH-1:0] wr_ptr;
    reg [AWIDTH:0]   count;

    assign enq_ready = (count < DEPTH);
    assign deq_valid = (count != 0);
    assign deq_data  = storage[rd_ptr];

    wire do_enq = enq_valid & enq_ready;
    wire do_deq = deq_ready & deq_valid;

    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) begin
            rd_ptr <= '0;
            wr_ptr <= '0;
            count  <= '0;
        end else begin
            if (do_enq) begin
                storage[wr_ptr] <= enq_data;
                if (wr_ptr == DEPTH - 1)
                    wr_ptr <= '0;
                else
                    wr_ptr <= wr_ptr + 1;
            end
            if (do_deq) begin
                if (rd_ptr == DEPTH - 1)
                    rd_ptr <= '0;
                else
                    rd_ptr <= rd_ptr + 1;
            end
            case ({do_enq, do_deq})
                2'b10:   count <= count + 1;
                2'b01:   count <= count - 1;
                default: ;
            endcase
        end
    end
endmodule
"#
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sanitize(name: &str) -> String {
    name.replace('.', "_")
}

fn width_decl(width: u64) -> String {
    if width <= 1 {
        String::new()
    } else {
        format!("[{}:0] ", width - 1)
    }
}

fn binop_sv(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::And => "&",
        BinOp::Or => "|",
        BinOp::Xor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Eq => "==",
        BinOp::Neq => "!=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Le => "<=",
        BinOp::Ge => ">=",
        BinOp::LogicalAnd => "&&",
        BinOp::LogicalOr => "||",
    }
}

fn unaryop_sv(op: &UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "~",
        UnaryOp::Neg => "-",
    }
}

/// Collect port names that use blocking `take()` (not `try_take()`) in a rule.
fn collect_blocking_takes(rule: &Rule) -> BTreeSet<String> {
    let mut takes = BTreeSet::new();
    for stmt in &rule.body {
        collect_takes_in_stmt(&stmt.node, &mut takes);
    }
    takes
}

fn collect_takes_in_stmt(stmt: &Stmt, takes: &mut BTreeSet<String>) {
    match stmt {
        Stmt::Let { value, .. } => collect_takes_in_expr(&value.node, takes),
        Stmt::Put { value, .. } => collect_takes_in_expr(&value.node, takes),
        Stmt::Expr(e) => collect_takes_in_expr(&e.node, takes),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            collect_takes_in_expr(&cond.node, takes);
            for s in then_body {
                collect_takes_in_stmt(&s.node, takes);
            }
            for s in else_body {
                collect_takes_in_stmt(&s.node, takes);
            }
        }
        Stmt::Match { scrutinee, arms } => {
            collect_takes_in_expr(&scrutinee.node, takes);
            for arm in arms {
                for s in &arm.body {
                    collect_takes_in_stmt(&s.node, takes);
                }
            }
        }
    }
}

fn collect_takes_in_expr(expr: &Expr, takes: &mut BTreeSet<String>) {
    match expr {
        Expr::Take { queue } => {
            takes.insert(queue.clone());
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_takes_in_expr(&lhs.node, takes);
            collect_takes_in_expr(&rhs.node, takes);
        }
        Expr::UnaryOp { expr, .. } => collect_takes_in_expr(&expr.node, takes),
        Expr::Tuple(items) => {
            for item in items {
                collect_takes_in_expr(&item.node, takes);
            }
        }
        Expr::Record { fields, .. } => {
            for (_, val) in fields {
                collect_takes_in_expr(&val.node, takes);
            }
        }
        Expr::FieldAccess { expr, .. } => collect_takes_in_expr(&expr.node, takes),
        Expr::Index { expr: e, index } => {
            collect_takes_in_expr(&e.node, takes);
            collect_takes_in_expr(&index.node, takes);
        }
        Expr::Update {
            expr: e,
            index,
            value,
        } => {
            collect_takes_in_expr(&e.node, takes);
            collect_takes_in_expr(&index.node, takes);
            collect_takes_in_expr(&value.node, takes);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_takes_in_expr(&arg.node, takes);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Lower scheduled networks to SystemVerilog files.
pub fn lower(scheduled: &[ScheduledNetwork], provenance: Option<[u8; 32]>) -> Vec<SvFile> {
    let mut files = vec![];

    let has_queues = scheduled.iter().any(|sn| {
        sn.network
            .graph
            .edge_weights()
            .any(|e| matches!(e.kind, QueueEdgeKind::Queue))
    });

    if has_queues {
        files.push(SvFile {
            name: "tbn_fifo.sv".to_string(),
            content: fifo_module_sv().to_string(),
        });
    }

    for sn in scheduled {
        let mut emitter = SvEmitter::new(sn, provenance);
        let content = emitter.emit();
        files.push(SvFile {
            name: format!("{}.sv", sn.network.name),
            content,
        });
    }

    files
}

// ---------------------------------------------------------------------------
// SV emitter
// ---------------------------------------------------------------------------

struct SvEmitter<'a> {
    net: &'a ScheduledNetwork,
    out: String,
    indent: usize,
    provenance: Option<[u8; 32]>,
}

/// Per-rule context for expression inlining.
#[allow(dead_code)]
struct RuleCtx<'a> {
    instance_name: String,
    rule_name: String,
    port_edges: &'a HashMap<String, EdgeIndex>,
    /// Variable name → inlined SV expression.
    vars: HashMap<String, String>,
}

impl<'a> SvEmitter<'a> {
    fn new(net: &'a ScheduledNetwork, provenance: Option<[u8; 32]>) -> Self {
        Self {
            net,
            out: String::new(),
            indent: 0,
            provenance,
        }
    }

    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
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

        self.line(&format!("module {pipe_name} ("));
        self.indent();
        self.line("input  wire clk,");
        self.line("input  wire rst_n");
        self.dedent();
        self.line(");");

        self.indent();
        if let Some(hash) = self.provenance {
            let hex = crate::provenance::hex(&hash);
            self.blank();
            self.line(&format!("// Tourbillon provenance: {hex}"));
            self.line(&format!(
                "localparam logic [255:0] TBN_PROVENANCE = 256'h{hex};"
            ));
        }
        self.blank();

        self.emit_queue_instances();
        self.emit_cell_declarations();
        self.emit_rule_enables();
        self.emit_rule_logic();
        self.emit_queue_wiring();
        self.emit_cell_writebacks();
        self.dedent();

        self.line("endmodule");
        self.out.clone()
    }

    // -----------------------------------------------------------------------
    // Queue FIFO declarations and instances
    // -----------------------------------------------------------------------

    fn emit_queue_instances(&mut self) {
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if !matches!(edge.kind, QueueEdgeKind::Queue) {
                continue;
            }
            let sname = sanitize(&edge.name);
            let w = bit_width(&edge.elem_ty);
            let wd = width_decl(w);

            self.line(&format!("// Queue: {}", edge.name));
            self.line(&format!("logic        q_{sname}_enq_valid;"));
            self.line(&format!("wire         q_{sname}_enq_ready;"));
            self.line(&format!("logic {wd}q_{sname}_enq_data;"));
            self.line(&format!("wire         q_{sname}_deq_valid;"));
            self.line(&format!("logic        q_{sname}_deq_ready;"));
            self.line(&format!("wire  {wd}q_{sname}_deq_data;"));
            self.blank();
            self.line(&format!(
                "tbn_fifo #(.WIDTH({w}), .DEPTH({})) q_{sname}_inst (",
                edge.depth
            ));
            self.indent();
            self.line(".clk(clk),");
            self.line(".rst_n(rst_n),");
            self.line(&format!(".enq_valid(q_{sname}_enq_valid),"));
            self.line(&format!(".enq_ready(q_{sname}_enq_ready),"));
            self.line(&format!(".enq_data(q_{sname}_enq_data),"));
            self.line(&format!(".deq_valid(q_{sname}_deq_valid),"));
            self.line(&format!(".deq_ready(q_{sname}_deq_ready),"));
            self.line(&format!(".deq_data(q_{sname}_deq_data)"));
            self.dedent();
            self.line(");");
            self.blank();
        }
    }

    // -----------------------------------------------------------------------
    // Cell register declarations
    // -----------------------------------------------------------------------

    fn emit_cell_declarations(&mut self) {
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if !matches!(edge.kind, QueueEdgeKind::Cell { .. }) {
                continue;
            }
            let sname = sanitize(&edge.name);
            let w = bit_width(&edge.elem_ty);
            let wd = width_decl(w);

            self.line(&format!("// Cell: {}", edge.name));
            self.line(&format!("logic {wd}c_{sname}_q;"));
            self.line(&format!("logic {wd}c_{sname}_d;"));
            self.line(&format!("logic c_{sname}_en;"));
            self.blank();
        }
    }

    // -----------------------------------------------------------------------
    // Rule enable signals (can_fire / will_fire)
    // -----------------------------------------------------------------------

    fn emit_rule_enables(&mut self) {
        for node_idx in self.net.network.graph.node_indices() {
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

                    // Blocking takes on Queue edges require deq_valid
                    for take_port in &blocking_takes[rule_idx] {
                        if let Some(&edge_idx) = port_edges.get(take_port) {
                            let edge = &self.net.network.graph[edge_idx];
                            if matches!(edge.kind, QueueEdgeKind::Queue) {
                                conditions.push(format!("q_{}_deq_valid", sanitize(&edge.name)));
                            }
                        }
                    }

                    // Puts on Queue edges require enq_ready
                    for put_port in &schedule.rule_resources[rule_idx].puts {
                        if let Some(&edge_idx) = port_edges.get(put_port) {
                            let edge = &self.net.network.graph[edge_idx];
                            if matches!(edge.kind, QueueEdgeKind::Queue) {
                                conditions.push(format!("q_{}_enq_ready", sanitize(&edge.name)));
                            }
                        }
                    }

                    let can_fire_expr = if conditions.is_empty() {
                        "1'b1".to_string()
                    } else {
                        conditions.join(" & ")
                    };

                    self.line(&format!(
                        "wire r_{inst}_{rule_name}_can_fire = {can_fire_expr};"
                    ));

                    // will_fire = can_fire with priority suppression
                    let will_fire_expr = if higher_will_fires.is_empty() {
                        format!("r_{inst}_{rule_name}_can_fire")
                    } else {
                        let suppress = higher_will_fires.join(" | ");
                        format!("r_{inst}_{rule_name}_can_fire & ~({suppress})")
                    };

                    self.line(&format!(
                        "wire r_{inst}_{rule_name}_will_fire = {will_fire_expr};"
                    ));
                }

                // Add this group's will_fires to suppression list
                for &rule_idx in &rule_indices {
                    let rule_name = &node.rules[rule_idx].name.node;
                    higher_will_fires.push(format!("r_{inst}_{rule_name}_will_fire"));
                }
            }
            self.blank();
        }
    }

    // -----------------------------------------------------------------------
    // Rule body logic (one big always_comb)
    // -----------------------------------------------------------------------

    fn emit_rule_logic(&mut self) {
        // Check if there's any logic to emit
        let has_cells = self
            .net
            .network
            .graph
            .edge_weights()
            .any(|e| matches!(e.kind, QueueEdgeKind::Cell { .. }));
        let has_queues = self
            .net
            .network
            .graph
            .edge_weights()
            .any(|e| matches!(e.kind, QueueEdgeKind::Queue));
        let has_rules = self
            .net
            .network
            .graph
            .node_weights()
            .any(|n| !n.rules.is_empty());

        if !has_cells && !has_queues && !has_rules {
            return;
        }

        self.line("always_comb begin");
        self.indent();

        // Default Cell signals
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if matches!(edge.kind, QueueEdgeKind::Cell { .. }) {
                let sname = sanitize(&edge.name);
                self.line(&format!("c_{sname}_d = c_{sname}_q;"));
                self.line(&format!("c_{sname}_en = 1'b0;"));
            }
        }

        // Default Queue enq signals
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if matches!(edge.kind, QueueEdgeKind::Queue) {
                let sname = sanitize(&edge.name);
                self.line(&format!("q_{sname}_enq_valid = 1'b0;"));
                self.line(&format!("q_{sname}_enq_data = '0;"));
            }
        }

        self.blank();

        // Rule bodies
        for node_idx in self.net.network.graph.node_indices() {
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
                    };

                    self.line(&format!("// Rule: {inst}.{rule_name}"));
                    self.line(&format!("if (r_{inst}_{rule_name}_will_fire) begin"));
                    self.indent();

                    for stmt in &rule.body {
                        self.emit_stmt(&stmt.node, &mut ctx);
                    }

                    self.dedent();
                    self.line("end");
                }
            }
        }

        self.dedent();
        self.line("end");
        self.blank();
    }

    // -----------------------------------------------------------------------
    // Queue wiring (deq_ready assigns)
    // -----------------------------------------------------------------------

    fn emit_queue_wiring(&mut self) {
        let mut deq_ready_drivers: HashMap<EdgeIndex, Vec<String>> = HashMap::new();

        for node_idx in self.net.network.graph.node_indices() {
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
                        if matches!(edge.kind, QueueEdgeKind::Queue) {
                            deq_ready_drivers
                                .entry(edge_idx)
                                .or_default()
                                .push(format!("r_{inst}_{rule_name}_will_fire"));
                        }
                    }
                }
            }
        }

        // Emit assign for each queue's deq_ready
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if !matches!(edge.kind, QueueEdgeKind::Queue) {
                continue;
            }
            let sname = sanitize(&edge.name);
            if let Some(drivers) = deq_ready_drivers.get(&edge_idx) {
                self.line(&format!(
                    "assign q_{sname}_deq_ready = {};",
                    drivers.join(" | ")
                ));
            } else {
                self.line(&format!("assign q_{sname}_deq_ready = 1'b0;"));
            }
        }

        if self
            .net
            .network
            .graph
            .edge_weights()
            .any(|e| matches!(e.kind, QueueEdgeKind::Queue))
        {
            self.blank();
        }
    }

    // -----------------------------------------------------------------------
    // Cell write-back (always_ff)
    // -----------------------------------------------------------------------

    fn emit_cell_writebacks(&mut self) {
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if let QueueEdgeKind::Cell { init, .. } = &edge.kind {
                let sname = sanitize(&edge.name);
                let w = bit_width(&edge.elem_ty);
                let init_val = init.map_or("'0".to_string(), |v| {
                    if w > 1 {
                        format!("{w}'d{v}")
                    } else if v <= 1 {
                        format!("1'b{v}")
                    } else {
                        format!("{v}")
                    }
                });

                self.line("always_ff @(posedge clk or negedge rst_n) begin");
                self.indent();
                self.line("if (!rst_n)");
                self.indent();
                self.line(&format!("c_{sname}_q <= {init_val};"));
                self.dedent();
                self.line(&format!("else if (c_{sname}_en)"));
                self.indent();
                self.line(&format!("c_{sname}_q <= c_{sname}_d;"));
                self.dedent();
                self.dedent();
                self.line("end");
                self.blank();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Statement emission
    // -----------------------------------------------------------------------

    fn emit_stmt(&mut self, stmt: &Stmt, ctx: &mut RuleCtx) {
        match stmt {
            Stmt::Let { pattern, value } => {
                let val_sv = self.emit_expr(&value.node, ctx);
                if let Pattern::Bind(name) = &pattern.node {
                    ctx.vars.insert(name.clone(), val_sv);
                }
                // Wildcard / tuple patterns: expression evaluated but no binding needed
            }
            Stmt::Put { target, value } => {
                let val_sv = self.emit_expr(&value.node, ctx);
                if let Some(&edge_idx) = ctx.port_edges.get(&target.node) {
                    let edge = &self.net.network.graph[edge_idx];
                    let sname = sanitize(&edge.name);
                    match &edge.kind {
                        QueueEdgeKind::Cell { .. } => {
                            self.line(&format!("c_{sname}_d = {val_sv};"));
                            self.line(&format!("c_{sname}_en = 1'b1;"));
                        }
                        QueueEdgeKind::Queue => {
                            self.line(&format!("q_{sname}_enq_data = {val_sv};"));
                            self.line(&format!("q_{sname}_enq_valid = 1'b1;"));
                        }
                    }
                }
            }
            Stmt::Expr(expr) => {
                // Expression as statement — evaluate for side effects (none in HW)
                let _ = self.emit_expr(&expr.node, ctx);
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                let cond_sv = self.emit_expr(&cond.node, ctx);
                self.line(&format!("if ({cond_sv}) begin"));
                self.indent();
                for s in then_body {
                    self.emit_stmt(&s.node, ctx);
                }
                self.dedent();
                if !else_body.is_empty() {
                    self.line("end else begin");
                    self.indent();
                    for s in else_body {
                        self.emit_stmt(&s.node, ctx);
                    }
                    self.dedent();
                }
                self.line("end");
            }
            Stmt::Match { scrutinee, arms } => {
                let scrut_sv = self.emit_expr(&scrutinee.node, ctx);
                for (i, arm) in arms.iter().enumerate() {
                    let cond = match &arm.pattern.node {
                        Pattern::Literal(Literal::Int(n)) => {
                            format!("{scrut_sv} == {n}")
                        }
                        Pattern::Literal(Literal::Bool(b)) => {
                            if *b {
                                scrut_sv.clone()
                            } else {
                                format!("!{scrut_sv}")
                            }
                        }
                        Pattern::Wildcard => "1'b1".to_string(),
                        Pattern::Bind(name) => {
                            ctx.vars.insert(name.clone(), scrut_sv.clone());
                            "1'b1".to_string()
                        }
                        _ => "1'b1".to_string(),
                    };

                    if i == 0 {
                        self.line(&format!("if ({cond}) begin"));
                    } else {
                        self.line(&format!("end else if ({cond}) begin"));
                    }
                    self.indent();
                    for s in &arm.body {
                        self.emit_stmt(&s.node, ctx);
                    }
                    self.dedent();
                }
                if !arms.is_empty() {
                    self.line("end");
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Expression emission (returns SV expression string)
    // -----------------------------------------------------------------------

    fn emit_expr(&self, expr: &Expr, ctx: &RuleCtx) -> String {
        match expr {
            Expr::Lit(Literal::Int(n)) => format!("{n}"),
            Expr::Lit(Literal::Bool(true)) => "1'b1".to_string(),
            Expr::Lit(Literal::Bool(false)) => "1'b0".to_string(),
            Expr::Var(name) => ctx.vars.get(name).cloned().unwrap_or_else(|| name.clone()),
            Expr::Take { queue } => {
                if let Some(&edge_idx) = ctx.port_edges.get(queue) {
                    let edge = &self.net.network.graph[edge_idx];
                    let sname = sanitize(&edge.name);
                    match &edge.kind {
                        QueueEdgeKind::Cell { .. } => format!("c_{sname}_q"),
                        QueueEdgeKind::Queue => format!("q_{sname}_deq_data"),
                    }
                } else {
                    format!("/* unknown port {queue} */ '0")
                }
            }
            Expr::Peek { queue } => {
                if let Some(&edge_idx) = ctx.port_edges.get(queue) {
                    let edge = &self.net.network.graph[edge_idx];
                    let sname = sanitize(&edge.name);
                    format!("c_{sname}_q")
                } else {
                    format!("/* unknown port {queue} */ '0")
                }
            }
            Expr::TryTake { queue } => {
                if let Some(&edge_idx) = ctx.port_edges.get(queue) {
                    let edge = &self.net.network.graph[edge_idx];
                    let sname = sanitize(&edge.name);
                    format!("{{q_{sname}_deq_valid, q_{sname}_deq_data}}")
                } else {
                    format!("/* unknown port {queue} */ '0")
                }
            }
            Expr::BinOp { op, lhs, rhs } => {
                let l = self.emit_expr(&lhs.node, ctx);
                let r = self.emit_expr(&rhs.node, ctx);
                let op_sv = binop_sv(op);
                format!("({l} {op_sv} {r})")
            }
            Expr::UnaryOp { op, expr } => {
                let e = self.emit_expr(&expr.node, ctx);
                let op_sv = unaryop_sv(op);
                format!("({op_sv}{e})")
            }
            Expr::Tuple(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|item| self.emit_expr(&item.node, ctx))
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
            Expr::Record { fields, .. } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(_, val)| self.emit_expr(&val.node, ctx))
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
            Expr::FieldAccess { expr, field } => {
                let e = self.emit_expr(&expr.node, ctx);
                // Simplified: emit as SV field access
                format!("{e}.{}", field.node)
            }
            Expr::Index { expr: e, index } => {
                let base = self.emit_expr(&e.node, ctx);
                let idx = self.emit_expr(&index.node, ctx);
                format!("{base}[{idx}]")
            }
            Expr::Call { func, args } => {
                let arg_strs: Vec<String> =
                    args.iter().map(|a| self.emit_expr(&a.node, ctx)).collect();
                format!("{func}({})", arg_strs.join(", "))
            }
            Expr::Update { .. } | Expr::MethodCall { .. } => {
                "/* unsupported expr */ '0".to_string()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_width_bits() {
        assert_eq!(bit_width(&Ty::Bits(32)), 32);
        assert_eq!(bit_width(&Ty::Bits(1)), 1);
        assert_eq!(bit_width(&Ty::Bits(64)), 64);
    }

    #[test]
    fn bit_width_bool() {
        assert_eq!(bit_width(&Ty::Bool), 1);
    }

    #[test]
    fn bit_width_tuple() {
        assert_eq!(bit_width(&Ty::Tuple(vec![Ty::Bits(16), Ty::Bits(8)])), 24);
    }

    #[test]
    fn bit_width_record() {
        assert_eq!(
            bit_width(&Ty::Record {
                name: "R".to_string(),
                fields: vec![
                    ("a".to_string(), Ty::Bits(16)),
                    ("b".to_string(), Ty::Bits(8)),
                ],
            }),
            24
        );
    }

    #[test]
    fn bit_width_enum() {
        // 3 variants: tag = ceil(log2(3)) = 2 bits
        // Max payload = 8 bits → total = 10
        assert_eq!(
            bit_width(&Ty::Enum {
                name: "E".to_string(),
                variants: vec![
                    ("A".to_string(), vec![Ty::Bits(8)]),
                    ("B".to_string(), vec![Ty::Bits(4)]),
                    ("C".to_string(), vec![]),
                ],
            }),
            10
        );
    }

    #[test]
    fn bit_width_array() {
        assert_eq!(
            bit_width(&Ty::Array {
                elem: Box::new(Ty::Bits(8)),
                size: 4,
            }),
            32
        );
    }

    #[test]
    fn bit_width_option() {
        assert_eq!(bit_width(&Ty::Option(Box::new(Ty::Bits(32)))), 33);
    }

    #[test]
    fn expr_binop_and_unary() {
        assert_eq!(binop_sv(&BinOp::Add), "+");
        assert_eq!(binop_sv(&BinOp::Eq), "==");
        assert_eq!(binop_sv(&BinOp::LogicalAnd), "&&");
        assert_eq!(unaryop_sv(&UnaryOp::Not), "~");
        assert_eq!(unaryop_sv(&UnaryOp::Neg), "-");
    }

    #[test]
    fn counter_sv_structure() {
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
        let files = crate::build(src, "test.tbn", None).unwrap();
        assert_eq!(files.len(), 1); // No queues → no tbn_fifo.sv
        let sv = &files[0].content;

        assert!(sv.contains("module Top"), "module declaration");
        assert!(sv.contains("always_ff"), "cell writeback");
        assert!(sv.contains("always_comb"), "rule logic");
        assert!(sv.contains("c_Counter_count_q"), "cell _q signal");
        assert!(sv.contains("c_Counter_count_d"), "cell _d signal");
        assert!(sv.contains("c_Counter_count_en"), "cell _en signal");
        assert!(sv.contains("rst_n"), "reset signal");
        assert!(sv.contains("32'd0"), "cell init value");
        assert!(sv.contains("endmodule"), "endmodule");
    }
}
