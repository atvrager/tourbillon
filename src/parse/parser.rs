use chumsky::{input::ValueInput, prelude::*};

use crate::ast::*;

use super::token::Token;

type Span = SimpleSpan;

/// Helper: wrap a value with the span from the parser.
fn spn<T>(node: T, span: Span) -> Spanned<T> {
    Spanned::new(node, span.start..span.end)
}

// ---------------------------------------------------------------------------
// Type expressions
// ---------------------------------------------------------------------------

fn type_expr_parser<'src, I>()
-> impl Parser<'src, I, Spanned<TypeExpr>, extra::Err<Rich<'src, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token<'src>, Span = Span>,
{
    recursive(|type_expr| {
        let queue_type = just(Token::Queue)
            .ignore_then(just(Token::LParen))
            .ignore_then(type_expr.clone())
            .then(
                just(Token::Comma)
                    .ignore_then(just(Token::Depth))
                    .ignore_then(just(Token::Assign))
                    .ignore_then(select! { Token::Int(n) => n as u64 })
                    .or_not(),
            )
            .then_ignore(just(Token::RParen))
            .map(|(elem, depth)| TypeExpr::Queue {
                elem: Box::new(elem),
                depth,
            });

        let async_queue_type = just(Token::AsyncQueue)
            .ignore_then(just(Token::LParen))
            .ignore_then(type_expr.clone())
            .then(
                just(Token::Comma)
                    .ignore_then(just(Token::Depth))
                    .ignore_then(just(Token::Assign))
                    .ignore_then(select! { Token::Int(n) => n as u64 })
                    .or_not(),
            )
            .then_ignore(just(Token::RParen))
            .map(|(elem, depth)| TypeExpr::AsyncQueue {
                elem: Box::new(elem),
                depth,
            });

        let cell_type = just(Token::Cell)
            .ignore_then(just(Token::LParen))
            .ignore_then(type_expr.clone())
            .then(
                just(Token::Comma)
                    .ignore_then(just(Token::Init))
                    .ignore_then(just(Token::Assign))
                    .ignore_then(
                        select! { Token::Int(n) => Expr::Lit(Literal::Int(n)) }
                            .or(select! { Token::Ident(s) => Expr::Var(s.to_string()) })
                            .map_with(|e, extra| spn(e, extra.span())),
                    )
                    .map(Box::new)
                    .or_not(),
            )
            .then_ignore(just(Token::RParen))
            .map(|(elem, init)| TypeExpr::Cell {
                elem: Box::new(elem),
                init,
            });

        let named_with_parens = select! { Token::Ident(s) => s }
            .then(
                type_expr
                    .clone()
                    .separated_by(just(Token::Comma))
                    .at_least(1)
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .map(|(name, args)| TypeExpr::Named {
                name: name.to_string(),
                args,
            });

        let named_with_int = select! { Token::Ident(s) => s }
            .then(select! { Token::Int(n) => n as u64 }.map_with(|n, e| {
                spn(
                    TypeExpr::Named {
                        name: format!("{n}"),
                        args: vec![],
                    },
                    e.span(),
                )
            }))
            .map(|(name, arg)| TypeExpr::Named {
                name: name.to_string(),
                args: vec![arg],
            });

        let named_bare = select! { Token::Ident(s) => s }.map(|name: &str| TypeExpr::Named {
            name: name.to_string(),
            args: vec![],
        });

        // Integer literals as type expressions (e.g. 32 in Array(32, Word))
        let int_as_type = select! { Token::Int(n) => n as u64 }.map(|n| TypeExpr::Named {
            name: format!("{n}"),
            args: vec![],
        });

        let paren_type = type_expr
            .clone()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(|s: Spanned<TypeExpr>| s.node);

        let atom = choice((
            queue_type,
            async_queue_type,
            cell_type,
            paren_type,
            named_with_parens,
            named_with_int,
            int_as_type,
            named_bare,
        ))
        .map_with(|node, e| spn(node, e.span()));

        // Product types: A × B × C
        atom.clone()
            .foldl(
                just(Token::Times).ignore_then(atom).repeated(),
                |a: Spanned<TypeExpr>, b: Spanned<TypeExpr>| {
                    let span_start = a.span.start;
                    let span_end = b.span.end;
                    let items = match a.node {
                        TypeExpr::Product(mut v) => {
                            v.push(b);
                            v
                        }
                        _ => vec![a, b],
                    };
                    Spanned::new(TypeExpr::Product(items), span_start..span_end)
                },
            )
            .boxed()
    })
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

fn pattern_parser<'src, I>()
-> impl Parser<'src, I, Spanned<Pattern>, extra::Err<Rich<'src, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token<'src>, Span = Span>,
{
    recursive(|pattern| {
        let wildcard = just(Token::Underscore).to(Pattern::Wildcard);
        let lit_pat = select! { Token::Int(n) => Pattern::Literal(Literal::Int(n)) };

        let tuple_pat = pattern
            .clone()
            .separated_by(just(Token::Comma))
            .at_least(2)
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(Pattern::Tuple);

        let some_variant = just(Token::SomeKw)
            .ignore_then(
                pattern
                    .clone()
                    .delimited_by(just(Token::LParen), just(Token::RParen))
                    .map(|p| vec![p])
                    .or_not(),
            )
            .map(|fields| Pattern::Variant {
                name: "Some".to_string(),
                fields: fields.unwrap_or_default(),
            });

        let none_variant = just(Token::NoneKw).to(Pattern::Variant {
            name: "None".to_string(),
            fields: vec![],
        });

        let variant_with_fields = select! { Token::Ident(s) => s }
            .then(
                pattern
                    .clone()
                    .separated_by(just(Token::Comma))
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .map(|(name, fields): (&str, _)| Pattern::Variant {
                name: name.to_string(),
                fields,
            });

        let ident_pat = select! { Token::Ident(s) => s }.map(|s: &str| {
            if s.starts_with(char::is_uppercase) {
                Pattern::Variant {
                    name: s.to_string(),
                    fields: vec![],
                }
            } else {
                Pattern::Bind(s.to_string())
            }
        });

        choice((
            wildcard,
            lit_pat,
            tuple_pat,
            some_variant,
            none_variant,
            variant_with_fields,
            ident_pat,
        ))
        .map_with(|node, e| spn(node, e.span()))
        .boxed()
    })
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

fn expr_parser<'src, I>()
-> impl Parser<'src, I, Spanned<Expr>, extra::Err<Rich<'src, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token<'src>, Span = Span>,
{
    recursive(|expr| {
        let ident = select! { Token::Ident(s) => s };

        let int_lit = select! { Token::Int(n) => Expr::Lit(Literal::Int(n)) }
            .map_with(|e, extra| spn(e, extra.span()));

        let bool_lit = choice((
            just(Token::True).to(Expr::Lit(Literal::Bool(true))),
            just(Token::False).to(Expr::Lit(Literal::Bool(false))),
        ))
        .map_with(|e, extra| spn(e, extra.span()));

        let some_expr = just(Token::SomeKw)
            .ignore_then(
                expr.clone()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .map(|inner| Expr::Call {
                func: "Some".to_string(),
                args: vec![inner],
            })
            .map_with(|e, extra| spn(e, extra.span()));

        let none_expr = just(Token::NoneKw)
            .to(Expr::Var("None".to_string()))
            .map_with(|e, extra| spn(e, extra.span()));

        let paren_or_tuple = expr
            .clone()
            .separated_by(just(Token::Comma))
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map_with(|mut items: Vec<Spanned<Expr>>, e| {
                let span = e.span();
                if items.len() == 1 {
                    items.pop().unwrap()
                } else {
                    spn(Expr::Tuple(items), span)
                }
            });

        let record_field = select! { Token::Ident(s) => s }
            .map_with(|s: &str, e| spn(s.to_string(), e.span()))
            .then_ignore(just(Token::Assign))
            .then(expr.clone());

        let record_expr = select! { Token::Ident(s) => s }
            .then(
                record_field
                    .separated_by(just(Token::Comma).or_not())
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            )
            .map_with(|(name, fields): (&str, _), e| {
                spn(
                    Expr::Record {
                        name: name.to_string(),
                        fields,
                    },
                    e.span(),
                )
            });

        let call_expr = select! { Token::Ident(s) => s }
            .then(
                expr.clone()
                    .separated_by(just(Token::Comma))
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .map_with(|(name, args): (&str, _), e| {
                spn(
                    Expr::Call {
                        func: name.to_string(),
                        args,
                    },
                    e.span(),
                )
            });

        let var_expr = ident
            .map(|s: &str| Expr::Var(s.to_string()))
            .map_with(|e, extra| spn(e, extra.span()));

        let atom = choice((
            int_lit,
            bool_lit,
            some_expr,
            none_expr,
            paren_or_tuple,
            record_expr,
            call_expr,
            var_expr,
        ))
        .boxed();

        // Postfix: .field, .method(args), [index], [i := v]
        let dot_postfix = just(Token::Dot)
            .ignore_then(ident.map_with(|s: &str, e| spn(s.to_string(), e.span())))
            .then(
                expr.clone()
                    .separated_by(just(Token::Comma))
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LParen), just(Token::RParen))
                    .or_not(),
            );

        // Bit slice: [int : int]
        let bit_slice_postfix = just(Token::LBrack)
            .ignore_then(select! { Token::Int(hi) => hi as u64 })
            .then_ignore(just(Token::Colon))
            .then(select! { Token::Int(lo) => lo as u64 })
            .then_ignore(just(Token::RBrack));

        let index_postfix = expr
            .clone()
            .then(just(Token::ColonEq).ignore_then(expr.clone()).or_not())
            .delimited_by(just(Token::LBrack), just(Token::RBrack));

        #[derive(Clone)]
        enum Postfix {
            Dot(Spanned<String>, Option<Vec<Spanned<Expr>>>),
            Index(Spanned<Expr>, Option<Spanned<Expr>>),
            BitSlice(u64, u64),
        }

        let postfix_op = choice((
            dot_postfix.map(|(name, args)| Postfix::Dot(name, args)),
            bit_slice_postfix.map(|(hi, lo)| Postfix::BitSlice(hi, lo)),
            index_postfix.map(|(idx, upd)| Postfix::Index(idx, upd)),
        ));

        let postfix = atom
            .foldl(postfix_op.repeated(), |lhs: Spanned<Expr>, op| match op {
                Postfix::Dot(name, Some(args)) => {
                    let end = args.last().map_or(name.span.end, |a| a.span.end);
                    Spanned::new(
                        Expr::MethodCall {
                            receiver: Box::new(lhs.clone()),
                            method: name,
                            args,
                        },
                        lhs.span.start..end,
                    )
                }
                Postfix::Dot(name, None) => {
                    let span = lhs.span.start..name.span.end;
                    Spanned::new(
                        Expr::FieldAccess {
                            expr: Box::new(lhs),
                            field: name,
                        },
                        span,
                    )
                }
                Postfix::Index(index, Some(value)) => {
                    let span = lhs.span.start..value.span.end;
                    Spanned::new(
                        Expr::Update {
                            expr: Box::new(lhs),
                            index: Box::new(index),
                            value: Box::new(value),
                        },
                        span,
                    )
                }
                Postfix::Index(index, None) => {
                    let span = lhs.span.start..index.span.end;
                    Spanned::new(
                        Expr::Index {
                            expr: Box::new(lhs),
                            index: Box::new(index),
                        },
                        span,
                    )
                }
                Postfix::BitSlice(hi, lo) => {
                    let span = lhs.span.start..lhs.span.end + 10; // approximate
                    Spanned::new(
                        Expr::BitSlice {
                            expr: Box::new(lhs),
                            hi,
                            lo,
                        },
                        span,
                    )
                }
            })
            .boxed();

        // Unary: ! -
        let unary = choice((
            just(Token::Bang).to(UnaryOp::Not),
            just(Token::Minus).to(UnaryOp::Neg),
        ))
        .map_with(|op, e| (op, e.span()))
        .repeated()
        .foldr(
            postfix,
            |(op, op_span): (UnaryOp, Span), rhs: Spanned<Expr>| {
                let span = op_span.start..rhs.span.end;
                Spanned::new(
                    Expr::UnaryOp {
                        op,
                        expr: Box::new(rhs),
                    },
                    span,
                )
            },
        )
        .boxed();

        // Binary operators by precedence (low to high)
        let mk_binop = |lhs: Spanned<Expr>, (op, rhs): (BinOp, Spanned<Expr>)| -> Spanned<Expr> {
            let span = lhs.span.start..rhs.span.end;
            Spanned::new(
                Expr::BinOp {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            )
        };

        let product = unary
            .clone()
            .foldl(
                just(Token::Star).to(BinOp::Mul).then(unary).repeated(),
                mk_binop,
            )
            .boxed();
        let sum = product
            .clone()
            .foldl(
                choice((
                    just(Token::Plus).to(BinOp::Add),
                    just(Token::Minus).to(BinOp::Sub),
                ))
                .then(product)
                .repeated(),
                mk_binop,
            )
            .boxed();
        let shift = sum
            .clone()
            .foldl(
                choice((
                    just(Token::Shl).to(BinOp::Shl),
                    just(Token::Shr).to(BinOp::Shr),
                ))
                .then(sum)
                .repeated(),
                mk_binop,
            )
            .boxed();
        let bitand = shift
            .clone()
            .foldl(
                just(Token::Ampersand).to(BinOp::And).then(shift).repeated(),
                mk_binop,
            )
            .boxed();
        let bitxor = bitand
            .clone()
            .foldl(
                just(Token::Caret).to(BinOp::Xor).then(bitand).repeated(),
                mk_binop,
            )
            .boxed();
        let bitor = bitxor
            .clone()
            .foldl(
                just(Token::Pipe_).to(BinOp::Or).then(bitxor).repeated(),
                mk_binop,
            )
            .boxed();
        let comparison = bitor
            .clone()
            .foldl(
                choice((
                    just(Token::Eq).to(BinOp::Eq),
                    just(Token::Neq).to(BinOp::Neq),
                    just(Token::Le).to(BinOp::Le),
                    just(Token::Ge).to(BinOp::Ge),
                    just(Token::Lt).to(BinOp::Lt),
                    just(Token::Gt).to(BinOp::Gt),
                ))
                .then(bitor)
                .repeated(),
                mk_binop,
            )
            .boxed();
        let logical_and = comparison
            .clone()
            .foldl(
                just(Token::LogicalAnd)
                    .to(BinOp::LogicalAnd)
                    .then(comparison)
                    .repeated(),
                mk_binop,
            )
            .boxed();
        logical_and
            .clone()
            .foldl(
                just(Token::LogicalOr)
                    .to(BinOp::LogicalOr)
                    .then(logical_and)
                    .repeated(),
                mk_binop,
            )
            .boxed()
    })
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

fn stmt_parser<'src, I>()
-> impl Parser<'src, I, Spanned<Stmt>, extra::Err<Rich<'src, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token<'src>, Span = Span>,
{
    recursive(|stmt| {
        let expr = expr_parser();

        let block = stmt
            .clone()
            .repeated()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBrace), just(Token::RBrace));

        let let_stmt = just(Token::Let)
            .ignore_then(pattern_parser())
            .then_ignore(just(Token::Assign))
            .then(expr.clone())
            .map(|(pattern, value)| Stmt::Let { pattern, value });

        let match_arm = pattern_parser()
            .then_ignore(just(Token::Arrow))
            .then(block.clone().or(stmt.clone().map(|s| vec![s])))
            .map(|(pattern, body)| MatchArm { pattern, body });

        let match_stmt = just(Token::Match)
            .ignore_then(expr.clone())
            .then(
                match_arm
                    .separated_by(just(Token::Comma).or_not())
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            )
            .map(|(scrutinee, arms)| Stmt::Match { scrutinee, arms });

        let if_stmt = just(Token::If)
            .ignore_then(expr.clone())
            .then(block.clone())
            .then(just(Token::Else).ignore_then(block).or_not())
            .map(|((cond, then_body), else_body)| Stmt::If {
                cond,
                then_body,
                else_body: else_body.unwrap_or_default(),
            });

        let expr_stmt = expr.map(Stmt::Expr);

        choice((let_stmt, match_stmt, if_stmt, expr_stmt))
            .map_with(|node, e| spn(node, e.span()))
            .boxed()
    })
}

// ---------------------------------------------------------------------------
// Top-level: Process, Pipe, Type definitions
// ---------------------------------------------------------------------------

fn port_parser<'src, I>()
-> impl Parser<'src, I, Port, extra::Err<Rich<'src, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token<'src>, Span = Span>,
{
    let port_kind = choice((
        just(Token::Consumes).to(PortKind::Consumes),
        just(Token::Produces).to(PortKind::Produces),
        just(Token::State).to(PortKind::State),
        just(Token::Peeks).to(PortKind::Peeks),
    ));

    port_kind
        .then_ignore(just(Token::Colon))
        .then(select! { Token::Ident(s) => s }.map_with(|s: &str, e| spn(s.to_string(), e.span())))
        .then(
            select! { Token::Int(n) => n as u64 }
                .delimited_by(just(Token::LBrack), just(Token::RBrack))
                .or_not(),
        )
        .then_ignore(just(Token::Colon))
        .then(type_expr_parser())
        .map(|(((kind, name), array_size), ty)| Port {
            kind,
            name,
            ty,
            array_size,
        })
}

fn rule_parser<'src, I>()
-> impl Parser<'src, I, Rule, extra::Err<Rich<'src, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token<'src>, Span = Span>,
{
    just(Token::Rule)
        .ignore_then(
            select! { Token::Ident(s) => s }.map_with(|s: &str, e| spn(s.to_string(), e.span())),
        )
        .then(
            stmt_parser()
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, body)| Rule { name, body })
}

#[derive(Clone)]
enum ProcessItem {
    Port(Port),
    Rule(Rule),
}

fn process_parser<'src, I>()
-> impl Parser<'src, I, Process, extra::Err<Rich<'src, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token<'src>, Span = Span>,
{
    just(Token::Process)
        .ignore_then(
            select! { Token::Ident(s) => s }.map_with(|s: &str, e| spn(s.to_string(), e.span())),
        )
        .then(
            choice((
                port_parser().map(ProcessItem::Port),
                rule_parser().map(ProcessItem::Rule),
            ))
            .repeated()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, items)| {
            let mut ports = vec![];
            let mut rules = vec![];
            for item in items {
                match item {
                    ProcessItem::Port(p) => ports.push(p),
                    ProcessItem::Rule(r) => rules.push(r),
                }
            }
            Process { name, ports, rules }
        })
}

#[derive(Clone)]
enum PipeItem {
    Queue(QueueDecl),
    Memory(MemoryDecl),
    AsyncQueue(AsyncQueueDecl),
    Domain(DomainDecl),
    Instance(Instance),
}

fn pipe_parser<'src, I>()
-> impl Parser<'src, I, Pipe, extra::Err<Rich<'src, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token<'src>, Span = Span>,
{
    let ident_spanned =
        select! { Token::Ident(s) => s }.map_with(|s: &str, e| spn(s.to_string(), e.span()));

    let queue_decl = just(Token::Let)
        .ignore_then(ident_spanned)
        .then_ignore(just(Token::Assign))
        .then(just(Token::External).or_not())
        .then_ignore(just(Token::Queue))
        .then_ignore(just(Token::LParen))
        .then(type_expr_parser())
        .then(
            just(Token::Comma)
                .ignore_then(just(Token::Depth))
                .ignore_then(just(Token::Assign))
                .ignore_then(select! { Token::Int(n) => n as u64 })
                .or_not(),
        )
        .then(
            just(Token::Comma)
                .ignore_then(just(Token::Init))
                .ignore_then(just(Token::Assign))
                .ignore_then(select! { Token::Int(n) => n })
                .or_not(),
        )
        .then_ignore(just(Token::RParen))
        .map(|((((name, ext), ty), depth), init_tokens)| QueueDecl {
            name,
            ty,
            depth,
            init_tokens,
            is_external: ext.is_some(),
        });

    // Memory(K → V, depth = N, latency = M)
    let memory_decl = just(Token::Let)
        .ignore_then(ident_spanned)
        .then_ignore(just(Token::Assign))
        .then_ignore(just(Token::Memory))
        .then_ignore(just(Token::LParen))
        .then(type_expr_parser())
        .then_ignore(just(Token::MapsTo))
        .then(type_expr_parser())
        .then(
            just(Token::Comma)
                .ignore_then(just(Token::Depth))
                .ignore_then(just(Token::Assign))
                .ignore_then(select! { Token::Int(n) => n as u64 }),
        )
        .then(
            just(Token::Comma)
                .ignore_then(just(Token::Latency))
                .ignore_then(just(Token::Assign))
                .ignore_then(select! { Token::Int(n) => n as u64 }),
        )
        .then_ignore(just(Token::RParen))
        .map(|((((name, key_ty), val_ty), depth), latency)| MemoryDecl {
            name,
            key_ty,
            val_ty,
            depth,
            latency,
        });

    // AsyncQueue declaration: let <ident> = AsyncQueue(<type>, depth = <int>)
    let async_queue_decl = just(Token::Let)
        .ignore_then(ident_spanned)
        .then_ignore(just(Token::Assign))
        .then_ignore(just(Token::AsyncQueue))
        .then_ignore(just(Token::LParen))
        .then(type_expr_parser())
        .then(
            just(Token::Comma)
                .ignore_then(just(Token::Depth))
                .ignore_then(just(Token::Assign))
                .ignore_then(select! { Token::Int(n) => n as u64 })
                .or_not(),
        )
        .then_ignore(just(Token::RParen))
        .map(|((name, ty), depth)| AsyncQueueDecl { name, ty, depth });

    // Domain declaration: domain <ident> : Clock
    let domain_decl = just(Token::Domain)
        .ignore_then(ident_spanned)
        .then_ignore(just(Token::Colon))
        .then_ignore(just(Token::Clock))
        .map(|name| DomainDecl { name });

    let port_binding = ident_spanned
        .then_ignore(just(Token::Assign))
        .then(
            select! { Token::Ident(s) => s }
                .separated_by(just(Token::Dot))
                .at_least(1)
                .collect::<Vec<_>>()
                .map_with(|parts: Vec<&str>, e| spn(parts.join("."), e.span())),
        )
        .map(|(port, target)| PortBinding { port, target });

    // Instance with optional domain annotation: Name [domain] { ... }
    let instance = ident_spanned
        .then(
            select! { Token::Ident(s) => s }
                .map_with(|s: &str, e| spn(s.to_string(), e.span()))
                .delimited_by(just(Token::LBrack), just(Token::RBrack))
                .or_not(),
        )
        .then(
            port_binding
                .separated_by(just(Token::Comma).or_not())
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|((process_name, domain), bindings)| Instance {
            process_name,
            domain,
            bindings,
        });

    just(Token::Pipe)
        .ignore_then(ident_spanned)
        .then(
            choice((
                memory_decl.map(PipeItem::Memory),
                async_queue_decl.map(PipeItem::AsyncQueue),
                queue_decl.map(PipeItem::Queue),
                domain_decl.map(PipeItem::Domain),
                instance.map(PipeItem::Instance),
            ))
            .repeated()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, items)| {
            let mut queue_decls = vec![];
            let mut memory_decls = vec![];
            let mut async_queue_decls = vec![];
            let mut domain_decls = vec![];
            let mut instances = vec![];
            for item in items {
                match item {
                    PipeItem::Queue(q) => queue_decls.push(q),
                    PipeItem::Memory(m) => memory_decls.push(m),
                    PipeItem::AsyncQueue(aq) => async_queue_decls.push(aq),
                    PipeItem::Domain(d) => domain_decls.push(d),
                    PipeItem::Instance(i) => instances.push(i),
                }
            }
            Pipe {
                name,
                queue_decls,
                memory_decls,
                async_queue_decls,
                domain_decls,
                instances,
            }
        })
}

fn type_def_parser<'src, I>()
-> impl Parser<'src, I, TypeDef, extra::Err<Rich<'src, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token<'src>, Span = Span>,
{
    let ident_spanned =
        select! { Token::Ident(s) => s }.map_with(|s: &str, e| spn(s.to_string(), e.span()));

    let alias = just(Token::Type)
        .ignore_then(ident_spanned)
        .then_ignore(just(Token::Assign))
        .then(type_expr_parser())
        .map(|(name, ty)| TypeDef {
            name,
            kind: TypeDefKind::Alias(ty),
        });

    let field = ident_spanned
        .then_ignore(just(Token::Colon))
        .then(type_expr_parser())
        .map(|(name, ty)| Field { name, ty });

    let record = just(Token::Record)
        .ignore_then(ident_spanned)
        .then(
            field
                .separated_by(just(Token::Comma).or_not())
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, fields)| TypeDef {
            name,
            kind: TypeDefKind::Record(fields),
        });

    let variant = select! { Token::Ident(s) => s }
        .map_with(|s: &str, e| spn(s.to_string(), e.span()))
        .then(
            type_expr_parser()
                .separated_by(just(Token::Comma))
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LParen), just(Token::RParen))
                .or_not(),
        )
        .map(|(name, fields)| Variant {
            name,
            fields: fields.unwrap_or_default(),
        });

    let none_variant = just(Token::NoneKw).map_with(|_, e| Variant {
        name: spn("None".to_string(), e.span()),
        fields: vec![],
    });

    let some_variant = just(Token::SomeKw)
        .map_with(|_, e| spn("Some".to_string(), e.span()))
        .then(
            type_expr_parser()
                .separated_by(just(Token::Comma))
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LParen), just(Token::RParen))
                .or_not(),
        )
        .map(|(name, fields)| Variant {
            name,
            fields: fields.unwrap_or_default(),
        });

    let any_variant = choice((none_variant, some_variant, variant));

    let enum_def = just(Token::Enum)
        .ignore_then(ident_spanned)
        .then_ignore(just(Token::Assign))
        .then(
            any_variant
                .separated_by(just(Token::Pipe_))
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .map(|(name, variants)| TypeDef {
            name,
            kind: TypeDefKind::Enum(variants),
        });

    choice((record, enum_def, alias))
}

/// Top-level parser: produces a SourceFile from a token stream.
pub fn source_file_parser<'src, I>()
-> impl Parser<'src, I, SourceFile, extra::Err<Rich<'src, Token<'src>, Span>>>
where
    I: ValueInput<'src, Token = Token<'src>, Span = Span>,
{
    let ident_sp =
        select! { Token::Ident(s) => s }.map_with(|s: &str, e| spn(s.to_string(), e.span()));

    // const NAME = <int>
    let const_def = just(Token::Const)
        .ignore_then(ident_sp)
        .then_ignore(just(Token::Assign))
        .then(select! { Token::Int(n) => n as u64 })
        .map(|(name, value)| Item::Const(ConstDef { name, value }));

    // external fn name(params) [-> RetTy]
    let ext_param = ident_sp
        .then_ignore(just(Token::Colon))
        .then(type_expr_parser());

    let external_fn_def = just(Token::External)
        .ignore_then(just(Token::Fn))
        .ignore_then(ident_sp)
        .then(
            ext_param
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .then(just(Token::MapsTo).ignore_then(type_expr_parser()).or_not())
        .map(|((name, params), return_ty)| {
            Item::ExternalFn(ExternalFnDef {
                name,
                params,
                return_ty,
            })
        });

    let item = choice((
        const_def,
        external_fn_def,
        process_parser().map(Item::Process),
        pipe_parser().map(Item::Pipe),
        type_def_parser().map(Item::TypeDef),
    ))
    .map_with(|node, e| spn(node, e.span()));

    item.repeated()
        .collect::<Vec<_>>()
        .then_ignore(end())
        .map(|items| SourceFile { items })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::lexer;

    fn parse_str(src: &str) -> SourceFile {
        let (tokens, lex_errs) = lexer::lexer().parse(src).into_output_errors();
        assert!(lex_errs.is_empty(), "lex errors: {lex_errs:?}");
        let tokens = tokens.unwrap();
        let eoi = SimpleSpan::from(src.len()..src.len());
        let (ast, parse_errs) = source_file_parser()
            .parse(tokens.as_slice().map(eoi, |(t, s)| (t, s)))
            .into_output_errors();
        assert!(parse_errs.is_empty(), "parse errors: {parse_errs:?}");
        ast.unwrap()
    }

    #[test]
    fn parse_type_alias() {
        let ast = parse_str("type Word = Bits 32");
        assert_eq!(ast.items.len(), 1);
        match &ast.items[0].node {
            Item::TypeDef(td) => assert_eq!(td.name.node, "Word"),
            _ => panic!("expected TypeDef"),
        }
    }

    #[test]
    fn parse_record() {
        let ast = parse_str("record Foo { x : Bits 32 y : Bool }");
        assert_eq!(ast.items.len(), 1);
        match &ast.items[0].node {
            Item::TypeDef(td) => match &td.kind {
                TypeDefKind::Record(fields) => assert_eq!(fields.len(), 2),
                _ => panic!("expected Record"),
            },
            _ => panic!("expected TypeDef"),
        }
    }

    #[test]
    fn parse_enum() {
        let ast = parse_str("enum MemOp = Load | Store | None");
        match &ast.items[0].node {
            Item::TypeDef(td) => match &td.kind {
                TypeDefKind::Enum(variants) => assert_eq!(variants.len(), 3),
                _ => panic!("expected Enum"),
            },
            _ => panic!("expected TypeDef"),
        }
    }

    #[test]
    fn parse_simple_process() {
        let src = r#"
process Counter {
    state: count : Cell(Bits 32, init = 0)

    rule tick {
        let c = count.take()
        count.put(c + 1)
    }
}
"#;
        let ast = parse_str(src);
        assert_eq!(ast.items.len(), 1);
        match &ast.items[0].node {
            Item::Process(p) => {
                assert_eq!(p.name.node, "Counter");
                assert_eq!(p.ports.len(), 1);
                assert_eq!(p.rules.len(), 1);
                assert_eq!(p.rules[0].name.node, "tick");
                assert_eq!(p.rules[0].body.len(), 2);
            }
            _ => panic!("expected Process"),
        }
    }

    #[test]
    fn parse_simple_pipe() {
        let src = r#"
pipe Top {
    let q = Queue(Bits 32, depth = 4)
    Producer { out = q }
    Consumer { input = q }
}
"#;
        let ast = parse_str(src);
        assert_eq!(ast.items.len(), 1);
        match &ast.items[0].node {
            Item::Pipe(p) => {
                assert_eq!(p.name.node, "Top");
                assert_eq!(p.queue_decls.len(), 1);
                assert_eq!(p.instances.len(), 2);
            }
            _ => panic!("expected Pipe"),
        }
    }

    #[test]
    fn parse_match_stmt() {
        let src = r#"
process Foo {
    consumes: input : Queue(Bits 32)

    rule go {
        let x = input.take()
        match x {
            0 => input.put(1)
            _ => input.put(x)
        }
    }
}
"#;
        let ast = parse_str(src);
        assert_eq!(ast.items.len(), 1);
    }

    #[test]
    fn parse_if_stmt() {
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
        let ast = parse_str(src);
        assert_eq!(ast.items.len(), 1);
    }

    #[test]
    fn parse_expressions() {
        let src = r#"
process Foo {
    state: x : Cell(Bits 32, init = 0)

    rule go {
        let a = x.take()
        let b = a + 1 * 2
        let c = !b
        let d = a == b && c != 0
        x.put(b)
    }
}
"#;
        let ast = parse_str(src);
        assert_eq!(ast.items.len(), 1);
    }
}
