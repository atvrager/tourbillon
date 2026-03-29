/// Internal type representation for the type checker.
/// Distinct from `ast::TypeExpr` which is syntactic.
#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    /// Fixed-width bit vector: `Bits N`
    Bits(u64),
    /// Boolean
    Bool,
    /// Tuple / product: `(A, B, C)`
    Tuple(Vec<Ty>),
    /// Named record: `record Decoded { ... }`
    Record {
        name: String,
        fields: Vec<(String, Ty)>,
    },
    /// Named enum: `enum MemOp = Load | Store | None`
    Enum {
        name: String,
        variants: Vec<(String, Vec<Ty>)>,
    },
    /// Fixed-size array: `Array(N, T)`
    Array { elem: Box<Ty>, size: u64 },
    /// Queue element type (for port type checking)
    Queue { elem: Box<Ty>, depth: Option<u64> },
    /// Cell element type (for port type checking)
    Cell { elem: Box<Ty> },
    /// AsyncQueue element type (for port type checking)
    AsyncQueue { elem: Box<Ty>, depth: Option<u64> },
    /// Option type (result of try_take/peek)
    Option(Box<Ty>),
    /// Named type reference (before resolution)
    Named(String),
    /// Type error placeholder — prevents cascading errors
    Error,
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Bits(n) => write!(f, "Bits {n}"),
            Ty::Bool => write!(f, "Bool"),
            Ty::Tuple(ts) => {
                write!(f, "(")?;
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 {
                        write!(f, " × ")?;
                    }
                    write!(f, "{t}")?;
                }
                write!(f, ")")
            }
            Ty::Record { name, .. } => write!(f, "{name}"),
            Ty::Enum { name, .. } => write!(f, "{name}"),
            Ty::Array { elem, size } => write!(f, "Array({size}, {elem})"),
            Ty::Queue { elem, .. } => write!(f, "Queue({elem})"),
            Ty::Cell { elem } => write!(f, "Cell({elem})"),
            Ty::AsyncQueue { elem, .. } => write!(f, "AsyncQueue({elem})"),
            Ty::Option(inner) => write!(f, "Option({inner})"),
            Ty::Named(name) => write!(f, "{name}"),
            Ty::Error => write!(f, "<error>"),
        }
    }
}
