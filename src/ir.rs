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
    /// Declared clock domain names (empty = single-domain, backward compatible).
    pub domains: Vec<String>,
    /// Instance name → domain name (None = default domain using clk/rst_n).
    pub domain_map: HashMap<String, Option<String>>,
}

/// A process instance node in the graph.
#[derive(Debug)]
pub struct ProcessNode {
    pub instance_name: String,
    pub process_name: String,
    pub rules: Vec<Rule>,
    pub ports: Vec<ResolvedPort>,
    pub span: Span,
    /// True for compiler-generated memory stub processes (_Mem_*).
    /// The lowerer skips these and exposes their queue edges as module ports.
    pub is_memory_stub: bool,
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
#[derive(Debug, Clone)]
pub struct QueueEdge {
    pub name: String,
    pub elem_ty: Ty,
    pub depth: u64,
    pub kind: QueueEdgeKind,
    pub span: Span,
}

/// Whether an edge is a regular queue, a cell, or an async queue (CDC FIFO).
#[derive(Debug, Clone)]
pub enum QueueEdgeKind {
    Queue {
        /// Number of initial tokens pre-loaded at reset (from `init = N`).
        init_tokens: u64,
    },
    Cell {
        peeker_instances: Vec<String>,
        init: Option<u64>,
    },
    /// Async FIFO for clock domain crossing. No init tokens, no peekers.
    AsyncQueue,
}
