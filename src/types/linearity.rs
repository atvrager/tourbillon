use std::collections::HashMap;

use crate::ast::*;
use crate::diagnostics::Diagnostic;

/// Cell linearity state within a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellState {
    /// Cell has not been touched yet.
    Untouched,
    /// Cell has been taken (must put before rule end).
    Taken,
    /// Cell has been put back.
    Put,
}

/// Check Cell linearity discipline for a single rule.
///
/// Rules:
/// - `take()` on a `state:` port: Untouched → Taken (error if already Taken)
/// - `put()` on a `state:` port: Taken → Put (error if not Taken)
/// - `peek()`: no state change (exempt from linearity)
/// - At `if`/`match` joins: both branches must agree on Cell state
/// - At rule end: every Taken Cell must be Put
pub fn check_rule_linearity(
    rule: &Rule,
    state_ports: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut cell_states: HashMap<String, CellState> = state_ports
        .iter()
        .map(|name| (name.clone(), CellState::Untouched))
        .collect();

    check_stmts_linearity(&rule.body, &mut cell_states, diagnostics);

    // At rule end: check all taken cells are put
    for (name, state) in &cell_states {
        if *state == CellState::Taken {
            diagnostics.push(Diagnostic::error(
                rule.name.span.clone(),
                format!(
                    "Cell `{name}` was taken but not put back in rule `{}`",
                    rule.name.node
                ),
            ));
        }
    }
}

fn check_stmts_linearity(
    stmts: &[Spanned<Stmt>],
    states: &mut HashMap<String, CellState>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for stmt in stmts {
        check_stmt_linearity(stmt, states, diagnostics);
    }
}

fn check_stmt_linearity(
    stmt: &Spanned<Stmt>,
    states: &mut HashMap<String, CellState>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &stmt.node {
        Stmt::Let { value, .. } => {
            check_expr_linearity(value, states, diagnostics);
        }
        Stmt::Put { target, value } => {
            check_expr_linearity(value, states, diagnostics);
            if let Some(state) = states.get(&target.node) {
                match state {
                    CellState::Untouched => {
                        diagnostics.push(Diagnostic::error(
                            target.span.clone(),
                            format!("Cell `{}` was put without being taken first", target.node),
                        ));
                    }
                    CellState::Taken => {
                        states.insert(target.node.clone(), CellState::Put);
                    }
                    CellState::Put => {
                        diagnostics.push(Diagnostic::error(
                            target.span.clone(),
                            format!("Cell `{}` was put twice", target.node),
                        ));
                    }
                }
            }
            // Non-state ports: no linearity tracking needed
        }
        Stmt::Expr(expr) => {
            check_expr_linearity(expr, states, diagnostics);
        }
        Stmt::Match { scrutinee, arms } => {
            check_expr_linearity(scrutinee, states, diagnostics);

            // Check each arm independently, then verify they agree
            let mut arm_states: Vec<HashMap<String, CellState>> = vec![];
            for arm in arms {
                let mut branch_states = states.clone();
                check_stmts_linearity(&arm.body, &mut branch_states, diagnostics);
                arm_states.push(branch_states);
            }

            // All arms must agree on cell states
            if let Some(first) = arm_states.first() {
                for (name, first_state) in first {
                    for (i, arm_state) in arm_states.iter().enumerate().skip(1) {
                        if let Some(other_state) = arm_state.get(name)
                            && first_state != other_state
                        {
                            diagnostics.push(Diagnostic::error(
                                arms[i].pattern.span.clone(),
                                format!("Cell `{name}` has inconsistent state across match arms"),
                            ));
                        }
                    }
                }
                // Use the first arm's state as the merged state
                *states = first.clone();
            }
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            check_expr_linearity(cond, states, diagnostics);

            let mut then_states = states.clone();
            check_stmts_linearity(then_body, &mut then_states, diagnostics);

            let mut else_states = states.clone();
            check_stmts_linearity(else_body, &mut else_states, diagnostics);

            // Both branches must agree
            for (name, then_state) in &then_states {
                if let Some(else_state) = else_states.get(name)
                    && then_state != else_state
                {
                    diagnostics.push(Diagnostic::error(
                        cond.span.clone(),
                        format!("Cell `{name}` has inconsistent state across if/else branches"),
                    ));
                }
            }
            *states = then_states;
        }
    }
}

fn check_expr_linearity(
    expr: &Spanned<Expr>,
    states: &mut HashMap<String, CellState>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &expr.node {
        Expr::Take { queue } => {
            if let Some(state) = states.get(queue) {
                match state {
                    CellState::Untouched => {
                        states.insert(queue.clone(), CellState::Taken);
                    }
                    CellState::Taken => {
                        diagnostics.push(Diagnostic::error(
                            expr.span.clone(),
                            format!("Cell `{queue}` was taken twice"),
                        ));
                    }
                    CellState::Put => {
                        diagnostics.push(Diagnostic::error(
                            expr.span.clone(),
                            format!("Cell `{queue}` was taken after being put back"),
                        ));
                    }
                }
            }
        }
        Expr::Peek { .. } => {
            // peek() is exempt from linearity — no state change
        }
        Expr::TryTake { queue } => {
            // try_take on a state port — same as take for linearity
            if let Some(state) = states.get(queue) {
                match state {
                    CellState::Untouched => {
                        states.insert(queue.clone(), CellState::Taken);
                    }
                    CellState::Taken => {
                        diagnostics.push(Diagnostic::error(
                            expr.span.clone(),
                            format!("Cell `{queue}` was taken twice"),
                        ));
                    }
                    CellState::Put => {
                        diagnostics.push(Diagnostic::error(
                            expr.span.clone(),
                            format!("Cell `{queue}` was taken after being put back"),
                        ));
                    }
                }
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            check_expr_linearity(lhs, states, diagnostics);
            check_expr_linearity(rhs, states, diagnostics);
        }
        Expr::UnaryOp { expr: inner, .. } => {
            check_expr_linearity(inner, states, diagnostics);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                check_expr_linearity(arg, states, diagnostics);
            }
        }
        Expr::Tuple(items) => {
            for item in items {
                check_expr_linearity(item, states, diagnostics);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desugar;
    use crate::parse;

    fn check_linearity(src: &str) -> Vec<Diagnostic> {
        let (cst, errors) = parse::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut desugar_diags = vec![];
        let ast = desugar::desugar(cst.unwrap(), &mut desugar_diags);
        assert!(
            desugar_diags.is_empty(),
            "desugar errors: {desugar_diags:?}"
        );

        let mut diagnostics = vec![];
        if let Item::Process(p) = &ast.items[0].node {
            let state_ports: Vec<String> = p
                .ports
                .iter()
                .filter(|port| port.kind == PortKind::State)
                .map(|port| port.name.node.clone())
                .collect();
            for rule in &p.rules {
                check_rule_linearity(rule, &state_ports, &mut diagnostics);
            }
        }
        diagnostics
    }

    #[test]
    fn valid_take_put() {
        let src = r#"
process Foo {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        x.put(v + 1)
    }
}
"#;
        let diags = check_linearity(src);
        assert!(diags.is_empty(), "unexpected errors: {diags:?}");
    }

    #[test]
    fn missing_put() {
        let src = r#"
process Foo {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
    }
}
"#;
        let diags = check_linearity(src);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("not put back"));
    }

    #[test]
    fn double_take() {
        let src = r#"
process Foo {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        let w = x.take()
        x.put(v)
    }
}
"#;
        let diags = check_linearity(src);
        assert!(diags.iter().any(|d| d.message.contains("taken twice")));
    }

    #[test]
    fn peek_exempt() {
        let src = r#"
process Foo {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.peek()
    }
}
"#;
        // peek() should NOT cause a "not put back" error
        let diags = check_linearity(src);
        assert!(diags.is_empty(), "unexpected errors: {diags:?}");
    }

    #[test]
    fn branching_consistent() {
        let src = r#"
process Foo {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        if v == 0 {
            x.put(1)
        } else {
            x.put(v + 1)
        }
    }
}
"#;
        let diags = check_linearity(src);
        assert!(diags.is_empty(), "unexpected errors: {diags:?}");
    }

    #[test]
    fn branching_inconsistent() {
        let src = r#"
process Foo {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        if v == 0 {
            x.put(1)
        } else {
        }
    }
}
"#;
        let diags = check_linearity(src);
        assert!(
            diags.iter().any(|d| d.message.contains("inconsistent")),
            "expected inconsistency error, got: {diags:?}"
        );
    }
}
