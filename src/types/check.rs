use crate::ast::*;
use crate::diagnostics::Diagnostic;

use super::env::TypeEnv;
use super::ty::Ty;

/// Infer the type of an expression.
pub fn check_expr(expr: &Spanned<Expr>, env: &TypeEnv, diagnostics: &mut Vec<Diagnostic>) -> Ty {
    match &expr.node {
        Expr::Lit(Literal::Int(_)) => Ty::Bits(32), // default width
        Expr::Lit(Literal::Bool(_)) => Ty::Bool,

        Expr::Var(name) => {
            if name == "None" {
                // Polymorphic None — type inferred from context
                return Ty::Option(Box::new(Ty::Error));
            }
            if let Some(ty) = env.lookup(name) {
                ty.clone()
            } else {
                diagnostics.push(Diagnostic::error(
                    expr.span.clone(),
                    format!("undefined variable `{name}`"),
                ));
                Ty::Error
            }
        }

        Expr::Take { queue } => {
            if let Some(ty) = env.lookup(queue) {
                match ty {
                    Ty::Queue { elem, .. } | Ty::Cell { elem } => *elem.clone(),
                    _ => {
                        diagnostics.push(Diagnostic::error(
                            expr.span.clone(),
                            format!("`{queue}` is not a queue or cell"),
                        ));
                        Ty::Error
                    }
                }
            } else {
                diagnostics.push(Diagnostic::error(
                    expr.span.clone(),
                    format!("undefined port `{queue}`"),
                ));
                Ty::Error
            }
        }

        Expr::TryTake { queue } => {
            if let Some(ty) = env.lookup(queue) {
                match ty {
                    Ty::Queue { elem, .. } | Ty::Cell { elem } => {
                        Ty::Option(Box::new(*elem.clone()))
                    }
                    _ => {
                        diagnostics.push(Diagnostic::error(
                            expr.span.clone(),
                            format!("`{queue}` is not a queue or cell"),
                        ));
                        Ty::Error
                    }
                }
            } else {
                diagnostics.push(Diagnostic::error(
                    expr.span.clone(),
                    format!("undefined port `{queue}`"),
                ));
                Ty::Error
            }
        }

        Expr::Peek { queue } => {
            if let Some(ty) = env.lookup(queue) {
                match ty {
                    Ty::Queue { elem, .. } | Ty::Cell { elem } => {
                        Ty::Option(Box::new(*elem.clone()))
                    }
                    _ => {
                        diagnostics.push(Diagnostic::error(
                            expr.span.clone(),
                            format!("`{queue}` is not a queue or cell"),
                        ));
                        Ty::Error
                    }
                }
            } else {
                diagnostics.push(Diagnostic::error(
                    expr.span.clone(),
                    format!("undefined port `{queue}`"),
                ));
                Ty::Error
            }
        }

        Expr::BinOp { op, lhs, rhs } => {
            let lty = check_expr(lhs, env, diagnostics);
            let rty = check_expr(rhs, env, diagnostics);

            match op {
                BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::And
                | BinOp::Or
                | BinOp::Xor
                | BinOp::Shl
                | BinOp::Shr => {
                    // Arithmetic/bitwise: both operands same type, result same type
                    if lty != Ty::Error && rty != Ty::Error && lty != rty {
                        diagnostics.push(Diagnostic::error(
                            expr.span.clone(),
                            format!("type mismatch: `{lty}` vs `{rty}`"),
                        ));
                    }
                    if lty != Ty::Error { lty } else { rty }
                }
                BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                    // Comparison: both operands same type, result Bool
                    if lty != Ty::Error && rty != Ty::Error && lty != rty {
                        diagnostics.push(Diagnostic::error(
                            expr.span.clone(),
                            format!("type mismatch in comparison: `{lty}` vs `{rty}`"),
                        ));
                    }
                    Ty::Bool
                }
                BinOp::LogicalAnd | BinOp::LogicalOr => {
                    // Logical: both Bool, result Bool
                    if lty != Ty::Error && lty != Ty::Bool {
                        diagnostics.push(Diagnostic::error(
                            lhs.span.clone(),
                            format!("expected Bool, got `{lty}`"),
                        ));
                    }
                    if rty != Ty::Error && rty != Ty::Bool {
                        diagnostics.push(Diagnostic::error(
                            rhs.span.clone(),
                            format!("expected Bool, got `{rty}`"),
                        ));
                    }
                    Ty::Bool
                }
            }
        }

        Expr::UnaryOp { op, expr: inner } => {
            let ty = check_expr(inner, env, diagnostics);
            match op {
                UnaryOp::Not => ty,
                UnaryOp::Neg => ty,
            }
        }

        Expr::Tuple(items) => Ty::Tuple(
            items
                .iter()
                .map(|e| check_expr(e, env, diagnostics))
                .collect(),
        ),

        Expr::FieldAccess { expr: inner, field } => {
            let ty = check_expr(inner, env, diagnostics);
            match &ty {
                Ty::Record { fields, .. } => {
                    if let Some((_, fty)) = fields.iter().find(|(n, _)| n == &field.node) {
                        fty.clone()
                    } else {
                        diagnostics.push(Diagnostic::error(
                            field.span.clone(),
                            format!("no field `{}` on type `{ty}`", field.node),
                        ));
                        Ty::Error
                    }
                }
                Ty::Error => Ty::Error,
                _ => {
                    diagnostics.push(Diagnostic::error(
                        inner.span.clone(),
                        format!("field access on non-record type `{ty}`"),
                    ));
                    Ty::Error
                }
            }
        }

        Expr::Index { expr: inner, .. } => {
            let ty = check_expr(inner, env, diagnostics);
            match &ty {
                Ty::Array { elem, .. } => *elem.clone(),
                Ty::Error => Ty::Error,
                _ => {
                    diagnostics.push(Diagnostic::error(
                        inner.span.clone(),
                        format!("indexing non-array type `{ty}`"),
                    ));
                    Ty::Error
                }
            }
        }

        Expr::Update { expr: inner, .. } => {
            let ty = check_expr(inner, env, diagnostics);
            match &ty {
                Ty::Array { .. } => ty,
                Ty::Error => Ty::Error,
                _ => {
                    diagnostics.push(Diagnostic::error(
                        inner.span.clone(),
                        format!("functional update on non-array type `{ty}`"),
                    ));
                    Ty::Error
                }
            }
        }

        Expr::Call { func, args } => {
            // Check arg types (for diagnostics), but we don't have function signatures yet
            for arg in args {
                check_expr(arg, env, diagnostics);
            }
            if func == "Some" && args.len() == 1 {
                let inner_ty = check_expr(&args[0], env, diagnostics);
                return Ty::Option(Box::new(inner_ty));
            }
            // Unknown function — return Error for now
            Ty::Error
        }

        Expr::Record { name, fields } => {
            if let Some(ty) = env.type_defs.get(name) {
                if let Ty::Record {
                    fields: def_fields, ..
                } = ty
                {
                    // Check each field
                    for (field_name, field_val) in fields {
                        let val_ty = check_expr(field_val, env, diagnostics);
                        if let Some((_, expected_ty)) =
                            def_fields.iter().find(|(n, _)| n == &field_name.node)
                        {
                            if val_ty != Ty::Error
                                && *expected_ty != Ty::Error
                                && val_ty != *expected_ty
                            {
                                diagnostics.push(Diagnostic::error(
                                    field_val.span.clone(),
                                    format!(
                                        "field `{}`: expected `{expected_ty}`, got `{val_ty}`",
                                        field_name.node
                                    ),
                                ));
                            }
                        } else {
                            diagnostics.push(Diagnostic::error(
                                field_name.span.clone(),
                                format!("unknown field `{}` on record `{name}`", field_name.node),
                            ));
                        }
                    }
                    ty.clone()
                } else {
                    diagnostics.push(Diagnostic::error(
                        0..0,
                        format!("`{name}` is not a record type"),
                    ));
                    Ty::Error
                }
            } else {
                diagnostics.push(Diagnostic::error(
                    0..0,
                    format!("undefined record type `{name}`"),
                ));
                Ty::Error
            }
        }

        // MethodCall should have been desugared away
        Expr::MethodCall { method, .. } => {
            diagnostics.push(Diagnostic::error(
                method.span.clone(),
                "unexpected method call in type checker (should have been desugared)",
            ));
            Ty::Error
        }
    }
}

/// Type-check a statement.
pub fn check_stmt(stmt: &Spanned<Stmt>, env: &mut TypeEnv, diagnostics: &mut Vec<Diagnostic>) {
    match &stmt.node {
        Stmt::Let { pattern, value } => {
            let ty = check_expr(value, env, diagnostics);
            bind_pattern(pattern, &ty, env, diagnostics);
        }
        Stmt::Put { target, value } => {
            let val_ty = check_expr(value, env, diagnostics);
            if let Some(port_ty) = env.lookup(&target.node) {
                let expected = match port_ty {
                    Ty::Queue { elem, .. } | Ty::Cell { elem } => Some(*elem.clone()),
                    _ => {
                        diagnostics.push(Diagnostic::error(
                            target.span.clone(),
                            format!("`{}` is not a queue or cell", target.node),
                        ));
                        None
                    }
                };
                if let Some(expected) = expected
                    && val_ty != Ty::Error
                    && expected != Ty::Error
                    && val_ty != expected
                {
                    diagnostics.push(Diagnostic::error(
                        value.span.clone(),
                        format!("put type mismatch: expected `{expected}`, got `{val_ty}`"),
                    ));
                }
            } else {
                diagnostics.push(Diagnostic::error(
                    target.span.clone(),
                    format!("undefined port `{}`", target.node),
                ));
            }
        }
        Stmt::Expr(expr) => {
            check_expr(expr, env, diagnostics);
        }
        Stmt::Match { scrutinee, arms } => {
            let _scrut_ty = check_expr(scrutinee, env, diagnostics);
            for arm in arms {
                env.push_scope();
                // TODO: bind pattern variables with proper types
                check_stmts(&arm.body, env, diagnostics);
                env.pop_scope();
            }
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            let cond_ty = check_expr(cond, env, diagnostics);
            if cond_ty != Ty::Error && cond_ty != Ty::Bool {
                diagnostics.push(Diagnostic::error(
                    cond.span.clone(),
                    format!("if condition must be Bool, got `{cond_ty}`"),
                ));
            }
            env.push_scope();
            check_stmts(then_body, env, diagnostics);
            env.pop_scope();
            env.push_scope();
            check_stmts(else_body, env, diagnostics);
            env.pop_scope();
        }
    }
}

pub fn check_stmts(stmts: &[Spanned<Stmt>], env: &mut TypeEnv, diagnostics: &mut Vec<Diagnostic>) {
    for stmt in stmts {
        check_stmt(stmt, env, diagnostics);
    }
}

/// Bind pattern variables into the environment.
fn bind_pattern(
    pattern: &Spanned<Pattern>,
    ty: &Ty,
    env: &mut TypeEnv,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &pattern.node {
        Pattern::Bind(name) => {
            env.define(name.clone(), ty.clone());
        }
        Pattern::Tuple(parts) => {
            if let Ty::Tuple(tys) = ty {
                if parts.len() == tys.len() {
                    for (p, t) in parts.iter().zip(tys.iter()) {
                        bind_pattern(p, t, env, diagnostics);
                    }
                } else {
                    diagnostics.push(Diagnostic::error(
                        pattern.span.clone(),
                        format!(
                            "tuple pattern has {} elements but type has {}",
                            parts.len(),
                            tys.len()
                        ),
                    ));
                }
            } else if *ty != Ty::Error {
                diagnostics.push(Diagnostic::error(
                    pattern.span.clone(),
                    format!("tuple pattern on non-tuple type `{ty}`"),
                ));
            }
        }
        Pattern::Wildcard => {}
        Pattern::Variant { .. } => {
            // TODO: check variant matches enum type and bind inner patterns
        }
        Pattern::Literal(_) => {}
    }
}
