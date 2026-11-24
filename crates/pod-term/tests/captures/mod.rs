//! The captures of real programs in `fixtures/` (`tests/grammar_boundary.rs`
//! says where each comes from), and the boundaries that the tracker finds
//! in them: shared by the tests of the grammar and of the modes.

#![allow(dead_code)] // each test file uses a subset.

use std::path::Path;
use std::time::Instant;

use pod_term::grammar::Tracker;

/// The captures that must be here.
pub const NAMED: [&str; 2] = ["prompt.bin", "vi.bin"];
/// The cuts of a capture: each offset, or this many spread over a longer one.
pub const CUTS: usize = 4096;
/// The chunks that the tests push: a pty gives a program's output in
/// pieces, which cut sequences.
pub const CHUNK: usize = 61;

/// Each capture, by its file name, in the order of the names.
pub fn fixtures() -> Vec<(String, Vec<u8>)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures");
    let mut found: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "bin"))
        .map(|path| {
            let name = path.file_name().expect("a file name").to_string_lossy().into_owned();
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            (name, bytes)
        })
        .collect();
    found.sort();
    for name in NAMED {
        assert!(found.iter().any(|(n, _)| n == name), "fixtures/{name} is missing");
    }
    found
}

pub fn fixture(name: &str) -> Vec<u8> {
    fixtures().into_iter().find(|(n, _)| n == name).map(|(_, bytes)| bytes).expect("a named capture")
}

/// The boundaries that the tracker finds in `bytes`, read in chunks that
/// came at one time.
pub fn boundaries(bytes: &[u8]) -> Vec<u64> {
    let mut tracker = Tracker::new();
    let now = Instant::now();
    let mut found = Vec::new();
    for chunk in bytes.chunks(CHUNK) {
        tracker.feed(chunk, now, |b| found.push(b.at));
    }
    found
}
