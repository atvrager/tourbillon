use std::fmt::Write;

use crate::deadlock::classify_non_blocking_edges;
use crate::ir::QueueEdgeKind;
use crate::schedule::ScheduledNetwork;

/// Emit the process network as a Graphviz DOT graph.
///
/// Styling conventions:
/// - `shape=box` for process nodes
/// - Solid edges = Queue
/// - Bold self-loops = Cell
/// - Dotted = non-blocking (try_take only) edges
/// - Dashed = edges with initial tokens
/// - Labels show queue name and depth
pub fn emit_dot(scheduled: &ScheduledNetwork) -> String {
    let network = &scheduled.network;
    let graph = &network.graph;
    let non_blocking = classify_non_blocking_edges(scheduled);

    let mut out = String::new();
    writeln!(out, "digraph {} {{", sanitize_id(&network.name)).unwrap();
    writeln!(out, "  rankdir=LR;").unwrap();
    writeln!(out, "  node [shape=box];").unwrap();

    // Nodes — group by domain if domains are declared
    let domains = &network.domains;
    if !domains.is_empty() {
        // Group nodes by domain
        for domain in domains {
            writeln!(out, "  subgraph cluster_{domain} {{").unwrap();
            writeln!(out, "    label=\"{domain}\";").unwrap();
            for idx in graph.node_indices() {
                let node = &graph[idx];
                if node.is_memory_stub {
                    continue;
                }
                if let Some(Some(d)) = network.domain_map.get(&node.instance_name)
                    && d == domain
                {
                    writeln!(out, "    {};", sanitize_id(&node.instance_name)).unwrap();
                }
            }
            writeln!(out, "  }}").unwrap();
        }
        // Default domain nodes (no annotation)
        for idx in graph.node_indices() {
            let node = &graph[idx];
            if node.is_memory_stub {
                continue;
            }
            if let Some(None) | None = network.domain_map.get(&node.instance_name) {
                writeln!(out, "  {};", sanitize_id(&node.instance_name)).unwrap();
            }
        }
    } else {
        for idx in graph.node_indices() {
            let node = &graph[idx];
            if node.is_memory_stub {
                continue;
            }
            writeln!(out, "  {};", sanitize_id(&node.instance_name)).unwrap();
        }
    }

    // Edges
    for eidx in graph.edge_indices() {
        let (src, dst) = graph.edge_endpoints(eidx).unwrap();
        let src_node = &graph[src];
        let dst_node = &graph[dst];

        // Skip edges from/to memory stubs (they're module ports)
        if src_node.is_memory_stub || dst_node.is_memory_stub {
            continue;
        }

        let data = &graph[eidx];

        let (style, color) = match &data.kind {
            // AsyncQueue always rendered distinctly regardless of non-blocking status
            QueueEdgeKind::AsyncQueue => ("dashed", "red"),
            _ if non_blocking.contains(&eidx) => ("dotted", "black"),
            QueueEdgeKind::Cell { init: Some(_), .. } => ("dashed", "black"),
            QueueEdgeKind::Cell { .. } => ("bold", "black"),
            QueueEdgeKind::Queue { .. } => ("solid", "black"),
        };

        let label = format!("{}\\ndepth={}", data.name, data.depth);

        writeln!(
            out,
            "  {} -> {} [label=\"{label}\" style={style} color={color}];",
            sanitize_id(&src_node.instance_name),
            sanitize_id(&dst_node.instance_name),
        )
        .unwrap();
    }

    writeln!(out, "}}").unwrap();
    out
}

/// Sanitize an identifier for DOT output (quote if needed).
fn sanitize_id(name: &str) -> String {
    if name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.is_empty() {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot_for_src(src: &str) -> String {
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

        let mut dots = vec![];
        for net in networks {
            let (scheduled, sched_diags) = crate::schedule::schedule(net);
            assert!(sched_diags.is_empty(), "sched errors: {sched_diags:?}");
            dots.push(emit_dot(&scheduled));
        }
        dots.join("\n")
    }

    #[test]
    fn dot_contains_process_nodes() {
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
        let dot = dot_for_src(src);
        assert!(dot.contains("digraph Top"), "missing graph name");
        assert!(dot.contains("Producer"), "missing Producer node");
        assert!(dot.contains("Consumer"), "missing Consumer node");
        assert!(dot.contains("depth=2"), "missing depth label");
    }

    #[test]
    fn dot_cell_self_loop() {
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
        let dot = dot_for_src(src);
        assert!(
            dot.contains("Counter -> Counter"),
            "missing self-loop for Cell"
        );
    }
}
