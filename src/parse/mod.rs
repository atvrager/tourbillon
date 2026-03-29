pub mod lexer;
pub mod parser;
pub mod token;

use chumsky::prelude::*;

use crate::ast::SourceFile;
use crate::diagnostics::Diagnostic;

/// Parse a source string into a CST.
/// Returns the CST (if successful) and any parse errors.
pub fn parse(src: &str) -> (Option<SourceFile>, Vec<Diagnostic>) {
    // Phase 1: Lex
    let (tokens, lex_errors) = lexer::lexer().parse(src).into_output_errors();

    let mut diagnostics: Vec<Diagnostic> = lex_errors
        .into_iter()
        .map(|e| {
            let span = e.span();
            Diagnostic::error(span.start..span.end, format!("unexpected character: {e}"))
        })
        .collect();

    let tokens = match tokens {
        Some(t) => t,
        None => return (None, diagnostics),
    };

    // Phase 2: Parse
    let len = src.len();
    let (ast, parse_errors) = parser::source_file_parser()
        .parse(
            tokens
                .as_slice()
                .map(SimpleSpan::new(len, len), |(t, s)| (t, s)),
        )
        .into_output_errors();

    for e in parse_errors {
        let span = e.span();
        diagnostics.push(Diagnostic::error(
            span.start..span.end,
            format!("parse error: {e}"),
        ));
    }

    (ast, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source_parses() {
        let (cst, errors) = parse("");
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert!(cst.is_some());
    }

    #[test]
    fn type_alias_parses() {
        let (cst, errors) = parse("type Word = Bits 32");
        assert!(errors.is_empty(), "errors: {errors:?}");
        let cst = cst.unwrap();
        assert_eq!(cst.items.len(), 1);
    }

    #[test]
    fn record_parses() {
        let src = r#"record Decoded {
            op : AluOp
            rd : RegIdx
        }"#;
        let (cst, errors) = parse(src);
        assert!(errors.is_empty(), "errors: {errors:?}");
        let cst = cst.unwrap();
        assert_eq!(cst.items.len(), 1);
    }

    #[test]
    fn enum_parses() {
        let src = "enum MemOp = Load | Store | None";
        let (cst, errors) = parse(src);
        assert!(errors.is_empty(), "errors: {errors:?}");
        let cst = cst.unwrap();
        assert_eq!(cst.items.len(), 1);
    }
}
