//! Each part of the bundles of podcrates v0.76.0 and v0.77.0 (cosign
//! v3.1.4, Rekor v1): the promise, the inclusion proof with its checkpoint,
//! and the RFC 3161 timestamp, each alone and each changed (call C820).

use super::super::*;
use super::{flipped, value_at};

/// A release: its tag, its `SHA256SUMS`, its bundle, the time of its
/// promise, the time of its timestamp, and the index of its entry.
struct Release {
    tag: &'static str,
    sums: &'static [u8],
    bundle: &'static str,
    promised: i64,
    stamped: i64,
    index: i64,
}

const RELEASES: [Release; 2] = [
    Release {
        tag: "v0.76.0",
        sums: include_bytes!("../../../tests/fixtures/v0.76.0/SHA256SUMS"),
        bundle: include_str!("../../../tests/fixtures/v0.76.0/SHA256SUMS.sigstore.json"),
        promised: 1795080477,
        stamped: 1795080477,
        index: 3992748924,
    },
    Release {
        tag: "v0.77.0",
        sums: include_bytes!("../../../tests/fixtures/v0.77.0/SHA256SUMS"),
        bundle: include_str!("../../../tests/fixtures/v0.77.0/SHA256SUMS.sigstore.json"),
        promised: 1795100163,
        stamped: 1795100159,
        index: 3999427887,
    },
];

const ENTRY: [&str; 3] = ["verificationMaterial", "tlogEntries", "0"];
const STAMP: [&str; 5] =
    ["verificationMaterial", "timestampVerificationData", "rfc3161Timestamps", "0", "signedTimestamp"];

/// `path` under the first entry.
fn entry(path: &[&'static str]) -> Vec<&'static str> {
    ENTRY.iter().chain(path).copied().collect()
}

impl Release {
    fn check(&self, bundle: &serde_json::Value) -> Result<Logged, String> {
        let who = Identity::github("PodUtils/podcrates", "release.yml", self.tag);
        verify(&serde_json::to_vec(bundle).unwrap(), self.sums, &who)
    }

    /// The bundle with no value at each of `paths`.
    fn without(&self, paths: &[&[&str]]) -> serde_json::Value {
        let mut b: serde_json::Value = serde_json::from_str(self.bundle).unwrap();
        for path in paths {
            let (last, parent) = path.split_last().unwrap();
            value_at(&mut b, parent).as_object_mut().unwrap().remove(*last).expect("the bundle has it");
        }
        b
    }

    /// The result of the bundle with the string at `path` changed by `change`.
    fn changed(&self, path: &[&str], change: impl Fn(&str) -> String) -> Result<Logged, String> {
        let mut b: serde_json::Value = serde_json::from_str(self.bundle).unwrap();
        let at = value_at(&mut b, path);
        *at = serde_json::Value::String(change(at.as_str().unwrap()));
        self.check(&b)
    }
}

#[test]
fn each_release_verifies_by_its_promise_or_by_its_proof_and_its_timestamp() {
    let promise: &[&str] = &entry(&["inclusionPromise"]);
    let proof: &[&str] = &entry(&["inclusionProof"]);
    let stamps: &[&str] = &["verificationMaterial", "timestampVerificationData"];
    for r in &RELEASES {
        let promised = Ok(Logged { time: r.promised, index: r.index });
        assert_eq!(r.check(&r.without(&[])), promised, "{}", r.tag);
        assert_eq!(r.check(&r.without(&[proof])), promised, "{}", r.tag);
        assert_eq!(r.check(&r.without(&[stamps])), promised, "{}", r.tag);
        assert_eq!(r.check(&r.without(&[proof, stamps])), promised, "{}", r.tag);
        // Rekor v2's form: the proof and the timestamp, with no promise.
        let stamped = Ok(Logged { time: r.stamped, index: r.index });
        assert_eq!(r.check(&r.without(&[promise])), stamped, "{}", r.tag);
        let why = r.check(&r.without(&[promise, stamps])).unwrap_err();
        assert!(why.contains("proves no time"), "{why}");
        let why = r.check(&r.without(&[promise, proof])).unwrap_err();
        assert!(why.contains("no promise and no inclusion proof"), "{why}");
    }
}

/// A changed hash, root, index or size of the proof: refused, also when
/// the promise is there.
#[test]
fn a_changed_inclusion_proof_is_refused() {
    for r in &RELEASES {
        for path in [entry(&["inclusionProof", "hashes", "0"]), entry(&["inclusionProof", "hashes", "21"])] {
            let why = r.changed(&path, |h| flipped(h, 7)).unwrap_err();
            assert!(why.contains("does not lead to its root"), "{why}");
        }
        let why = r.changed(&entry(&["inclusionProof", "rootHash"]), |h| flipped(h, 31)).unwrap_err();
        assert!(why.contains("does not lead to its root"), "{why}");
        for name in ["logIndex", "treeSize"] {
            let why = r.changed(&entry(&["inclusionProof", name]), |n| (n.parse::<u64>().unwrap() + 1).to_string());
            assert!(why.unwrap_err().contains("the entry's inclusion"), "{}", r.tag);
        }
    }
}

/// A changed origin, size, root or signature of the checkpoint, or a
/// checkpoint with no signature of the log: refused.
#[test]
fn a_changed_checkpoint_is_refused() {
    let path = entry(&["inclusionProof", "checkpoint", "envelope"]);
    for r in &RELEASES {
        let refused = |change: &dyn Fn(&str) -> String, words: &str| {
            let why = r.changed(&path, change).unwrap_err();
            assert!(why.contains("the checkpoint") && why.contains(words), "{why}");
        };
        refused(&|e| e.replacen("rekor.sigstore.dev - ", "rekor.example - ", 1), "its origin");
        refused(&|e| e.replacen("\n3", "\n4", 1), "its size");
        let root = |e: &str| e.split('\n').nth(2).unwrap().to_string();
        refused(&|e| e.replacen(&root(e), &flipped(&root(e), 0), 1), "its root");
        let signature = |e: &str| e.rsplit(' ').next().unwrap().trim_end().to_string();
        refused(&|e| e.replacen(&signature(e), &flipped(&signature(e), 30), 1), "the log's signature");
        refused(&|e| e.replacen(&signature(e), &flipped(&signature(e), 0), 1), "no signature of the log");
        refused(&|e| e.split("\n\n").next().unwrap().to_string() + "\n\n", "no signature of the log");
        refused(&|e| e.replacen("\u{2014} rekor.sigstore.dev ", "\u{2014} witness.example ", 1), "no signature");
    }
}

/// A changed time or imprint of the timestamp, a changed signature of the
/// authority, or another authority: refused, also when the promise is
/// there.
#[test]
fn a_changed_timestamp_is_refused() {
    let der = |text: &str| base64::engine::general_purpose::STANDARD.decode(text).unwrap();
    let encoded = |der: Vec<u8>| base64::engine::general_purpose::STANDARD.encode(der);
    for r in &RELEASES {
        // A second of the genTime of the TSTInfo: its digest changes.
        let time = |t: &str| {
            let mut d = der(t);
            let at = d.windows(2).position(|w| w == [0x18, 0x0f]).unwrap() + 15;
            d[at] = if d[at] == b'0' { b'1' } else { b'0' };
            encoded(d)
        };
        let why = r.changed(&STAMP, time).unwrap_err();
        assert!(why.contains("do not name the TSTInfo"), "{why}");
        // The last byte of the token: the end of its signature.
        let why = r.changed(&STAMP, |t| flipped(t, der(t).len() - 1)).unwrap_err();
        assert!(why.contains("the timestamp's signature"), "{why}");
        let why = r.changed(&STAMP, |t| encoded([der(t), vec![0]].concat())).unwrap_err();
        assert!(why.contains("bytes after its end"), "{why}");
        let mut b: serde_json::Value = serde_json::from_str(r.bundle).unwrap();
        let first = value_at(&mut b, &STAMP[..4]).clone();
        value_at(&mut b, &STAMP[..3]).as_array_mut().unwrap().push(first);
        assert_eq!(r.check(&b), Ok(Logged { time: r.promised, index: r.index }), "two timestamps");
        let who = Identity::github("PodUtils/podcrates", "release.yml", r.tag);
        let fulcio = Roots { tsa: Roots::sigstore().fulcio, ..Roots::sigstore() };
        let why = verify_with(r.bundle.as_bytes(), r.sums, &who, &fulcio).unwrap_err();
        assert!(why.contains("the timestamp's signature"), "{why}");
        let none = Roots { tsa: Vec::new(), ..Roots::sigstore() };
        let why = verify_with(r.bundle.as_bytes(), r.sums, &who, &none).unwrap_err();
        assert!(why.contains("no chain of a timestamp authority"), "{why}");
    }
}

/// The timestamp of one signature is not one of another.
#[test]
fn a_timestamp_of_another_signature_is_refused() {
    let r = &RELEASES[0];
    let mut b: serde_json::Value = serde_json::from_str(r.bundle).unwrap();
    let token = base64::engine::general_purpose::STANDARD.decode(value_at(&mut b, &STAMP).as_str().unwrap()).unwrap();
    let roots = Roots::sigstore();
    let tsa = certificates(&roots.tsa, "the chain").unwrap();
    let why = stamp::check(&token, b"another signature", &tsa).unwrap_err();
    assert!(why.contains("of another signature"), "{why}");
}
