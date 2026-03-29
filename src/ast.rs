/// Span information for error reporting.
pub type Span = std::ops::Range<usize>;

/// A node annotated with its source span.
#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

// ---------------------------------------------------------------------------
// Top-level
// ---------------------------------------------------------------------------

/// A complete source file.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub items: Vec<Spanned<Item>>,
}

/// Top-level items.
#[derive(Debug, Clone)]
pub enum Item {
    TypeDef(TypeDef),
    Process(Process),
    Pipe(Pipe),
}

// ---------------------------------------------------------------------------
// Type definitions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: Spanned<String>,
    pub kind: TypeDefKind,
}

#[derive(Debug, Clone)]
pub enum TypeDefKind {
    Alias(Spanned<TypeExpr>),
    Record(Vec<Field>),
    Enum(Vec<Variant>),
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: Spanned<String>,
    pub ty: Spanned<TypeExpr>,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub name: Spanned<String>,
    pub fields: Vec<Spanned<TypeExpr>>,
}

// ---------------------------------------------------------------------------
// Type expressions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum TypeExpr {
    /// Named type: `Word`, `Bits 32`, `Array(32, Word)`
    Named {
        name: String,
        args: Vec<Spanned<TypeExpr>>,
    },
    /// Product type: `A × B`
    Product(Vec<Spanned<TypeExpr>>),
    /// Queue type: `Queue(T, depth = N)`
    Queue {
        elem: Box<Spanned<TypeExpr>>,
        depth: Option<u64>,
    },
    /// Cell type: `Cell(T, init = expr)`
    Cell {
        elem: Box<Spanned<TypeExpr>>,
        init: Option<Box<Spanned<Expr>>>,
    },
}

// ---------------------------------------------------------------------------
// Processes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Process {
    pub name: Spanned<String>,
    pub ports: Vec<Port>,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone)]
pub struct Port {
    pub kind: PortKind,
    pub name: Spanned<String>,
    pub ty: Spanned<TypeExpr>,
    /// Array port size: `regs[32] : Cell(Word)` has `array_size = Some(32)`.
    /// Expanded by desugar into 32 individual ports before type checking.
    pub array_size: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortKind {
    Consumes,
    Produces,
    State,
    Peeks,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub name: Spanned<String>,
    pub body: Vec<Spanned<Stmt>>,
}

// ---------------------------------------------------------------------------
// Statements and expressions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Stmt {
    /// `let x = expr`
    Let {
        pattern: Spanned<Pattern>,
        value: Spanned<Expr>,
    },
    /// `queue.put(expr)`
    Put {
        target: Spanned<String>,
        value: Spanned<Expr>,
    },
    /// Expression used as statement
    Expr(Spanned<Expr>),
    /// `match expr { arms }`
    Match {
        scrutinee: Spanned<Expr>,
        arms: Vec<MatchArm>,
    },
    /// `if cond { stmts } [else { stmts }]`
    If {
        cond: Spanned<Expr>,
        then_body: Vec<Spanned<Stmt>>,
        else_body: Vec<Spanned<Stmt>>,
    },
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Spanned<Pattern>,
    pub body: Vec<Spanned<Stmt>>,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    /// `_`
    Wildcard,
    /// `x`
    Bind(String),
    /// `(a, b)`
    Tuple(Vec<Spanned<Pattern>>),
    /// `Some(x)`, `None`, `Load`
    Variant {
        name: String,
        fields: Vec<Spanned<Pattern>>,
    },
    /// `0`, `0x8000_0000`
    Literal(Literal),
}

#[derive(Debug, Clone)]
pub enum Expr {
    /// Integer literal
    Lit(Literal),
    /// Variable / path reference
    Var(String),
    /// `a.b` field access
    FieldAccess {
        expr: Box<Spanned<Expr>>,
        field: Spanned<String>,
    },
    /// `a[i]` index
    Index {
        expr: Box<Spanned<Expr>>,
        index: Box<Spanned<Expr>>,
    },
    /// `a[i := v]` functional update
    Update {
        expr: Box<Spanned<Expr>>,
        index: Box<Spanned<Expr>>,
        value: Box<Spanned<Expr>>,
    },
    /// `(a, b)` tuple construction
    Tuple(Vec<Spanned<Expr>>),
    /// `RecordName { field = val, ... }` record construction
    Record {
        name: String,
        fields: Vec<(Spanned<String>, Spanned<Expr>)>,
    },
    /// `receiver.method(args)` — resolved during desugaring
    MethodCall {
        receiver: Box<Spanned<Expr>>,
        method: Spanned<String>,
        args: Vec<Spanned<Expr>>,
    },
    /// `queue.take()` — produced by desugaring
    Take { queue: String },
    /// `queue.try_take()` — produced by desugaring
    TryTake { queue: String },
    /// `queue.peek()` — produced by desugaring
    Peek { queue: String },
    /// Function/built-in call: `alu(op, a, b, imm)`
    Call {
        func: String,
        args: Vec<Spanned<Expr>>,
    },
    /// Binary operation
    BinOp {
        op: BinOp,
        lhs: Box<Spanned<Expr>>,
        rhs: Box<Spanned<Expr>>,
    },
    /// Unary operation
    UnaryOp {
        op: UnaryOp,
        expr: Box<Spanned<Expr>>,
    },
}

#[derive(Debug, Clone)]
pub enum Literal {
    Int(u64),
    Bool(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Eq,
    Neq,
    Lt,
    Gt,
    Le,
    Ge,
    LogicalAnd,
    LogicalOr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
}

// ---------------------------------------------------------------------------
// Pipes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Pipe {
    pub name: Spanned<String>,
    pub queue_decls: Vec<QueueDecl>,
    pub memory_decls: Vec<MemoryDecl>,
    pub instances: Vec<Instance>,
}

#[derive(Debug, Clone)]
pub struct QueueDecl {
    pub name: Spanned<String>,
    pub ty: Spanned<TypeExpr>,
    pub depth: Option<u64>,
}

/// Memory(K → V, depth = N, latency = M) declaration in a pipe.
#[derive(Debug, Clone)]
pub struct MemoryDecl {
    pub name: Spanned<String>,
    pub key_ty: Spanned<TypeExpr>,
    pub val_ty: Spanned<TypeExpr>,
    pub depth: u64,
    pub latency: u64,
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub process_name: Spanned<String>,
    pub bindings: Vec<PortBinding>,
}

#[derive(Debug, Clone)]
pub struct PortBinding {
    pub port: Spanned<String>,
    pub target: Spanned<String>,
}
