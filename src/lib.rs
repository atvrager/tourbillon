pub mod ast;
pub mod desugar;
pub mod diagnostics;
pub mod parse;
pub mod types;

use diagnostics::{report_errors, Errors};

/// Run the Phase 0 pipeline: parse → desugar → type-check.
pub fn check(src: &str, filename: &str) -> Result<(), Errors> {
    let (cst, parse_errors) = parse::parse(src);

    if !parse_errors.is_empty() {
        report_errors(src, filename, &parse_errors);
        return Err(Errors { diagnostics: parse_errors });
    }

    let cst = cst.expect("no CST produced despite zero parse errors");
    let ast = desugar::desugar(cst);

    let type_errors = types::check(&ast);
    if !type_errors.is_empty() {
        report_errors(src, filename, &type_errors);
        return Err(Errors { diagnostics: type_errors });
    }

    Ok(())
}
