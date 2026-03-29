use std::collections::{HashMap, HashSet};

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
    let (processes, pipes) = index_definitions(source);

    // Find pipes that are instantiated as children of other pipes — skip elaborating them
    // as top-level, since they'll be elaborated inline when the parent is elaborated.
    let mut child_pipes: HashSet<String> = HashSet::new();
    for pipe in pipes.values() {
        for inst in &pipe.instances {
            if pipes.contains_key(&inst.process_name.node) {
                child_pipes.insert(inst.process_name.node.clone());
            }
        }
    }

    let mut networks = vec![];
    for item in &source.items {
        if let Item::Pipe(pipe) = &item.node
            && !child_pipes.contains(&pipe.name.node)
            && let Some(net) = elaborate_pipe(pipe, &processes, &pipes, type_env, &mut diagnostics)
        {
            networks.push(net);
        }
    }

    (networks, diagnostics)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build name → Process and name → Pipe maps from all definitions.
fn index_definitions(source: &SourceFile) -> (HashMap<String, &Process>, HashMap<String, &Pipe>) {
    let mut processes = HashMap::new();
    let mut pipes = HashMap::new();
    for item in &source.items {
        match &item.node {
            Item::Process(p) => {
                processes.insert(p.name.node.clone(), p);
            }
            Item::Pipe(p) => {
                pipes.insert(p.name.node.clone(), p);
            }
            _ => {}
        }
    }
    (processes, pipes)
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

/// Extract the element type from a Queue, Cell, or AsyncQueue type.
fn port_element_type(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::Queue { elem, .. } => Some(*elem.clone()),
        Ty::Cell { elem } => Some(*elem.clone()),
        Ty::AsyncQueue { elem, .. } => Some(*elem.clone()),
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
    pipes: &HashMap<String, &Pipe>,
    type_env: &TypeEnv,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ProcessNetwork> {
    elaborate_pipe_inner(pipe, processes, pipes, type_env, diagnostics, false)
}

fn elaborate_pipe_inner(
    pipe: &Pipe,
    processes: &HashMap<String, &Process>,
    pipes: &HashMap<String, &Pipe>,
    type_env: &TypeEnv,
    diagnostics: &mut Vec<Diagnostic>,
    allow_dangling: bool,
) -> Option<ProcessNetwork> {
    let mut graph = petgraph::graph::DiGraph::new();
    let mut instances: HashMap<String, NodeIndex> = HashMap::new();
    let mut had_error = false;
    // Domain mappings from pipe instances (applied during domain collection phase)
    let mut pipe_instance_domains: HashMap<String, Option<String>> = HashMap::new();
    // Cross-pipe edge substitutions: child_prefixed_edge_name → parent_queue_name
    // These are applied after the parent's queue edges are created.
    let mut cross_pipe_subs: Vec<(String, String)> = vec![];

    // Phase 1 + 2: Create nodes from instances
    for (idx, inst) in pipe.instances.iter().enumerate() {
        let proc_name = &inst.process_name.node;

        // Check if it's a pipe instance
        if let Some(child_pipe) = pipes.get(proc_name) {
            // Skip self-reference
            if proc_name == &pipe.name.node {
                diagnostics.push(Diagnostic::error(
                    inst.process_name.span.clone(),
                    format!("pipe `{proc_name}` cannot instantiate itself"),
                ));
                had_error = true;
                continue;
            }

            // Recursively elaborate the child pipe (allow dangling endpoints)
            let child_net =
                elaborate_pipe_inner(child_pipe, processes, pipes, type_env, diagnostics, true);
            let Some(child_net) = child_net else {
                had_error = true;
                continue;
            };

            // Generate instance prefix for namespacing
            let inst_prefix = {
                let base = proc_name.clone();
                if instances.contains_key(&base) {
                    format!("{base}_{idx}")
                } else {
                    base
                }
            };

            // Merge child graph nodes into parent, remapping indices
            let mut child_node_map: HashMap<NodeIndex, NodeIndex> = HashMap::new();
            for child_node_idx in child_net.graph.node_indices() {
                let child_node = &child_net.graph[child_node_idx];
                let node = ProcessNode {
                    instance_name: format!("{}_{}", inst_prefix, child_node.instance_name),
                    process_name: child_node.process_name.clone(),
                    rules: child_node.rules.clone(),
                    ports: child_node
                        .ports
                        .iter()
                        .map(|p| ResolvedPort {
                            name: p.name.clone(),
                            kind: p.kind,
                            ty: p.ty.clone(),
                            bound_to: None, // Re-wire after edge creation
                        })
                        .collect(),
                    span: child_node.span.clone(),
                    is_memory_stub: child_node.is_memory_stub,
                };
                let new_idx = graph.add_node(node);
                child_node_map.insert(child_node_idx, new_idx);
                instances.insert(
                    format!("{}_{}", inst_prefix, child_node.instance_name),
                    new_idx,
                );
            }

            // Merge child graph edges, remapping node indices and peeker names
            let mut child_edge_map: HashMap<EdgeIndex, EdgeIndex> = HashMap::new();
            for child_edge_idx in child_net.graph.edge_indices() {
                let child_edge = &child_net.graph[child_edge_idx];
                let (src, dst) = child_net.graph.edge_endpoints(child_edge_idx).unwrap();
                let new_src = child_node_map[&src];
                let new_dst = child_node_map[&dst];
                let mut edge = child_edge.clone();
                edge.name = format!("{}_{}", inst_prefix, edge.name);
                // Prefix peeker instance names for Cell edges
                if let QueueEdgeKind::Cell {
                    ref mut peeker_instances,
                    ..
                } = edge.kind
                {
                    *peeker_instances = peeker_instances
                        .iter()
                        .map(|p| format!("{}_{}", inst_prefix, p))
                        .collect();
                }
                let new_edge_idx = graph.add_edge(new_src, new_dst, edge);
                child_edge_map.insert(child_edge_idx, new_edge_idx);
            }

            // Re-wire port bindings on merged nodes
            for child_node_idx in child_net.graph.node_indices() {
                let child_node = &child_net.graph[child_node_idx];
                let new_node_idx = child_node_map[&child_node_idx];
                for (port_idx, port) in child_node.ports.iter().enumerate() {
                    if let Some(old_edge) = port.bound_to
                        && let Some(&new_edge) = child_edge_map.get(&old_edge)
                    {
                        graph[new_node_idx].ports[port_idx].bound_to = Some(new_edge);
                    }
                }
            }

            // Store domain mappings from child instances.
            for (child_inst, child_domain) in &child_net.domain_map {
                let parent_inst_name = format!("{}_{}", inst_prefix, child_inst);
                if let Some(ref domain_ann) = inst.domain {
                    pipe_instance_domains.insert(parent_inst_name, Some(domain_ann.node.clone()));
                } else {
                    pipe_instance_domains.insert(parent_inst_name, child_domain.clone());
                }
            }

            // Cross-pipe wiring: each binding maps a child queue name to a parent queue.
            // Record substitutions: child's prefixed edge → parent queue name.
            for binding in &inst.bindings {
                let child_edge_name = format!("{}_{}", inst_prefix, binding.port.node);
                let parent_queue_name = binding.target.node.clone();
                cross_pipe_subs.push((child_edge_name, parent_queue_name));
            }

            continue;
        }

        let Some(proc_def) = processes.get(proc_name) else {
            diagnostics.push(Diagnostic::error(
                inst.process_name.span.clone(),
                format!("unknown process or pipe `{proc_name}`"),
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

    // Domain collection: validate domain declarations and instance annotations
    let mut domain_set: HashSet<String> = HashSet::new();
    for decl in &pipe.domain_decls {
        if !domain_set.insert(decl.name.node.clone()) {
            diagnostics.push(Diagnostic::error(
                decl.name.span.clone(),
                format!("duplicate domain declaration `{}`", decl.name.node),
            ));
            had_error = true;
        }
    }

    let mut domain_map: HashMap<String, Option<String>> = HashMap::new();
    for (idx, inst) in pipe.instances.iter().enumerate() {
        let proc_name = &inst.process_name.node;
        let instance_name = find_instance_name(&instances, proc_name, idx);
        let Some(instance_name) = instance_name else {
            continue;
        };

        if let Some(ref domain_ann) = inst.domain {
            if !domain_set.contains(&domain_ann.node) {
                diagnostics.push(Diagnostic::error(
                    domain_ann.span.clone(),
                    format!("unknown domain `{}`", domain_ann.node),
                ));
                had_error = true;
            } else {
                domain_map.insert(instance_name, Some(domain_ann.node.clone()));
            }
        } else {
            domain_map.insert(instance_name, None);
        }
    }

    // Merge domain mappings from pipe instances
    for (inst_name, domain) in pipe_instance_domains {
        domain_map.insert(inst_name, domain);
    }

    let domains: Vec<String> = pipe
        .domain_decls
        .iter()
        .map(|d| d.name.node.clone())
        .collect();

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
            kind: QueueEdgeKind::Queue {
                init_tokens: decl.init_tokens.unwrap_or(0),
            },
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

    // AsyncQueue edge creation (parallel to queue edges above)
    for decl in &pipe.async_queue_decls {
        let ty = type_env.resolve_type_expr(&decl.ty.node, diagnostics);
        let elem_ty = match &ty {
            Ty::AsyncQueue { elem, .. } => *elem.clone(),
            Ty::Queue { elem, .. } => *elem.clone(),
            _ => ty,
        };
        let queue_name = decl.name.node.clone();

        let depth = match decl.depth {
            Some(d) => d,
            None => {
                diagnostics.push(Diagnostic::error(
                    decl.name.span.clone(),
                    format!("AsyncQueue `{queue_name}` requires explicit depth"),
                ));
                had_error = true;
                continue;
            }
        };

        // Validate power-of-2
        if depth == 0 || (depth & (depth - 1)) != 0 {
            diagnostics.push(Diagnostic::error(
                decl.name.span.clone(),
                format!("AsyncQueue `{queue_name}` depth must be a power of 2, got {depth}"),
            ));
            had_error = true;
            continue;
        }

        let edge_data = QueueEdge {
            name: queue_name.clone(),
            elem_ty,
            depth,
            kind: QueueEdgeKind::AsyncQueue,
            span: decl.name.span.clone(),
        };

        pending_edges.insert(
            queue_name,
            PendingEdge {
                edge_idx: None,
                edge_data,
                writer: None,
                reader: None,
                decl_span: decl.name.span.clone(),
            },
        );
    }

    if had_error {
        return None;
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

    // Helper: collect all deferred edge names (queues + async queues) in declaration order
    let deferred_names: Vec<String> = pipe
        .queue_decls
        .iter()
        .map(|d| d.name.node.clone())
        .chain(pipe.async_queue_decls.iter().map(|d| d.name.node.clone()))
        .collect();

    for queue_name in &deferred_names {
        let Some(pending) = pending_edges.get(queue_name) else {
            continue;
        };
        if pending.edge_idx.is_some() {
            continue; // Already in graph (shouldn't happen for queue decls)
        }

        let writer_node = pending
            .writer
            .as_ref()
            .and_then(|(w, _)| instances.get(w))
            .copied();
        let reader_node = pending
            .reader
            .as_ref()
            .and_then(|(r, _)| instances.get(r))
            .copied();

        match (writer_node, reader_node) {
            (Some(src), Some(dst)) => {
                let edge_idx = graph.add_edge(src, dst, pending.edge_data.clone());
                queue_edge_map.insert(queue_name.clone(), edge_idx);
            }
            (Some(node), None) if allow_dangling => {
                // Dangling: only writer. Create self-loop; parent will rewire.
                let edge_idx = graph.add_edge(node, node, pending.edge_data.clone());
                queue_edge_map.insert(queue_name.clone(), edge_idx);
            }
            (None, Some(node)) if allow_dangling => {
                // Dangling: only reader. Create self-loop; parent will rewire.
                let edge_idx = graph.add_edge(node, node, pending.edge_data.clone());
                queue_edge_map.insert(queue_name.clone(), edge_idx);
            }
            _ => {} // Missing both — skip
        }
    }

    // Phase 4a: Apply cross-pipe edge substitutions.
    // For each binding on a pipe instance, replace the child's merged edge
    // with the parent's queue edge, rebinding all process ports.
    for (child_edge_name, parent_queue_name) in &cross_pipe_subs {
        // Find the child edge in the graph by name
        let child_edge_idx = graph
            .edge_indices()
            .find(|&ei| graph[ei].name == *child_edge_name);
        let Some(child_edge_idx) = child_edge_idx else {
            continue; // Edge not found (may have been a self-loop or similar)
        };

        // Get the parent queue's edge index
        let parent_edge_idx = queue_edge_map.get(parent_queue_name).copied();

        // If parent queue edge exists, rebind all ports from child edge to parent edge
        if let Some(parent_edge_idx) = parent_edge_idx {
            for node_idx in graph.node_indices() {
                for port in &mut graph[node_idx].ports {
                    if port.bound_to == Some(child_edge_idx) {
                        port.bound_to = Some(parent_edge_idx);
                    }
                }
            }
        } else {
            // Parent queue is deferred or pending — record the child edge's endpoints
            // so the parent queue inherits them. Look up the child edge endpoints.
            let (child_src, child_dst) = graph.edge_endpoints(child_edge_idx).unwrap();
            let child_src_name = graph[child_src].instance_name.clone();
            let child_dst_name = graph[child_dst].instance_name.clone();
            let child_span = graph[child_edge_idx].span.clone();

            if let Some(pending) = pending_edges.get_mut(parent_queue_name) {
                // Check: if child edge src→dst, and it's a queue edge, then
                // src is writer, dst is reader. Register them in parent pending.
                if pending.writer.is_none() {
                    pending.writer = Some((child_src_name.clone(), child_span.clone()));
                }
                if pending.reader.is_none() {
                    pending.reader = Some((child_dst_name.clone(), child_span.clone()));
                }
            }

            // Don't remove child edge yet — we need it for port bindings.
            // It will be replaced when the parent deferred edge is created.
            // Store a deferred substitution to apply after parent edges exist.
        }
    }

    // Re-create any deferred queue edges that gained endpoints from cross-pipe subs
    for queue_name in &deferred_names {
        if queue_edge_map.contains_key(queue_name) {
            continue; // Already created
        }
        let Some(pending) = pending_edges.get(queue_name) else {
            continue;
        };
        if pending.edge_idx.is_some() {
            continue;
        }
        if let (Some((writer, _)), Some((reader, _))) = (&pending.writer, &pending.reader)
            && let (Some(&src_node), Some(&dst_node)) =
                (instances.get(writer), instances.get(reader))
        {
            let edge_idx = graph.add_edge(src_node, dst_node, pending.edge_data.clone());
            queue_edge_map.insert(queue_name.clone(), edge_idx);

            // Now rebind any child process ports that were on the old child edge
            for (child_edge_name, parent_q) in &cross_pipe_subs {
                if parent_q != queue_name {
                    continue;
                }
                let old_child_edge = graph
                    .edge_indices()
                    .find(|&ei| graph[ei].name == *child_edge_name);
                if let Some(old_ei) = old_child_edge {
                    for node_idx in graph.node_indices() {
                        for port in &mut graph[node_idx].ports {
                            if port.bound_to == Some(old_ei) {
                                port.bound_to = Some(edge_idx);
                            }
                        }
                    }
                }
            }
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

    // Check every queue has exactly one writer and one reader.
    // When allow_dangling is true (child pipe), queues with missing endpoints
    // become pipe ports rather than errors.
    for (queue_name, pending) in &pending_edges {
        // Skip self-loop edges (they are implicitly wired)
        if queue_name.contains('.') {
            continue;
        }

        if !allow_dangling {
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

    // Cross-domain validation
    if !domains.is_empty() {
        for edge_idx in graph.edge_indices() {
            let edge = &graph[edge_idx];
            let (src_node, dst_node) = graph.edge_endpoints(edge_idx).unwrap();

            // Skip edges involving memory stubs — they're replaced by external
            // memory modules in the testbench and always operate in the same domain
            // as the process that uses them.
            if graph[src_node].is_memory_stub || graph[dst_node].is_memory_stub {
                continue;
            }

            let src_inst = &graph[src_node].instance_name;
            let dst_inst = &graph[dst_node].instance_name;
            let src_domain = domain_map.get(src_inst).cloned().flatten();
            let dst_domain = domain_map.get(dst_inst).cloned().flatten();

            match &edge.kind {
                QueueEdgeKind::Queue { .. } => {
                    // Sync queue crossing domains → error
                    if src_domain != dst_domain {
                        diagnostics.push(Diagnostic::error(
                            edge.span.clone(),
                            format!(
                                "queue `{}` connects domains `{}` and `{}`; use AsyncQueue for cross-domain communication",
                                edge.name,
                                src_domain.as_deref().unwrap_or("default"),
                                dst_domain.as_deref().unwrap_or("default"),
                            ),
                        ));
                        had_error = true;
                    }
                }
                QueueEdgeKind::AsyncQueue => {
                    // Async queue within same domain → warning
                    if src_domain == dst_domain {
                        diagnostics.push(Diagnostic::warning(
                            edge.span.clone(),
                            format!(
                                "AsyncQueue `{}` connects processes in the same domain; use Queue instead",
                                edge.name
                            ),
                        ));
                    }
                }
                QueueEdgeKind::Cell {
                    peeker_instances, ..
                } => {
                    // Cell peek across domains → error
                    // Self-loops are same node (always same domain), but cross-instance
                    // peekers listed in peeker_instances must be in the same domain as the owner.
                    let owner_domain = &src_domain;
                    for peeker_name in peeker_instances {
                        let peeker_domain = domain_map.get(peeker_name).cloned().flatten();
                        if *owner_domain != peeker_domain {
                            diagnostics.push(Diagnostic::error(
                                edge.span.clone(),
                                format!(
                                    "Cell `{}` peek crosses domains `{}` and `{}`; cross-domain peek is not supported",
                                    edge.name,
                                    owner_domain.as_deref().unwrap_or("default"),
                                    peeker_domain.as_deref().unwrap_or("default"),
                                ),
                            ));
                            had_error = true;
                        }
                    }
                }
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
        type_defs,
        domains,
        domain_map,
        constants: type_env.constants.clone(),
        external_fns: type_env.external_fns.clone(),
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
