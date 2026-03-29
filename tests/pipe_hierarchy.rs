/// Tests for pipe-in-pipe instantiation (Phase 4).

#[test]
fn basic_pipe_hierarchy() {
    // A child pipe with its own internal queues, instantiated in a parent.
    let src = r#"
type Word = Bits 32

process Inner {
    state: count : Cell(Bits 32, init = 0)
    rule tick {
        let c = count.take()
        count.put(c + 1)
    }
}

pipe Child {
    Inner {}
}

pipe Parent {
    Child {}
}
"#;
    tbn::check(src, "test.tbn").unwrap();
}

#[test]
fn pipe_hierarchy_produces_sv() {
    let src = r#"
type Word = Bits 32

process Counter {
    state: count : Cell(Bits 32, init = 0)
    rule tick {
        let c = count.take()
        count.put(c + 1)
    }
}

pipe Inner {
    Counter {}
}

pipe Top {
    Inner {}
}
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    // Should produce SV for the parent pipe (flattened)
    let sv = files.iter().find(|f| f.name == "Top.sv").unwrap();
    // The child's Counter instance should be in the flattened output
    assert!(
        sv.content.contains("Inner_Counter"),
        "expected flattened instance name 'Inner_Counter' in SV:\n{}",
        sv.content
    );
}

#[test]
fn pipe_hierarchy_with_domain() {
    let src = r#"
type Word = Bits 32

process Counter {
    state: count : Cell(Bits 32, init = 0)
    rule tick {
        let c = count.take()
        count.put(c + 1)
    }
}

pipe Child {
    Counter {}
}

pipe Top {
    domain fast : Clock
    Child [fast] {}
}
"#;
    tbn::check(src, "test.tbn").unwrap();
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = files.iter().find(|f| f.name == "Top.sv").unwrap();
    // Child's counter should use the fast domain clock
    assert!(
        sv.content.contains("fast_clk"),
        "expected fast_clk in SV:\n{}",
        sv.content
    );
}

#[test]
fn pipe_hierarchy_multiple_processes() {
    let src = r#"
type Word = Bits 32

process Producer {
    produces: out : Queue(Bits 32)
    rule go {
        out.put(42)
    }
}

process Consumer {
    consumes: input : Queue(Bits 32)
    rule go {
        let _ = input.take()
    }
}

pipe Inner {
    let q = Queue(Bits 32, depth = 2)
    Producer { out = q }
    Consumer { input = q }
}

pipe Top {
    Inner {}
}
"#;
    tbn::check(src, "test.tbn").unwrap();
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = files.iter().find(|f| f.name == "Top.sv").unwrap();
    assert!(
        sv.content.contains("Inner_Producer") && sv.content.contains("Inner_Consumer"),
        "expected flattened instance names in SV:\n{}",
        sv.content
    );
}

#[test]
fn pipe_hierarchy_self_reference_error() {
    let src = r#"
type Word = Bits 32

process P {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        x.put(v + 1)
    }
}

pipe SelfRef {
    SelfRef {}
    P {}
}
"#;
    let result = tbn::check(src, "test.tbn");
    assert!(result.is_err(), "expected error for self-referencing pipe");
}
