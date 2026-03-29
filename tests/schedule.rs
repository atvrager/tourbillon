/// Integration tests for the schedule stage.

#[test]
fn single_rule_pipe_schedules() {
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
    // Should pass through the full pipeline including schedule
    tbn::check(src, "test.tbn").unwrap();
}

#[test]
fn multi_rule_pipe_schedules() {
    let src = r#"
process Dual {
    state: a : Cell(Bits 32, init = 0)
    state: b : Cell(Bits 32, init = 0)
    rule inc_a {
        let v = a.take()
        a.put(v + 1)
    }
    rule inc_b {
        let v = b.take()
        b.put(v + 1)
    }
}

pipe Top {
    Dual {}
}
"#;
    // Two non-conflicting rules should schedule without error
    tbn::check(src, "test.tbn").unwrap();
}
