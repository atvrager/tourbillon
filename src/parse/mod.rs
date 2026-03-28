use crate::ast::SourceFile;
use crate::diagnostics::Diagnostic;

/// Parse a source string into a CST.
/// Returns the CST (if successful) and any parse errors.
pub fn parse(src: &str) -> (Option<SourceFile>, Vec<Diagnostic>) {
    let _ = src;
    // TODO: implement chumsky parser
    (
        Some(SourceFile { items: vec![] }),
        vec![],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source_parses() {
        let (cst, errors) = parse("");
        assert!(errors.is_empty());
        assert!(cst.is_some());
    }
}
