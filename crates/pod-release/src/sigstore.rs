//! The keyless signature of a release's `SHA256SUMS`, as `cosign sign-blob
//! --bundle` makes it in a release workflow: a Sigstore bundle whose leaf
//! certificate Fulcio gave the workflow at the tag, whose signature a
//! transparency log (Rekor v1 or v2) logged, and whose signature a
//! timestamp authority can have timed. The check needs no network and no
//! C: the digest, the leaf's signature of the file, an entry of a log that
//! the roots name with each proof that it holds (Rekor's promise, the
//! inclusion proof with its signed checkpoint), each RFC 3161 timestamp by
//! the timestamp authority of the roots, the leaf's chain to Fulcio's root
//! at each time that the bundle proves, the leaf's identity (the caller's
//! [`Identity`]), and the entry's body against the file, the signature and
//! the leaf. The roots of Sigstore's public instance are compiled in
//! ([`Roots::sigstore`]); a caller can give others.
//!
//! The base is podssh's `update/sigstore.rs`; podnet's `update/cosign.rs`
//! made the same checks with no check of Rekor.

mod chain;
mod proof;
mod rekor;
mod stamp;

use base64::Engine as _;
use x509_parser::prelude::*;

pub use chain::{ISSUER_V1, ISSUER_V2};

/// The issuer of the OIDC tokens of GitHub Actions, which Fulcio writes in
/// the leaf.
pub const GITHUB_ISSUER: &str = "https://token.actions.githubusercontent.com";

/// Who must have signed: the URI of the leaf's subject alternative name
/// (for a workflow of GitHub Actions, the workflow file at its ref) and the
/// issuer of the OIDC token. The ruling "Services: what stays" keeps the
/// identity of each product with the product: the caller gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub uri: String,
    pub issuer: String,
}

impl Identity {
    /// The workflow `workflow` (a file name in `.github/workflows/`) of the
    /// GitHub repository `repo` (`OWNER/REPO`) at the tag `tag`, with the
    /// issuer of GitHub Actions.
    pub fn github(repo: &str, workflow: &str, tag: &str) -> Identity {
        Identity {
            uri: format!("https://github.com/{repo}/.github/workflows/{workflow}@refs/tags/{tag}"),
            issuer: GITHUB_ISSUER.to_string(),
        }
    }
}

// Sigstore's public trusted root (sigstore/root-signing,
// `targets/trusted_root.json`; the crate's SOURCES names its commit, and a
// test compares these files with it).
const FULCIO: &str = include_str!("sigstore/fulcio.pem");
const REKOR: &str = include_str!("sigstore/rekor.pem");
const REKOR_2025: &str = include_str!("sigstore/rekor-log2025-1.pem");
const TSA: &str = include_str!("sigstore/tsa.pem");

/// A transparency log: its name (the host of its URL, which its
/// checkpoints name) and its key (a SubjectPublicKeyInfo in DER).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Log {
    pub name: String,
    pub key: Vec<u8>,
}

impl Log {
    /// The id of the log, as a bundle names it: for an Ed25519 key (Rekor
    /// v2), the key id of a signed note: SHA-256 of the name, a newline,
    /// the byte 1 and the key; for another key (Rekor v1), SHA-256 of the
    /// key's DER.
    pub fn id(&self) -> Result<[u8; 32], String> {
        let (_, key) =
            SubjectPublicKeyInfo::from_der(&self.key).map_err(|e| format!("the key of the log {}: {e}", self.name))?;
        if key.algorithm.algorithm.to_id_string() != chain::ED25519 {
            return Ok(crate::sha256(&self.key));
        }
        let mut named = format!("{}\n\u{1}", self.name).into_bytes();
        named.extend_from_slice(&key.subject_public_key.data);
        Ok(crate::sha256(&named))
    }
}

/// What a bundle is checked against: Fulcio's chain, from the leaf's issuer
/// to its root, each in DER; the logs; and the chain of the timestamp
/// authority, from its leaf to its root, each in DER.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    pub fulcio: Vec<Vec<u8>>,
    pub logs: Vec<Log>,
    pub tsa: Vec<Vec<u8>>,
}

impl Roots {
    /// The roots of Sigstore's public instance, compiled in, from its
    /// trusted root: Fulcio, Rekor v1 (`rekor.sigstore.dev`), the shard
    /// `log2025-1` of Rekor v2, and the timestamp authority.
    pub fn sigstore() -> Roots {
        Roots::from_pem(FULCIO, &[("rekor.sigstore.dev", REKOR), ("log2025-1.rekor.sigstore.dev", REKOR_2025)], TSA)
    }

    /// The chain's certificates, each log's name and key, and the chain of
    /// the timestamp authority, from PEM text.
    pub fn from_pem(fulcio: &str, logs: &[(&str, &str)], tsa: &str) -> Roots {
        let logs = logs
            .iter()
            .filter_map(|(name, pem)| pem_ders(pem).into_iter().next().map(|key| Log { name: name.to_string(), key }))
            .collect();
        Roots { fulcio: pem_ders(fulcio), logs, tsa: pem_ders(tsa) }
    }
}

/// What a bundle that verified proves: the time of the signature (in
/// seconds since the epoch: Rekor's promise, or else the first timestamp),
/// and the index of the entry in its log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Logged {
    pub time: i64,
    pub index: i64,
}

fn b64(text: &str, what: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD.decode(text.trim()).map_err(|e| format!("{what} is not base64: {e}"))
}

fn text<'a>(v: &'a serde_json::Value, path: &[&str]) -> Result<&'a str, String> {
    let mut at = v;
    for key in path {
        at = at.get(*key).ok_or_else(|| format!("the bundle has no {}", path.join(".")))?;
    }
    at.as_str().ok_or_else(|| format!("{} is not a string", path.join(".")))
}

/// The DER of each block of a PEM text.
fn pem_ders(pem: &str) -> Vec<Vec<u8>> {
    x509_parser::pem::Pem::iter_from_buffer(pem.as_bytes()).filter_map(Result::ok).map(|p| p.contents).collect()
}

/// The certificates of `ders`.
fn certificates<'a>(ders: &'a [Vec<u8>], what: &str) -> Result<Vec<X509Certificate<'a>>, String> {
    ders.iter()
        .map(|der| X509Certificate::from_der(der).map(|(_, c)| c))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("{what}: {e}"))
}

/// The time of each RFC 3161 timestamp of the bundle `b` of the signature
/// `signature`; each must verify by the timestamp authority of `roots`.
fn timestamps(b: &serde_json::Value, signature: &[u8], roots: &Roots) -> Result<Vec<i64>, String> {
    let Some(list) = b["verificationMaterial"]["timestampVerificationData"]["rfc3161Timestamps"].as_array() else {
        return Ok(Vec::new());
    };
    let tsa = certificates(&roots.tsa, "the chain of the timestamp authority")?;
    list.iter().map(|t| stamp::check(&b64(text(t, &["signedTimestamp"])?, "a timestamp")?, signature, &tsa)).collect()
}

/// Whether `bundle` signs `artifact`, as `who`, by Sigstore's roots.
pub fn verify(bundle: &[u8], artifact: &[u8], who: &Identity) -> Result<Logged, String> {
    verify_with(bundle, artifact, who, &Roots::sigstore())
}

/// [`verify()`] by `roots`. The bundle needs an entry of a log of `roots`
/// with Rekor's promise or an inclusion proof, and a time: the promise's,
/// or an RFC 3161 timestamp. Each proof and each timestamp that the bundle
/// holds must verify (call C820).
pub fn verify_with(bundle: &[u8], artifact: &[u8], who: &Identity, roots: &Roots) -> Result<Logged, String> {
    let b: serde_json::Value = serde_json::from_slice(bundle).map_err(|e| format!("the bundle is not JSON: {e}"))?;
    if !text(&b, &["mediaType"])?.starts_with("application/vnd.dev.sigstore.bundle") {
        return Err("not a Sigstore bundle".into());
    }
    let digest = crate::sha256(artifact);
    // The bundle names its digest as the protobuf enum SHA2_256 does.
    if !text(&b, &["messageSignature", "messageDigest", "algorithm"])?.eq_ignore_ascii_case("sha2_256")
        || b64(text(&b, &["messageSignature", "messageDigest", "digest"])?, "the digest")? != digest
    {
        return Err("the bundle signs another file".into());
    }
    let signature = b64(text(&b, &["messageSignature", "signature"])?, "the signature")?;
    let material = &b["verificationMaterial"];
    let leaf_b64 = text(material, &["certificate", "rawBytes"])
        .ok()
        .or_else(|| material["x509CertificateChain"]["certificates"][0]["rawBytes"].as_str())
        .ok_or("the bundle has no leaf certificate")?;
    let leaf_der = b64(leaf_b64, "the leaf")?;
    let (_, leaf) = X509Certificate::from_der(&leaf_der).map_err(|e| format!("the leaf: {e}"))?;
    chain::by_key(leaf.public_key(), artifact, &signature).map_err(|why| format!("the file's signature: {why}"))?;
    let entries = material["tlogEntries"].as_array().ok_or("the bundle has no entry of Rekor")?;
    let mut refused = Vec::new();
    let mut found = None;
    for entry in entries {
        match rekor::entry(entry, &roots.logs) {
            Ok(ok) => {
                found = Some(ok);
                break;
            }
            Err(why) => refused.push(why),
        }
    }
    let Some(entry) = found else {
        return Err(if refused.is_empty() { "the bundle has no entry of Rekor".into() } else { refused.join("; ") });
    };
    let times: Vec<i64> = entry.time.into_iter().chain(timestamps(&b, &signature, roots)?).collect();
    let Some(&time) = times.first() else {
        return Err("the bundle proves no time: it has no promise of Rekor and no timestamp".into());
    };
    let chain = certificates(&roots.fulcio, "Fulcio's chain")?;
    let Some(root) = chain.last() else { return Err("no chain of Fulcio to check the leaf by".into()) };
    // Each certificate by the next, the leaf first; the root by itself; at
    // each time that the bundle proves.
    for at in times {
        let mut cert = &leaf;
        for issuer in &chain {
            chain::signed_by(cert, issuer, at)?;
            cert = issuer;
        }
        chain::signed_by(root, root, at)?;
    }
    chain::identity(&leaf, who)?;
    rekor::names(&entry.body, &digest, &signature, &leaf_der)?;
    Ok(Logged { time, index: entry.index })
}

#[cfg(test)]
mod tests;
