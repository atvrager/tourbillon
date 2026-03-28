use crate::ast::SourceFile;
use crate::diagnostics::Diagnostic;

/// Type-check an AST.
///
/// Checks:
/// - Hindley-Milner type inference and unification
/// - Cell linearity: take() must be followed by exactly one put() on every path
/// - peek() is exempt from linearity obligations
/// - Queue protocol conformance
pub fn check(source: &SourceFile) -> Vec<Diagnostic> {
    let _ = source;
    // TODO: implement type checker
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source_typechecks() {
        let source = SourceFile { items: vec![] };
        let errors = check(&source);
        assert!(errors.is_empty());
    }
}
