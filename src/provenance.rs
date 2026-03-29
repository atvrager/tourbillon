use std::path::PathBuf;

/// BLAKE3-hash a single source file's content.
pub fn hash_source(content: &[u8]) -> [u8; 32] {
    *blake3::hash(content).as_bytes()
}

/// Compute the Merkle root hash over a set of source files.
///
/// Files are sorted by name to ensure deterministic ordering regardless of
/// the order they are supplied. Each file is individually hashed, then all
/// per-file hashes are concatenated and hashed again to produce the root.
pub fn source_root(files: &[(&str, &[u8])]) -> [u8; 32] {
    let mut sorted: Vec<(&str, &[u8])> = files.to_vec();
    sorted.sort_by_key(|(name, _)| *name);

    let mut concatenated = Vec::with_capacity(sorted.len() * 32);
    for (_, content) in &sorted {
        let h = hash_source(content);
        concatenated.extend_from_slice(&h);
    }

    *blake3::hash(&concatenated).as_bytes()
}

/// Format a 32-byte hash as a 64-character lowercase hex string.
pub fn hex(hash: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in hash {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Build a JSON manifest describing all source files and their hashes.
pub fn source_manifest(files: &[(&str, &[u8])]) -> serde_json::Value {
    let mut sorted: Vec<(&str, &[u8])> = files.to_vec();
    sorted.sort_by_key(|(name, _)| *name);

    let file_entries: Vec<serde_json::Value> = sorted
        .iter()
        .map(|(name, content)| {
            serde_json::json!({
                "file": name,
                "hash": hex(&hash_source(content)),
            })
        })
        .collect();

    let root = source_root(files);

    serde_json::json!({
        "source_root": hex(&root),
        "files": file_entries,
    })
}

/// Return the cache directory path for a given source root hash.
///
/// Layout: `$HOME/.tbn/store/<hex>/`
pub fn cache_dir(hash: &[u8; 32]) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".tbn")
        .join("store")
        .join(hex(hash))
}
