//! The stable part of the fuzz targets: each target on its seeds and on
//! deterministic mutations of them, with no nightly and no libFuzzer, so
//! that a panic also fails a test on stable Rust (podnet
//! `src/fuzzing.rs`).

use std::path::{Path, PathBuf};

use crate::TARGETS;

fn here() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The names of the entries of `dir`, sorted, with no `.rs`.
fn names(dir: &Path) -> Vec<String> {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    let mut names: Vec<String> = entries
        .map(|entry| entry.expect("an entry").file_name().to_string_lossy().trim_end_matches(".rs").to_string())
        .collect();
    names.sort();
    names
}

/// The seeds of `target`: each file of `seeds/TARGET/`.
fn seeds(target: &str) -> Vec<Vec<u8>> {
    let dir = here().join("seeds").join(target);
    names(&dir).iter().map(|name| std::fs::read(dir.join(name)).expect("a seed")).collect()
}

/// Each target has its binary in Cargo.toml and in `fuzz_targets/`, and
/// its seeds; each binary and each directory of seeds is a target.
#[test]
fn each_target_has_a_binary_and_seeds() {
    let mut targets: Vec<String> = TARGETS.iter().map(|(name, _)| name.to_string()).collect();
    targets.sort();
    assert_eq!(names(&here().join("fuzz_targets")), targets);
    assert_eq!(names(&here().join("seeds")), targets);
    let manifest = std::fs::read_to_string(here().join("Cargo.toml")).expect("Cargo.toml");
    assert_eq!(manifest.matches("[[bin]]").count(), targets.len());
    for name in &targets {
        let bin = format!("[[bin]]\nname = \"{name}\"\npath = \"fuzz_targets/{name}.rs\"\n");
        assert!(manifest.contains(&bin), "Cargo.toml has no binary {name}");
        let source = std::fs::read_to_string(here().join("fuzz_targets").join(format!("{name}.rs"))).expect("a target");
        assert!(source.contains(&format!("podcrates_fuzz::targets::{name}(data)")), "{name}.rs calls another target");
    }
}

/// Each target on its seeds and on 2000 mutations of them (a fixed
/// xorshift sequence): no panic.
#[test]
fn each_target_survives_mutations() {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for (name, target) in TARGETS {
        let seeds = seeds(name);
        assert!(!seeds.is_empty(), "{name} has no seed");
        for seed in &seeds {
            target(seed);
        }
        for _ in 0..2000 {
            let mut input = seeds[(next() % seeds.len() as u64) as usize].clone();
            for _ in 0..(1 + next() % 4) {
                let r = next();
                match r % 4 {
                    0 if !input.is_empty() => {
                        let i = (r >> 8) as usize % input.len();
                        input[i] ^= (r >> 16) as u8;
                    }
                    1 if !input.is_empty() => {
                        let i = (r >> 8) as usize % input.len();
                        input[i] = [0, 0xff, 0x7f, 0x80][(r >> 16) as usize % 4];
                    }
                    2 => {
                        let i = (r >> 8) as usize % (input.len() + 1);
                        input.insert(i, (r >> 16) as u8);
                    }
                    _ => {
                        let keep = (r >> 8) as usize % (input.len() + 1);
                        input.truncate(keep);
                    }
                }
            }
            target(&input);
        }
    }
}

/// The seeds of the targets of pod-e2e reach past the first check: the
/// proofs of the identity of the seed read, a proof of another key does not,
/// and the states of a handover read.
#[test]
fn the_seeds_of_the_e2e_targets_read() {
    use pod_e2e::hold::ChannelState;
    let dir = here().join("seeds");
    let identity = pod_identity::Identity::from_seed(&[1; 32]);
    let read = |name: &str| {
        let bytes = std::fs::read(dir.join("e2e_proof").join(name)).expect("a seed");
        pod_e2e::fuzzing::proof(&bytes[1..], Some(identity.dh_public()), bytes[0] & 1 == 1)
    };
    for name in ["plain", "routed", "exit", "net_form", "granted"] {
        read(name).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    assert!(read("other_key").is_err(), "a key that does not sign the Noise key");
    for name in ["held", "fresh"] {
        let bytes = std::fs::read(dir.join("e2e_handover").join(name)).expect("a seed");
        ChannelState::decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

/// The seeds of the targets of pod-session reach past the first check:
/// the states of a handover read, and the records decode.
#[test]
fn the_seeds_of_the_session_targets_read() {
    use pod_session::decode::Decoder;
    use pod_session::handover::LayerState;
    let dir = here().join("seeds");
    for name in ["closed", "carried"] {
        let bytes = std::fs::read(dir.join("session_handover").join(name)).expect("a seed");
        LayerState::decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    for name in ["greeting", "open", "refuse", "data_ack"] {
        let bytes = std::fs::read(dir.join("session_record").join(name)).expect("a seed");
        let mut decoder = Decoder::new();
        decoder.push(&bytes[1..]);
        assert!(decoder.next().unwrap_or_else(|e| panic!("{name}: {e}")).is_some(), "{name}");
    }
}
