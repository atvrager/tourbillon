/// Integration tests for the elaboration stage.

#[test]
fn process_only_no_pipe() {
    let src = r#"
type Word = Bits 32

process Counter {
    state: count : Cell(Bits 32, init = 0)
    rule tick {
        let c = count.take()
        count.put(c + 1)
    }
}

process Adder {
    consumes: a : Queue(Bits 32)
    consumes: b : Queue(Bits 32)
    produces: sum : Queue(Bits 32)
    rule go {
        let x = a.take()
        let y = b.take()
        sum.put(x + y)
    }
}
"#;
    // File with only processes and no pipe → should pass (empty graph)
    tbn::check(src, "test.tbn").unwrap();
}

#[test]
fn counter_pipe() {
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
    tbn::check(src, "test.tbn").unwrap();
}
