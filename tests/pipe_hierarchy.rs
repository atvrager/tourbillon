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
fn pipe_hierarchy_cross_pipe_wiring() {
    // A child pipe with an internal queue that gets wired to a parent queue.
    // The child has a Producer writing to an internal queue. The parent
    // provides a Consumer that reads from a parent queue, and the child's
    // internal queue is bound to the parent queue.
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
}

pipe Top {
    let bridge = Queue(Bits 32, depth = 4)
    Inner { q = bridge }
    Consumer { input = bridge }
}
"#;
    tbn::check(src, "test.tbn").unwrap();
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = files.iter().find(|f| f.name == "Top.sv").unwrap();
    // The bridge queue should exist and be wired
    assert!(
        sv.content.contains("bridge"),
        "expected 'bridge' queue in SV:\n{}",
        sv.content
    );
}

#[test]
fn pipe_hierarchy_cross_pipe_bidirectional() {
    // Child pipe has both input and output queues wired through the parent
    let src = r#"
type Word = Bits 32

process Echo {
    consumes: req : Queue(Bits 32)
    produces: resp : Queue(Bits 32)
    rule go {
        let v = req.take()
        resp.put(v + 1)
    }
}

pipe EchoPipe {
    let req_q  = Queue(Bits 32, depth = 2)
    let resp_q = Queue(Bits 32, depth = 2)
    Echo { req = req_q, resp = resp_q }
}

process Driver {
    produces: out : Queue(Bits 32)
    rule go {
        out.put(100)
    }
}

process Sink {
    consumes: input : Queue(Bits 32)
    rule go {
        let _ = input.take()
    }
}

pipe Top {
    let bus_req  = Queue(Bits 32, depth = 2)
    let bus_resp = Queue(Bits 32, depth = 2)
    Driver { out = bus_req }
    EchoPipe { req_q = bus_req, resp_q = bus_resp }
    Sink { input = bus_resp }
}
"#;
    tbn::check(src, "test.tbn").unwrap();
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = files.iter().find(|f| f.name == "Top.sv").unwrap();
    assert!(
        sv.content.contains("bus_req") && sv.content.contains("bus_resp"),
        "expected bus_req and bus_resp in SV:\n{}",
        sv.content
    );
}

#[test]
fn pipe_hierarchy_self_reference_error() {
    // Self-referencing pipe should error. Wrap it in a parent so it gets elaborated.
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

pipe Top {
    SelfRef {}
}
"#;
    let result = tbn::check(src, "test.tbn");
    assert!(result.is_err(), "expected error for self-referencing pipe");
}
