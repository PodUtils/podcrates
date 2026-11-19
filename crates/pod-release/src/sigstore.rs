//! The keyless signature of a release's `SHA256SUMS`, as `cosign sign-blob
//! --bundle` makes it in a release workflow: a Sigstore bundle whose leaf
//! certificate Fulcio gave the workflow at the tag, and whose signature
//! Rekor logged. The check needs no network and no C: the digest, the
//! leaf's signature of the file, the leaf's chain to Fulcio's root at the
//! time that Rekor gives, the leaf's identity (the caller's
//! [`Identity`]), Rekor's promise of the entry (its signed entry
//! timestamp), and the entry's body against the file, the signature and
//! the leaf. The roots of Sigstore's public instance are compiled in
//! ([`Roots::sigstore`]); a caller can give others.
//!
//! The base is podssh's `update/sigstore.rs`; podnet's `update/cosign.rs`
//! made the same checks with no check of Rekor.

mod chain;
mod rekor;

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

const FULCIO: &str = include_str!("sigstore/fulcio.pem");
const REKOR: &str = include_str!("sigstore/rekor.pem");

/// What a bundle is checked against: Fulcio's chain, from the leaf's issuer
/// to its root, each in DER, and Rekor's key (a SubjectPublicKeyInfo in
/// DER).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    pub fulcio: Vec<Vec<u8>>,
    pub rekor: Vec<u8>,
}

impl Roots {
    /// The roots of Sigstore's public instance, compiled in, from its
    /// trusted root (sigstore/root-signing).
    pub fn sigstore() -> Roots {
        Roots::from_pem(FULCIO, REKOR)
    }

    /// The chain's certificates and Rekor's key, from PEM text.
    pub fn from_pem(fulcio: &str, rekor: &str) -> Roots {
        Roots { fulcio: pem_ders(fulcio), rekor: pem_ders(rekor).into_iter().next().unwrap_or_default() }
    }
}

/// What Rekor said of a bundle that verified: when it logged the entry (in
/// seconds since the epoch), and its index in the log.
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

/// Whether `bundle` signs `artifact`, as `who`, by Sigstore's roots.
pub fn verify(bundle: &[u8], artifact: &[u8], who: &Identity) -> Result<Logged, String> {
    verify_with(bundle, artifact, who, &Roots::sigstore())
}

/// [`verify`] by `roots`.
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
    let algorithm = if chain::curve(leaf.public_key()).as_deref() == Some(chain::P384) {
        chain::ECDSA_SHA384
    } else {
        chain::ECDSA_SHA256
    };
    chain::ecdsa(leaf.public_key(), algorithm, artifact, &signature)
        .map_err(|why| format!("the file's signature: {why}"))?;
    let entries = material["tlogEntries"].as_array().ok_or("the bundle has no entry of Rekor")?;
    let mut refused = Vec::new();
    let mut found = None;
    for entry in entries {
        match rekor::promise(entry, &roots.rekor) {
            Ok(ok) => {
                found = Some(ok);
                break;
            }
            Err(why) => refused.push(why),
        }
    }
    let Some((logged, body)) = found else {
        return Err(if refused.is_empty() { "the bundle has no entry of Rekor".into() } else { refused.join("; ") });
    };
    let chain: Vec<X509Certificate<'_>> = roots
        .fulcio
        .iter()
        .map(|der| X509Certificate::from_der(der).map(|(_, c)| c))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("Fulcio's chain: {e}"))?;
    let Some(root) = chain.last() else { return Err("no chain of Fulcio to check the leaf by".into()) };
    // Each certificate by the next, the leaf first; the root by itself.
    let mut cert = &leaf;
    for issuer in &chain {
        chain::signed_by(cert, issuer, logged.time)?;
        cert = issuer;
    }
    chain::signed_by(root, root, logged.time)?;
    chain::identity(&leaf, who)?;
    // The entry logged this file, this signature and this leaf.
    let spec = &body["spec"];
    let logged_leaf = pem_ders(&String::from_utf8_lossy(&b64(
        text(spec, &["signature", "publicKey", "content"])?,
        "the logged key",
    )?));
    if text(spec, &["data", "hash", "value"])? != crate::hex(&digest)
        || b64(text(spec, &["signature", "content"])?, "the logged signature")? != signature
        || logged_leaf.first() != Some(&leaf_der)
    {
        return Err("Rekor's entry is of another file, signature or leaf".into());
    }
    Ok(logged)
}

#[cfg(test)]
mod tests;
