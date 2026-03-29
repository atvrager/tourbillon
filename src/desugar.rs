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
    // Desugar Memory declarations into queue declarations + internal processes
    let mut new_items: Vec<Spanned<Item>> = vec![];
    for item in &mut source.items {
        match &mut item.node {
            Item::Process(process) => {
                for rule in &mut process.rules {
                    let new_body = desugar_stmts(&rule.body, diagnostics);
                    rule.body = new_body;
                }
            }
            Item::Pipe(pipe) => {
                for mem in &pipe.memory_decls {
                    let (queues, process, instance) = desugar_memory(mem);
                    pipe.queue_decls.extend(queues);
                    pipe.instances.push(instance);
                    new_items.push(Spanned::new(Item::Process(process), mem.name.span.clone()));
                }
                pipe.memory_decls.clear();
            }
            Item::TypeDef(_) => {}
        }
    }
    // Insert generated processes before the pipes that use them
    // Find the first Pipe item and insert before it
    if !new_items.is_empty() {
        let pipe_pos = source
            .items
            .iter()
            .position(|i| matches!(i.node, Item::Pipe(_)))
            .unwrap_or(source.items.len());
        for (i, item) in new_items.into_iter().enumerate() {
            source.items.insert(pipe_pos + i, item);
        }
    }
    source
}

/// Desugar a Memory declaration into queue declarations and a latency-modeling process.
///
/// `Memory(K → V, depth = N, latency = M)` becomes:
/// - `{name}_read_req  : Queue(K, depth = 1)`
/// - `{name}_read_resp : Queue(V, depth = 1)`
/// - `{name}_write_req : Queue(K × V, depth = 1)`
/// - An internal process `_Mem_{name}` that models the memory
fn desugar_memory(mem: &MemoryDecl) -> (Vec<QueueDecl>, Process, Instance) {
    let name = &mem.name.node;
    let span = mem.name.span.clone();

    let read_req = QueueDecl {
        name: Spanned::new(format!("{name}_read_req"), span.clone()),
        ty: Spanned::new(
            TypeExpr::Queue {
                elem: Box::new(mem.key_ty.clone()),
                depth: Some(1),
            },
            span.clone(),
        ),
        depth: Some(1),
    };

    let read_resp = QueueDecl {
        name: Spanned::new(format!("{name}_read_resp"), span.clone()),
        ty: Spanned::new(
            TypeExpr::Queue {
                elem: Box::new(mem.val_ty.clone()),
                depth: Some(1),
            },
            span.clone(),
        ),
        depth: Some(1),
    };

    let write_req = QueueDecl {
        name: Spanned::new(format!("{name}_write_req"), span.clone()),
        ty: Spanned::new(
            TypeExpr::Queue {
                elem: Box::new(Spanned::new(
                    TypeExpr::Product(vec![mem.key_ty.clone(), mem.val_ty.clone()]),
                    span.clone(),
                )),
                depth: Some(1),
            },
            span.clone(),
        ),
        depth: Some(1),
    };

    // Internal latency-modeling process:
    // consumes read_req, produces read_resp, consumes write_req
    // For latency=0: read_resp.put(storage[read_req.take()])
    // For latency>0: pipeline of registers (simplified: just pass through for now)
    let process_name = format!("_Mem_{name}");
    let process = Process {
        name: Spanned::new(process_name, span.clone()),
        ports: vec![
            Port {
                kind: PortKind::Consumes,
                name: Spanned::new("read_req".to_string(), span.clone()),
                ty: Spanned::new(
                    TypeExpr::Queue {
                        elem: Box::new(mem.key_ty.clone()),
                        depth: Some(1),
                    },
                    span.clone(),
                ),
            },
            Port {
                kind: PortKind::Produces,
                name: Spanned::new("read_resp".to_string(), span.clone()),
                ty: Spanned::new(
                    TypeExpr::Queue {
                        elem: Box::new(mem.val_ty.clone()),
                        depth: Some(1),
                    },
                    span.clone(),
                ),
            },
            Port {
                kind: PortKind::Consumes,
                name: Spanned::new("write_req".to_string(), span.clone()),
                ty: Spanned::new(
                    TypeExpr::Queue {
                        elem: Box::new(Spanned::new(
                            TypeExpr::Product(vec![mem.key_ty.clone(), mem.val_ty.clone()]),
                            span.clone(),
                        )),
                        depth: Some(1),
                    },
                    span.clone(),
                ),
            },
        ],
        rules: vec![
            // Rule: read — take addr from read_req, respond with data
            Rule {
                name: Spanned::new("read".to_string(), span.clone()),
                body: vec![
                    Spanned::new(
                        Stmt::Let {
                            pattern: Spanned::new(Pattern::Bind("addr".to_string()), span.clone()),
                            value: Spanned::new(
                                Expr::Take {
                                    queue: "read_req".to_string(),
                                },
                                span.clone(),
                            ),
                        },
                        span.clone(),
                    ),
                    // read_resp.put(0) — placeholder; actual memory behavior is in the SV wrapper
                    Spanned::new(
                        Stmt::Put {
                            target: Spanned::new("read_resp".to_string(), span.clone()),
                            value: Spanned::new(Expr::Lit(Literal::Int(0)), span.clone()),
                        },
                        span.clone(),
                    ),
                ],
            },
            // Rule: write — take (addr, data) from write_req
            Rule {
                name: Spanned::new("write".to_string(), span.clone()),
                body: vec![Spanned::new(
                    Stmt::Let {
                        pattern: Spanned::new(
                            Pattern::Tuple(vec![
                                Spanned::new(Pattern::Bind("waddr".to_string()), span.clone()),
                                Spanned::new(Pattern::Bind("wdata".to_string()), span.clone()),
                            ]),
                            span.clone(),
                        ),
                        value: Spanned::new(
                            Expr::Take {
                                queue: "write_req".to_string(),
                            },
                            span.clone(),
                        ),
                    },
                    span.clone(),
                )],
            },
        ],
    };

    // Generate the instance that wires the memory process to its queues
    let instance = Instance {
        process_name: Spanned::new(process.name.node.clone(), span.clone()),
        bindings: vec![
            PortBinding {
                port: Spanned::new("read_req".to_string(), span.clone()),
                target: Spanned::new(format!("{name}_read_req"), span.clone()),
            },
            PortBinding {
                port: Spanned::new("read_resp".to_string(), span.clone()),
                target: Spanned::new(format!("{name}_read_resp"), span.clone()),
            },
            PortBinding {
                port: Spanned::new("write_req".to_string(), span.clone()),
                target: Spanned::new(format!("{name}_write_req"), span.clone()),
            },
        ],
    };

    (vec![read_req, read_resp, write_req], process, instance)
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
