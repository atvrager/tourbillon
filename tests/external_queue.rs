/// Tests for `external Queue` declarations.

#[test]
fn parse_external_queue() {
    let src = r#"
type Word = Bits 32
process Tx {
    produces: out : Queue(Bits 1)
    state: sr : Cell(Bits 8, init = 0)
    rule tick {
        let s = sr.take()
        out.put(s[0:0])
        sr.put(s + 1)
    }
}
pipe Top {
    let tx_pin = external Queue(Bits 1, depth = 1)
    Tx { out = tx_pin }
}
"#;
    tbn::check(src, "test.tbn").unwrap();
}

#[test]
fn external_queue_no_fifo_emitted() {
    let src = r#"
type Word = Bits 32
process Tx {
    produces: out : Queue(Bits 1)
    state: sr : Cell(Bits 8, init = 0)
    rule tick {
        let s = sr.take()
        out.put(s[0:0])
        sr.put(s + 1)
    }
}
pipe Top {
    let tx_pin = external Queue(Bits 1, depth = 1)
    Tx { out = tx_pin }
}
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = files.iter().find(|f| f.name == "Top.sv").unwrap();
    // External queue should NOT have a FIFO instance
    assert!(
        !sv.content.contains("q_tx_pin_inst"),
        "external queue should not have a FIFO instance:\n{}",
        sv.content
    );
    // But should have module port signals
    assert!(
        sv.content.contains("q_tx_pin_enq_valid"),
        "external queue should have enq_valid port:\n{}",
        sv.content
    );
}

#[test]
fn external_queue_consumer_port() {
    let src = r#"
type Word = Bits 32
process Rx {
    consumes: inp : Queue(Bits 1)
    state: sr : Cell(Bits 8, init = 0)
    rule tick {
        let bit = inp.take()
        let s = sr.take()
        sr.put((s << 1) | bit)
    }
}
pipe Top {
    let rx_pin = external Queue(Bits 1, depth = 1)
    Rx { inp = rx_pin }
}
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = files.iter().find(|f| f.name == "Top.sv").unwrap();
    // External queue where internal process is the reader → deq signals are module ports
    assert!(
        sv.content.contains("q_rx_pin_deq_valid"),
        "external queue (consumer) should have deq_valid port:\n{}",
        sv.content
    );
}

#[test]
fn external_queue_in_pipe_hierarchy() {
    let src = r#"
type Word = Bits 32
process Tx {
    produces: out : Queue(Bits 1)
    state: sr : Cell(Bits 8, init = 0)
    rule tick {
        let s = sr.take()
        out.put(s[0:0])
        sr.put(s + 1)
    }
}
pipe Inner {
    let tx_pin = external Queue(Bits 1, depth = 1)
    Tx { out = tx_pin }
}
pipe Top {
    Inner {}
}
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = files.iter().find(|f| f.name == "Top.sv").unwrap();
    // Port should bubble up to Top
    assert!(
        sv.content.contains("enq_valid"),
        "external queue should bubble up through hierarchy:\n{}",
        sv.content
    );
}
