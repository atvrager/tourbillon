use std::path::Path;

/// Integration tests for the Chisel lowering stage.

// ---------------------------------------------------------------------------
// Golden file comparison tests
// ---------------------------------------------------------------------------

/// Compile an example .tbn to Chisel, compare every generated file against its golden .scala.
fn check_golden_chisel(example_name: &str) {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tbn_path = manifest_dir
        .join("examples")
        .join(format!("{example_name}.tbn"));
    let golden_dir = manifest_dir
        .join("examples/golden_chisel")
        .join(example_name);

    let src = std::fs::read_to_string(&tbn_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", tbn_path.display()));

    let files = tbn::build_chisel(&src, tbn_path.to_str().unwrap()).unwrap_or_else(|_| {
        panic!("{example_name}.tbn failed to compile to Chisel");
    });

    // Every golden file must be produced, and every produced file must match.
    let mut golden_files: Vec<String> = std::fs::read_dir(&golden_dir)
        .unwrap_or_else(|e| panic!("cannot read golden dir {}: {e}", golden_dir.display()))
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".scala") {
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
             --- To update golden files, run: cargo run -- build examples/{example_name}.tbn --target chisel -o examples/golden_chisel/{example_name}/",
            file.name
        );
    }
}

#[test]
fn golden_chisel_counter() {
    check_golden_chisel("counter");
}

#[test]
fn golden_chisel_producer_consumer() {
    check_golden_chisel("producer_consumer");
}

#[test]
fn golden_chisel_peek() {
    check_golden_chisel("peek");
}

#[test]
fn golden_chisel_priority() {
    check_golden_chisel("priority");
}

#[test]
fn golden_chisel_branch() {
    check_golden_chisel("branch");
}

// ---------------------------------------------------------------------------
// Complex design compilation tests
// ---------------------------------------------------------------------------

#[test]
fn chisel_async_example_compiles() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(manifest_dir.join("examples/async_example.tbn")).unwrap();
    let files = tbn::build_chisel(&src, "async_example.tbn").unwrap();
    assert_eq!(files.len(), 1);
    let scala = &files[0].content;

    assert!(scala.contains("TbnAsyncFifo"), "AsyncQueue BlackBox");
    assert!(scala.contains("when ("), "when blocks");
}

#[test]
fn chisel_spi_v2_compiles() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(manifest_dir.join("examples/spi2tlul_v2.tbn")).unwrap();
    let files = tbn::build_chisel(&src, "spi2tlul_v2.tbn").unwrap();
    assert_eq!(files.len(), 1);
    let scala = &files[0].content;

    // Records as Bundle classes
    assert!(scala.contains("class DmaDesc extends Bundle"));
    assert!(scala.contains("class TlA extends Bundle"));
    assert!(scala.contains("class TlD extends Bundle"));

    // External queues as IO
    assert!(scala.contains("Decoupled("));
    assert!(scala.contains("Flipped(Decoupled("));

    // AsyncQueues
    assert!(scala.contains("TbnAsyncFifo"));

    // Record construction via Cat
    assert!(scala.contains("Cat("));

    // No SV constructs
    assert!(!scala.contains("always_ff"));
    assert!(!scala.contains("always_comb"));
}

#[test]
fn chisel_marie_soc_compiles() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let cpu_src = std::fs::read_to_string(manifest_dir.join("examples/cpu_core.tbn")).unwrap();
    let marie_src = std::fs::read_to_string(manifest_dir.join("examples/marie.tbn")).unwrap();
    let combined = format!("{cpu_src}\n{marie_src}");

    let files = tbn::build_chisel(&combined, "marie.tbn").unwrap();
    // Marie emits one module (Marie pipe)
    assert!(!files.is_empty(), "should produce at least one file");
    let scala = &files
        .iter()
        .find(|f| f.name == "Marie.scala")
        .unwrap()
        .content;

    // Records and enums
    assert!(scala.contains("class Decoded extends Bundle"));
    assert!(scala.contains("class BusReq extends Bundle"));
    assert!(scala.contains("object MemOp {"));

    // Constants
    assert!(scala.contains("val BAUD_DIV = 33.U"));
    assert!(scala.contains("val MEM_REGION = 8.U"));

    // External queues (UART TX pin, memory ports)
    assert!(scala.contains("Decoupled("));

    // AsyncQueues for CDC
    assert!(scala.contains("TbnAsyncFifo"));

    // DPI calls replaced with 0.U placeholders (Chisel has no DPI)
    // Verify the module compiles despite DPI-heavy code

    // No SV constructs
    assert!(!scala.contains("always_ff"));
    assert!(!scala.contains("endmodule"));
}

// ---------------------------------------------------------------------------
// Structural tests
// ---------------------------------------------------------------------------

#[test]
fn chisel_package_and_imports() {
    let src = r#"
process Counter {
    state: count : Cell(Bits 32, init = 0)
    rule tick {
        let c = count.take()
        count.put(c + 1)
    }
}
pipe Top { Counter {} }
"#;
    let files = tbn::build_chisel(src, "test.tbn").unwrap();
    let scala = &files[0].content;
    assert!(scala.starts_with("package tbn\n"));
    assert!(scala.contains("import chisel3._"));
    assert!(scala.contains("import chisel3.util._"));
}

#[test]
fn chisel_no_always_ff_or_always_comb() {
    // Chisel output should never contain SV constructs
    let src = r#"
process Counter {
    state: count : Cell(Bits 32, init = 0)
    rule tick {
        let c = count.take()
        count.put(c + 1)
    }
}
pipe Top { Counter {} }
"#;
    let files = tbn::build_chisel(src, "test.tbn").unwrap();
    let scala = &files[0].content;
    assert!(
        !scala.contains("always_ff"),
        "no SV always_ff in Chisel output"
    );
    assert!(
        !scala.contains("always_comb"),
        "no SV always_comb in Chisel output"
    );
    assert!(!scala.contains("logic "), "no SV logic in Chisel output");
    assert!(!scala.contains("wire "), "no SV wire in Chisel output");
    assert!(
        !scala.contains("endmodule"),
        "no SV endmodule in Chisel output"
    );
}

#[test]
fn chisel_cell_assignment_in_when() {
    // Cell assignments happen inside when(will_fire) blocks
    let src = r#"
process Counter {
    state: count : Cell(Bits 32, init = 0)
    rule tick {
        let c = count.take()
        count.put(c + 1)
    }
}
pipe Top { Counter {} }
"#;
    let files = tbn::build_chisel(src, "test.tbn").unwrap();
    let scala = &files[0].content;
    // Cell has a default self-assignment + assignment in when block
    let assigns: Vec<&str> = scala
        .lines()
        .filter(|l| l.contains("c_Counter_count :="))
        .collect();
    assert!(
        assigns.len() >= 2,
        "cell should have default + rule assignment"
    );
    assert!(
        scala.contains("when (r_Counter_tick_will_fire)"),
        "assignment in when block"
    );
}

#[test]
fn chisel_multi_rule_when_blocks() {
    // Each conflicting rule gets its own when block (last connect semantics)
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
pipe Top { Counter {} }
"#;
    let files = tbn::build_chisel(src, "test.tbn").unwrap();
    let scala = &files[0].content;
    assert!(
        scala.contains("when (r_Counter_inc_will_fire)"),
        "inc when block"
    );
    assert!(
        scala.contains("when (r_Counter_dec_will_fire)"),
        "dec when block"
    );
    // Priority: dec_will_fire is suppressed by inc
    assert!(
        scala.contains("r_Counter_dec_can_fire && !r_Counter_inc_will_fire"),
        "priority suppression"
    );
}
