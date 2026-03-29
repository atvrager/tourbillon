/// Basic end-to-end smoke tests.

#[test]
fn type_alias_ok() {
    let src = "type Word = Bits 32";
    tbn::check(src, "test.tbn").unwrap();
}

#[test]
fn multiple_type_defs() {
    let src = r#"
type Word = Bits 32
type Addr = Bits 32
type RegIdx = Bits 5
"#;
    tbn::check(src, "test.tbn").unwrap();
}

#[test]
fn record_def() {
    let src = r#"
record Foo {
    x : Bits 32
    y : Bool
}
"#;
    tbn::check(src, "test.tbn").unwrap();
}

#[test]
fn enum_def() {
    let src = "enum MemOp = Load | Store | None";
    tbn::check(src, "test.tbn").unwrap();
}

#[test]
fn empty_file() {
    tbn::check("", "test.tbn").unwrap();
}

#[test]
fn comments_only() {
    let src = r#"
-- This is a comment
-- Another comment
"#;
    tbn::check(src, "test.tbn").unwrap();
}
