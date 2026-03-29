use std::collections::HashMap;

use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};

use crate::ast::{Rule, Span};
use crate::types::ty::Ty;

/// A process network graph produced by elaboration.
/// One per pipe declaration in the source file.
pub struct ProcessNetwork {
    pub name: String,
    pub graph: DiGraph<ProcessNode, QueueEdge>,
    pub instances: HashMap<String, NodeIndex>,
}

/// A process instance node in the graph.
pub struct ProcessNode {
    pub instance_name: String,
    pub process_name: String,
    pub rules: Vec<Rule>,
    pub ports: Vec<ResolvedPort>,
    pub span: Span,
}

/// A port on a process instance with its resolved type and optional edge binding.
pub struct ResolvedPort {
    pub name: String,
    pub kind: crate::ast::PortKind,
    pub ty: Ty,
    pub bound_to: Option<EdgeIndex>,
}

/// A queue (or cell) edge in the process network.
pub struct QueueEdge {
    pub name: String,
    pub elem_ty: Ty,
    pub depth: u64,
    pub kind: QueueEdgeKind,
    pub span: Span,
}

/// Whether an edge is a regular queue or a cell (with possible cross-instance peekers).
pub enum QueueEdgeKind {
    Queue,
    Cell { peeker_instances: Vec<String> },
}
