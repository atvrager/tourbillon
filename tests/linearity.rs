/// Linearity error integration tests.
/// These test that programs which violate the Cell discipline produce errors.

#[test]
fn missing_put_fails() {
    let src = r#"
process Bad {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
    }
}
"#;
    let result = tbn::check(src, "test.tbn");
    assert!(result.is_err(), "missing put should fail type check");
}

#[test]
fn double_take_fails() {
    let src = r#"
process Bad {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        let w = x.take()
        x.put(v)
    }
}
"#;
    let result = tbn::check(src, "test.tbn");
    assert!(result.is_err(), "double take should fail");
}

#[test]
fn peek_is_exempt() {
    let src = r#"
process Good {
    peeks: x : Cell(Bits 32)
    rule go {
        let v = x.peek()
    }
}
"#;
    let result = tbn::check(src, "test.tbn");
    assert!(result.is_ok(), "peek should not require put: {result:?}");
}

#[test]
fn valid_take_put_succeeds() {
    let src = r#"
process Good {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        x.put(v + 1)
    }
}
"#;
    let result = tbn::check(src, "test.tbn");
    assert!(result.is_ok(), "valid take/put should succeed: {result:?}");
}

#[test]
fn branching_both_put_succeeds() {
    let src = r#"
process Good {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        if v == 0 {
            x.put(1)
        } else {
            x.put(v)
        }
    }
}
"#;
    let result = tbn::check(src, "test.tbn");
    assert!(
        result.is_ok(),
        "both branches put should succeed: {result:?}"
    );
}

#[test]
fn branching_one_missing_put_fails() {
    let src = r#"
process Bad {
    state: x : Cell(Bits 32, init = 0)
    rule go {
        let v = x.take()
        if v == 0 {
            x.put(1)
        } else {
        }
    }
}
"#;
    let result = tbn::check(src, "test.tbn");
    assert!(
        result.is_err(),
        "inconsistent branches should fail: {result:?}"
    );
}
