//! The seeds of the fuzz target `relay_bearer`: the vectors `bearer` and
//! `bearer-refused` of `crates/pod-relay/tests/vectors/grants.json`, each
//! named by the words of its vector, each set of bytes once. The seeds on
//! disk are the ones that this file makes. Write them again with
//! `cargo test --manifest-path fuzz/Cargo.toml --lib write_the_bearer_seeds -- --ignored`.

use std::path::PathBuf;

use pod_e2e::Binding;
use pod_relay::grant::bearer::{Bearer, Bearers};

const TARGET: &str = "relay_bearer";

/// The bytes of a hex string of a vector.
fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2), "an even count of hex digits");
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex")).collect()
}

/// The name, the kind and the bytes of each seed.
fn seeds() -> Vec<(String, String, Vec<u8>)> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/pod-relay/tests/vectors/grants.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let file: serde_json::Value = serde_json::from_str(&text).expect("grants.json is JSON");
    let mut out: Vec<(String, String, Vec<u8>)> = Vec::new();
    for vector in file["vectors"].as_array().expect("a list of vectors") {
        let kind = vector["kind"].as_str().expect("a kind");
        if !kind.starts_with("bearer") {
            continue;
        }
        let name = vector["name"].as_str().expect("a name").replace(' ', "_");
        let bytes = unhex(vector["hex"].as_str().expect("the bytes"));
        if !out.iter().any(|(_, _, b)| *b == bytes) {
            out.push((name, kind.to_string(), bytes));
        }
    }
    out
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("seeds").join(TARGET)
}

/// The seeds on disk are the ones of [`seeds`]: two bearers that read and
/// write back to their bytes, and the refused vectors with other bytes; the
/// target takes each.
#[test]
fn the_seeds_of_relay_bearer_are_the_bearer_vectors() {
    let seeds = seeds();
    for (name, _, bytes) in &seeds {
        let on_disk = std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("{TARGET}/{name}: {e}"));
        assert!(on_disk == *bytes, "{TARGET}/{name} is stale: run write_the_bearer_seeds");
    }
    let mut on_disk: Vec<String> =
        std::fs::read_dir(dir()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    on_disk.sort();
    let mut made: Vec<String> = seeds.iter().map(|s| s.0.clone()).collect();
    made.sort();
    assert_eq!(on_disk, made, "{TARGET}: a seed that this file does not make");
    let bearers: Vec<_> = seeds.iter().filter(|s| s.1 == "bearer").collect();
    assert_eq!(bearers.len(), 2, "the two vectors of a bearer");
    for (name, _, bytes) in bearers {
        let read = Bearer::read(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(read.to_bytes()[..], bytes[..], "{name}");
    }
    // The first bearer holds on the channel of the target's verifier, so the
    // target reaches the record of its `jti`.
    let first = &seeds[0].2;
    let read = Bearer::read(first).unwrap();
    let binding = Binding::of_hash(&(0..32).collect::<Vec<u8>>());
    Bearers::default().verify(first, Some(&binding), &read.holder, |_| true, read.iat).expect("the first bearer holds");
    for (_, _, bytes) in &seeds {
        crate::targets::relay_bearer(bytes);
    }
}

#[test]
#[ignore = "writes the seeds of relay_bearer into fuzz/seeds/"]
fn write_the_bearer_seeds() {
    std::fs::create_dir_all(dir()).unwrap();
    for (name, _, bytes) in seeds() {
        std::fs::write(dir().join(name), bytes).unwrap();
    }
}
