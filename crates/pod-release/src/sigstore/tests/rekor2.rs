//! Bundles of Rekor v2 (sigstore-conformance, `rekor2-happy-path` and
//! `rekor2-checkpoint-origin-not-first`, with the staging trusted root of
//! their test), and the compiled roots against Sigstore's public trusted
//! root (sigstore/root-signing). The files and their licence are in
//! `tests/fixtures/sigstore/`.

use super::super::*;
use super::{flipped, value_at};

const ARTIFACT: &[u8] = include_bytes!("../../../tests/fixtures/sigstore/rekor2/a.txt");
const BUNDLE: &str = include_str!("../../../tests/fixtures/sigstore/rekor2/bundle.sigstore.json");
const WITNESSED: &str = include_str!("../../../tests/fixtures/sigstore/rekor2/origin-not-first.sigstore.json");
const STAGING: &str = include_str!("../../../tests/fixtures/sigstore/rekor2/trusted_root.json");
const PUBLIC: &str = include_str!("../../../tests/fixtures/sigstore/trusted_root.json");

/// The workflow of sigstore-conformance that signed the bundles.
fn who() -> Identity {
    Identity {
        uri: "https://github.com/sigstore-conformance/extremely-dangerous-public-oidc-beacon/.github/workflows/\
              extremely-dangerous-oidc-beacon.yml@refs/heads/main"
            .into(),
        issuer: GITHUB_ISSUER.into(),
    }
}

/// The roots of a trusted root of Sigstore (its JSON): the chain of its
/// last certificate authority, each log of an ECDSA or Ed25519 key with the
/// host of its URL as its name, and the chain of its last timestamp
/// authority.
fn roots_of(trusted_root: &str) -> Roots {
    let root: serde_json::Value = serde_json::from_str(trusted_root).unwrap();
    let ders = |v: &serde_json::Value| -> Vec<Vec<u8>> {
        let certs = v["certChain"]["certificates"].as_array().unwrap();
        certs.iter().map(|c| b64(c["rawBytes"].as_str().unwrap(), "a certificate").unwrap()).collect()
    };
    let logs = root["tlogs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| {
            ["PKIX_ECDSA_P256_SHA_256", "PKIX_ED25519"].contains(&l["publicKey"]["keyDetails"].as_str().unwrap())
        })
        .map(|l| Log {
            name: l["baseUrl"].as_str().unwrap().trim_start_matches("https://").to_string(),
            key: b64(l["publicKey"]["rawBytes"].as_str().unwrap(), "a key").unwrap(),
        })
        .collect();
    let last = |name: &str| root[name].as_array().unwrap().last().unwrap().clone();
    Roots { fulcio: ders(&last("certificateAuthorities")), logs, tsa: ders(&last("timestampAuthorities")) }
}

fn check(bundle: &serde_json::Value) -> Result<Logged, String> {
    verify_with(&serde_json::to_vec(bundle).unwrap(), ARTIFACT, &who(), &roots_of(STAGING))
}

fn parsed(bundle: &str) -> serde_json::Value {
    serde_json::from_str(bundle).unwrap()
}

/// The time of the timestamp (2025-06-12T12:02:20Z) and the index of the
/// entry in the shard `log2025-alpha1`.
const LOGGED: Logged = Logged { time: 1749729740, index: 735 };

#[test]
fn the_compiled_roots_are_those_of_sigstore_s_trusted_root() {
    assert_eq!(roots_of(PUBLIC), Roots::sigstore());
    let ids: Vec<String> = Roots::sigstore().logs.iter().map(|l| crate::hex(&l.id().unwrap())).collect();
    // The log ids of the trusted root.
    assert_eq!(
        ids,
        [
            "c0d23d6ad406973f9559f3ba2d1ca01f84147d8ffc5b8445c224f98b9591801d",
            "cf1199155bddd051268d1f16ac5c0c75c009f6fb5a63f4177f8e18d7051e3fa0"
        ]
    );
}

#[test]
fn a_bundle_of_rekor_v2_verifies_by_its_proof_and_its_timestamp() {
    assert_eq!(check(&parsed(BUNDLE)), Ok(LOGGED));
    // A witness signs the checkpoint first: its line is not read.
    assert_eq!(check(&parsed(WITNESSED)), Ok(LOGGED));
    // The public roots do not name the staging log.
    let why = verify(BUNDLE.as_bytes(), ARTIFACT, &who()).unwrap_err();
    assert!(why.contains("not compiled in"), "{why}");
    let mut other = ARTIFACT.to_vec();
    other[0] ^= 1;
    let why = verify_with(BUNDLE.as_bytes(), &other, &who(), &roots_of(STAGING)).unwrap_err();
    assert!(why.contains("signs another file"), "{why}");
}

#[test]
fn a_bundle_of_rekor_v2_with_a_part_missing_or_changed_is_refused() {
    let entry = ["verificationMaterial", "tlogEntries", "0"];
    let mut b = parsed(BUNDLE);
    value_at(&mut b, &["verificationMaterial"]).as_object_mut().unwrap().remove("timestampVerificationData");
    assert!(check(&b).unwrap_err().contains("proves no time"));
    let mut b = parsed(BUNDLE);
    value_at(&mut b, &entry).as_object_mut().unwrap().remove("inclusionProof");
    assert!(check(&b).unwrap_err().contains("no promise and no inclusion proof"));
    for (path, at, words) in [
        (&["inclusionProof", "hashes", "3"][..], 5, "does not lead to its root"),
        (&["canonicalizedBody"][..], 40, "does not lead to its root"),
    ] {
        let mut b = parsed(BUNDLE);
        let value = value_at(&mut b, &[&entry[..], path].concat());
        *value = flipped(value.as_str().unwrap(), at).into();
        let why = check(&b).unwrap_err();
        assert!(why.contains(words), "{why}");
    }
    let mut b = parsed(BUNDLE);
    let envelope = value_at(&mut b, &[&entry[..], &["inclusionProof", "checkpoint", "envelope"]].concat());
    let text = envelope.as_str().unwrap().to_string();
    let signature = text.rsplit(' ').next().unwrap().trim_end();
    *envelope = text.replacen(signature, &flipped(signature, 40), 1).into();
    let why = check(&b).unwrap_err();
    assert!(why.contains("the log's signature"), "{why}");
}
