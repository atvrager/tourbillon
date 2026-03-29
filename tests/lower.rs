use std::path::Path;

/// Integration tests for the lowering stage.

// ---------------------------------------------------------------------------
// Golden file comparison tests
// ---------------------------------------------------------------------------

/// Compile an example .tbn, compare every generated file against its golden .sv.
fn check_golden(example_name: &str) {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tbn_path = manifest_dir
        .join("examples")
        .join(format!("{example_name}.tbn"));
    let golden_dir = manifest_dir.join("examples/golden").join(example_name);

    let src = std::fs::read_to_string(&tbn_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", tbn_path.display()));

    let files = tbn::build(&src, tbn_path.to_str().unwrap(), None).unwrap_or_else(|_| {
        panic!("{example_name}.tbn failed to compile");
    });

    // Every golden file must be produced, and every produced file must match.
    let mut golden_files: Vec<String> = std::fs::read_dir(&golden_dir)
        .unwrap_or_else(|e| panic!("cannot read golden dir {}: {e}", golden_dir.display()))
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".sv") {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    golden_files.sort();

    let mut produced_names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
    produced_names.sort();

    assert_eq!(
        produced_names, golden_files,
        "{example_name}: produced files != golden files"
    );

    for file in &files {
        let golden_path = golden_dir.join(&file.name);
        let expected = std::fs::read_to_string(&golden_path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", golden_path.display()));
        assert_eq!(
            file.content, expected,
            "{example_name}/{}: output differs from golden.\n\
             --- To update golden files, run: cargo run -- build examples/{example_name}.tbn -o examples/golden/{example_name}/",
            file.name
        );
    }
}

#[test]
fn golden_counter() {
    check_golden("counter");
}

#[test]
fn golden_producer_consumer() {
    check_golden("producer_consumer");
}

#[test]
fn golden_priority() {
    check_golden("priority");
}

#[test]
fn golden_peek() {
    check_golden("peek");
}

#[test]
fn golden_branch() {
    check_golden("branch");
}

// ---------------------------------------------------------------------------
// Structural assertion tests
// ---------------------------------------------------------------------------

#[test]
fn counter_generates_sv() {
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
    let files = tbn::build(src, "test.tbn", None).unwrap();
    assert!(!files.is_empty());

    let sv = &files[0].content;
    assert!(sv.contains("module Top"));
    assert!(sv.contains("c_Counter_count_q"));
    assert!(sv.contains("c_Counter_count_d"));
    assert!(sv.contains("c_Counter_count_en"));
    assert!(sv.contains("always_ff"));
    assert!(sv.contains("always_comb"));
    assert!(sv.contains("endmodule"));
    // Cell init value
    assert!(sv.contains("32'd0"), "should contain cell init value");
}

#[test]
fn producer_consumer_generates_sv() {
    let src = r#"
process Producer {
    produces: out : Queue(Bits 32)
    rule go {
        out.put(42)
    }
}

process Consumer {
    consumes: input : Queue(Bits 32)
    rule go {
        let x = input.take()
    }
}

pipe Top {
    let q = Queue(Bits 32, depth = 4)
    Producer { out = q }
    Consumer { input = q }
}
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    // Should have tbn_fifo.sv and Top.sv
    assert!(
        files.len() >= 2,
        "expected at least 2 files, got {}",
        files.len()
    );

    let fifo_file = files.iter().find(|f| f.name == "tbn_fifo.sv").unwrap();
    assert!(fifo_file.content.contains("module tbn_fifo"));

    let top_file = files.iter().find(|f| f.name == "Top.sv").unwrap();
    let sv = &top_file.content;
    assert!(sv.contains("tbn_fifo #"), "should instantiate FIFO");
    assert!(sv.contains("q_q_deq_data"), "should have deq_data signal");
    assert!(sv.contains("q_q_enq_valid"), "should have enq_valid signal");
    assert!(sv.contains("q_q_deq_ready"), "should have deq_ready wiring");
}

#[test]
fn two_conflicting_rules_priority() {
    let src = r#"
process Counter {
    state: count : Cell(Bits 32, init = 0)
    rule inc {
        let c = count.take()
        count.put(c + 1)
    }
    rule dec {
        let c = count.take()
        count.put(c - 1)
    }
}

pipe Top {
    Counter {}
}
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = &files[0].content;

    // Should have priority suppression: dec suppressed by inc
    assert!(sv.contains("r_Counter_inc_can_fire"), "inc can_fire");
    assert!(sv.contains("r_Counter_inc_will_fire"), "inc will_fire");
    assert!(sv.contains("r_Counter_dec_can_fire"), "dec can_fire");
    assert!(sv.contains("r_Counter_dec_will_fire"), "dec will_fire");
    // dec should be suppressed by inc
    assert!(
        sv.contains("~(r_Counter_inc_will_fire)"),
        "dec should be suppressed by inc: {}",
        sv
    );
}

#[test]
fn build_subcommand_writes_file() {
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
    let dir = std::env::temp_dir().join("tbn_test_build");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let input_file = dir.join("counter.tbn");
    std::fs::write(&input_file, src).unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tbn"))
        .args([
            "build",
            input_file.to_str().unwrap(),
            "--output",
            dir.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run tbn build");

    assert!(
        output.status.success(),
        "tbn build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let sv_file = dir.join("Top.sv");
    assert!(sv_file.exists(), "Top.sv should exist");
    let content = std::fs::read_to_string(&sv_file).unwrap();
    assert!(content.contains("module Top"));
    // CLI build always embeds provenance
    assert!(
        content.contains("TBN_PROVENANCE"),
        "CLI build should embed provenance localparam"
    );
    assert!(
        content.contains("// Tourbillon provenance:"),
        "CLI build should embed provenance comment"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Stage 2.0 — Lowerer completeness tests
// ---------------------------------------------------------------------------

#[test]
fn struct_packed_typedef_emitted() {
    let src = r#"
record Decoded {
    op  : Bits 32
    rd  : Bits 32
    imm : Bits 32
}

process Dec {
    state: buf : Cell(Decoded, init = 0)
    rule go {
        let d = buf.take()
        buf.put(Decoded { op = 0, rd = 1, imm = 42 })
    }
}

pipe Top {
    Dec {}
}
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = &files[0].content;
    assert!(
        sv.contains("typedef struct packed"),
        "should emit typedef struct packed: {sv}"
    );
    assert!(
        sv.contains("logic [31:0] op;"),
        "should emit op field: {sv}"
    );
    assert!(
        sv.contains("logic [31:0] rd;"),
        "should emit rd field: {sv}"
    );
    assert!(
        sv.contains("logic [31:0] imm;"),
        "should emit imm field: {sv}"
    );
    assert!(
        sv.contains("} Decoded;"),
        "should close with type name: {sv}"
    );
    // Record construction should use typed cast
    assert!(
        sv.contains("Decoded'"),
        "should use typed cast for record construction: {sv}"
    );
}

#[test]
fn enum_typedef_emitted() {
    let src = r#"
enum MemOp = Load | Store | None
process Ex {
    state: op : Cell(MemOp, init = 0)
    rule go {
        let o = op.take()
        op.put(o)
    }
}

pipe Top {
    Ex {}
}
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = &files[0].content;
    assert!(
        sv.contains("typedef enum logic"),
        "should emit typedef enum: {sv}"
    );
    assert!(sv.contains("Load = 0"), "should emit Load variant: {sv}");
    assert!(sv.contains("Store = 1"), "should emit Store variant: {sv}");
    assert!(sv.contains("None = 2"), "should emit None variant: {sv}");
    assert!(sv.contains("} MemOp;"), "should close with type name: {sv}");
}

#[test]
fn tuple_destructuring() {
    let src = r#"
process Pair {
    consumes: input : Queue(Bits 32 × Bits 32)
    produces: out   : Queue(Bits 32)
    rule go {
        let (a, b) = input.take()
        out.put(a)
    }
}

process Src {
    produces: out : Queue(Bits 32 × Bits 32)
    rule go {
        out.put((42, 7))
    }
}

process Sink {
    consumes: inp : Queue(Bits 32)
    rule go { let _ = inp.take() }
}

pipe Top {
    let q = Queue(Bits 32 × Bits 32, depth = 2)
    let r = Queue(Bits 32, depth = 2)
    Src  { out = q }
    Pair { input = q, out = r }
    Sink { inp = r }
}
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let top = files.iter().find(|f| f.name == "Top.sv").unwrap();
    let sv = &top.content;
    // Tuple destructuring should produce bit-slices
    // (a, b) on Bits 32 × Bits 32 = 64 bits total
    // a = [63:32], b = [31:0]
    assert!(
        sv.contains("[63:32]"),
        "should emit bit-slice for first tuple element: {sv}"
    );
    assert!(
        sv.contains("[31:0]"),
        "should emit bit-slice for second tuple element: {sv}"
    );
}

#[test]
fn variant_match_option() {
    let src = r#"
process TryConsumer {
    consumes: input : Queue(Bits 32)
    produces: out   : Queue(Bits 32)
    rule go {
        match input.try_take() {
            Some(x) => out.put(x)
            None    => out.put(0)
        }
    }
}

process Src {
    produces: out : Queue(Bits 32)
    rule go { out.put(42) }
}

process Sink {
    consumes: inp : Queue(Bits 32)
    rule go { let _ = inp.take() }
}

pipe Top {
    let q = Queue(Bits 32, depth = 2)
    let r = Queue(Bits 32, depth = 2)
    Src          { out = q }
    TryConsumer  { input = q, out = r }
    Sink         { inp = r }
}
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let top = files.iter().find(|f| f.name == "Top.sv").unwrap();
    let sv = &top.content;
    // Should have valid-bit test for Some pattern
    assert!(
        sv.contains("[32]"),
        "should test valid bit for Some pattern: {sv}"
    );
    // try_take deq_ready should be driven
    assert!(
        sv.contains("deq_ready"),
        "should wire deq_ready for try_take: {sv}"
    );
}

#[test]
fn record_field_access() {
    let src = r#"
record Pair {
    x : Bits 16
    y : Bits 16
}

process FieldTest {
    state: buf : Cell(Pair, init = 0)
    produces: out : Queue(Bits 16)
    rule go {
        let p = buf.take()
        out.put(p.x)
        buf.put(p)
    }
}

process Sink {
    consumes: inp : Queue(Bits 16)
    rule go { let _ = inp.take() }
}

pipe Top {
    let q = Queue(Bits 16, depth = 2)
    FieldTest { out = q }
    Sink      { inp = q }
}
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let top = files.iter().find(|f| f.name == "Top.sv").unwrap();
    let sv = &top.content;
    // With struct packed typedef, .x field access should appear in SV
    assert!(sv.contains(".x"), "should emit field access .x: {sv}");
}

#[test]
fn try_take_deq_ready_wired() {
    let src = r#"
process TryRead {
    consumes: input : Queue(Bits 32)
    rule go {
        let _ = input.try_take()
    }
}

process Src {
    produces: out : Queue(Bits 32)
    rule go { out.put(42) }
}

pipe Top {
    let q = Queue(Bits 32, depth = 2)
    Src     { out = q }
    TryRead { input = q }
}
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let top = files.iter().find(|f| f.name == "Top.sv").unwrap();
    let sv = &top.content;
    // try_take should drive deq_ready conditionally
    assert!(
        sv.contains("q_q_deq_valid)"),
        "try_take deq_ready should include deq_valid condition: {sv}"
    );
}

#[test]
fn memory_primitive_desugars() {
    let src = r#"
type Addr = Bits 32
type Word = Bits 32

process Requester {
    produces: rreq  : Queue(Addr)
    consumes: rresp : Queue(Word)
    produces: wreq  : Queue(Addr × Word)
    rule read_mem {
        rreq.put(0)
        let _ = rresp.take()
    }
    rule write_mem {
        wreq.put((0, 42))
    }
}

pipe Top {
    let imem = Memory(Addr -> Word, depth = 1024, latency = 1)
    Requester {
        rreq  = imem_read_req
        rresp = imem_read_resp
        wreq  = imem_write_req
    }
}
"#;
    // Memory should parse and desugar — the pipe will have memory queues
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let top = files.iter().find(|f| f.name == "Top.sv").unwrap();
    let sv = &top.content;
    // Should have FIFO instances for the desugared memory queues
    assert!(
        sv.contains("imem_read_req") || sv.contains("imem_write_req"),
        "should have desugared memory queues: {sv}"
    );
}
