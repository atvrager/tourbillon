/// Integration tests for the lowering stage.

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
    let files = tbn::build(src, "test.tbn").unwrap();
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
    let files = tbn::build(src, "test.tbn").unwrap();
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
    let files = tbn::build(src, "test.tbn").unwrap();
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

    let _ = std::fs::remove_dir_all(&dir);
}
