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
