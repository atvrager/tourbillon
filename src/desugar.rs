use crate::ast::*;
use crate::diagnostics::Diagnostic;

/// Desugar a parsed CST into the core AST.
///
/// Transformations:
/// - `MethodCall(.take())` → `Take { queue }`
/// - `MethodCall(.try_take())` → `TryTake { queue }`
/// - `MethodCall(.peek())` → `Peek { queue }`
/// - `MethodCall(.put(val))` → `Stmt::Put { target, value }`
/// - Cell declarations → depth-1 Queue with linearity annotations (future)
/// - Pattern matching → decision trees (future)
pub fn desugar(mut source: SourceFile, diagnostics: &mut Vec<Diagnostic>) -> SourceFile {
    for item in &mut source.items {
        if let Item::Process(ref mut process) = item.node {
            for rule in &mut process.rules {
                let new_body = desugar_stmts(&rule.body, diagnostics);
                rule.body = new_body;
            }
        }
    }
    source
}

fn desugar_stmts(stmts: &[Spanned<Stmt>], diagnostics: &mut Vec<Diagnostic>) -> Vec<Spanned<Stmt>> {
    stmts.iter().map(|s| desugar_stmt(s, diagnostics)).collect()
}

fn desugar_stmt(stmt: &Spanned<Stmt>, diagnostics: &mut Vec<Diagnostic>) -> Spanned<Stmt> {
    let node = match &stmt.node {
        Stmt::Let { pattern, value } => Stmt::Let {
            pattern: pattern.clone(),
            value: desugar_expr(value, diagnostics),
        },
        Stmt::Expr(expr) => {
            let desugared = desugar_expr(expr, diagnostics);
            // Check if this is a .put() call — desugar to Stmt::Put
            if let Expr::MethodCall {
                receiver,
                method,
                args,
            } = &desugared.node
                && method.node == "put"
            {
                if let Some(receiver_name) = extract_ident(&receiver.node) {
                    if args.len() == 1 {
                        return Spanned::new(
                            Stmt::Put {
                                target: Spanned::new(
                                    receiver_name.to_string(),
                                    receiver.span.clone(),
                                ),
                                value: desugar_expr(&args[0], diagnostics),
                            },
                            stmt.span.clone(),
                        );
                    } else {
                        diagnostics.push(Diagnostic::error(
                            method.span.clone(),
                            "put() takes exactly one argument",
                        ));
                    }
                } else {
                    diagnostics.push(Diagnostic::error(
                        receiver.span.clone(),
                        "put() receiver must be a simple identifier",
                    ));
                }
            }
            Stmt::Expr(desugared)
        }
        Stmt::Put { target, value } => Stmt::Put {
            target: target.clone(),
            value: desugar_expr(value, diagnostics),
        },
        Stmt::Match { scrutinee, arms } => Stmt::Match {
            scrutinee: desugar_expr(scrutinee, diagnostics),
            arms: arms
                .iter()
                .map(|arm| MatchArm {
                    pattern: arm.pattern.clone(),
                    body: desugar_stmts(&arm.body, diagnostics),
                })
                .collect(),
        },
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => Stmt::If {
            cond: desugar_expr(cond, diagnostics),
            then_body: desugar_stmts(then_body, diagnostics),
            else_body: desugar_stmts(else_body, diagnostics),
        },
    };
    Spanned::new(node, stmt.span.clone())
}

fn desugar_expr(expr: &Spanned<Expr>, diagnostics: &mut Vec<Diagnostic>) -> Spanned<Expr> {
    let node = match &expr.node {
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let receiver = desugar_expr(receiver, diagnostics);
            let args: Vec<_> = args.iter().map(|a| desugar_expr(a, diagnostics)).collect();

            match method.node.as_str() {
                "take" => {
                    if let Some(name) = extract_ident(&receiver.node) {
                        if !args.is_empty() {
                            diagnostics.push(Diagnostic::error(
                                method.span.clone(),
                                "take() takes no arguments",
                            ));
                        }
                        Expr::Take {
                            queue: name.to_string(),
                        }
                    } else {
                        diagnostics.push(Diagnostic::error(
                            receiver.span.clone(),
                            "take() receiver must be a simple identifier",
                        ));
                        return expr.clone();
                    }
                }
                "try_take" => {
                    if let Some(name) = extract_ident(&receiver.node) {
                        if !args.is_empty() {
                            diagnostics.push(Diagnostic::error(
                                method.span.clone(),
                                "try_take() takes no arguments",
                            ));
                        }
                        Expr::TryTake {
                            queue: name.to_string(),
                        }
                    } else {
                        diagnostics.push(Diagnostic::error(
                            receiver.span.clone(),
                            "try_take() receiver must be a simple identifier",
                        ));
                        return expr.clone();
                    }
                }
                "peek" => {
                    if let Some(name) = extract_ident(&receiver.node) {
                        if !args.is_empty() {
                            diagnostics.push(Diagnostic::error(
                                method.span.clone(),
                                "peek() takes no arguments",
                            ));
                        }
                        Expr::Peek {
                            queue: name.to_string(),
                        }
                    } else {
                        diagnostics.push(Diagnostic::error(
                            receiver.span.clone(),
                            "peek() receiver must be a simple identifier",
                        ));
                        return expr.clone();
                    }
                }
                "put" => {
                    // put() as expression — will be caught at stmt level normally,
                    // but could appear in nested positions. Keep as MethodCall for now.
                    return Spanned::new(
                        Expr::MethodCall {
                            receiver: Box::new(receiver),
                            method: method.clone(),
                            args,
                        },
                        expr.span.clone(),
                    );
                }
                other => {
                    diagnostics.push(Diagnostic::error(
                        method.span.clone(),
                        format!("unknown method `{other}`"),
                    ));
                    return expr.clone();
                }
            }
        }
        Expr::BinOp { op, lhs, rhs } => Expr::BinOp {
            op: *op,
            lhs: Box::new(desugar_expr(lhs, diagnostics)),
            rhs: Box::new(desugar_expr(rhs, diagnostics)),
        },
        Expr::UnaryOp { op, expr: inner } => Expr::UnaryOp {
            op: *op,
            expr: Box::new(desugar_expr(inner, diagnostics)),
        },
        Expr::Call { func, args } => Expr::Call {
            func: func.clone(),
            args: args.iter().map(|a| desugar_expr(a, diagnostics)).collect(),
        },
        Expr::Tuple(items) => {
            Expr::Tuple(items.iter().map(|e| desugar_expr(e, diagnostics)).collect())
        }
        Expr::Record { name, fields } => Expr::Record {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|(n, v)| (n.clone(), desugar_expr(v, diagnostics)))
                .collect(),
        },
        Expr::FieldAccess { expr: inner, field } => Expr::FieldAccess {
            expr: Box::new(desugar_expr(inner, diagnostics)),
            field: field.clone(),
        },
        Expr::Index { expr: inner, index } => Expr::Index {
            expr: Box::new(desugar_expr(inner, diagnostics)),
            index: Box::new(desugar_expr(index, diagnostics)),
        },
        Expr::Update {
            expr: inner,
            index,
            value,
        } => Expr::Update {
            expr: Box::new(desugar_expr(inner, diagnostics)),
            index: Box::new(desugar_expr(index, diagnostics)),
            value: Box::new(desugar_expr(value, diagnostics)),
        },
        // Atoms — no transformation needed
        Expr::Lit(_)
        | Expr::Var(_)
        | Expr::Take { .. }
        | Expr::TryTake { .. }
        | Expr::Peek { .. } => {
            return expr.clone();
        }
    };
    Spanned::new(node, expr.span.clone())
}

fn extract_ident(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Var(name) => Some(name.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn desugar_identity_on_empty() {
        let source = SourceFile { items: vec![] };
        let mut diags = vec![];
        let result = desugar(source, &mut diags);
        assert!(result.items.is_empty());
        assert!(diags.is_empty());
    }

    #[test]
    fn desugar_take_method() {
        let src = r#"
process Foo {
    consumes: q : Queue(Bits 32)
    rule go {
        let x = q.take()
    }
}
"#;
        let (cst, errors) = parse::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut diags = vec![];
        let ast = desugar(cst.unwrap(), &mut diags);
        assert!(diags.is_empty(), "desugar errors: {diags:?}");

        if let Item::Process(p) = &ast.items[0].node {
            if let Stmt::Let { value, .. } = &p.rules[0].body[0].node {
                assert!(
                    matches!(value.node, Expr::Take { ref queue } if queue == "q"),
                    "expected Take, got {:?}",
                    value.node
                );
            } else {
                panic!("expected Let");
            }
        } else {
            panic!("expected Process");
        }
    }

    #[test]
    fn desugar_put_method() {
        let src = r#"
process Foo {
    produces: q : Queue(Bits 32)
    rule go {
        q.put(42)
    }
}
"#;
        let (cst, errors) = parse::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut diags = vec![];
        let ast = desugar(cst.unwrap(), &mut diags);
        assert!(diags.is_empty(), "desugar errors: {diags:?}");

        if let Item::Process(p) = &ast.items[0].node {
            assert!(
                matches!(&p.rules[0].body[0].node, Stmt::Put { target, .. } if target.node == "q"),
                "expected Put, got {:?}",
                p.rules[0].body[0].node
            );
        } else {
            panic!("expected Process");
        }
    }

    #[test]
    fn desugar_unknown_method() {
        let src = r#"
process Foo {
    consumes: q : Queue(Bits 32)
    rule go {
        q.foo()
    }
}
"#;
        let (cst, errors) = parse::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut diags = vec![];
        let _ast = desugar(cst.unwrap(), &mut diags);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("unknown method"));
    }
}
