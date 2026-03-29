/// Tests for `external fn` declarations (Phase 3).

#[test]
fn parse_external_fn_void() {
    let src = r#"
external fn uart_tx(ch : Bits 8)

type Word = Bits 32
process UartDev {
    state: data : Cell(Bits 32, init = 0)
    rule go {
        let d = data.take()
        uart_tx(d[7:0])
        data.put(d + 1)
    }
}
pipe Top { UartDev {} }
"#;
    tbn::check(src, "test.tbn").unwrap();
}

#[test]
fn external_fn_with_return() {
    let src = r#"
external fn read_sensor() -> Bits 32

type Word = Bits 32
process Sampler {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let old = x.take()
        let val = read_sensor()
        x.put(val)
    }
}
pipe Top { Sampler {} }
"#;
    tbn::check(src, "test.tbn").unwrap();
}

#[test]
fn external_fn_dpi_import_emitted() {
    let src = r#"
external fn uart_tx(ch : Bits 8)

type Word = Bits 32
process UartDev {
    state: data : Cell(Bits 32, init = 0)
    rule go {
        let d = data.take()
        uart_tx(d[7:0])
        data.put(d + 1)
    }
}
pipe Top { UartDev {} }
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = files.iter().find(|f| f.name == "Top.sv").unwrap();
    assert!(
        sv.content.contains("import \"DPI-C\""),
        "expected DPI import in SV:\n{}",
        sv.content
    );
    assert!(
        sv.content.contains("uart_tx"),
        "expected uart_tx in DPI import:\n{}",
        sv.content
    );
}

#[test]
fn external_fn_multiple_params() {
    let src = r#"
external fn set_pixel(x : Bits 16, y : Bits 16, color : Bits 32)

type Word = Bits 32
process Drawer {
    state: count : Cell(Bits 32, init = 0)
    rule go {
        let c = count.take()
        set_pixel(0, 0, c)
        count.put(c + 1)
    }
}
pipe Top { Drawer {} }
"#;
    tbn::check(src, "test.tbn").unwrap();
}

#[test]
fn external_fn_call_in_sv() {
    let src = r#"
external fn uart_tx(ch : Bits 8)

type Word = Bits 32
process UartDev {
    state: data : Cell(Bits 32, init = 0)
    rule go {
        let d = data.take()
        uart_tx(d[7:0])
        data.put(d + 1)
    }
}
pipe Top { UartDev {} }
"#;
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = files.iter().find(|f| f.name == "Top.sv").unwrap();
    // The call should appear in the always_comb block
    assert!(
        sv.content.contains("uart_tx("),
        "expected uart_tx call in SV:\n{}",
        sv.content
    );
}
