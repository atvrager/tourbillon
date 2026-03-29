use std::collections::{BTreeSet, HashMap, HashSet};

use petgraph::graph::{EdgeIndex, NodeIndex};

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
        Ty::Queue { .. } | Ty::Cell { .. } | Ty::AsyncQueue { .. } | Ty::Named(_) | Ty::Error => 0,
    }
}

// ---------------------------------------------------------------------------
// FIFO module
// ---------------------------------------------------------------------------

fn fifo_module_sv() -> &'static str {
    r#"module tbn_fifo #(
    parameter WIDTH = 8,
    parameter DEPTH = 2,
    parameter INIT_COUNT = 0,
    parameter [WIDTH-1:0] INIT_VALUE = '0
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
            count  <= INIT_COUNT;
            wr_ptr <= INIT_COUNT[AWIDTH-1:0];
            for (integer _i = 0; _i < INIT_COUNT && _i < DEPTH; _i++)
                storage[_i] <= INIT_VALUE;
        end else begin
            if (do_enq) begin
                storage[wr_ptr] <= enq_data;
                if (wr_ptr == AWIDTH'(DEPTH - 1))
                    wr_ptr <= '0;
                else
                    wr_ptr <= wr_ptr + 1;
            end
            if (do_deq) begin
                if (rd_ptr == AWIDTH'(DEPTH - 1))
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

fn async_fifo_module_sv() -> &'static str {
    r#"module tbn_async_fifo #(
    parameter WIDTH = 8,
    parameter DEPTH = 4
)(
    input  wire              wr_clk,
    input  wire              wr_rst_n,
    input  wire              enq_valid,
    output wire              enq_ready,
    input  wire [WIDTH-1:0]  enq_data,
    input  wire              rd_clk,
    input  wire              rd_rst_n,
    output wire              deq_valid,
    input  wire              deq_ready,
    output wire [WIDTH-1:0]  deq_data
);
    localparam AWIDTH = $clog2(DEPTH);

    reg [WIDTH-1:0] storage [0:DEPTH-1];

    // Write-side pointers (wr_clk domain)
    reg [AWIDTH:0] wr_ptr_bin;
    wire [AWIDTH:0] wr_ptr_gray;
    reg [AWIDTH:0] wr_ptr_gray_sync1;
    reg [AWIDTH:0] wr_ptr_gray_sync2;

    // Read-side pointers (rd_clk domain)
    reg [AWIDTH:0] rd_ptr_bin;
    wire [AWIDTH:0] rd_ptr_gray;
    reg [AWIDTH:0] rd_ptr_gray_sync1;
    reg [AWIDTH:0] rd_ptr_gray_sync2;

    // Binary-to-gray conversion
    assign wr_ptr_gray = wr_ptr_bin ^ (wr_ptr_bin >> 1);
    assign rd_ptr_gray = rd_ptr_bin ^ (rd_ptr_bin >> 1);

    // Full: MSBs differ, remaining bits match (gray code property)
    wire full  = (wr_ptr_gray[AWIDTH] != rd_ptr_gray_sync2[AWIDTH]) &&
                 (wr_ptr_gray[AWIDTH-1] != rd_ptr_gray_sync2[AWIDTH-1]) &&
                 (wr_ptr_gray[AWIDTH-2:0] == rd_ptr_gray_sync2[AWIDTH-2:0]);
    // Empty: gray pointers equal
    wire empty = (rd_ptr_gray == wr_ptr_gray_sync2);

    assign enq_ready = ~full;
    assign deq_valid = ~empty;
    assign deq_data  = storage[rd_ptr_bin[AWIDTH-1:0]];

    wire do_enq = enq_valid & enq_ready;
    wire do_deq = deq_ready & deq_valid;

    // Write-side logic (wr_clk)
    always_ff @(posedge wr_clk or negedge wr_rst_n) begin
        if (!wr_rst_n) begin
            wr_ptr_bin <= '0;
            rd_ptr_gray_sync1 <= '0;
            rd_ptr_gray_sync2 <= '0;
        end else begin
            // 2-FF synchronizer for read pointer into write domain
            rd_ptr_gray_sync1 <= rd_ptr_gray;
            rd_ptr_gray_sync2 <= rd_ptr_gray_sync1;
            if (do_enq) begin
                storage[wr_ptr_bin[AWIDTH-1:0]] <= enq_data;
                wr_ptr_bin <= wr_ptr_bin + 1;
            end
        end
    end

    // Read-side logic (rd_clk)
    always_ff @(posedge rd_clk or negedge rd_rst_n) begin
        if (!rd_rst_n) begin
            rd_ptr_bin <= '0;
            wr_ptr_gray_sync1 <= '0;
            wr_ptr_gray_sync2 <= '0;
        end else begin
            // 2-FF synchronizer for write pointer into read domain
            wr_ptr_gray_sync1 <= wr_ptr_gray;
            wr_ptr_gray_sync2 <= wr_ptr_gray_sync1;
            if (do_deq) begin
                rd_ptr_bin <= rd_ptr_bin + 1;
            end
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

/// Wrap an SV expression in a width cast `W'(expr)` to match the target width.
/// Avoids redundant casts for plain literals and already-matching widths.
fn width_cast(expr: &str, target_width: u64) -> String {
    if target_width <= 1 {
        expr.to_string()
    } else {
        format!("{target_width}'({expr})")
    }
}

/// Get the SV type declaration for a signal carrying a value of the given type.
/// Returns the type name for records/enums, or `logic [N-1:0]` for primitives.
fn sv_type_decl(ty: &Ty) -> String {
    match ty {
        Ty::Record { name, .. } => format!("{name} "),
        Ty::Enum { name, .. } => format!("{name} "),
        _ => {
            let w = bit_width(ty);
            if w <= 1 {
                "logic ".to_string()
            } else {
                format!("logic [{}:0] ", w - 1)
            }
        }
    }
}

/// Map a Tourbillon type to a DPI-C type string.
fn dpi_type_str(ty: &Ty) -> String {
    match ty {
        Ty::Bool => "bit".to_string(),
        Ty::Bits(n) if *n <= 8 => "byte unsigned".to_string(),
        Ty::Bits(n) if *n <= 16 => "shortint unsigned".to_string(),
        Ty::Bits(n) if *n <= 32 => "int unsigned".to_string(),
        Ty::Bits(_) => "longint unsigned".to_string(),
        _ => "int unsigned".to_string(),
    }
}

/// Extract the data portion from a try_take scrutinee expression.
/// try_take emits `{q_foo_deq_valid, q_foo_deq_data}`.
/// For the Some(x) binding, we want just `q_foo_deq_data`.
fn extract_try_take_data(scrut_sv: &str, inner_w: u64) -> String {
    // If scrut_sv matches `{X, Y}`, extract Y (the data part)
    if let Some(inner) = scrut_sv.strip_prefix('{')
        && let Some(inner) = inner.strip_suffix('}')
        && let Some(comma_pos) = inner.find(", ")
    {
        return inner[comma_pos + 2..].to_string();
    }
    // Fallback: bit-slice
    if inner_w == 1 {
        format!("{scrut_sv}[0]")
    } else {
        format!("{scrut_sv}[{}:0]", inner_w - 1)
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

/// Collect port names that are unconditionally `put()` in a rule.
/// Only top-level puts count — puts inside if/match branches are conditional.
fn collect_unconditional_puts(rule: &Rule) -> BTreeSet<String> {
    let mut puts = BTreeSet::new();
    for stmt in &rule.body {
        if let Stmt::Put { target, .. } = &stmt.node {
            puts.insert(target.node.clone());
        }
        // Don't recurse into if/match — those are conditional
    }
    puts
}

/// Collect port names that use blocking `take()` (not `try_take()`) in a rule.
fn collect_blocking_takes(rule: &Rule) -> BTreeSet<String> {
    let mut takes = BTreeSet::new();
    for stmt in &rule.body {
        collect_takes_in_stmt(&stmt.node, &mut takes);
    }
    takes
}

/// Collect port names that use `try_take()` in a rule.
fn collect_try_takes(rule: &Rule) -> BTreeSet<String> {
    let mut try_takes = BTreeSet::new();
    for stmt in &rule.body {
        collect_try_takes_in_stmt(&stmt.node, &mut try_takes);
    }
    try_takes
}

fn collect_try_takes_in_stmt(stmt: &Stmt, try_takes: &mut BTreeSet<String>) {
    match stmt {
        Stmt::Let { value, .. } => collect_try_takes_in_expr(&value.node, try_takes),
        Stmt::Put { value, .. } => collect_try_takes_in_expr(&value.node, try_takes),
        Stmt::Expr(e) => collect_try_takes_in_expr(&e.node, try_takes),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            collect_try_takes_in_expr(&cond.node, try_takes);
            for s in then_body {
                collect_try_takes_in_stmt(&s.node, try_takes);
            }
            for s in else_body {
                collect_try_takes_in_stmt(&s.node, try_takes);
            }
        }
        Stmt::Match { scrutinee, arms } => {
            collect_try_takes_in_expr(&scrutinee.node, try_takes);
            for arm in arms {
                for s in &arm.body {
                    collect_try_takes_in_stmt(&s.node, try_takes);
                }
            }
        }
    }
}

fn collect_try_takes_in_expr(expr: &Expr, try_takes: &mut BTreeSet<String>) {
    match expr {
        Expr::TryTake { queue } => {
            try_takes.insert(queue.clone());
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_try_takes_in_expr(&lhs.node, try_takes);
            collect_try_takes_in_expr(&rhs.node, try_takes);
        }
        Expr::UnaryOp { expr, .. } => collect_try_takes_in_expr(&expr.node, try_takes),
        Expr::Tuple(items) => {
            for item in items {
                collect_try_takes_in_expr(&item.node, try_takes);
            }
        }
        Expr::Record { fields, .. } => {
            for (_, val) in fields {
                collect_try_takes_in_expr(&val.node, try_takes);
            }
        }
        Expr::FieldAccess { expr, .. } => collect_try_takes_in_expr(&expr.node, try_takes),
        Expr::Index { expr: e, index } => {
            collect_try_takes_in_expr(&e.node, try_takes);
            collect_try_takes_in_expr(&index.node, try_takes);
        }
        Expr::Update {
            expr: e,
            index,
            value,
        } => {
            collect_try_takes_in_expr(&e.node, try_takes);
            collect_try_takes_in_expr(&index.node, try_takes);
            collect_try_takes_in_expr(&value.node, try_takes);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_try_takes_in_expr(&arg.node, try_takes);
            }
        }
        Expr::BitSlice { expr, .. } => collect_try_takes_in_expr(&expr.node, try_takes),
        _ => {}
    }
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
        Expr::BitSlice { expr, .. } => collect_takes_in_expr(&expr.node, takes),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Lower scheduled networks to SystemVerilog files.
/// Returns true for Queue and AsyncQueue edge kinds (not Cell).
fn is_queue_like(kind: &QueueEdgeKind) -> bool {
    matches!(
        kind,
        QueueEdgeKind::Queue { .. } | QueueEdgeKind::AsyncQueue
    )
}

pub fn lower(scheduled: &[ScheduledNetwork], provenance: Option<[u8; 32]>) -> Vec<SvFile> {
    let mut files = vec![];

    let has_sync_queues = scheduled.iter().any(|sn| {
        sn.network
            .graph
            .edge_weights()
            .any(|e| matches!(e.kind, QueueEdgeKind::Queue { .. }))
    });

    let has_async_queues = scheduled.iter().any(|sn| {
        sn.network
            .graph
            .edge_weights()
            .any(|e| matches!(e.kind, QueueEdgeKind::AsyncQueue))
    });

    if has_sync_queues {
        files.push(SvFile {
            name: "tbn_fifo.sv".to_string(),
            content: fifo_module_sv().to_string(),
        });
    }

    if has_async_queues {
        files.push(SvFile {
            name: "tbn_async_fifo.sv".to_string(),
            content: async_fifo_module_sv().to_string(),
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
    /// Edge indices for memory-exposed edges. Value is true if CPU is the writer (enq side).
    memory_edges: HashMap<EdgeIndex, bool>,
    /// Node indices for memory stub process nodes.
    memory_stub_nodes: HashSet<NodeIndex>,
    /// Queue edges that are ONLY accessed via try_take (not blocking take).
    /// These need deq_ready driven from always_comb, not assign.
    try_take_only_edges: HashSet<EdgeIndex>,
}

/// Per-rule context for expression inlining.
#[allow(dead_code)]
struct RuleCtx<'a> {
    instance_name: String,
    rule_name: String,
    port_edges: &'a HashMap<String, EdgeIndex>,
    /// Variable name → inlined SV expression.
    vars: HashMap<String, String>,
    /// Variable name → resolved type (for tuple destructuring, field access).
    var_types: HashMap<String, Ty>,
    /// Counter for generating unique temporary variable names.
    temp_counter: usize,
    /// deq_ready signals to assert inside match Some arms.
    /// Set during emit_match_condition for try_take patterns.
    pending_deq_readys: Vec<String>,
}

impl<'a> SvEmitter<'a> {
    fn new(net: &'a ScheduledNetwork, provenance: Option<[u8; 32]>) -> Self {
        let mut memory_edges = HashMap::new();
        let mut memory_stub_nodes = HashSet::new();

        for node_idx in net.network.graph.node_indices() {
            if net.network.graph[node_idx].is_memory_stub {
                memory_stub_nodes.insert(node_idx);
            }
        }

        for edge_idx in net.network.graph.edge_indices() {
            let edge = &net.network.graph[edge_idx];

            // Memory stub edges
            if let Some((src, dst)) = net.network.graph.edge_endpoints(edge_idx)
                && (memory_stub_nodes.contains(&src) || memory_stub_nodes.contains(&dst))
            {
                // CPU is writer if the memory stub is the destination (consumer)
                let cpu_is_writer = memory_stub_nodes.contains(&dst);
                memory_edges.insert(edge_idx, cpu_is_writer);
                continue;
            }

            // External queue edges (self-loops or directed edges with is_external flag)
            if let QueueEdgeKind::Queue {
                is_external: true, ..
            } = &edge.kind
            {
                // Determine direction by checking port kinds that reference this edge
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
                // cpu_is_writer=true means internal process writes → expose enq side as output
                memory_edges.insert(edge_idx, has_writer);
            }
        }

        // Compute try_take-only edges: edges accessed via try_take but NOT blocking take
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
            provenance,
            memory_edges,
            memory_stub_nodes,
            try_take_only_edges,
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
        let mem_ports = self.collect_memory_port_decls();

        let domains = &self.net.network.domains;
        let has_domains = !domains.is_empty();

        // Determine if any non-stub process uses the default domain (no annotation)
        let has_default_domain = !has_domains
            || self.net.network.domain_map.iter().any(|(inst, d)| {
                d.is_none()
                    && !self
                        .memory_stub_nodes
                        .iter()
                        .any(|&ni| self.net.network.graph[ni].instance_name == *inst)
            });

        self.line("/* verilator lint_off UNUSEDSIGNAL */");
        self.line("/* verilator lint_off UNUSEDPARAM */");
        self.line(&format!("module {pipe_name} ("));
        self.indent();

        let mut all_ports: Vec<String> = vec![];

        if has_default_domain || !has_domains {
            all_ports.push("input  wire clk".to_string());
            all_ports.push("input  wire rst_n".to_string());
        }

        if has_domains {
            for domain in domains {
                all_ports.push(format!("input  wire {domain}_clk"));
                all_ports.push(format!("input  wire {domain}_rst_n"));
            }
        }

        all_ports.extend(mem_ports);

        for (i, port) in all_ports.iter().enumerate() {
            if i + 1 < all_ports.len() {
                self.line(&format!("{port},"));
            } else {
                self.line(port);
            }
        }

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

        self.emit_constants();
        self.emit_dpi_imports();
        self.emit_type_declarations();
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
    // Constants and DPI imports
    // -----------------------------------------------------------------------

    fn emit_constants(&mut self) {
        let constants = &self.net.network.constants;
        if constants.is_empty() {
            return;
        }
        let mut sorted: Vec<(&String, &u64)> = constants.iter().collect();
        sorted.sort_by_key(|(name, _)| (*name).clone());
        for (name, value) in sorted {
            self.line(&format!("localparam {name} = {value};"));
        }
        self.blank();
    }

    fn emit_dpi_imports(&mut self) {
        let external_fns = &self.net.network.external_fns;
        if external_fns.is_empty() {
            return;
        }
        #[allow(clippy::type_complexity)]
        let mut sorted: Vec<(&String, &(Vec<Ty>, Option<Ty>))> = external_fns.iter().collect();
        sorted.sort_by_key(|(name, _)| (*name).clone());
        for (name, (param_tys, ret_ty)) in sorted {
            let ret_sv = if let Some(ty) = ret_ty {
                dpi_type_str(ty)
            } else {
                "void".to_string()
            };
            let params: Vec<String> = param_tys
                .iter()
                .enumerate()
                .map(|(i, ty)| format!("input {} p{i}", dpi_type_str(ty)))
                .collect();
            self.line(&format!(
                "import \"DPI-C\" function {ret_sv} {name}({});",
                params.join(", ")
            ));
        }
        self.blank();
    }

    // -----------------------------------------------------------------------
    // Type declarations (struct packed, enum)
    // -----------------------------------------------------------------------

    /// Collect all record/enum types reachable from the network and emit
    /// `typedef struct packed` / `typedef enum logic` at the top of the module.
    fn emit_type_declarations(&mut self) {
        let type_defs = &self.net.network.type_defs;
        if type_defs.is_empty() {
            return;
        }

        // Sort by name for deterministic output
        let mut sorted: Vec<(&String, &Ty)> = type_defs.iter().collect();
        sorted.sort_by_key(|(name, _)| (*name).clone());

        // Collect which types are actually referenced by the network.
        // For simplicity, emit all records and enums from the type registry.
        let mut emitted_any = false;

        for (name, ty) in &sorted {
            match ty {
                Ty::Record { fields, .. } => {
                    self.line("typedef struct packed {");
                    self.indent();
                    for (fname, fty) in fields {
                        let w = bit_width(fty);
                        let wd = width_decl(w);
                        self.line(&format!("logic {wd}{fname};"));
                    }
                    self.dedent();
                    self.line(&format!("}} {name};"));
                    self.blank();
                    emitted_any = true;
                }
                Ty::Enum { variants, .. } => {
                    let num_variants = variants.len() as u64;
                    let tag_bits = if num_variants <= 1 {
                        1
                    } else {
                        (num_variants as f64).log2().ceil() as u64
                    };
                    self.line(&format!("typedef enum logic [{}:0] {{", tag_bits - 1));
                    self.indent();
                    for (i, (vname, _)) in variants.iter().enumerate() {
                        let sep = if i + 1 < variants.len() { "," } else { "" };
                        self.line(&format!("{vname} = {i}{sep}"));
                    }
                    self.dedent();
                    self.line(&format!("}} {name};"));
                    self.blank();
                    emitted_any = true;
                }
                _ => {}
            }
        }

        let _ = emitted_any;
    }

    // -----------------------------------------------------------------------
    // Memory port declarations (for memory stub edges exposed as module ports)
    // -----------------------------------------------------------------------

    fn collect_memory_port_decls(&self) -> Vec<String> {
        let mut ports = vec![];
        // Sort by edge index for deterministic output
        let mut sorted: Vec<(EdgeIndex, bool)> =
            self.memory_edges.iter().map(|(&k, &v)| (k, v)).collect();
        sorted.sort_by_key(|(idx, _)| idx.index());

        for (edge_idx, cpu_is_writer) in &sorted {
            let edge = &self.net.network.graph[*edge_idx];
            let sname = sanitize(&edge.name);
            let w = bit_width(&edge.elem_ty);
            let wd = width_decl(w);

            if *cpu_is_writer {
                // CPU writes (enq side is CPU-facing)
                ports.push(format!("output logic        q_{sname}_enq_valid"));
                ports.push(format!("input  wire         q_{sname}_enq_ready"));
                ports.push(format!("output logic {wd}q_{sname}_enq_data"));
            } else {
                // CPU reads (deq side is CPU-facing)
                ports.push(format!("input  wire         q_{sname}_deq_valid"));
                ports.push(format!("output logic        q_{sname}_deq_ready"));
                ports.push(format!("input  wire  {wd}q_{sname}_deq_data"));
            }
        }
        ports
    }

    // -----------------------------------------------------------------------
    // Queue FIFO declarations and instances
    // -----------------------------------------------------------------------

    /// Determine the clock signal name for an instance based on its domain.
    fn clock_for_instance(&self, instance_name: &str) -> String {
        if let Some(Some(domain)) = self.net.network.domain_map.get(instance_name) {
            format!("{domain}_clk")
        } else {
            "clk".to_string()
        }
    }

    /// Determine the reset signal name for an instance based on its domain.
    fn reset_for_instance(&self, instance_name: &str) -> String {
        if let Some(Some(domain)) = self.net.network.domain_map.get(instance_name) {
            format!("{domain}_rst_n")
        } else {
            "rst_n".to_string()
        }
    }

    fn emit_queue_instances(&mut self) {
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if !is_queue_like(&edge.kind) {
                continue;
            }
            // Skip memory-exposed edges — no FIFO, signals are module ports
            if self.memory_edges.contains_key(&edge_idx) {
                continue;
            }
            let sname = sanitize(&edge.name);
            let w = bit_width(&edge.elem_ty);
            let td = sv_type_decl(&edge.elem_ty);

            // Use typed signals for struct/enum data; raw logic for primitives.
            // FIFO connects via raw wires; we cast via separate typed signals if needed.
            let is_structured = matches!(edge.elem_ty, Ty::Record { .. } | Ty::Enum { .. });
            let wd = width_decl(w);

            let kind_label = if matches!(edge.kind, QueueEdgeKind::AsyncQueue) {
                "AsyncQueue"
            } else {
                "Queue"
            };
            self.line(&format!("// {kind_label}: {}", edge.name));
            self.line(&format!("logic        q_{sname}_enq_valid;"));
            self.line(&format!("wire         q_{sname}_enq_ready;"));
            if is_structured {
                self.line(&format!("{td}q_{sname}_enq_data;"));
            } else {
                self.line(&format!("logic {wd}q_{sname}_enq_data;"));
            }
            self.line(&format!("wire         q_{sname}_deq_valid;"));
            self.line(&format!("logic        q_{sname}_deq_ready;"));
            if is_structured {
                // For structured types, we need a raw wire from FIFO + typed alias
                self.line(&format!("wire  {wd}q_{sname}_deq_data_raw;"));
                self.line(&format!("{td}q_{sname}_deq_data;"));
                self.line(&format!(
                    "assign q_{sname}_deq_data = q_{sname}_deq_data_raw;"
                ));
            } else {
                self.line(&format!("wire  {wd}q_{sname}_deq_data;"));
            }
            self.blank();

            if matches!(edge.kind, QueueEdgeKind::AsyncQueue) {
                // Async FIFO: look up writer/reader domains for clock wiring
                let (src_node, dst_node) = self.net.network.graph.edge_endpoints(edge_idx).unwrap();
                let wr_clk =
                    self.clock_for_instance(&self.net.network.graph[src_node].instance_name);
                let wr_rst =
                    self.reset_for_instance(&self.net.network.graph[src_node].instance_name);
                let rd_clk =
                    self.clock_for_instance(&self.net.network.graph[dst_node].instance_name);
                let rd_rst =
                    self.reset_for_instance(&self.net.network.graph[dst_node].instance_name);

                self.line(&format!(
                    "tbn_async_fifo #(.WIDTH({w}), .DEPTH({})) aq_{sname}_inst (",
                    edge.depth
                ));
                self.indent();
                self.line(&format!(".wr_clk({wr_clk}),"));
                self.line(&format!(".wr_rst_n({wr_rst}),"));
                self.line(&format!(".enq_valid(q_{sname}_enq_valid),"));
                self.line(&format!(".enq_ready(q_{sname}_enq_ready),"));
                self.line(&format!(".enq_data(q_{sname}_enq_data),"));
                self.line(&format!(".deq_valid(q_{sname}_deq_valid),"));
                self.line(&format!(".deq_ready(q_{sname}_deq_ready),"));
                self.line(&format!(".rd_clk({rd_clk}),"));
                self.line(&format!(".rd_rst_n({rd_rst}),"));
                if is_structured {
                    self.line(&format!(".deq_data(q_{sname}_deq_data_raw)"));
                } else {
                    self.line(&format!(".deq_data(q_{sname}_deq_data)"));
                }
                self.dedent();
                self.line(");");
                self.blank();
                continue;
            }

            // Sync FIFO — use domain clock if both endpoints are in the same domain
            let (fifo_src, _fifo_dst) = self.net.network.graph.edge_endpoints(edge_idx).unwrap();
            let fifo_clk = self.clock_for_instance(&self.net.network.graph[fifo_src].instance_name);
            let fifo_rst = self.reset_for_instance(&self.net.network.graph[fifo_src].instance_name);
            let init_value = match &edge.kind {
                QueueEdgeKind::Queue { init_tokens, .. } if *init_tokens > 0 => Some(*init_tokens),
                _ => None,
            };
            if let Some(val) = init_value {
                self.line(&format!(
                    "tbn_fifo #(.WIDTH({w}), .DEPTH({}), .INIT_COUNT(1), .INIT_VALUE({w}'d{val})) q_{sname}_inst (",
                    edge.depth
                ));
            } else {
                self.line(&format!(
                    "tbn_fifo #(.WIDTH({w}), .DEPTH({})) q_{sname}_inst (",
                    edge.depth
                ));
            }
            self.indent();
            self.line(&format!(".clk({fifo_clk}),"));
            self.line(&format!(".rst_n({fifo_rst}),"));
            self.line(&format!(".enq_valid(q_{sname}_enq_valid),"));
            self.line(&format!(".enq_ready(q_{sname}_enq_ready),"));
            self.line(&format!(".enq_data(q_{sname}_enq_data),"));
            self.line(&format!(".deq_valid(q_{sname}_deq_valid),"));
            self.line(&format!(".deq_ready(q_{sname}_deq_ready),"));
            if is_structured {
                self.line(&format!(".deq_data(q_{sname}_deq_data_raw)"));
            } else {
                self.line(&format!(".deq_data(q_{sname}_deq_data)"));
            }
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
            let td = sv_type_decl(&edge.elem_ty);

            self.line(&format!("// Cell: {}", edge.name));
            self.line(&format!("{td}c_{sname}_q;"));
            self.line(&format!("{td}c_{sname}_d;"));
            self.line(&format!("logic c_{sname}_en;"));
            self.blank();
        }
    }

    // -----------------------------------------------------------------------
    // Rule enable signals (can_fire / will_fire)
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

                    // Blocking takes on Queue/AsyncQueue edges require deq_valid
                    for take_port in &blocking_takes[rule_idx] {
                        if let Some(&edge_idx) = port_edges.get(take_port) {
                            let edge = &self.net.network.graph[edge_idx];
                            if is_queue_like(&edge.kind) {
                                conditions.push(format!("q_{}_deq_valid", sanitize(&edge.name)));
                            }
                        }
                    }

                    // Only unconditional puts require enq_ready in can_fire.
                    // Conditional puts (inside if/match) are gated by branch
                    // conditions in the always_comb block — enq_valid is only
                    // asserted when the put actually fires.
                    for put_port in &unconditional_puts[rule_idx] {
                        if let Some(&edge_idx) = port_edges.get(put_port) {
                            let edge = &self.net.network.graph[edge_idx];
                            if is_queue_like(&edge.kind) {
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
            .any(|e| is_queue_like(&e.kind));
        let has_rules = self
            .net
            .network
            .graph
            .node_weights()
            .any(|n| !n.rules.is_empty());

        if !has_cells && !has_queues && !has_rules {
            return;
        }

        self.line("/* verilator lint_off LATCH */  // all signals have explicit defaults above");
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

        // Default Queue enq signals (skip dead-side memory edges)
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if is_queue_like(&edge.kind) {
                if let Some(&cpu_is_writer) = self.memory_edges.get(&edge_idx)
                    && !cpu_is_writer
                {
                    continue;
                }
                let sname = sanitize(&edge.name);
                self.line(&format!("q_{sname}_enq_valid = 1'b0;"));
                self.line(&format!("q_{sname}_enq_data = '0;"));
            }
        }

        // Default deq_ready for try_take-only queues (driven from always_comb)
        {
            let tt_defaults: Vec<String> = self
                .try_take_only_edges
                .iter()
                .map(|ei| sanitize(&self.net.network.graph[*ei].name))
                .collect();
            for sname in tt_defaults {
                self.line(&format!("q_{sname}_deq_ready = 1'b0;"));
            }
        }

        self.blank();

        // Rule bodies (skip memory stub nodes)
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

                // try_take: deq_ready driven from always_comb (not assign).
                // Skip here — handled by pending_deq_readys in match arm emission.
            }
        }

        // Emit assign for each queue's deq_ready
        for edge_idx in self.net.network.graph.edge_indices() {
            let edge = &self.net.network.graph[edge_idx];
            if !is_queue_like(&edge.kind) {
                continue;
            }
            // Skip memory edges where CPU is writer — deq side is dead
            if let Some(&cpu_is_writer) = self.memory_edges.get(&edge_idx)
                && cpu_is_writer
            {
                continue;
            }
            // Skip try_take-only edges — their deq_ready is driven from always_comb
            if self.try_take_only_edges.contains(&edge_idx) {
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
            .any(|e| is_queue_like(&e.kind))
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

                // Determine the clock domain for this cell (from the owning process)
                let (src_node, _) = self.net.network.graph.edge_endpoints(edge_idx).unwrap();
                let inst_name = &self.net.network.graph[src_node].instance_name;
                let clk = self.clock_for_instance(inst_name);
                let rst = self.reset_for_instance(inst_name);

                self.line(&format!(
                    "always_ff @(posedge {clk} or negedge {rst}) begin"
                ));
                self.indent();
                self.line(&format!("if (!{rst})"));
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
                let val_ty = self.infer_expr_type(&value.node, ctx);
                self.bind_pattern(&pattern.node, &val_sv, &val_ty, ctx);
            }
            Stmt::Put { target, value } => {
                // Special handling for array update expressions (regs[rd := val])
                let val_sv = if let Expr::Update {
                    expr: base,
                    index,
                    value: upd_val,
                } = &value.node
                {
                    self.emit_array_update(&base.node, &index.node, &upd_val.node, ctx)
                } else {
                    self.emit_expr(&value.node, ctx)
                };
                if let Some(&edge_idx) = ctx.port_edges.get(&target.node) {
                    let edge = &self.net.network.graph[edge_idx];
                    let sname = sanitize(&edge.name);
                    let target_w = bit_width(&edge.elem_ty);
                    let expr_ty = self.infer_expr_type(&value.node, ctx);
                    let expr_w = bit_width(&expr_ty);
                    let final_val = if expr_w != target_w && target_w > 1 && expr_w > 0 {
                        width_cast(&val_sv, target_w)
                    } else {
                        val_sv
                    };
                    match &edge.kind {
                        QueueEdgeKind::Cell { .. } => {
                            self.line(&format!("c_{sname}_d = {final_val};"));
                            self.line(&format!("c_{sname}_en = 1'b1;"));
                        }
                        QueueEdgeKind::Queue { .. } | QueueEdgeKind::AsyncQueue => {
                            self.line(&format!("q_{sname}_enq_data = {final_val};"));
                            self.line(&format!("q_{sname}_enq_valid = 1'b1;"));
                        }
                    }
                }
            }
            Stmt::Expr(expr) => {
                // Expression as statement — emit for side effects (DPI calls)
                let sv = self.emit_expr(&expr.node, ctx);
                if matches!(&expr.node, Expr::Call { .. }) {
                    self.line(&format!("{sv};"));
                }
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
                let scrut_ty = self.infer_expr_type(&scrutinee.node, ctx);

                for (i, arm) in arms.iter().enumerate() {
                    ctx.pending_deq_readys.clear();
                    let cond =
                        self.emit_match_condition(&arm.pattern.node, &scrut_sv, &scrut_ty, ctx);

                    if i == 0 {
                        self.line(&format!("if ({cond}) begin"));
                    } else {
                        self.line(&format!("end else if ({cond}) begin"));
                    }
                    self.indent();
                    // Emit deq_ready for try_take queues consumed in this arm
                    for dr in &ctx.pending_deq_readys.clone() {
                        self.line(&format!("{dr} = 1'b1;"));
                    }
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
            Expr::Var(name) => {
                if let Some(sv) = ctx.vars.get(name) {
                    sv.clone()
                } else if let Some(&val) = self.net.network.constants.get(name) {
                    format!("{val}")
                } else {
                    name.clone()
                }
            }
            Expr::Take { queue } => {
                if let Some(&edge_idx) = ctx.port_edges.get(queue) {
                    let edge = &self.net.network.graph[edge_idx];
                    let sname = sanitize(&edge.name);
                    match &edge.kind {
                        QueueEdgeKind::Cell { .. } => format!("c_{sname}_q"),
                        QueueEdgeKind::Queue { .. } | QueueEdgeKind::AsyncQueue => {
                            format!("q_{sname}_deq_data")
                        }
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
                // For shift/bitwise ops, widen narrower operand to match the wider one.
                // This prevents Verilator WIDTHEXPAND warnings on mixed-width expressions.
                if matches!(op, BinOp::Shl | BinOp::Or | BinOp::And | BinOp::Xor) {
                    let lty = self.infer_expr_type(&lhs.node, ctx);
                    let rty = self.infer_expr_type(&rhs.node, ctx);
                    let lw = bit_width(&lty);
                    let rw = bit_width(&rty);
                    if lw > 0 && rw > 0 && lw != rw {
                        let target = lw.max(rw);
                        let lc = if lw < target {
                            width_cast(&l, target)
                        } else {
                            l
                        };
                        let rc = if rw < target && !matches!(op, BinOp::Shl | BinOp::Shr) {
                            width_cast(&r, target)
                        } else {
                            r
                        };
                        return format!("({lc} {op_sv} {rc})");
                    }
                }
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
            Expr::Record { name, fields } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(fname, val)| {
                        let v = self.emit_expr(&val.node, ctx);
                        format!("{}: {v}", fname.node)
                    })
                    .collect();
                // Use typed assignment pattern for struct packed
                if self.net.network.type_defs.contains_key(name) {
                    format!("{name}'{{{}}}", parts.join(", "))
                } else {
                    format!("{{{}}}", parts.join(", "))
                }
            }
            Expr::FieldAccess { expr, field } => {
                let e = self.emit_expr(&expr.node, ctx);
                // Simplified: emit as SV field access
                format!("{e}.{}", field.node)
            }
            Expr::Index { expr: e, index } => {
                let base = self.emit_expr(&e.node, ctx);
                let idx = self.emit_expr(&index.node, ctx);
                let base_ty = self.infer_expr_type(&e.node, ctx);
                if let Ty::Array { elem, .. } = &base_ty {
                    let elem_w = bit_width(elem);
                    format!("{base}[{idx} * {elem_w} +: {elem_w}]")
                } else {
                    format!("{base}[{idx}]")
                }
            }
            Expr::Call { func, args } => {
                // Look up external function signature for argument width casting
                let param_types = self
                    .net
                    .network
                    .external_fns
                    .get(func)
                    .map(|(params, _)| params.clone());
                let arg_strs: Vec<String> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let sv = self.emit_expr(&a.node, ctx);
                        if let Some(ref pts) = param_types
                            && i < pts.len()
                        {
                            let param_w = bit_width(&pts[i]);
                            let arg_ty = self.infer_expr_type(&a.node, ctx);
                            let arg_w = bit_width(&arg_ty);
                            if param_w > 0 && arg_w > 0 && param_w != arg_w {
                                width_cast(&sv, param_w)
                            } else {
                                sv
                            }
                        } else {
                            sv
                        }
                    })
                    .collect();
                format!("{func}({})", arg_strs.join(", "))
            }
            Expr::Update {
                expr: e,
                index,
                value,
            } => {
                // Functional array update: a[i := v]
                // We emit this as an inline function using SV automatic variable
                let base = self.emit_expr(&e.node, ctx);
                let idx = self.emit_expr(&index.node, ctx);
                let val = self.emit_expr(&value.node, ctx);
                // The caller must use this in a context where we can emit a block.
                // For now, we track that an update is needed and the caller
                // handles it. As a simple approach, emit a helper signal.
                format!("/* update({base}, {idx}, {val}) */")
            }
            Expr::BitSlice { expr, hi, lo } => {
                let e = self.emit_expr(&expr.node, ctx);
                format!("({e}[{hi}:{lo}])")
            }
            Expr::MethodCall { .. } => "/* unsupported expr */ '0".to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // Pattern binding (tuple destructuring, variant matching)
    // -----------------------------------------------------------------------

    /// Bind a pattern to an SV expression, populating ctx.vars and ctx.var_types.
    fn bind_pattern(&self, pattern: &Pattern, val_sv: &str, val_ty: &Ty, ctx: &mut RuleCtx) {
        match pattern {
            Pattern::Bind(name) => {
                ctx.vars.insert(name.clone(), val_sv.to_string());
                ctx.var_types.insert(name.clone(), val_ty.clone());
            }
            Pattern::Tuple(sub_pats) => {
                // Destructure by bit-slicing the concatenated value.
                // In SV, {a, b} has `a` in the MSB and `b` in the LSB.
                //
                if let Ty::Tuple(elem_tys) = val_ty {
                    let total_width: u64 = elem_tys.iter().map(bit_width).sum();
                    let base = val_sv;
                    let mut bit_offset = 0u64; // from LSB
                    // Process elements in reverse (last element = LSB)
                    let elems: Vec<_> = sub_pats.iter().zip(elem_tys.iter()).collect();
                    let mut slices: Vec<(usize, String, Ty)> = vec![];
                    for (i, (_pat, ety)) in elems.iter().enumerate().rev() {
                        let w = bit_width(ety);
                        let sv_slice = if total_width == w && elems.len() == 1 {
                            base.to_string()
                        } else if w == 1 {
                            format!("{base}[{bit_offset}]")
                        } else {
                            format!("{base}[{}:{}]", bit_offset + w - 1, bit_offset)
                        };
                        slices.push((i, sv_slice, (*ety).clone()));
                        bit_offset += w;
                    }
                    slices.sort_by_key(|(i, _, _)| *i);
                    for (i, sv_slice, ety) in slices {
                        self.bind_pattern(&sub_pats[i].node, &sv_slice, &ety, ctx);
                    }
                }
                // If type is unknown, fall back to no binding
            }
            Pattern::Wildcard => {}                             // discard
            Pattern::Variant { .. } | Pattern::Literal(_) => {} // handled in match arms
        }
    }

    // -----------------------------------------------------------------------
    // Match condition emission
    // -----------------------------------------------------------------------

    /// Emit a match arm condition and bind any sub-pattern variables.
    fn emit_match_condition(
        &self,
        pattern: &Pattern,
        scrut_sv: &str,
        scrut_ty: &Ty,
        ctx: &mut RuleCtx,
    ) -> String {
        match pattern {
            Pattern::Literal(Literal::Int(n)) => format!("{scrut_sv} == {n}"),
            Pattern::Literal(Literal::Bool(b)) => {
                if *b {
                    scrut_sv.to_string()
                } else {
                    format!("!{scrut_sv}")
                }
            }
            Pattern::Wildcard => "1'b1".to_string(),
            Pattern::Bind(name) => {
                ctx.vars.insert(name.clone(), scrut_sv.to_string());
                ctx.var_types.insert(name.clone(), scrut_ty.clone());
                "1'b1".to_string()
            }
            Pattern::Variant { name, fields } => {
                // Case 0: Non-Option, non-Enum scrutinee with Some/None pattern
                // (e.g. Cell peek — value is always valid, not wrapped in Option SV encoding)
                if !matches!(scrut_ty, Ty::Option(_) | Ty::Enum { .. }) {
                    match name.as_str() {
                        "Some" => {
                            if fields.len() == 1 {
                                self.bind_pattern(&fields[0].node, scrut_sv, scrut_ty, ctx);
                            }
                            return "1'b1".to_string();
                        }
                        "None" => return "1'b0".to_string(),
                        _ => {}
                    }
                }

                // Case 1: Option type from try_take — scrutinee is {valid, data}
                if let Ty::Option(inner_ty) = scrut_ty {
                    let inner_w = bit_width(inner_ty);
                    let total_w = 1 + inner_w;
                    let valid_bit = if total_w > 1 {
                        format!("{scrut_sv}[{}]", total_w - 1)
                    } else {
                        scrut_sv.to_string()
                    };
                    match name.as_str() {
                        "Some" => {
                            // Record deq_ready for try_take queues
                            // scrut_sv is {q_foo_deq_valid, q_foo_deq_data}
                            // Extract q_foo_deq_ready from the pattern
                            if let Some(inner) = scrut_sv.strip_prefix('{')
                                && let Some(inner) = inner.strip_suffix('}')
                                && let Some(comma) = inner.find(", ")
                            {
                                let valid_signal = &inner[..comma]; // q_foo_deq_valid
                                if let Some(base) = valid_signal.strip_suffix("_deq_valid") {
                                    ctx.pending_deq_readys.push(format!("{base}_deq_ready"));
                                }
                            }

                            // Bind the inner value (lower bits of Option encoding).
                            // For tuple inner types, bind each element directly from
                            // the scrutinee to avoid chained bit-slices.
                            if fields.len() == 1 {
                                if let Ty::Tuple(ref elem_tys) = **inner_ty
                                    && let Pattern::Tuple(ref sub_pats) = fields[0].node
                                    && sub_pats.len() == elem_tys.len()
                                {
                                    // Direct tuple destructuring from Option bits
                                    let mut bit_off = 0u64;
                                    let elems: Vec<_> =
                                        sub_pats.iter().zip(elem_tys.iter()).collect();
                                    let mut slices: Vec<(usize, String, Ty)> = vec![];
                                    for (i, (_p, ety)) in elems.iter().enumerate().rev() {
                                        let w = bit_width(ety);
                                        let sv = if w == 1 {
                                            format!("{scrut_sv}[{bit_off}]")
                                        } else {
                                            format!("{scrut_sv}[{}:{}]", bit_off + w - 1, bit_off)
                                        };
                                        slices.push((i, sv, (*ety).clone()));
                                        bit_off += w;
                                    }
                                    slices.sort_by_key(|(i, _, _)| *i);
                                    for (i, sv, ety) in slices {
                                        self.bind_pattern(&sub_pats[i].node, &sv, &ety, ctx);
                                    }
                                } else {
                                    // For try_take, scrut_sv is {deq_valid, deq_data}.
                                    // Extract just the deq_data signal by stripping
                                    // the {valid, ...} wrapper if present.
                                    let data_sv = extract_try_take_data(scrut_sv, inner_w);
                                    self.bind_pattern(&fields[0].node, &data_sv, inner_ty, ctx);
                                }
                            }
                            return valid_bit;
                        }
                        "None" => return format!("!{valid_bit}"),
                        _ => {}
                    }
                }

                // Case 2: User-defined enum — compare tag against variant name
                if let Ty::Enum { variants, .. } = scrut_ty {
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

                    if max_payload == 0 {
                        // Pure enum (no payloads) — direct comparison
                        return format!("{scrut_sv} == {name}");
                    }

                    // Enum with payloads: tag is in the MSBs
                    let total_w = tag_bits + max_payload;
                    let tag_sv = format!("{scrut_sv}[{}:{}]", total_w - 1, max_payload);
                    // Bind payload fields if any
                    if let Some((_, variant_fields)) = variants.iter().find(|(vn, _)| vn == name)
                        && !variant_fields.is_empty()
                        && !fields.is_empty()
                    {
                        let mut offset = 0u64;
                        for (pat, vty) in fields.iter().zip(variant_fields.iter()).rev() {
                            let w = bit_width(vty);
                            let sv_slice = if w == 1 {
                                format!("{scrut_sv}[{offset}]")
                            } else {
                                format!("{scrut_sv}[{}:{}]", offset + w - 1, offset)
                            };
                            self.bind_pattern(&pat.node, &sv_slice, vty, ctx);
                            offset += w;
                        }
                    }
                    return format!("{tag_sv} == {name}");
                }

                // Fallback: try as a simple enum constant name
                format!("{scrut_sv} == {name}")
            }
            Pattern::Tuple(_) => {
                self.bind_pattern(pattern, scrut_sv, scrut_ty, ctx);
                "1'b1".to_string()
            }
        }
    }

    // -----------------------------------------------------------------------
    // Lightweight type inference for expressions
    // -----------------------------------------------------------------------

    /// Infer the type of an expression from context. Returns Ty::Error if unknown.
    fn infer_expr_type(&self, expr: &Expr, ctx: &RuleCtx) -> Ty {
        match expr {
            Expr::Lit(Literal::Int(_)) => Ty::Bits(32), // default width
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
                let lt = self.infer_expr_type(&lhs.node, ctx);
                let lw = bit_width(&lt);
                let rw = bit_width(&self.infer_expr_type(&rhs.node, ctx));
                // For bitwise/shift ops, result width is the max of operands
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
    // Array update statement-level emission
    // -----------------------------------------------------------------------

    /// Emit an array update using copy + indexed part-select overwrite.
    /// Arrays are packed as flat bit vectors: element i occupies [i*W +: W].
    /// Emits the copy and overwrite as statements, returns the name of
    /// the signal holding the result (which is the base itself, modified in-place
    /// in the combinational block via a helper signal).
    fn emit_array_update(
        &mut self,
        base_expr: &Expr,
        index_expr: &Expr,
        value_expr: &Expr,
        ctx: &mut RuleCtx,
    ) -> String {
        let base = self.emit_expr(base_expr, ctx);
        let idx = self.emit_expr(index_expr, ctx);
        let val = self.emit_expr(value_expr, ctx);
        let base_ty = self.infer_expr_type(base_expr, ctx);

        let tmp_name = format!("_upd_{}", ctx.temp_counter);
        ctx.temp_counter += 1;

        if let Ty::Array { elem, .. } = &base_ty {
            let elem_w = bit_width(elem);
            let total_w = bit_width(&base_ty);
            let wd = width_decl(total_w);
            // Declare a flat packed temporary, copy base, overwrite one element
            self.line(&format!("logic {wd}{tmp_name};"));
            self.line(&format!("{tmp_name} = {base};"));
            self.line(&format!(
                "{tmp_name}[{idx} * {elem_w} +: {elem_w}] = {val};"
            ));
        }

        tmp_name
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
