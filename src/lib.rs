pub mod ast;
pub mod deadlock;
pub mod desugar;
pub mod diagnostics;
pub mod elaborate;
pub mod graph;
pub mod ir;
pub mod lower;
pub mod lower_chisel;
pub mod parse;
pub mod provenance;
pub mod schedule;
pub mod types;
pub mod wave;

use diagnostics::{Errors, report_errors};
use schedule::ScheduledNetwork;

/// Run the common pipeline: parse → desugar → type-check → elaborate → schedule → deadlock.
/// Returns the scheduled networks.
fn run_pipeline(src: &str, filename: &str) -> Result<Vec<ScheduledNetwork>, Errors> {
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

        let deadlock_diags = deadlock::analyze(&sched);
        if !deadlock_diags.is_empty() {
            report_errors(src, filename, &deadlock_diags);
            return Err(Errors {
                diagnostics: deadlock_diags,
            });
        }

        scheduled.push(sched);
    }

    Ok(scheduled)
}

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
        let (scheduled, sched_diags) = schedule::schedule(network);
        if !sched_diags.is_empty() {
            report_errors(src, filename, &sched_diags);
            return Err(Errors {
                diagnostics: sched_diags,
            });
        }

        let deadlock_diags = deadlock::analyze(&scheduled);
        if !deadlock_diags.is_empty() {
            report_errors(src, filename, &deadlock_diags);
            return Err(Errors {
                diagnostics: deadlock_diags,
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
    let scheduled = run_pipeline(src, filename)?;
    Ok(lower::lower(&scheduled, provenance))
}

/// Run the full pipeline through Chisel lowering, returning generated Scala files.
pub fn build_chisel(src: &str, filename: &str) -> Result<Vec<lower_chisel::ChiselFile>, Errors> {
    let scheduled = run_pipeline(src, filename)?;
    Ok(lower_chisel::lower_chisel(&scheduled))
}

/// Run the pipeline through scheduling and emit DOT graph(s) for each network.
pub fn emit_graph(src: &str, filename: &str) -> Result<Vec<String>, Errors> {
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

    let mut dots = vec![];
    for network in networks {
        let (scheduled, sched_diags) = schedule::schedule(network);
        if !sched_diags.is_empty() {
            report_errors(src, filename, &sched_diags);
            return Err(Errors {
                diagnostics: sched_diags,
            });
        }
        dots.push(graph::emit_dot(&scheduled));
    }

    Ok(dots)
}
