//! The reading of the vector files, shared by each part of `wire_vectors`.

use std::path::PathBuf;

use serde_json::Value;

/// The version of `docs/wire.md` that these vectors are of.
pub const WIRE: u64 = 2;

/// Each file of `tests/vectors/`.
pub const PARTS: [&str; 6] = ["closes", "forward", "grants", "layer", "reverse", "tokens"];

/// The repository's root.
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The vectors of `part`, once its file names this version and this part.
pub fn vectors(part: &str) -> Vec<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors").join(format!("{part}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(doc["wire"].as_u64(), Some(WIRE), "{part}.json: its wire version");
    assert_eq!(doc["part"].as_str(), Some(part), "{part}.json: its part");
    doc["vectors"].as_array().unwrap_or_else(|| panic!("{part}.json: no vectors")).clone()
}

/// The vectors of `part` of `kind`, at least one: a kind that no vector
/// has checks nothing.
pub fn of_kind(part: &str, kind: &str) -> Vec<Value> {
    let found: Vec<Value> = vectors(part).into_iter().filter(|v| v["kind"] == kind).collect();
    assert!(!found.is_empty(), "{part}.json: no vector of kind {kind}");
    found
}

pub fn name(v: &Value) -> &str {
    v["name"].as_str().unwrap_or("?")
}

pub fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_else(|| panic!("{}: no text {key}", name(v)))
}

pub fn number(v: &Value, key: &str) -> u64 {
    v[key].as_u64().unwrap_or_else(|| panic!("{}: no number {key}", name(v)))
}

pub fn small(v: &Value, key: &str) -> u8 {
    u8::try_from(number(v, key)).unwrap_or_else(|_| panic!("{}: {key} is no byte", name(v)))
}

/// The bytes of hex `text`.
pub fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2), "odd hex: {text}");
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex")).collect()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn bytes(v: &Value, key: &str) -> Vec<u8> {
    unhex(text(v, key))
}

/// The `N` bytes of hex at `key`.
pub fn array<const N: usize>(v: &Value, key: &str) -> [u8; N] {
    bytes(v, key).try_into().unwrap_or_else(|_| panic!("{}: {key} is not {N} bytes", name(v)))
}

pub fn strings(v: &Value, key: &str) -> Vec<String> {
    let list = v[key].as_array().unwrap_or_else(|| panic!("{}: no list {key}", name(v)));
    list.iter().map(|s| s.as_str().expect("a string").to_string()).collect()
}
