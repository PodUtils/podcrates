//! The bundle of podcrates v0.76.0's `SHA256SUMS`, as its release published
//! it, and each thing that it must refuse. podssh's tests of its own
//! release (v1.0.0-beta.2) have the same form.

use super::*;

mod parts;
mod rekor2;

const SUMS: &[u8] = include_bytes!("../../tests/fixtures/v0.76.0/SHA256SUMS");
const BUNDLE: &str = include_str!("../../tests/fixtures/v0.76.0/SHA256SUMS.sigstore.json");

fn who() -> Identity {
    Identity::github("PodUtils/podcrates", "release.yml", "v0.76.0")
}

/// The value at `path` of `v`; a number in `path` is an index of an array.
fn value_at<'a>(v: &'a mut serde_json::Value, path: &[&str]) -> &'a mut serde_json::Value {
    path.iter().fold(v, |at, key| if let Ok(i) = key.parse::<usize>() { &mut at[i] } else { &mut at[*key] })
}

/// `BUNDLE` with the string at `path` replaced by `with`.
fn bundle_with(path: &[&str], with: &str) -> Vec<u8> {
    let mut b: serde_json::Value = serde_json::from_str(BUNDLE).unwrap();
    *value_at(&mut b, path) = serde_json::Value::String(with.into());
    serde_json::to_vec(&b).unwrap()
}

/// `text`, base64, with one bit of its byte `at` changed.
fn flipped(text: &str, at: usize) -> String {
    let mut bytes = base64::engine::general_purpose::STANDARD.decode(text).unwrap();
    bytes[at] ^= 1;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn field(path: &[&str]) -> String {
    let mut b: serde_json::Value = serde_json::from_str(BUNDLE).unwrap();
    value_at(&mut b, path).as_str().unwrap().to_string()
}

#[test]
fn the_published_bundle_signs_the_published_sums() {
    let logged = verify(BUNDLE.as_bytes(), SUMS, &who()).expect("v0.76.0's bundle verifies");
    assert_eq!(logged, Logged { time: 1795080477, index: 3992748924 });
}

#[test]
fn another_tag_or_another_repository_is_refused() {
    let tag =
        verify(BUNDLE.as_bytes(), SUMS, &Identity::github("PodUtils/podcrates", "release.yml", "v0.75.0")).unwrap_err();
    assert!(
        tag.contains("not https://github.com/PodUtils/podcrates/.github/workflows/release.yml@refs/tags/v0.75.0"),
        "{tag}"
    );
    let repo =
        verify(BUNDLE.as_bytes(), SUMS, &Identity::github("someone/podcrates", "release.yml", "v0.76.0")).unwrap_err();
    assert!(repo.contains("someone/podcrates"), "{repo}");
    let workflow =
        verify(BUNDLE.as_bytes(), SUMS, &Identity::github("PodUtils/podcrates", "ci.yml", "v0.76.0")).unwrap_err();
    assert!(workflow.contains("ci.yml"), "{workflow}");
}

/// The URI of the leaf, with the token of another issuer: refused.
#[test]
fn another_issuer_is_refused() {
    let who = Identity { issuer: "https://accounts.google.com".into(), ..who() };
    let why = verify(BUNDLE.as_bytes(), SUMS, &who).unwrap_err();
    assert!(why.contains("not https://accounts.google.com"), "{why}");
}

#[test]
fn a_changed_file_is_refused() {
    for at in [0, SUMS.len() / 2, SUMS.len() - 1] {
        let mut sums = SUMS.to_vec();
        sums[at] ^= 1;
        assert!(verify(BUNDLE.as_bytes(), &sums, &who()).unwrap_err().contains("signs another file"));
    }
}

#[test]
fn a_changed_signature_promise_or_body_is_refused() {
    let sig = field(&["messageSignature", "signature"]);
    let why = verify(&bundle_with(&["messageSignature", "signature"], &flipped(&sig, 20)), SUMS, &who()).unwrap_err();
    assert!(why.contains("the file's signature"), "{why}");
    let promise_path = ["verificationMaterial", "tlogEntries", "0", "inclusionPromise", "signedEntryTimestamp"];
    let promise = field(&promise_path);
    let why = verify(&bundle_with(&promise_path, &flipped(&promise, 20)), SUMS, &who()).unwrap_err();
    assert!(why.contains("Rekor's promise of the entry"), "{why}");
    let body_path = ["verificationMaterial", "tlogEntries", "0", "canonicalizedBody"];
    let body = field(&body_path);
    let why = verify(&bundle_with(&body_path, &flipped(&body, 40)), SUMS, &who()).unwrap_err();
    assert!(why.contains("Rekor's promise of the entry"), "{why}");
    let time_path = ["verificationMaterial", "tlogEntries", "0", "integratedTime"];
    let why = verify(&bundle_with(&time_path, "1795080478"), SUMS, &who()).unwrap_err();
    assert!(why.contains("Rekor's promise of the entry"), "{why}");
}

/// One changed byte of the bundle's file, in each value that a check
/// reads, at its start, its middle and near its end: refused.
#[test]
fn each_changed_byte_of_a_checked_value_is_refused() {
    let values = [
        field(&["messageSignature", "signature"]),
        field(&["messageSignature", "messageDigest", "digest"]),
        field(&["verificationMaterial", "certificate", "rawBytes"]),
        field(&["verificationMaterial", "tlogEntries", "0", "canonicalizedBody"]),
        field(&["verificationMaterial", "tlogEntries", "0", "inclusionPromise", "signedEntryTimestamp"]),
        field(&["verificationMaterial", "tlogEntries", "0", "logId", "keyId"]),
    ];
    let mut tried = 0;
    for value in values {
        let start = BUNDLE.find(&value).expect("the value is in the file");
        // The last characters of base64 can hold padding bits only: they stay.
        for at in [start, start + value.len() / 2, start + value.len() - 4] {
            let mut bytes = BUNDLE.as_bytes().to_vec();
            bytes[at] = if bytes[at] == b'A' { b'B' } else { b'A' };
            assert!(verify(&bytes, SUMS, &who()).is_err(), "a change at {at} verified");
            tried += 1;
        }
    }
    assert_eq!(tried, 18);
}

#[test]
fn a_leaf_that_fulcio_did_not_sign_is_refused() {
    let leaf_path = ["verificationMaterial", "certificate", "rawBytes"];
    let leaf = field(&leaf_path);
    // A bit of the leaf's signature, near its end, changed.
    let len = base64::engine::general_purpose::STANDARD.decode(&leaf).unwrap().len();
    let why = verify(&bundle_with(&leaf_path, &flipped(&leaf, len - 10)), SUMS, &who()).unwrap_err();
    // The chain's own words: the entry's check of the leaf, later, would
    // refuse it too, and must not be what this test reads.
    assert!(why.contains("'s signature by") && why.contains("sigstore-intermediate"), "{why}");
}

/// Other roots of Fulcio, or another key of Rekor: refused.
#[test]
fn other_roots_refuse_the_bundle() {
    let roots = Roots::sigstore();
    let no_rekor = Roots { logs: Vec::new(), ..roots.clone() };
    assert!(verify_with(BUNDLE.as_bytes(), SUMS, &who(), &no_rekor).is_err());
    let no_chain = Roots { fulcio: Vec::new(), ..roots.clone() };
    let why = verify_with(BUNDLE.as_bytes(), SUMS, &who(), &no_chain).unwrap_err();
    assert!(why.contains("no chain of Fulcio"), "{why}");
    // The root alone, with no intermediate: the leaf's issuer is not it.
    let root_only = Roots { fulcio: roots.fulcio[roots.fulcio.len() - 1..].to_vec(), ..roots };
    let why = verify_with(BUNDLE.as_bytes(), SUMS, &who(), &root_only).unwrap_err();
    assert!(why.contains("was not issued by"), "{why}");
}

#[test]
fn a_document_that_is_not_a_bundle_is_refused() {
    for (bytes, words) in [
        (&b"not json"[..], "not JSON"),
        (&br#"{"mediaType":"application/json"}"#[..], "not a Sigstore bundle"),
        (&br#"{"mediaType":"application/vnd.dev.sigstore.bundle.v0.3+json"}"#[..], "the bundle has no"),
    ] {
        let why = verify(bytes, SUMS, &who()).unwrap_err();
        assert!(why.contains(words), "{why}");
    }
}

/// A leaf of Fulcio has no subject (its signer is in its SAN): each line of
/// the chain names it by its place, and the line of the time gives its
/// window and the time (PodUtils/podssh#334).
#[test]
fn a_leaf_with_no_subject_is_named_by_its_place_with_its_window() {
    let der = b64(&field(&["verificationMaterial", "certificate", "rawBytes"]), "the leaf").unwrap();
    let (_, leaf) = X509Certificate::from_der(&der).unwrap();
    assert_eq!(leaf.subject().to_string(), "");
    let roots = Roots::sigstore();
    let fulcio = certificates(&roots.fulcio, "Fulcio's chain").unwrap();
    let (intermediate, root) = (&fulcio[0], &fulcio[fulcio.len() - 1]);
    // Python: datetime.fromtimestamp(t, timezone.utc) of the leaf's window,
    // 1795080477 to 1795081077.
    let window = "the leaf certificate (valid 2026-11-19T09:27:57Z to 2026-11-19T09:37:57Z)";
    for (time, at) in [(1795080476, "2026-11-19T09:27:56Z"), (1795081078, "2026-11-19T09:37:58Z")] {
        let why = chain::signed_by(&leaf, intermediate, time).unwrap_err();
        assert_eq!(why, format!("{window} did not hold at the time of the signature ({at})"));
    }
    let why = chain::signed_by(&leaf, root, 1795080477).unwrap_err();
    assert_eq!(
        why,
        "the leaf certificate was not issued by O=sigstore.dev, CN=sigstore: it names O=sigstore.dev, CN=sigstore-intermediate"
    );
    assert!(!why.contains("  "), "{why}");
}
