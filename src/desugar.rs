use crate::ast::SourceFile;

/// Desugar a parsed CST into the core AST.
///
/// Transformations:
/// - Cell declarations → depth-1 Queue with linearity annotations
/// - Pattern matching → decision trees (future)
pub fn desugar(source: SourceFile) -> SourceFile {
    // TODO: implement desugaring passes
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desugar_identity_on_empty() {
        let source = SourceFile { items: vec![] };
        let result = desugar(source);
        assert!(result.items.is_empty());
    }
}
