//! Each kind of each vector file, and the reader of it here: a test of this
//! crate, or a skip with its reason. A kind that this table does not know
//! fails, so a new kind of a new version of the page cannot pass unread.

use crate::common::{PARTS, name, text, vectors};

/// Who reads the vectors of a kind.
enum Reader {
    /// A test of `wire_vectors` reads each one.
    Run,
    /// No code of podcrates reads it, for this reason.
    Skip(&'static str),
}

/// The reader of the vectors of `kind` in `part`, or `None` for a kind that
/// the table does not know.
fn reader(part: &str, kind: &str) -> Option<Reader> {
    let run = match (part, kind) {
        ("closes", "close") => true,
        ("forward", "frame" | "frame-refused") => true,
        ("grants", "grant" | "grant-forged" | "grant-refused" | "bearer" | "bearer-refused") => cfg!(feature = "grant"),
        ("layer", "record" | "refused" | "partial" | "secret") => true,
        (
            "reverse",
            "node-frame" | "operator-frame" | "node-frame-refused" | "too-long" | "chunks" | "control-out"
            | "control-in" | "control-cut",
        ) => true,
        ("tokens", "mint" | "check") => return Some(Reader::Skip(crate::tokens::WHY)),
        _ => return None,
    };
    Some(if run { Reader::Run } else { Reader::Skip("the feature grant is off") })
}

#[test]
fn each_vector_runs_or_is_skipped_by_its_kind_with_a_reason() {
    let (mut run, mut skipped, mut total) = (0, 0, 0);
    for part in PARTS {
        for v in vectors(part) {
            total += 1;
            let kind = text(&v, "kind");
            match reader(part, kind) {
                Some(Reader::Run) => run += 1,
                Some(Reader::Skip(why)) => {
                    skipped += 1;
                    println!("skipped {part}.json {kind}: {}: {why}", name(&v));
                }
                None => panic!("{part}.json: {}: no reader of the kind {kind}", name(&v)),
            }
        }
    }
    println!("wire vectors: {total} in all, {run} run, {skipped} skipped");
    assert_eq!(run + skipped, total);
    assert!(run > 0, "no vector runs");
}
