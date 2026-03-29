use std::collections::HashMap;

use crate::ast::*;
use crate::diagnostics::Diagnostic;

/// Desugar a parsed CST into the core AST.
///
/// Transformations:
/// - `MethodCall(.take())` → `Take { queue }`
/// - `MethodCall(.try_take())` → `TryTake { queue }`
/// - `MethodCall(.peek())` → `Peek { queue }`
/// - `MethodCall(.put(val))` → `Stmt::Put { target, value }`
/// - `regs[32] : Cell(T)` → `regs_0 : Cell(T), ..., regs_31 : Cell(T)`
/// - `regs[idx].take()` → `match idx { 0 => regs_0.take(), ... }`
/// - Cell declarations → depth-1 Queue with linearity annotations (future)
/// - Pattern matching → decision trees (future)
pub fn desugar(mut source: SourceFile, diagnostics: &mut Vec<Diagnostic>) -> SourceFile {
    // Phase 0: Collect array port info from all processes (name → size)
    let mut process_array_ports: HashMap<String, HashMap<String, u64>> = HashMap::new();
    for item in &source.items {
        if let Item::Process(process) = &item.node {
            let mut array_ports = HashMap::new();
            for port in &process.ports {
                if let Some(size) = port.array_size {
                    array_ports.insert(port.name.node.clone(), size);
                }
            }
            if !array_ports.is_empty() {
                process_array_ports.insert(process.name.node.clone(), array_ports);
            }
        }
    }

    // Phase 1: Expand array ports and desugar method calls in processes
    let mut new_items: Vec<Spanned<Item>> = vec![];
    for item in &mut source.items {
        match &mut item.node {
            Item::Process(process) => {
                let array_ports = process_array_ports
                    .get(&process.name.node)
                    .cloned()
                    .unwrap_or_default();

                // Expand array ports into individual ports
                if !array_ports.is_empty() {
                    let mut new_ports = vec![];
                    for port in &process.ports {
                        if let Some(size) = port.array_size {
                            for i in 0..size {
                                new_ports.push(Port {
                                    kind: port.kind,
                                    name: Spanned::new(
                                        format!("{}_{}", port.name.node, i),
                                        port.name.span.clone(),
                                    ),
                                    ty: port.ty.clone(),
                                    array_size: None,
                                });
                            }
                        } else {
                            new_ports.push(port.clone());
                        }
                    }
                    process.ports = new_ports;
                }

                for rule in &mut process.rules {
                    let new_body = desugar_stmts(&rule.body, &array_ports, diagnostics);
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

                // Expand array port bindings in pipe instances
                expand_pipe_array_bindings(pipe, &process_array_ports);
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

/// Expand array port bindings in pipe instances.
///
/// `Decode { regs = Writeback.regs }` expands to:
/// `Decode { regs_0 = Writeback.regs_0, ..., regs_31 = Writeback.regs_31 }`
fn expand_pipe_array_bindings(
    pipe: &mut Pipe,
    process_array_ports: &HashMap<String, HashMap<String, u64>>,
) {
    for inst in &mut pipe.instances {
        let proc_name = &inst.process_name.node;
        let Some(array_ports) = process_array_ports.get(proc_name) else {
            continue;
        };

        let mut new_bindings = vec![];
        for binding in &inst.bindings {
            let port_name = &binding.port.node;
            if let Some(&size) = array_ports.get(port_name) {
                // Expand: regs = X.regs → regs_0 = X.regs_0, ..., regs_N = X.regs_N
                let target = &binding.target.node;
                for i in 0..size {
                    let expanded_target = if let Some(dot_pos) = target.find('.') {
                        // Dotted: "Writeback.regs" → "Writeback.regs_0"
                        format!("{}.{}_{}", &target[..dot_pos], &target[dot_pos + 1..], i)
                    } else {
                        // Simple: "regs" → "regs_0"
                        format!("{}_{}", target, i)
                    };
                    new_bindings.push(PortBinding {
                        port: Spanned::new(
                            format!("{}_{}", port_name, i),
                            binding.port.span.clone(),
                        ),
                        target: Spanned::new(expanded_target, binding.target.span.clone()),
                    });
                }
            } else {
                new_bindings.push(binding.clone());
            }
        }
        inst.bindings = new_bindings;
    }
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
        init_tokens: None,
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
        init_tokens: None,
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
        init_tokens: None,
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
                array_size: None,
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
                array_size: None,
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
                array_size: None,
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

fn desugar_stmts(
    stmts: &[Spanned<Stmt>],
    array_ports: &HashMap<String, u64>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Spanned<Stmt>> {
    let mut result = vec![];
    for s in stmts {
        desugar_stmt_into(s, array_ports, diagnostics, &mut result);
    }
    result
}

/// Desugar a statement, potentially expanding it into multiple statements.
///
/// For indexed put (`regs[idx].put(val)`), this expands into an if/else-if chain
/// at the statement level since put is a statement, not an expression.
fn desugar_stmt_into(
    stmt: &Spanned<Stmt>,
    array_ports: &HashMap<String, u64>,
    diagnostics: &mut Vec<Diagnostic>,
    out: &mut Vec<Spanned<Stmt>>,
) {
    let node = match &stmt.node {
        Stmt::Let { pattern, value } => {
            // Check for indexed array port: let x = regs[idx].take/peek/try_take()
            if let Expr::MethodCall {
                receiver,
                method,
                args: _,
            } = &value.node
                && matches!(method.node.as_str(), "take" | "try_take" | "peek")
                && let Some((array_name, index_expr)) = extract_indexed_ident(&receiver.node)
                && let Some(&size) = array_ports.get(array_name)
            {
                let desugared_idx = desugar_expr(index_expr, array_ports, diagnostics);
                out.push(expand_indexed_let(
                    array_name,
                    size,
                    &desugared_idx,
                    &method.node,
                    pattern,
                    &stmt.span,
                ));
                return;
            }
            Stmt::Let {
                pattern: pattern.clone(),
                value: desugar_expr(value, array_ports, diagnostics),
            }
        }
        Stmt::Expr(expr) => {
            // Check for raw (pre-desugar) indexed put: regs[idx].put(val)
            if let Expr::MethodCall {
                receiver,
                method,
                args,
            } = &expr.node
                && method.node == "put"
                && let Some((array_name, index_expr)) = extract_indexed_ident(&receiver.node)
                && let Some(&size) = array_ports.get(array_name)
                && args.len() == 1
            {
                let desugared_idx = desugar_expr(index_expr, array_ports, diagnostics);
                let put_val = desugar_expr(&args[0], array_ports, diagnostics);
                out.push(expand_indexed_put(
                    array_name,
                    size,
                    &desugared_idx,
                    &put_val,
                    &stmt.span,
                ));
                return;
            }

            let desugared = desugar_expr(expr, array_ports, diagnostics);
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
                        out.push(Spanned::new(
                            Stmt::Put {
                                target: Spanned::new(
                                    receiver_name.to_string(),
                                    receiver.span.clone(),
                                ),
                                value: desugar_expr(&args[0], array_ports, diagnostics),
                            },
                            stmt.span.clone(),
                        ));
                        return;
                    } else {
                        diagnostics.push(Diagnostic::error(
                            method.span.clone(),
                            "put() takes exactly one argument",
                        ));
                    }
                } else {
                    diagnostics.push(Diagnostic::error(
                        receiver.span.clone(),
                        "put() receiver must be a simple identifier or indexed array port",
                    ));
                }
            }
            Stmt::Expr(desugared)
        }
        Stmt::Put { target, value } => Stmt::Put {
            target: target.clone(),
            value: desugar_expr(value, array_ports, diagnostics),
        },
        Stmt::Match { scrutinee, arms } => Stmt::Match {
            scrutinee: desugar_expr(scrutinee, array_ports, diagnostics),
            arms: arms
                .iter()
                .map(|arm| MatchArm {
                    pattern: arm.pattern.clone(),
                    body: desugar_stmts(&arm.body, array_ports, diagnostics),
                })
                .collect(),
        },
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => Stmt::If {
            cond: desugar_expr(cond, array_ports, diagnostics),
            then_body: desugar_stmts(then_body, array_ports, diagnostics),
            else_body: desugar_stmts(else_body, array_ports, diagnostics),
        },
    };
    out.push(Spanned::new(node, stmt.span.clone()));
}

#[allow(clippy::only_used_in_recursion)] // array_ports is used at the stmt level; threaded here for future expression-level expansion
fn desugar_expr(
    expr: &Spanned<Expr>,
    array_ports: &HashMap<String, u64>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Spanned<Expr> {
    let node = match &expr.node {
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let receiver = desugar_expr(receiver, array_ports, diagnostics);
            let args: Vec<_> = args
                .iter()
                .map(|a| desugar_expr(a, array_ports, diagnostics))
                .collect();

            match method.node.as_str() {
                "take" | "try_take" | "peek" => {
                    if let Some(name) = extract_ident(&receiver.node) {
                        if !args.is_empty() {
                            diagnostics.push(Diagnostic::error(
                                method.span.clone(),
                                format!("{}() takes no arguments", method.node),
                            ));
                        }
                        match method.node.as_str() {
                            "take" => Expr::Take {
                                queue: name.to_string(),
                            },
                            "try_take" => Expr::TryTake {
                                queue: name.to_string(),
                            },
                            "peek" => Expr::Peek {
                                queue: name.to_string(),
                            },
                            _ => unreachable!(),
                        }
                    } else {
                        // Indexed array port calls should be handled at stmt level;
                        // if we reach here, keep as MethodCall for stmt-level handler
                        return Spanned::new(
                            Expr::MethodCall {
                                receiver: Box::new(receiver),
                                method: method.clone(),
                                args,
                            },
                            expr.span.clone(),
                        );
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
            lhs: Box::new(desugar_expr(lhs, array_ports, diagnostics)),
            rhs: Box::new(desugar_expr(rhs, array_ports, diagnostics)),
        },
        Expr::UnaryOp { op, expr: inner } => Expr::UnaryOp {
            op: *op,
            expr: Box::new(desugar_expr(inner, array_ports, diagnostics)),
        },
        Expr::Call { func, args } => Expr::Call {
            func: func.clone(),
            args: args
                .iter()
                .map(|a| desugar_expr(a, array_ports, diagnostics))
                .collect(),
        },
        Expr::Tuple(items) => Expr::Tuple(
            items
                .iter()
                .map(|e| desugar_expr(e, array_ports, diagnostics))
                .collect(),
        ),
        Expr::Record { name, fields } => Expr::Record {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|(n, v)| (n.clone(), desugar_expr(v, array_ports, diagnostics)))
                .collect(),
        },
        Expr::FieldAccess { expr: inner, field } => Expr::FieldAccess {
            expr: Box::new(desugar_expr(inner, array_ports, diagnostics)),
            field: field.clone(),
        },
        Expr::Index { expr: inner, index } => Expr::Index {
            expr: Box::new(desugar_expr(inner, array_ports, diagnostics)),
            index: Box::new(desugar_expr(index, array_ports, diagnostics)),
        },
        Expr::Update {
            expr: inner,
            index,
            value,
        } => Expr::Update {
            expr: Box::new(desugar_expr(inner, array_ports, diagnostics)),
            index: Box::new(desugar_expr(index, array_ports, diagnostics)),
            value: Box::new(desugar_expr(value, array_ports, diagnostics)),
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

/// Extract `(array_name, index_expr)` from `Expr::Index { Var(name), idx }`.
fn extract_indexed_ident(expr: &Expr) -> Option<(&str, &Spanned<Expr>)> {
    if let Expr::Index { expr: inner, index } = expr
        && let Expr::Var(name) = &inner.node
    {
        Some((name.as_str(), index))
    } else {
        None
    }
}

/// Expand `regs[rd].put(val)` into a match statement:
/// ```text
/// match rd {
///   0 => { regs_0.put(val) }
///   1 => { regs_1.put(val) }
///   ...
/// }
/// ```
fn expand_indexed_put(
    array_name: &str,
    size: u64,
    index_expr: &Spanned<Expr>,
    value: &Spanned<Expr>,
    span: &Span,
) -> Spanned<Stmt> {
    let arms: Vec<MatchArm> = (0..size)
        .map(|i| MatchArm {
            pattern: Spanned::new(Pattern::Literal(Literal::Int(i)), span.clone()),
            body: vec![Spanned::new(
                Stmt::Put {
                    target: Spanned::new(format!("{array_name}_{i}"), span.clone()),
                    value: value.clone(),
                },
                span.clone(),
            )],
        })
        .collect();
    Spanned::new(
        Stmt::Match {
            scrutinee: index_expr.clone(),
            arms,
        },
        span.clone(),
    )
}

/// Expand `let x = regs[rd].take()` (or peek/try_take) into a match statement:
/// ```text
/// match rd {
///   0 => { let x = regs_0.take() }
///   1 => { let x = regs_1.take() }
///   ...
/// }
/// ```
/// The variable `x` is visible after the match in the generated SV because
/// the lowerer declares all variables at the top of the always_comb block.
fn expand_indexed_let(
    array_name: &str,
    size: u64,
    index_expr: &Spanned<Expr>,
    method: &str,
    pattern: &Spanned<Pattern>,
    span: &Span,
) -> Spanned<Stmt> {
    let arms: Vec<MatchArm> = (0..size)
        .map(|i| {
            let port_name = format!("{array_name}_{i}");
            let expr_node = match method {
                "take" => Expr::Take {
                    queue: port_name.clone(),
                },
                "try_take" => Expr::TryTake {
                    queue: port_name.clone(),
                },
                "peek" => Expr::Peek {
                    queue: port_name.clone(),
                },
                _ => unreachable!(),
            };
            MatchArm {
                pattern: Spanned::new(Pattern::Literal(Literal::Int(i)), span.clone()),
                body: vec![Spanned::new(
                    Stmt::Let {
                        pattern: pattern.clone(),
                        value: Spanned::new(expr_node, span.clone()),
                    },
                    span.clone(),
                )],
            }
        })
        .collect();
    Spanned::new(
        Stmt::Match {
            scrutinee: index_expr.clone(),
            arms,
        },
        span.clone(),
    )
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

    #[test]
    fn desugar_indexed_cell_ports() {
        let src = r#"
type Word = Bits 32
process Writer {
    state: regs[4] : Cell(Word, init = 0)
    rule go {
        let old = regs[2].take()
        regs[2].put(42)
    }
}
"#;
        let (cst, errors) = parse::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut diags = vec![];
        let ast = desugar(cst.unwrap(), &mut diags);
        assert!(diags.is_empty(), "desugar errors: {diags:?}");

        if let Item::Process(p) = &ast.items[1].node {
            // Array port regs[4] should expand to 4 individual ports
            assert_eq!(p.ports.len(), 4, "expected 4 ports, got {:?}", p.ports);
            assert_eq!(p.ports[0].name.node, "regs_0");
            assert_eq!(p.ports[1].name.node, "regs_1");
            assert_eq!(p.ports[2].name.node, "regs_2");
            assert_eq!(p.ports[3].name.node, "regs_3");

            // Rule body should have 2 match statements (take and put)
            let body = &p.rules[0].body;
            assert_eq!(body.len(), 2, "expected 2 stmts, got {:?}", body);

            // First stmt: match for take
            assert!(
                matches!(&body[0].node, Stmt::Match { arms, .. } if arms.len() == 4),
                "expected match with 4 arms for take, got {:?}",
                body[0].node
            );

            // Second stmt: match for put
            assert!(
                matches!(&body[1].node, Stmt::Match { arms, .. } if arms.len() == 4),
                "expected match with 4 arms for put, got {:?}",
                body[1].node
            );
        } else {
            panic!("expected Process");
        }
    }

    #[test]
    fn desugar_indexed_peek() {
        let src = r#"
type Word = Bits 32
process Reader {
    peeks: regs[4] : Cell(Word)
    rule go {
        let v = regs[1].peek()
    }
}
"#;
        let (cst, errors) = parse::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut diags = vec![];
        let ast = desugar(cst.unwrap(), &mut diags);
        assert!(diags.is_empty(), "desugar errors: {diags:?}");

        if let Item::Process(p) = &ast.items[1].node {
            assert_eq!(p.ports.len(), 4);
            // Rule body: match statement for peek
            let body = &p.rules[0].body;
            assert_eq!(body.len(), 1);
            if let Stmt::Match { arms, .. } = &body[0].node {
                assert_eq!(arms.len(), 4);
                // Check arm 0 has peek on regs_0
                if let Stmt::Let { value, .. } = &arms[0].body[0].node {
                    assert!(
                        matches!(&value.node, Expr::Peek { queue } if queue == "regs_0"),
                        "expected Peek(regs_0), got {:?}",
                        value.node
                    );
                } else {
                    panic!("expected Let in arm");
                }
            } else {
                panic!("expected Match");
            }
        } else {
            panic!("expected Process");
        }
    }

    #[test]
    fn desugar_pipe_array_bindings() {
        let src = r#"
type Word = Bits 32
process Writer {
    state: regs[2] : Cell(Word, init = 0)
    rule go {
        let old = regs[0].take()
        regs[0].put(42)
    }
}
process Reader {
    peeks: regs[2] : Cell(Word)
    rule go {
        let v = regs[0].peek()
    }
}
pipe Top {
    Writer {}
    Reader { regs = Writer.regs }
}
"#;
        let (cst, errors) = parse::parse(src);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let mut diags = vec![];
        let ast = desugar(cst.unwrap(), &mut diags);
        assert!(diags.is_empty(), "desugar errors: {diags:?}");

        // Find the pipe
        let pipe = ast
            .items
            .iter()
            .find_map(|i| {
                if let Item::Pipe(p) = &i.node {
                    Some(p)
                } else {
                    None
                }
            })
            .expect("expected pipe");

        // Reader instance should have expanded bindings
        let reader = pipe
            .instances
            .iter()
            .find(|i| i.process_name.node == "Reader")
            .unwrap();
        assert_eq!(
            reader.bindings.len(),
            2,
            "expected 2 bindings, got {:?}",
            reader.bindings
        );
        assert_eq!(reader.bindings[0].port.node, "regs_0");
        assert_eq!(reader.bindings[0].target.node, "Writer.regs_0");
        assert_eq!(reader.bindings[1].port.node, "regs_1");
        assert_eq!(reader.bindings[1].target.node, "Writer.regs_1");
    }
}
