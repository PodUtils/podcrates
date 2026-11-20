//! An entry of a transparency log in a bundle: the log is one that the
//! caller trusts, and each proof that the entry holds verifies: Rekor's
//! promise (its signed entry timestamp over the entry's canonical JSON)
//! and the inclusion proof with its signed checkpoint ([`super::proof`]).
//! Rekor v1 gives both; Rekor v2 gives the proof alone. The body of the
//! entry must name the file's digest, its signature and the leaf.

use x509_parser::prelude::*;

use super::{Log, b64, chain, pem_ders, proof, text};

/// An entry that verified: the time of its promise (none without a
/// promise), its index in the log, and its body.
pub(super) struct Entry {
    pub time: Option<i64>,
    pub index: i64,
    pub body: serde_json::Value,
}

/// A number of the entry, which protobuf's JSON writes as a string.
fn number(entry: &serde_json::Value, name: &str) -> Result<i64, String> {
    let v = entry.get(name).ok_or(format!("the entry has no {name}"))?;
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).ok_or(format!("the entry's {name} is no number"))
}

/// `entry` when it is of a log of `logs`, and each of its proofs verifies;
/// an entry with neither a promise nor an inclusion proof is refused.
pub(super) fn entry(entry: &serde_json::Value, logs: &[Log]) -> Result<Entry, String> {
    let log_id = b64(text(entry, &["logId", "keyId"])?, "the entry's log id")?;
    let (log, id) = logs
        .iter()
        .find_map(|log| log.id().ok().filter(|id| id[..] == log_id[..]).map(|id| (log, id)))
        .ok_or("the entry is of a log whose key is not compiled in")?;
    let index = number(entry, "logIndex")?;
    let body_b64 = text(entry, &["canonicalizedBody"])?;
    let body = b64(body_b64, "the entry's body")?;
    let promise = entry.get("inclusionPromise");
    let inclusion = entry.get("inclusionProof");
    if promise.is_none() && inclusion.is_none() {
        return Err("the entry has no promise and no inclusion proof".into());
    }
    let mut time = None;
    if promise.is_some() {
        let (_, key) = SubjectPublicKeyInfo::from_der(&log.key).map_err(|e| format!("the key of the log: {e}"))?;
        let integrated = number(entry, "integratedTime")?;
        let promise = b64(text(entry, &["inclusionPromise", "signedEntryTimestamp"])?, "the entry's promise")?;
        let signed = canonical(body_b64, integrated, &crate::hex(&log_id), index);
        chain::by_key(&key, signed.as_bytes(), &promise)
            .map_err(|why| format!("Rekor's promise of the entry: {why}"))?;
        time = Some(integrated);
    }
    if let Some(inclusion) = inclusion {
        proof::check(inclusion, &body, log, &id).map_err(|why| format!("the entry's inclusion: {why}"))?;
    }
    let body = serde_json::from_slice(&body).map_err(|e| format!("the entry's body: {e}"))?;
    Ok(Entry { time, index, body })
}

/// The entry's body names the file's digest `digest`, its signature
/// `signature` and the leaf `leaf_der`: a `hashedrekord` of version 0.0.1
/// (Rekor v1) or 0.0.2 (Rekor v2).
pub(super) fn names(body: &serde_json::Value, digest: &[u8], signature: &[u8], leaf_der: &[u8]) -> Result<(), String> {
    if text(body, &["kind"])? != "hashedrekord" {
        return Err("Rekor's entry is not a hashedrekord".into());
    }
    let (logged_digest, logged_signature, logged_leaf) = match text(body, &["apiVersion"])? {
        "0.0.1" => {
            let spec = &body["spec"];
            let leaf = pem_ders(&String::from_utf8_lossy(&b64(
                text(spec, &["signature", "publicKey", "content"])?,
                "the logged key",
            )?));
            let hash = text(spec, &["data", "hash", "value"])?;
            let same = hash == crate::hex(digest) && text(spec, &["data", "hash", "algorithm"])? == "sha256";
            (same, b64(text(spec, &["signature", "content"])?, "the logged signature")?, leaf.into_iter().next())
        }
        "0.0.2" => {
            let spec = &body["spec"]["hashedRekordV002"];
            let hash = b64(text(spec, &["data", "digest"])?, "the logged digest")?;
            let same = hash == digest && text(spec, &["data", "algorithm"])? == "SHA2_256";
            let leaf = b64(text(spec, &["signature", "verifier", "x509Certificate", "rawBytes"])?, "the logged leaf")?;
            (same, b64(text(spec, &["signature", "content"])?, "the logged signature")?, Some(leaf))
        }
        other => return Err(format!("Rekor's entry is of the version {other}")),
    };
    if !logged_digest || logged_signature != signature || logged_leaf.as_deref() != Some(leaf_der) {
        return Err("Rekor's entry is of another file, signature or leaf".into());
    }
    Ok(())
}

/// The JSON that Rekor signs: its keys sorted, no spaces. Written by hand:
/// a `serde_json::Map` keeps the order of insertion when a crate of the
/// same binary turns on the feature `preserve_order`.
pub(super) fn canonical(body: &str, time: i64, log_id: &str, index: i64) -> String {
    let quoted = |s: &str| serde_json::Value::from(s).to_string();
    format!("{{\"body\":{},\"integratedTime\":{time},\"logID\":{},\"logIndex\":{index}}}", quoted(body), quoted(log_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys in the order of their bytes, whatever the features of
    /// serde_json.
    #[test]
    fn the_canonical_json_has_its_keys_sorted() {
        let made = canonical("Ym9keQ==", 1795080477, "c0d2", 3992748924);
        assert_eq!(made, r#"{"body":"Ym9keQ==","integratedTime":1795080477,"logID":"c0d2","logIndex":3992748924}"#);
    }
}
