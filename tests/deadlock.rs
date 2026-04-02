/// Integration tests for deadlock analysis and DOT graph export.

fn check_src(src: &str) -> Result<(), tbn::diagnostics::Errors> {
    tbn::check(src, "test.tbn")
}

fn graph_src(src: &str) -> Vec<String> {
    tbn::emit_graph(src, "test.tbn").unwrap()
}

// ---------------------------------------------------------------------------
// Deadlock analysis (via check)
// ---------------------------------------------------------------------------

#[test]
fn rv32i_no_deadlock_warnings() {
    let core = std::fs::read_to_string("examples/cpu_core.tbn").unwrap();
    let rv32i = std::fs::read_to_string("examples/rv32i.tbn").unwrap();
    let src = format!("{core}\n\n{rv32i}");
    // The RV32I pipeline uses a non-speculative next_pc queue with init — should pass clean.
    check_src(&src).unwrap();
}

#[test]
fn simple_counter_no_deadlock() {
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
    check_src(src).unwrap();
}

#[test]
fn linear_pipeline_no_deadlock() {
    let src = r#"
process Src {
    produces: out : Queue(Bits 32)
    rule go { out.put(1) }
}

process Sink {
    consumes: inp : Queue(Bits 32)
    rule go { let _ = inp.take() }
}

pipe Top {
    let q = Queue(Bits 32, depth = 4)
    Src { out = q }
    Sink { inp = q }
}
"#;
    check_src(src).unwrap();
}

// ---------------------------------------------------------------------------
// DOT graph export
// ---------------------------------------------------------------------------

#[test]
fn dot_rv32i_structure() {
    let core = std::fs::read_to_string("examples/cpu_core.tbn").unwrap();
    let rv32i = std::fs::read_to_string("examples/rv32i.tbn").unwrap();
    let src = format!("{core}\n\n{rv32i}");
    let dots = graph_src(&src);
    // Two pipes: CPUCore and CPU — find the CPU top-level graph
    assert!(dots.len() >= 1, "expected at least one DOT graph");
    let dot = dots
        .iter()
        .find(|d| d.starts_with("digraph CPU {"))
        .expect("missing CPU graph");

    // All four pipeline stages present (prefixed with CPUCore_ from pipe hierarchy)
    assert!(dot.contains("CPUCore_Fetch"), "missing Fetch");
    assert!(dot.contains("CPUCore_Decode"), "missing Decode");
    assert!(dot.contains("CPUCore_Execute"), "missing Execute");
    assert!(dot.contains("CPUCore_Writeback"), "missing Writeback");

    // Key interconnects
    assert!(dot.contains("CPUCore_fetch_q"), "missing fetch_q edge");
    assert!(dot.contains("CPUCore_decode_q"), "missing decode_q edge");
    assert!(dot.contains("CPUCore_wb_q"), "missing wb_q edge");
    assert!(
        dot.contains("CPUCore_next_pcc_q"),
        "missing next_pcc_q edge"
    );

    // Cell self-loops
    assert!(
        dot.contains("CPUCore_Writeback -> CPUCore_Writeback"),
        "missing regfile Cell self-loop"
    );

    // Valid DOT structure
    assert!(dot.starts_with("digraph CPU {"));
    assert!(dot.trim_end().ends_with('}'));
}

#[test]
fn dot_producer_consumer() {
    let src = r#"
process P {
    produces: out : Queue(Bits 32)
    rule go { out.put(42) }
}

process C {
    consumes: inp : Queue(Bits 32)
    rule go { let _ = inp.take() }
}

pipe Pipeline {
    let q = Queue(Bits 32, depth = 2)
    P { out = q }
    C { inp = q }
}
"#;
    let dots = graph_src(src);
    assert_eq!(dots.len(), 1);
    let dot = &dots[0];
    assert!(dot.contains("P -> C"), "missing P→C edge");
    assert!(dot.contains("depth=2"), "missing depth label");
}
