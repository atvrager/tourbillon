use std::collections::HashSet;

use petgraph::algo::tarjan_scc;
use petgraph::graph::EdgeIndex;
use petgraph::visit::EdgeRef;

use crate::diagnostics::Diagnostic;
use crate::ir::QueueEdgeKind;
use crate::schedule::ScheduledNetwork;

/// Analyse the process network for potential deadlocks.
///
/// Returns a list of warnings (never errors) for:
/// - Cycles with zero initial tokens (A1: Petri net marking)
/// - Cycles where total queue depth < process count (A2: KPN capacity)
///
/// Edges accessed only via `try_take()` or `peek()` across all rules are
/// considered non-blocking and excluded from cycle analysis (A3).
pub fn analyze(scheduled: &ScheduledNetwork) -> Vec<Diagnostic> {
    let network = &scheduled.network;
    let graph = &network.graph;
    let mut diags = vec![];

    // A3: Classify edges as blocking vs non-blocking.
    // An edge is non-blocking if every rule that accesses it does so only via
    // try_take() or peek() (never a blocking take() or put()).
    let non_blocking_edges = classify_non_blocking_edges(scheduled);

    // Build a filtered view: only blocking edges participate in SCC analysis.
    // We do this by finding SCCs on the full graph, then filtering edges within each SCC.
    let sccs = tarjan_scc(graph);

    for scc in &sccs {
        let scc_set: HashSet<_> = scc.iter().copied().collect();

        // Skip trivial SCCs (single node with no self-loop blocking edge)
        if scc.len() == 1 {
            let node = scc[0];
            let has_blocking_self_loop = graph
                .edges(node)
                .any(|e| e.target() == node && !non_blocking_edges.contains(&e.id()));
            if !has_blocking_self_loop {
                continue;
            }
        }

        // Collect blocking edges within this SCC
        let mut cycle_edges: Vec<EdgeIndex> = vec![];
        for &node in scc {
            for edge in graph.edges(node) {
                if scc_set.contains(&edge.target()) && !non_blocking_edges.contains(&edge.id()) {
                    cycle_edges.push(edge.id());
                }
            }
        }

        if cycle_edges.is_empty() {
            continue;
        }

        // Collect process names for the cycle description
        let process_names: Vec<&str> = scc
            .iter()
            .filter(|&&n| !graph[n].is_memory_stub)
            .map(|&n| graph[n].instance_name.as_str())
            .collect();
        let non_stub_count = process_names.len();

        if non_stub_count == 0 {
            continue;
        }

        // A1: Token check — sum initial tokens on cycle edges
        let mut total_tokens: u64 = 0;
        for &eidx in &cycle_edges {
            let edge = &graph[eidx];
            match &edge.kind {
                QueueEdgeKind::Cell { init: Some(_), .. } => {
                    // Cell with init = occupied at reset, counts as 1 token
                    total_tokens += 1;
                }
                QueueEdgeKind::Queue { init_tokens, .. } => {
                    total_tokens += init_tokens;
                }
                _ => {}
            }
        }

        let cycle_desc = process_names.join(" → ");

        if total_tokens == 0 {
            let span = graph[scc[0]].span.clone();
            diags.push(Diagnostic::error(
                span,
                format!(
                    "deadlock: cycle [{cycle_desc}] has no initial tokens \
                     (add `init = N` to a queue on this cycle)",
                ),
            ));
        }

        // A2: KPN capacity check — total depth must be >= process count
        let total_depth: u64 = cycle_edges.iter().map(|&e| graph[e].depth).sum();
        if total_depth < non_stub_count as u64 {
            let span = graph[scc[0]].span.clone();
            diags.push(Diagnostic::error(
                span,
                format!(
                    "deadlock: cycle [{cycle_desc}] total queue depth ({total_depth}) \
                     < process count ({non_stub_count})"
                ),
            ));
        }
    }

    diags
}

/// Classify edges that are only accessed via non-blocking operations
/// (try_take / peek) across all rules in all processes.
pub fn classify_non_blocking_edges(scheduled: &ScheduledNetwork) -> HashSet<EdgeIndex> {
    let network = &scheduled.network;
    let graph = &network.graph;

    // Start with all edges as potentially non-blocking, then remove any that
    // are accessed via a blocking operation.
    let mut non_blocking: HashSet<EdgeIndex> = graph.edge_indices().collect();

    // Edges to/from memory stubs are always non-blocking — the memory
    // infrastructure guarantees responses, so these don't cause deadlocks.
    // Keep them in non_blocking (don't remove them below).
    let memory_stub_edges: HashSet<EdgeIndex> = graph
        .edge_indices()
        .filter(|&eidx| {
            let (src, dst) = graph.edge_endpoints(eidx).unwrap();
            graph[src].is_memory_stub || graph[dst].is_memory_stub
        })
        .collect();

    // AsyncQueue edges are always non-blocking for deadlock analysis:
    // cross-clock synchronization latency makes single-clock token counting invalid.
    // They're already in the non_blocking set; ensure they're never removed below.
    let async_edges: HashSet<EdgeIndex> = graph
        .edge_indices()
        .filter(|&eidx| matches!(graph[eidx].kind, QueueEdgeKind::AsyncQueue))
        .collect();

    for node_idx in graph.node_indices() {
        let node = &graph[node_idx];
        let schedule = match scheduled.schedules.get(&node_idx) {
            Some(s) => s,
            None => continue,
        };

        // Build port name → edge index mapping (excluding memory stub edges)
        let port_map: std::collections::HashMap<&str, EdgeIndex> = node
            .ports
            .iter()
            .filter_map(|p| p.bound_to.map(|e| (p.name.as_str(), e)))
            .collect();

        for resources in &schedule.rule_resources {
            // Blocking takes: takes that are NOT try_takes
            for port_name in &resources.takes {
                if !resources.try_takes.contains(port_name)
                    && let Some(&edge) = port_map.get(port_name.as_str())
                    && !memory_stub_edges.contains(&edge)
                    && !async_edges.contains(&edge)
                {
                    non_blocking.remove(&edge);
                }
            }
            // All puts are blocking (unless to memory stubs or async queues)
            for port_name in &resources.puts {
                if let Some(&edge) = port_map.get(port_name.as_str())
                    && !memory_stub_edges.contains(&edge)
                    && !async_edges.contains(&edge)
                {
                    non_blocking.remove(&edge);
                }
            }
            // peeks are non-blocking — do nothing
        }
    }

    non_blocking
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Diagnostic;

    fn analyze_src(src: &str) -> Vec<Diagnostic> {
        let (cst, errors) = crate::parse::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut desugar_diags = vec![];
        let ast = crate::desugar::desugar(cst.unwrap(), &mut desugar_diags);
        assert!(
            desugar_diags.is_empty(),
            "desugar errors: {desugar_diags:?}"
        );
        let (env, type_diags) = crate::types::check(&ast);
        assert!(type_diags.is_empty(), "type errors: {type_diags:?}");
        let (networks, elab_diags) = crate::elaborate::elaborate(&ast, &env);
        assert!(elab_diags.is_empty(), "elab errors: {elab_diags:?}");

        let mut all_diags = vec![];
        for net in networks {
            let (scheduled, sched_diags) = crate::schedule::schedule(net);
            assert!(sched_diags.is_empty(), "sched errors: {sched_diags:?}");
            all_diags.extend(analyze(&scheduled));
        }
        all_diags
    }

    #[test]
    fn single_cell_no_warning() {
        // A single Cell with init is a self-loop with a token — no deadlock.
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
        let diags = analyze_src(src);
        assert!(diags.is_empty(), "unexpected warnings: {diags:?}");
    }

    #[test]
    fn producer_consumer_no_cycle() {
        // Linear pipeline: no cycle, no deadlock possible.
        let src = r#"
process Producer {
    produces: out : Queue(Bits 32)
    rule go {
        out.put(42)
    }
}

process Consumer {
    consumes: inp : Queue(Bits 32)
    rule go {
        let _ = inp.take()
    }
}

pipe Top {
    let q = Queue(Bits 32, depth = 2)
    Producer { out = q }
    Consumer { inp = q }
}
"#;
        let diags = analyze_src(src);
        assert!(diags.is_empty(), "unexpected warnings: {diags:?}");
    }

    #[test]
    fn cycle_with_init_token_no_warning() {
        // A single process with a Cell (init=0) is a self-loop with a token.
        // The Cell starts occupied at reset, so the cycle has 1 token → no warning.
        // This mirrors the credit-token pattern: the Cell is always occupied.
        let src = r#"
process Ping {
    state: token : Cell(Bits 32, init = 0)
    produces: out : Queue(Bits 32)
    consumes: inp : Queue(Bits 32)
    rule go {
        let t = token.take()
        out.put(t)
        let v = inp.take()
        token.put(v)
    }
}

process Pong {
    consumes: inp : Queue(Bits 32)
    produces: out : Queue(Bits 32)
    rule go {
        let v = inp.take()
        out.put(v + 1)
    }
}

pipe Top {
    let q1 = Queue(Bits 32, depth = 1)
    let q2 = Queue(Bits 32, depth = 1)
    Ping { out = q1, inp = q2 }
    Pong { inp = q1, out = q2 }
}
"#;
        let diags = analyze_src(src);
        // The cycle Ping→q1→Pong→q2→Ping has the Cell self-loop on Ping
        // with init token, so no zero-token warning.
        let token_warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("no initial tokens"))
            .collect();
        assert!(
            token_warnings.is_empty(),
            "unexpected token warning: {token_warnings:?}"
        );
    }

    #[test]
    fn cycle_without_token_warns() {
        // Two processes in a cycle, no initial tokens anywhere.
        let src = r#"
process A {
    consumes: inp : Queue(Bits 32)
    produces: out : Queue(Bits 32)
    rule go {
        let v = inp.take()
        out.put(v)
    }
}

process B {
    consumes: inp : Queue(Bits 32)
    produces: out : Queue(Bits 32)
    rule go {
        let v = inp.take()
        out.put(v)
    }
}

pipe Top {
    let q1 = Queue(Bits 32, depth = 1)
    let q2 = Queue(Bits 32, depth = 1)
    A { inp = q2, out = q1 }
    B { inp = q1, out = q2 }
}
"#;
        let diags = analyze_src(src);
        let token_warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("no initial tokens"))
            .collect();
        assert!(
            !token_warnings.is_empty(),
            "expected zero-token error for cycle"
        );
    }

    #[test]
    fn queue_init_tokens_break_cycle() {
        // Same cycle as above, but q2 has init = 1 — no deadlock.
        let src = r#"
process A {
    consumes: inp : Queue(Bits 32)
    produces: out : Queue(Bits 32)
    rule go {
        let v = inp.take()
        out.put(v)
    }
}

process B {
    consumes: inp : Queue(Bits 32)
    produces: out : Queue(Bits 32)
    rule go {
        let v = inp.take()
        out.put(v)
    }
}

pipe Top {
    let q1 = Queue(Bits 32, depth = 1)
    let q2 = Queue(Bits 32, depth = 1, init = 1)
    A { inp = q2, out = q1 }
    B { inp = q1, out = q2 }
}
"#;
        let diags = analyze_src(src);
        assert!(
            diags.is_empty(),
            "unexpected errors with init token: {diags:?}"
        );
    }
}
