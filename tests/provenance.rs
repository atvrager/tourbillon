use tbn::provenance::*;

#[test]
fn source_root_deterministic() {
    let files = vec![("a.tbn", b"hello" as &[u8]), ("b.tbn", b"world")];
    let h1 = source_root(&files);
    let h2 = source_root(&files);
    assert_eq!(h1, h2);
}

#[test]
fn source_root_order_independent() {
    let h1 = source_root(&[("a.tbn", b"hello"), ("b.tbn", b"world")]);
    let h2 = source_root(&[("b.tbn", b"world"), ("a.tbn", b"hello")]);
    assert_eq!(h1, h2, "file order should not affect source root hash");
}

#[test]
fn source_root_changes_on_content_change() {
    let h1 = source_root(&[("a.tbn", b"hello")]);
    let h2 = source_root(&[("a.tbn", b"hello!")]);
    assert_ne!(h1, h2, "different content should produce different hash");
}

#[test]
fn provenance_embedded_in_sv() {
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
    let hash = source_root(&[("test.tbn", src.as_bytes())]);
    let files = tbn::build(src, "test.tbn", Some(hash)).unwrap();
    let sv = &files[0].content;
    let hex_str = hex(&hash);

    assert!(
        sv.contains(&format!("// Tourbillon provenance: {hex_str}")),
        "SV should contain provenance comment"
    );
    assert!(
        sv.contains(&format!(
            "localparam logic [255:0] TBN_PROVENANCE = 256'h{hex_str};"
        )),
        "SV should contain provenance localparam"
    );
}

#[test]
fn no_provenance_when_none() {
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
    let files = tbn::build(src, "test.tbn", None).unwrap();
    let sv = &files[0].content;
    assert!(
        !sv.contains("TBN_PROVENANCE"),
        "SV should not contain provenance when None"
    );
    assert!(
        !sv.contains("Tourbillon provenance"),
        "SV should not contain provenance comment when None"
    );
}

#[test]
fn source_manifest_json_structure() {
    let files = vec![("a.tbn", b"hello" as &[u8]), ("b.tbn", b"world")];
    let manifest = source_manifest(&files);

    assert!(manifest["source_root"].is_string());
    assert_eq!(manifest["source_root"].as_str().unwrap().len(), 64);

    let file_arr = manifest["files"].as_array().unwrap();
    assert_eq!(file_arr.len(), 2);
    assert!(file_arr[0]["file"].is_string());
    assert!(file_arr[0]["hash"].is_string());
    assert_eq!(file_arr[0]["hash"].as_str().unwrap().len(), 64);
}

#[test]
fn hex_format() {
    let hash = [0u8; 32];
    let h = hex(&hash);
    assert_eq!(h.len(), 64);
    assert_eq!(
        h,
        "0000000000000000000000000000000000000000000000000000000000000000"
    );
}
