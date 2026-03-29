use std::collections::HashMap;

use petgraph::graph::{EdgeIndex, NodeIndex};

use crate::ast::*;
use crate::diagnostics::Diagnostic;
use crate::ir::*;
use crate::types::env::TypeEnv;
use crate::types::ty::Ty;

/// Elaborate all pipes in the source file into process network graphs.
///
/// Returns one `ProcessNetwork` per pipe, plus any diagnostics.
/// A file with only process definitions (no pipe) produces an empty vec.
pub fn elaborate(
    source: &SourceFile,
    type_env: &TypeEnv,
) -> (Vec<ProcessNetwork>, Vec<Diagnostic>) {
    let mut diagnostics = vec![];
    let processes = index_processes(source);

    let mut networks = vec![];
    for item in &source.items {
        if let Item::Pipe(pipe) = &item.node
            && let Some(net) = elaborate_pipe(pipe, &processes, type_env, &mut diagnostics)
        {
            networks.push(net);
        }
    }

    (networks, diagnostics)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a name → Process map from all process definitions.
fn index_processes(source: &SourceFile) -> HashMap<String, &Process> {
    let mut map = HashMap::new();
    for item in &source.items {
        if let Item::Process(p) = &item.node {
            map.insert(p.name.node.clone(), p);
        }
    }
    map
}

/// Split a dotted target like "Writeback.regfile" into ("Writeback", "regfile").
fn parse_dotted_target(target: &str) -> Option<(&str, &str)> {
    let dot = target.find('.')?;
    Some((&target[..dot], &target[dot + 1..]))
}

/// Extract the init literal value from a Cell type expression.
fn extract_init_value(ty_expr: &TypeExpr) -> Option<u64> {
    if let TypeExpr::Cell {
        init: Some(init_expr),
        ..
    } = ty_expr
    {
        match &init_expr.node {
            Expr::Lit(Literal::Int(n)) => Some(*n),
            Expr::Lit(Literal::Bool(b)) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
    } else {
        None
    }
}

/// Extract the element type from a Queue or Cell type.
fn port_element_type(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::Queue { elem, .. } => Some(*elem.clone()),
        Ty::Cell { elem } => Some(*elem.clone()),
        _ => None,
    }
}

/// Resolve a QueueDecl's type expression to (element_type, depth).
fn resolve_queue_decl(
    decl: &QueueDecl,
    type_env: &TypeEnv,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Ty, u64) {
    let ty = type_env.resolve_type_expr(&decl.ty.node, diagnostics);
    match &ty {
        Ty::Queue { elem, depth } => {
            let d = decl.depth.or(*depth).unwrap_or(1);
            (*elem.clone(), d)
        }
        Ty::Cell { elem } => (*elem.clone(), 1),
        _ => {
            // Bare type used as queue element — treat as Queue(T, depth)
            let d = decl.depth.unwrap_or(1);
            (ty, d)
        }
    }
}

// ---------------------------------------------------------------------------
// Per-pipe elaboration
// ---------------------------------------------------------------------------

/// Tracks a queue edge during wiring: who writes, who reads, and peek bindings.
struct PendingEdge {
    edge_idx: EdgeIndex,
    writer: Option<(String, Span)>, // (instance_name, binding_span)
    reader: Option<(String, Span)>,
    decl_span: Span,
}

fn elaborate_pipe(
    pipe: &Pipe,
    processes: &HashMap<String, &Process>,
    type_env: &TypeEnv,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ProcessNetwork> {
    let mut graph = petgraph::graph::DiGraph::new();
    let mut instances: HashMap<String, NodeIndex> = HashMap::new();
    let mut had_error = false;

    // Phase 1 + 2: Create nodes from instances
    for (idx, inst) in pipe.instances.iter().enumerate() {
        let proc_name = &inst.process_name.node;
        let Some(proc_def) = processes.get(proc_name) else {
            diagnostics.push(Diagnostic::error(
                inst.process_name.span.clone(),
                format!("unknown process `{proc_name}`"),
            ));
            had_error = true;
            continue;
        };

        // Generate instance name: use process name, disambiguate with index if needed
        let instance_name = {
            let base = proc_name.clone();
            if instances.contains_key(&base) {
                format!("{base}_{idx}")
            } else {
                base
            }
        };

        let ports: Vec<ResolvedPort> = proc_def
            .ports
            .iter()
            .map(|p| {
                let ty = type_env.resolve_type_expr(&p.ty.node, diagnostics);
                ResolvedPort {
                    name: p.name.node.clone(),
                    kind: p.kind,
                    ty,
                    bound_to: None,
                }
            })
            .collect();

        let node = ProcessNode {
            instance_name: instance_name.clone(),
            process_name: proc_name.clone(),
            rules: proc_def.rules.clone(),
            ports,
            span: inst.process_name.span.clone(),
        };

        let node_idx = graph.add_node(node);
        instances.insert(instance_name, node_idx);
    }

    if had_error {
        return None;
    }

    // Phase 3: Create queue edges and wire bindings
    let mut pending_edges: HashMap<String, PendingEdge> = HashMap::new();

    // Create edges from queue declarations
    // We use a placeholder (first node or a dummy) for initial edge creation,
    // then fix up endpoints during wiring.
    for decl in &pipe.queue_decls {
        let (elem_ty, depth) = resolve_queue_decl(decl, type_env, diagnostics);
        let queue_name = decl.name.node.clone();

        // Create a temporary self-loop on first node; endpoints get fixed in wiring
        // We need both endpoints to create an edge in petgraph, so we use a sentinel approach:
        // store the edge data separately and build the final graph edges after wiring.
        // Actually, petgraph requires valid nodes. Let's create edges after we know endpoints.

        // For now, just record the queue info; edges created after wiring analysis.
        let edge = QueueEdge {
            name: queue_name.clone(),
            elem_ty,
            depth,
            kind: QueueEdgeKind::Queue,
            span: decl.name.span.clone(),
        };

        // We'll add the edge to the graph once we know writer and reader.
        // Store the edge data temporarily.
        // Use a sentinel — we'll create the real edge after wiring.
        // For now, add a self-loop on an arbitrary node that we'll replace.
        if let Some((&_, &first_node)) = instances.iter().next() {
            let edge_idx = graph.add_edge(first_node, first_node, edge);
            pending_edges.insert(
                queue_name,
                PendingEdge {
                    edge_idx,
                    writer: None,
                    reader: None,
                    decl_span: decl.name.span.clone(),
                },
            );
        }
    }

    // Wire bindings: iterate instances and their port bindings
    for (idx, inst) in pipe.instances.iter().enumerate() {
        let proc_name = &inst.process_name.node;
        let instance_name = find_instance_name(&instances, proc_name, idx);
        let Some(instance_name) = instance_name else {
            continue;
        };
        let node_idx = instances[&instance_name];

        let Some(proc_def) = processes.get(proc_name) else {
            continue;
        };

        let mut bound_ports: HashMap<String, bool> = HashMap::new();

        for binding in &inst.bindings {
            let port_name = &binding.port.node;
            let target = &binding.target.node;

            // Find the port definition
            let Some(port_def) = proc_def.ports.iter().find(|p| &p.name.node == port_name) else {
                diagnostics.push(Diagnostic::error(
                    binding.port.span.clone(),
                    format!("process `{proc_name}` has no port `{port_name}`"),
                ));
                had_error = true;
                continue;
            };

            bound_ports.insert(port_name.clone(), true);

            let port_ty = type_env.resolve_type_expr(&port_def.ty.node, diagnostics);

            // Check for dotted reference (cross-instance peek)
            if let Some((ref_instance, ref_port)) = parse_dotted_target(target) {
                // Dotted reference: must be a peeks: port
                if port_def.kind != PortKind::Peeks {
                    diagnostics.push(Diagnostic::error(
                        binding.target.span.clone(),
                        format!("dotted reference `{target}` can only be used with `peeks:` ports"),
                    ));
                    had_error = true;
                    continue;
                }

                // Find the referenced instance
                let Some(&ref_node_idx) = instances.get(ref_instance) else {
                    diagnostics.push(Diagnostic::error(
                        binding.target.span.clone(),
                        format!("unknown instance `{ref_instance}` in dotted reference"),
                    ));
                    had_error = true;
                    continue;
                };

                // Find the state port on the referenced instance
                let ref_proc_name = &graph[ref_node_idx].process_name;
                let ref_proc = processes.get(ref_proc_name.as_str());
                let has_state_port = ref_proc.is_some_and(|p| {
                    p.ports
                        .iter()
                        .any(|port| port.name.node == ref_port && port.kind == PortKind::State)
                });

                if !has_state_port {
                    diagnostics.push(Diagnostic::error(
                        binding.target.span.clone(),
                        format!("instance `{ref_instance}` has no state port `{ref_port}`"),
                    ));
                    had_error = true;
                    continue;
                }

                // Find or create the self-loop edge for that state port
                let self_loop_name = format!("{ref_instance}.{ref_port}");
                if let Some(pending) = pending_edges.get_mut(&self_loop_name) {
                    // Add this instance as a peeker
                    if let Some(edge) = graph.edge_weight_mut(pending.edge_idx)
                        && let QueueEdgeKind::Cell {
                            ref mut peeker_instances,
                            ..
                        } = edge.kind
                    {
                        peeker_instances.push(instance_name.clone());
                    }
                    // Bind port to edge
                    let node = &mut graph[node_idx];
                    if let Some(rp) = node.ports.iter_mut().find(|p| p.name == *port_name) {
                        rp.bound_to = Some(pending.edge_idx);
                    }
                }
                // If the self-loop hasn't been created yet (it will be in the implicit phase),
                // we need to defer. For simplicity, we handle this in the implicit self-loop phase.

                continue;
            }

            // Simple target: bind to a named queue
            let Some(pending) = pending_edges.get_mut(target) else {
                diagnostics.push(Diagnostic::error(
                    binding.target.span.clone(),
                    format!("unknown queue `{target}`"),
                ));
                had_error = true;
                continue;
            };

            // Type check: port element type must match queue element type
            let port_elem = port_element_type(&port_ty);
            if let Some(ref port_elem) = port_elem {
                let edge = &graph[pending.edge_idx];
                if *port_elem != Ty::Error
                    && edge.elem_ty != Ty::Error
                    && *port_elem != edge.elem_ty
                {
                    diagnostics.push(Diagnostic::error(
                        binding.target.span.clone(),
                        format!(
                            "type mismatch: port `{port_name}` has element type `{port_elem}` but queue `{target}` has element type `{}`",
                            edge.elem_ty
                        ),
                    ));
                    had_error = true;
                    continue;
                }
            }

            // Record writer/reader based on port kind
            match port_def.kind {
                PortKind::Produces => {
                    if let Some((existing, _)) = &pending.writer {
                        diagnostics.push(Diagnostic::error(
                            binding.target.span.clone(),
                            format!(
                                "queue `{target}` already has writer `{existing}` — single-writer violated"
                            ),
                        ));
                        had_error = true;
                    } else {
                        pending.writer = Some((instance_name.clone(), binding.target.span.clone()));
                    }
                }
                PortKind::Consumes => {
                    if let Some((existing, _)) = &pending.reader {
                        diagnostics.push(Diagnostic::error(
                            binding.target.span.clone(),
                            format!(
                                "queue `{target}` already has reader `{existing}` — single-reader violated"
                            ),
                        ));
                        had_error = true;
                    } else {
                        pending.reader = Some((instance_name.clone(), binding.target.span.clone()));
                    }
                }
                PortKind::State => {
                    // State ports bound to explicit queues act as both writer and reader
                    if pending.writer.is_none() {
                        pending.writer = Some((instance_name.clone(), binding.target.span.clone()));
                    }
                    if pending.reader.is_none() {
                        pending.reader = Some((instance_name.clone(), binding.target.span.clone()));
                    }
                }
                PortKind::Peeks => {
                    // Peek doesn't count as reader — just add to peekers
                    if let Some(edge) = graph.edge_weight_mut(pending.edge_idx)
                        && let QueueEdgeKind::Cell {
                            ref mut peeker_instances,
                            ..
                        } = edge.kind
                    {
                        peeker_instances.push(instance_name.clone());
                    }
                }
            }

            // Bind the port to the edge
            let node = &mut graph[node_idx];
            if let Some(rp) = node.ports.iter_mut().find(|p| p.name == *port_name) {
                rp.bound_to = Some(pending.edge_idx);
            }
        }

        // Implicit self-loops for unbound state: ports
        for port_def in &proc_def.ports {
            if port_def.kind == PortKind::State && !bound_ports.contains_key(&port_def.name.node) {
                let port_ty = type_env.resolve_type_expr(&port_def.ty.node, diagnostics);
                let elem_ty = port_element_type(&port_ty).unwrap_or(port_ty.clone());

                let self_loop_name = format!("{instance_name}.{}", port_def.name.node);
                let init = extract_init_value(&port_def.ty.node);
                let edge = QueueEdge {
                    name: self_loop_name.clone(),
                    elem_ty,
                    depth: 1,
                    kind: QueueEdgeKind::Cell {
                        peeker_instances: vec![],
                        init,
                    },
                    span: port_def.name.span.clone(),
                };

                let edge_idx = graph.add_edge(node_idx, node_idx, edge);
                pending_edges.insert(
                    self_loop_name,
                    PendingEdge {
                        edge_idx,
                        writer: Some((instance_name.clone(), port_def.name.span.clone())),
                        reader: Some((instance_name.clone(), port_def.name.span.clone())),
                        decl_span: port_def.name.span.clone(),
                    },
                );

                // Bind port
                let node = &mut graph[node_idx];
                if let Some(rp) = node.ports.iter_mut().find(|p| p.name == port_def.name.node) {
                    rp.bound_to = Some(edge_idx);
                }
            }
        }
    }

    // Phase 4: Validate

    // Check all consumes/produces ports are bound
    for (idx, inst) in pipe.instances.iter().enumerate() {
        let proc_name = &inst.process_name.node;
        let Some(instance_name) = find_instance_name(&instances, proc_name, idx) else {
            continue;
        };
        let node_idx = instances[&instance_name];
        let node = &graph[node_idx];

        for port in &node.ports {
            if matches!(
                port.kind,
                PortKind::Consumes | PortKind::Produces | PortKind::Peeks
            ) && port.bound_to.is_none()
            {
                diagnostics.push(Diagnostic::error(
                    node.span.clone(),
                    format!(
                        "port `{}` on instance `{}` is not bound",
                        port.name, instance_name
                    ),
                ));
                had_error = true;
            }
        }
    }

    // Check every queue has exactly one writer and one reader
    for (queue_name, pending) in &pending_edges {
        // Skip self-loop edges (they are implicitly wired)
        if queue_name.contains('.') {
            continue;
        }

        if pending.writer.is_none() {
            diagnostics.push(Diagnostic::error(
                pending.decl_span.clone(),
                format!("queue `{queue_name}` has no writer"),
            ));
            had_error = true;
        }
        if pending.reader.is_none() {
            diagnostics.push(Diagnostic::error(
                pending.decl_span.clone(),
                format!("queue `{queue_name}` has no reader"),
            ));
            had_error = true;
        }
    }

    // Fix up edge endpoints now that we know writers and readers.
    //
    // petgraph uses swap_remove internally, so removing edges one-at-a-time
    // causes index reuse that corrupts later lookups. Instead: collect all
    // edges to relocate, remove them all first, then re-add in batch.
    let mut to_relocate: Vec<(EdgeIndex, NodeIndex, NodeIndex, EdgeIndex)> = vec![];
    for (queue_name, pending) in &pending_edges {
        if queue_name.contains('.') {
            continue; // Self-loops already have correct endpoints
        }
        if let (Some((writer, _)), Some((reader, _))) = (&pending.writer, &pending.reader)
            && let (Some(&src_node), Some(&dst_node)) =
                (instances.get(writer), instances.get(reader))
        {
            to_relocate.push((pending.edge_idx, src_node, dst_node, pending.edge_idx));
        }
    }

    // Sort by edge index descending so removals don't shift earlier indices
    // (petgraph swaps the last edge into the removed slot).
    to_relocate.sort_by(|a, b| b.0.cmp(&a.0));

    // Remove all, collecting edge data
    let mut removed: Vec<(QueueEdge, NodeIndex, NodeIndex, EdgeIndex)> = vec![];
    for (edge_idx, src_node, dst_node, old_idx) in &to_relocate {
        if let Some(edge_data) = graph.remove_edge(*edge_idx) {
            removed.push((edge_data, *src_node, *dst_node, *old_idx));
        }
    }

    // Re-add with correct endpoints, building old→new index map
    let mut index_map: HashMap<EdgeIndex, EdgeIndex> = HashMap::new();
    for (edge_data, src_node, dst_node, old_idx) in removed {
        let new_idx = graph.add_edge(src_node, dst_node, edge_data);
        index_map.insert(old_idx, new_idx);
    }

    // Apply index mapping to all port bindings in a single pass
    // (avoids collisions from incremental updates)
    for node_idx in graph.node_indices().collect::<Vec<_>>() {
        let node = &mut graph[node_idx];
        for port in &mut node.ports {
            if let Some(old) = port.bound_to
                && let Some(&new) = index_map.get(&old)
            {
                port.bound_to = Some(new);
            }
        }
    }

    if had_error {
        return None;
    }

    Some(ProcessNetwork {
        name: pipe.name.node.clone(),
        graph,
        instances,
    })
}

/// Find the instance name for a given pipe instance index.
/// Uses process name, with disambiguation suffix if needed.
fn find_instance_name(
    instances: &HashMap<String, NodeIndex>,
    proc_name: &str,
    idx: usize,
) -> Option<String> {
    if instances.contains_key(proc_name) {
        Some(proc_name.to_string())
    } else {
        let suffixed = format!("{proc_name}_{idx}");
        if instances.contains_key(&suffixed) {
            Some(suffixed)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desugar;
    use crate::parse;
    use crate::types;

    /// Parse → desugar → typecheck → elaborate helper.
    fn elaborate_src(src: &str) -> (Vec<ProcessNetwork>, Vec<Diagnostic>) {
        let (cst, errors) = parse::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut desugar_diags = vec![];
        let ast = desugar::desugar(cst.unwrap(), &mut desugar_diags);
        assert!(
            desugar_diags.is_empty(),
            "desugar errors: {desugar_diags:?}"
        );
        let (env, type_diags) = types::check(&ast);
        assert!(type_diags.is_empty(), "type errors: {type_diags:?}");
        elaborate(&ast, &env)
    }

    #[test]
    fn simple_two_process_pipe() {
        let src = r#"
type Word = Bits 32

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
        let (networks, diags) = elaborate_src(src);
        assert!(diags.is_empty(), "errors: {diags:?}");
        assert_eq!(networks.len(), 1);
        let net = &networks[0];
        assert_eq!(net.name, "Top");
        assert_eq!(net.graph.node_count(), 2);
        assert_eq!(net.graph.edge_count(), 1);

        // Check the edge connects Producer → Consumer
        let edge = net.graph.edge_indices().next().unwrap();
        let (src_node, dst_node) = net.graph.edge_endpoints(edge).unwrap();
        assert_eq!(net.graph[src_node].process_name, "Producer");
        assert_eq!(net.graph[dst_node].process_name, "Consumer");

        // Check element type
        let edge_data = &net.graph[edge];
        assert_eq!(edge_data.elem_ty, Ty::Bits(32));
        assert_eq!(edge_data.depth, 4);
    }

    #[test]
    fn cell_self_loop() {
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
        let (networks, diags) = elaborate_src(src);
        assert!(diags.is_empty(), "errors: {diags:?}");
        assert_eq!(networks.len(), 1);
        let net = &networks[0];
        assert_eq!(net.graph.node_count(), 1);
        assert_eq!(net.graph.edge_count(), 1);

        // Self-loop: source == target
        let edge = net.graph.edge_indices().next().unwrap();
        let (src_node, dst_node) = net.graph.edge_endpoints(edge).unwrap();
        assert_eq!(src_node, dst_node);

        let edge_data = &net.graph[edge];
        assert!(matches!(edge_data.kind, QueueEdgeKind::Cell { .. }));
        assert_eq!(edge_data.depth, 1);
        assert_eq!(edge_data.elem_ty, Ty::Bits(32));
    }

    #[test]
    fn cross_instance_peek() {
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
        let (networks, diags) = elaborate_src(src);
        assert!(diags.is_empty(), "errors: {diags:?}");
        assert_eq!(networks.len(), 1);
        let net = &networks[0];
        assert_eq!(net.graph.node_count(), 2);
        // 1 self-loop for Writer.reg
        assert_eq!(net.graph.edge_count(), 1);

        // Check peeker_instances
        let edge = net.graph.edge_indices().next().unwrap();
        let edge_data = &net.graph[edge];
        if let QueueEdgeKind::Cell {
            ref peeker_instances,
            ..
        } = edge_data.kind
        {
            assert_eq!(peeker_instances, &["Reader"]);
        } else {
            panic!("expected Cell edge kind");
        }
    }

    #[test]
    fn single_writer_violation() {
        let src = r#"
process P1 {
    produces: out : Queue(Bits 32)
    rule go { out.put(1) }
}

process P2 {
    produces: out : Queue(Bits 32)
    rule go { out.put(2) }
}

process C {
    consumes: input : Queue(Bits 32)
    rule go { let x = input.take() }
}

pipe Top {
    let q = Queue(Bits 32)
    P1 { out = q }
    P2 { out = q }
    C { input = q }
}
"#;
        let (_networks, diags) = elaborate_src(src);
        assert!(
            diags.iter().any(|d| d.message.contains("single-writer")),
            "expected single-writer violation, got: {diags:?}"
        );
    }

    #[test]
    fn single_reader_violation() {
        let src = r#"
process P {
    produces: out : Queue(Bits 32)
    rule go { out.put(1) }
}

process C1 {
    consumes: input : Queue(Bits 32)
    rule go { let x = input.take() }
}

process C2 {
    consumes: input : Queue(Bits 32)
    rule go { let x = input.take() }
}

pipe Top {
    let q = Queue(Bits 32)
    P { out = q }
    C1 { input = q }
    C2 { input = q }
}
"#;
        let (_networks, diags) = elaborate_src(src);
        assert!(
            diags.iter().any(|d| d.message.contains("single-reader")),
            "expected single-reader violation, got: {diags:?}"
        );
    }

    #[test]
    fn unbound_port_error() {
        let src = r#"
process P {
    consumes: input : Queue(Bits 32)
    produces: out : Queue(Bits 32)
    rule go {
        let x = input.take()
        out.put(x)
    }
}

pipe Top {
    let q = Queue(Bits 32)
    P { out = q }
}
"#;
        let (_networks, diags) = elaborate_src(src);
        assert!(
            diags.iter().any(|d| d.message.contains("not bound")),
            "expected unbound port error, got: {diags:?}"
        );
    }

    #[test]
    fn unknown_process_error() {
        let src = r#"
pipe Top {
    Nonexistent {}
}
"#;
        let (cst, errors) = parse::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut desugar_diags = vec![];
        let ast = desugar::desugar(cst.unwrap(), &mut desugar_diags);
        let (env, _type_diags) = types::check(&ast);
        let (_networks, diags) = elaborate(&ast, &env);
        assert!(
            diags.iter().any(|d| d.message.contains("unknown process")),
            "expected unknown process error, got: {diags:?}"
        );
    }

    #[test]
    fn type_mismatch_error() {
        let src = r#"
process P {
    produces: out : Queue(Bits 16)
    rule go { out.put(1) }
}

process C {
    consumes: input : Queue(Bits 32)
    rule go { let x = input.take() }
}

pipe Top {
    let q = Queue(Bits 32)
    P { out = q }
    C { input = q }
}
"#;
        let (cst, errors) = parse::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut desugar_diags = vec![];
        let ast = desugar::desugar(cst.unwrap(), &mut desugar_diags);
        let (env, _type_diags) = types::check(&ast);
        let (_networks, diags) = elaborate(&ast, &env);
        assert!(
            diags.iter().any(|d| d.message.contains("type mismatch")),
            "expected type mismatch error, got: {diags:?}"
        );
    }

    #[test]
    fn invalid_dotted_ref() {
        let src = r#"
process Writer {
    state: reg : Cell(Bits 32, init = 0)
    rule go {
        let v = reg.take()
        reg.put(v)
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
    Reader { reg = Nonexistent.cell }
}
"#;
        let (cst, errors) = parse::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut desugar_diags = vec![];
        let ast = desugar::desugar(cst.unwrap(), &mut desugar_diags);
        let (env, _type_diags) = types::check(&ast);
        let (_networks, diags) = elaborate(&ast, &env);
        assert!(
            diags.iter().any(|d| d.message.contains("unknown instance")),
            "expected invalid dotted ref error, got: {diags:?}"
        );
    }
}
