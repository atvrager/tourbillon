pub mod ast;
pub mod desugar;
pub mod diagnostics;
pub mod elaborate;
pub mod ir;
pub mod parse;
pub mod types;

use diagnostics::{Errors, report_errors};

/// Run the pipeline: parse → desugar → type-check → elaborate.
pub fn check(src: &str, filename: &str) -> Result<(), Errors> {
    let (cst, parse_errors) = parse::parse(src);

    if !parse_errors.is_empty() {
        report_errors(src, filename, &parse_errors);
        return Err(Errors {
            diagnostics: parse_errors,
        });
    }

    let cst = cst.expect("no CST produced despite zero parse errors");

    let mut desugar_errors = vec![];
    let ast = desugar::desugar(cst, &mut desugar_errors);

    if !desugar_errors.is_empty() {
        report_errors(src, filename, &desugar_errors);
        return Err(Errors {
            diagnostics: desugar_errors,
        });
    }

    let (type_env, type_errors) = types::check(&ast);
    if !type_errors.is_empty() {
        report_errors(src, filename, &type_errors);
        return Err(Errors {
            diagnostics: type_errors,
        });
    }

    let (_networks, elab_errors) = elaborate::elaborate(&ast, &type_env);
    if !elab_errors.is_empty() {
        report_errors(src, filename, &elab_errors);
        return Err(Errors {
            diagnostics: elab_errors,
        });
    }

    Ok(())
}
