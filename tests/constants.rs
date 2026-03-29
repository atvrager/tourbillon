/// Tests for `const` declarations (Phase 1).

#[test]
fn parse_const() {
    let src = r#"
const FOO = 42
const BAR = 0xFF

type Word = Bits 32
process P {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        x.put(v + FOO)
    }
}
pipe Top { P {} }
"#;
    tbn::check(src, "test.tbn").unwrap();
}

#[test]
fn const_in_expression() {
    let src = r#"
const OFFSET = 100
type Word = Bits 32
process P {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        if v == OFFSET {
            x.put(0)
        } else {
            x.put(v + 1)
        }
    }
}
pipe Top { P {} }
"#;
    tbn::check(src, "test.tbn").unwrap();
}

#[test]
fn const_lowered_to_localparam() {
    let src = r#"
const MAGIC = 0xDEAD
type Word = Bits 32
process P {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        x.put(MAGIC)
    }
}
pipe Top { P {} }
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = files.iter().find(|f| f.name == "Top.sv").unwrap();
    assert!(
        sv.content.contains("localparam MAGIC = 57005;"),
        "expected localparam MAGIC, got:\n{}",
        sv.content
    );
}

#[test]
fn const_value_inlined() {
    let src = r#"
const VAL = 7
type Word = Bits 32
process P {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        x.put(v + VAL)
    }
}
pipe Top { P {} }
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = files.iter().find(|f| f.name == "Top.sv").unwrap();
    // The constant should be inlined as its value in expressions
    assert!(
        sv.content.contains("7"),
        "expected constant value 7 in SV output"
    );
}

#[test]
fn multiple_constants() {
    let src = r#"
const A = 1
const B = 2
const C = 3

type Word = Bits 32
process P {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        x.put(v + A + B + C)
    }
}
pipe Top { P {} }
"#;
    tbn::check(src, "test.tbn").unwrap();
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = files.iter().find(|f| f.name == "Top.sv").unwrap();
    assert!(sv.content.contains("localparam A = 1;"));
    assert!(sv.content.contains("localparam B = 2;"));
    assert!(sv.content.contains("localparam C = 3;"));
}

#[test]
fn const_hex_value() {
    let src = r#"
const ADDR = 0x8000_0000

type Word = Bits 32
process P {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        x.put(ADDR)
    }
}
pipe Top { P {} }
"#;
    tbn::check(src, "test.tbn").unwrap();
}
