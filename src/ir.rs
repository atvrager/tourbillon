use std::collections::HashMap;

use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};

use crate::ast::{Rule, Span};
use crate::types::ty::Ty;

/// A process network graph produced by elaboration.
/// One per pipe declaration in the source file.
#[derive(Debug)]
pub struct ProcessNetwork {
    pub name: String,
    pub graph: DiGraph<ProcessNode, QueueEdge>,
    pub instances: HashMap<String, NodeIndex>,
    /// Type definitions reachable from this network (records, enums).
    pub type_defs: HashMap<String, Ty>,
}

/// A process instance node in the graph.
#[derive(Debug)]
pub struct ProcessNode {
    pub instance_name: String,
    pub process_name: String,
    pub rules: Vec<Rule>,
    pub ports: Vec<ResolvedPort>,
    pub span: Span,
}

/// A port on a process instance with its resolved type and optional edge binding.
#[derive(Debug)]
pub struct ResolvedPort {
    pub name: String,
    pub kind: crate::ast::PortKind,
    pub ty: Ty,
    pub bound_to: Option<EdgeIndex>,
}

/// A queue (or cell) edge in the process network.
#[derive(Debug)]
pub struct QueueEdge {
    pub name: String,
    pub elem_ty: Ty,
    pub depth: u64,
    pub kind: QueueEdgeKind,
    pub span: Span,
}

/// Whether an edge is a regular queue or a cell (with possible cross-instance peekers).
#[derive(Debug)]
pub enum QueueEdgeKind {
    Queue,
    Cell {
        peeker_instances: Vec<String>,
        init: Option<u64>,
    },
}
