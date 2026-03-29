pub mod ast;
pub mod desugar;
pub mod diagnostics;
pub mod elaborate;
pub mod ir;
pub mod lower;
pub mod parse;
pub mod provenance;
pub mod schedule;
pub mod types;

use diagnostics::{Errors, report_errors};

/// Run the pipeline: parse → desugar → type-check → elaborate → schedule.
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

    let (networks, elab_errors) = elaborate::elaborate(&ast, &type_env);
    if !elab_errors.is_empty() {
        report_errors(src, filename, &elab_errors);
        return Err(Errors {
            diagnostics: elab_errors,
        });
    }

    for network in networks {
        let (_scheduled, sched_diags) = schedule::schedule(network);
        if !sched_diags.is_empty() {
            report_errors(src, filename, &sched_diags);
            return Err(Errors {
                diagnostics: sched_diags,
            });
        }
    }

    Ok(())
}

/// Run the full pipeline through lowering, returning generated SV files.
///
/// When `provenance` is `Some`, a BLAKE3 hash comment and `localparam` are
/// embedded in each generated module. Pass `None` to omit provenance (e.g.
/// for golden-file tests that must remain stable).
pub fn build(
    src: &str,
    filename: &str,
    provenance: Option<[u8; 32]>,
) -> Result<Vec<lower::SvFile>, Errors> {
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

    let (networks, elab_errors) = elaborate::elaborate(&ast, &type_env);
    if !elab_errors.is_empty() {
        report_errors(src, filename, &elab_errors);
        return Err(Errors {
            diagnostics: elab_errors,
        });
    }

    let mut scheduled = vec![];
    for network in networks {
        let (sched, sched_diags) = schedule::schedule(network);
        if !sched_diags.is_empty() {
            report_errors(src, filename, &sched_diags);
            return Err(Errors {
                diagnostics: sched_diags,
            });
        }
        scheduled.push(sched);
    }

    Ok(lower::lower(&scheduled, provenance))
}
