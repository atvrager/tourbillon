//! Integration tests for AsyncQueue (clock domain crossing primitive).

use tbn::diagnostics::Diagnostic;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_ok(src: &str) -> tbn::ast::SourceFile {
    let (cst, errors) = tbn::parse::parse(src);
    assert!(errors.is_empty(), "parse errors: {errors:?}");
    cst.unwrap()
}

fn elaborate_src(src: &str) -> (Vec<tbn::ir::ProcessNetwork>, Vec<Diagnostic>) {
    let (cst, errors) = tbn::parse::parse(src);
    assert!(errors.is_empty(), "parse errors: {errors:?}");
    let mut desugar_diags = vec![];
    let ast = tbn::desugar::desugar(cst.unwrap(), &mut desugar_diags);
    assert!(
        desugar_diags.is_empty(),
        "desugar errors: {desugar_diags:?}"
    );
    let (env, type_diags) = tbn::types::check(&ast);
    assert!(type_diags.is_empty(), "type errors: {type_diags:?}");
    tbn::elaborate::elaborate(&ast, &env)
}

fn check_src(src: &str) -> Result<(), tbn::diagnostics::Errors> {
    tbn::check(src, "test.tbn")
}

fn build_src(src: &str) -> Result<Vec<tbn::lower::SvFile>, tbn::diagnostics::Errors> {
    tbn::build(src, "test.tbn", None)
}

fn dot_for_src(src: &str) -> Vec<String> {
    tbn::emit_graph(src, "test.tbn").unwrap()
}

// ---------------------------------------------------------------------------
// 1. Parse: domain + AsyncQueue syntax
// ---------------------------------------------------------------------------

#[test]
fn parse_domain_decl() {
    let src = r#"
pipe Top {
    domain fast : Clock
    domain slow : Clock
}
"#;
    let ast = parse_ok(src);
    let pipe = match &ast.items[0].node {
        tbn::ast::Item::Pipe(p) => p,
        _ => panic!("expected Pipe"),
    };
    assert_eq!(pipe.domain_decls.len(), 2);
    assert_eq!(pipe.domain_decls[0].name.node, "fast");
    assert_eq!(pipe.domain_decls[1].name.node, "slow");
}

#[test]
fn parse_async_queue_decl() {
    let src = r#"
type Word = Bits 32
pipe Top {
    let aq = AsyncQueue(Word, depth = 4)
}
"#;
    let ast = parse_ok(src);
    let pipe = match &ast.items[1].node {
        tbn::ast::Item::Pipe(p) => p,
        _ => panic!("expected Pipe"),
    };
    assert_eq!(pipe.async_queue_decls.len(), 1);
    assert_eq!(pipe.async_queue_decls[0].name.node, "aq");
    assert_eq!(pipe.async_queue_decls[0].depth, Some(4));
}

// ---------------------------------------------------------------------------
// 2. Parse: instance domain annotation
// ---------------------------------------------------------------------------

#[test]
fn parse_instance_domain_annotation() {
    let src = r#"
process P {
    produces: out : Queue(Bits 32)
    rule go { out.put(0) }
}

pipe Top {
    domain fast : Clock
    let q = AsyncQueue(Bits 32, depth = 4)
    P [fast] { out = q }
}
"#;
    let ast = parse_ok(src);
    let pipe = match &ast.items[1].node {
        tbn::ast::Item::Pipe(p) => p,
        _ => panic!("expected Pipe"),
    };
    let inst = &pipe.instances[0];
    assert!(inst.domain.is_some());
    assert_eq!(inst.domain.as_ref().unwrap().node, "fast");
}

// ---------------------------------------------------------------------------
// 3. Elaborate: two-domain pipe with AsyncQueue succeeds
// ---------------------------------------------------------------------------

#[test]
fn elaborate_two_domain_async_queue() {
    let src = r#"
type Word = Bits 32

process Producer {
    produces: out : Queue(Word)
    rule go { out.put(42) }
}

process Consumer {
    consumes: inp : Queue(Word)
    rule go { let _ = inp.take() }
}

pipe Top {
    domain fast : Clock
    domain slow : Clock

    let aq = AsyncQueue(Word, depth = 4)
    Producer [fast] { out = aq }
    Consumer [slow] { inp = aq }
}
"#;
    let (networks, diags) = elaborate_src(src);
    // Filter out warnings (same-domain warnings etc.)
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.kind, tbn::diagnostics::DiagnosticKind::Error))
        .collect();
    assert!(errors.is_empty(), "errors: {errors:?}");
    assert_eq!(networks.len(), 1);
    let net = &networks[0];
    assert_eq!(net.graph.node_count(), 2);
    assert_eq!(net.graph.edge_count(), 1);
    assert_eq!(net.domains.len(), 2);
}

// ---------------------------------------------------------------------------
// 4. Elaborate error: non-power-of-2 depth
// ---------------------------------------------------------------------------

#[test]
fn elaborate_error_non_power_of_two_depth() {
    let src = r#"
type Word = Bits 32

process P {
    produces: out : Queue(Word)
    rule go { out.put(0) }
}

process C {
    consumes: inp : Queue(Word)
    rule go { let _ = inp.take() }
}

pipe Top {
    domain fast : Clock
    domain slow : Clock

    let aq = AsyncQueue(Word, depth = 3)
    P [fast] { out = aq }
    C [slow] { inp = aq }
}
"#;
    let (networks, diags) = elaborate_src(src);
    assert!(
        diags.iter().any(|d| d.message.contains("power of 2")),
        "expected power-of-2 error, got: {diags:?}"
    );
    assert!(networks.is_empty());
}

// ---------------------------------------------------------------------------
// 5. Elaborate error: sync Queue crossing domains
// ---------------------------------------------------------------------------

#[test]
fn elaborate_error_sync_queue_cross_domain() {
    let src = r#"
type Word = Bits 32

process P {
    produces: out : Queue(Word)
    rule go { out.put(0) }
}

process C {
    consumes: inp : Queue(Word)
    rule go { let _ = inp.take() }
}

pipe Top {
    domain fast : Clock
    domain slow : Clock

    let q = Queue(Word, depth = 4)
    P [fast] { out = q }
    C [slow] { inp = q }
}
"#;
    let (networks, diags) = elaborate_src(src);
    assert!(
        diags.iter().any(|d| d.message.contains("use AsyncQueue")),
        "expected cross-domain sync queue error, got: {diags:?}"
    );
    assert!(networks.is_empty());
}

// ---------------------------------------------------------------------------
// 6. Elaborate warning: AsyncQueue within same domain
// ---------------------------------------------------------------------------

#[test]
fn elaborate_warning_async_queue_same_domain() {
    let src = r#"
type Word = Bits 32

process P {
    produces: out : Queue(Word)
    rule go { out.put(0) }
}

process C {
    consumes: inp : Queue(Word)
    rule go { let _ = inp.take() }
}

pipe Top {
    domain fast : Clock

    let aq = AsyncQueue(Word, depth = 4)
    P [fast] { out = aq }
    C [fast] { inp = aq }
}
"#;
    let (networks, diags) = elaborate_src(src);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("same domain") && d.message.contains("use Queue")),
        "expected same-domain AsyncQueue warning, got: {diags:?}"
    );
    // Warnings don't prevent network creation
    assert_eq!(networks.len(), 1);
}

// ---------------------------------------------------------------------------
// 7. Elaborate error: cross-domain Cell peek
// ---------------------------------------------------------------------------

#[test]
fn elaborate_error_cross_domain_cell_peek() {
    let src = r#"
type Word = Bits 32

process Writer {
    state: reg : Cell(Word, init = 0)
    rule go {
        let v = reg.take()
        reg.put(v + 1)
    }
}

process Reader {
    peeks: reg : Cell(Word)
    rule go {
        let _ = reg.peek()
    }
}

pipe Top {
    domain fast : Clock
    domain slow : Clock

    Writer [fast] {}
    Reader [slow] { reg = Writer.reg }
}
"#;
    let (networks, diags) = elaborate_src(src);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("peek crosses domains")),
        "expected cross-domain peek error, got: {diags:?}"
    );
    assert!(networks.is_empty());
}

// ---------------------------------------------------------------------------
// 8. Deadlock: AsyncQueue edges don't cause zero-token cycle errors
// ---------------------------------------------------------------------------

#[test]
fn deadlock_async_queue_no_false_alarm() {
    // Two processes in a cycle connected by AsyncQueues.
    // Without the async-edge exemption this would report zero tokens.
    let src = r#"
type Word = Bits 32

process A {
    consumes: inp : Queue(Word)
    produces: out : Queue(Word)
    rule go {
        let v = inp.take()
        out.put(v)
    }
}

process B {
    consumes: inp : Queue(Word)
    produces: out : Queue(Word)
    rule go {
        let v = inp.take()
        out.put(v)
    }
}

pipe Top {
    domain fast : Clock
    domain slow : Clock

    let q1 = AsyncQueue(Word, depth = 4)
    let q2 = AsyncQueue(Word, depth = 4)
    A [fast] { inp = q2, out = q1 }
    B [slow] { inp = q1, out = q2 }
}
"#;
    let result = check_src(src);
    assert!(result.is_ok(), "unexpected deadlock error: {result:?}");
}

// ---------------------------------------------------------------------------
// 9. Lower: generated SV contains tbn_async_fifo instance with correct clocks
// ---------------------------------------------------------------------------

#[test]
fn lower_async_fifo_instance() {
    let src = r#"
type Word = Bits 32

process Producer {
    produces: out : Queue(Word)
    rule go { out.put(42) }
}

process Consumer {
    consumes: inp : Queue(Word)
    rule go { let _ = inp.take() }
}

pipe Top {
    domain fast : Clock
    domain slow : Clock

    let aq = AsyncQueue(Word, depth = 4)
    Producer [fast] { out = aq }
    Consumer [slow] { inp = aq }
}
"#;
    let files = build_src(src).unwrap();
    // Should have tbn_async_fifo.sv and Top.sv
    assert!(
        files.iter().any(|f| f.name == "tbn_async_fifo.sv"),
        "missing tbn_async_fifo.sv"
    );

    let top = files.iter().find(|f| f.name == "Top.sv").unwrap();
    let sv = &top.content;

    assert!(
        sv.contains("tbn_async_fifo"),
        "missing tbn_async_fifo instantiation"
    );
    assert!(
        sv.contains(".wr_clk(fast_clk)"),
        "missing wr_clk wiring: {sv}"
    );
    assert!(
        sv.contains(".rd_clk(slow_clk)"),
        "missing rd_clk wiring: {sv}"
    );
    // Each side uses its domain-specific reset.
    assert!(
        sv.contains(".wr_rst_n(fast_rst_n)"),
        "wr_rst_n should use source domain reset: {sv}"
    );
    assert!(
        sv.contains(".rd_rst_n(slow_rst_n)"),
        "rd_rst_n should use dest domain reset: {sv}"
    );
}

// ---------------------------------------------------------------------------
// 10. Lower: module ports include per-domain clk/rst_n
// ---------------------------------------------------------------------------

#[test]
fn lower_per_domain_ports() {
    let src = r#"
type Word = Bits 32

process Producer {
    produces: out : Queue(Word)
    rule go { out.put(42) }
}

process Consumer {
    consumes: inp : Queue(Word)
    rule go { let _ = inp.take() }
}

pipe Top {
    domain fast : Clock
    domain slow : Clock

    let aq = AsyncQueue(Word, depth = 4)
    Producer [fast] { out = aq }
    Consumer [slow] { inp = aq }
}
"#;
    let files = build_src(src).unwrap();
    let top = files.iter().find(|f| f.name == "Top.sv").unwrap();
    let sv = &top.content;

    assert!(sv.contains("fast_clk"), "missing fast_clk port");
    assert!(sv.contains("fast_rst_n"), "missing fast_rst_n port");
    assert!(sv.contains("slow_clk"), "missing slow_clk port");
    assert!(sv.contains("slow_rst_n"), "missing slow_rst_n port");
}

// ---------------------------------------------------------------------------
// 11. DOT: async edges styled distinctly
// ---------------------------------------------------------------------------

#[test]
fn dot_async_queue_styling() {
    let src = r#"
type Word = Bits 32

process Producer {
    produces: out : Queue(Word)
    rule go { out.put(42) }
}

process Consumer {
    consumes: inp : Queue(Word)
    rule go { let _ = inp.take() }
}

pipe Top {
    domain fast : Clock
    domain slow : Clock

    let aq = AsyncQueue(Word, depth = 4)
    Producer [fast] { out = aq }
    Consumer [slow] { inp = aq }
}
"#;
    let dots = dot_for_src(src);
    assert_eq!(dots.len(), 1);
    let dot = &dots[0];

    assert!(
        dot.contains("color=red"),
        "missing red color for async edge"
    );
    assert!(dot.contains("cluster_fast"), "missing fast domain subgraph");
    assert!(dot.contains("cluster_slow"), "missing slow domain subgraph");
}

// ---------------------------------------------------------------------------
// 12. Backward compat: no domains = identical to current output
// ---------------------------------------------------------------------------

#[test]
fn backward_compat_no_domains() {
    let src = r#"
process Counter {
    state: count : Cell(Bits 32, init = 0)
    rule tick {
        let c = count.take()
        count.put(c + 1)
    }
}

pipe Top {
    Counter {}
}
"#;
    let files = build_src(src).unwrap();
    let sv = &files[0].content;
    // Should still use clk/rst_n, not per-domain
    assert!(sv.contains("input  wire clk"), "missing clk port");
    assert!(sv.contains("input  wire rst_n"), "missing rst_n port");
    assert!(!sv.contains("_clk"), "unexpected domain clock");
}

// ---------------------------------------------------------------------------
// 13. AsyncQueue type expression in port declarations
// ---------------------------------------------------------------------------

#[test]
fn parse_async_queue_type_expr() {
    let src = r#"
type Word = Bits 32

process P {
    consumes: inp : AsyncQueue(Word, depth = 8)
    rule go { let _ = inp.take() }
}
"#;
    let ast = parse_ok(src);
    if let tbn::ast::Item::Process(p) = &ast.items[1].node {
        match &p.ports[0].ty.node {
            tbn::ast::TypeExpr::AsyncQueue { depth, .. } => {
                assert_eq!(*depth, Some(8));
            }
            other => panic!("expected AsyncQueue type expr, got: {other:?}"),
        }
    } else {
        panic!("expected Process");
    }
}
