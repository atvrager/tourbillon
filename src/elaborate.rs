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
    /// Graph edge index — Some for self-loops (already in graph), None for deferred queue edges.
    edge_idx: Option<EdgeIndex>,
    /// The edge data, stored until we know both endpoints for queue edges.
    edge_data: QueueEdge,
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
            is_memory_stub: proc_name.starts_with("_Mem_"),
        };

        let node_idx = graph.add_node(node);
        instances.insert(instance_name, node_idx);
    }

    if had_error {
        return None;
    }

    // Phase 3: Collect queue edge data (deferred — added to graph after wiring)
    let mut pending_edges: HashMap<String, PendingEdge> = HashMap::new();

    for decl in &pipe.queue_decls {
        let (elem_ty, depth) = resolve_queue_decl(decl, type_env, diagnostics);
        let queue_name = decl.name.node.clone();

        let edge_data = QueueEdge {
            name: queue_name.clone(),
            elem_ty,
            depth,
            kind: QueueEdgeKind::Queue,
            span: decl.name.span.clone(),
        };

        // Don't add to graph yet — we need both endpoints first.
        pending_edges.insert(
            queue_name,
            PendingEdge {
                edge_idx: None, // deferred
                edge_data,
                writer: None,
                reader: None,
                decl_span: decl.name.span.clone(),
            },
        );
    }

    // Phase 3a: Create implicit self-loop edges for all state ports first
    // (needed so dotted references like Writeback.regfile resolve correctly)
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

        // Collect explicitly bound port names from this instance
        let explicit_bindings: HashMap<String, bool> = inst
            .bindings
            .iter()
            .map(|b| (b.port.node.clone(), true))
            .collect();

        for port_def in &proc_def.ports {
            if port_def.kind == PortKind::State
                && !explicit_bindings.contains_key(&port_def.name.node)
            {
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

                let edge_idx = graph.add_edge(node_idx, node_idx, edge.clone());
                pending_edges.insert(
                    self_loop_name,
                    PendingEdge {
                        edge_idx: Some(edge_idx),
                        edge_data: edge,
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

    // Phase 3b: Wire bindings — iterate instances and their port bindings
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
                    // Add this instance as a peeker on the self-loop cell edge
                    if let Some(edge_idx) = pending.edge_idx {
                        if let Some(edge) = graph.edge_weight_mut(edge_idx)
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
                            rp.bound_to = Some(edge_idx);
                        }
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
            if let Some(ref port_elem) = port_elem
                && *port_elem != Ty::Error
                && pending.edge_data.elem_ty != Ty::Error
                && *port_elem != pending.edge_data.elem_ty
            {
                diagnostics.push(Diagnostic::error(
                    binding.target.span.clone(),
                    format!(
                        "type mismatch: port `{port_name}` has element type `{port_elem}` but queue `{target}` has element type `{}`",
                        pending.edge_data.elem_ty
                    ),
                ));
                had_error = true;
                continue;
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
                    if let Some(edge_idx) = pending.edge_idx {
                        if let Some(edge) = graph.edge_weight_mut(edge_idx)
                            && let QueueEdgeKind::Cell {
                                ref mut peeker_instances,
                                ..
                            } = edge.kind
                        {
                            peeker_instances.push(instance_name.clone());
                        }
                    } else {
                        // For deferred queue edges, record peeker in edge_data
                        if let QueueEdgeKind::Cell {
                            ref mut peeker_instances,
                            ..
                        } = pending.edge_data.kind
                        {
                            peeker_instances.push(instance_name.clone());
                        }
                    }
                }
            }

            // Bind the port to the edge (deferred for queue edges — stored as port name → queue name)
            if let Some(edge_idx) = pending.edge_idx {
                // Self-loop or already-created edge — bind immediately
                let node = &mut graph[node_idx];
                if let Some(rp) = node.ports.iter_mut().find(|p| p.name == *port_name) {
                    rp.bound_to = Some(edge_idx);
                }
            }
            // For deferred queue edges (edge_idx == None), we record the binding
            // and resolve it after edges are created. Store in a deferred list.
            let _ = (&instance_name, port_name, target); // used below in deferred binding
        }

        // (Implicit self-loops already created in Phase 3a above)
    }

    // Phase 4: Create deferred queue edges + bind ports, then validate.

    // Create deferred queue edges now that we know writers and readers.
    // Iterate in pipe declaration order for deterministic output.
    let mut queue_edge_map: HashMap<String, EdgeIndex> = HashMap::new();
    for decl in &pipe.queue_decls {
        let queue_name = &decl.name.node;
        let Some(pending) = pending_edges.get(queue_name) else {
            continue;
        };
        if pending.edge_idx.is_some() {
            continue; // Already in graph (shouldn't happen for queue decls)
        }
        if let (Some((writer, _)), Some((reader, _))) = (&pending.writer, &pending.reader)
            && let (Some(&src_node), Some(&dst_node)) =
                (instances.get(writer), instances.get(reader))
        {
            let edge_idx = graph.add_edge(src_node, dst_node, pending.edge_data.clone());
            queue_edge_map.insert(queue_name.clone(), edge_idx);
        }
    }

    // Bind deferred port bindings for queue edges.
    for (idx, inst) in pipe.instances.iter().enumerate() {
        let proc_name = &inst.process_name.node;
        let instance_name = find_instance_name(&instances, proc_name, idx);
        let Some(instance_name) = instance_name else {
            continue;
        };
        let node_idx = instances[&instance_name];

        for binding in &inst.bindings {
            let port_name = &binding.port.node;
            let target = &binding.target.node;

            // Skip dotted references (already bound in Phase 3b)
            if target.contains('.') {
                continue;
            }

            if let Some(&edge_idx) = queue_edge_map.get(target) {
                let node = &mut graph[node_idx];
                if let Some(rp) = node.ports.iter_mut().find(|p| p.name == *port_name)
                    && rp.bound_to.is_none()
                {
                    rp.bound_to = Some(edge_idx);
                }
            }
        }
    }

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

    if had_error {
        return None;
    }

    // Collect reachable type definitions (records and enums)
    let mut type_defs = HashMap::new();
    for (name, ty) in &type_env.type_defs {
        if matches!(ty, Ty::Record { .. } | Ty::Enum { .. }) {
            type_defs.insert(name.clone(), ty.clone());
        }
    }

    Some(ProcessNetwork {
        name: pipe.name.node.clone(),
        graph,
        instances,
        type_defs,
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
