//! Rekor's promise of an entry: its signed entry timestamp over the
//! entry's canonical JSON, checked with Rekor's key.

use x509_parser::prelude::*;

use super::{Logged, b64, chain, text};

/// The entry's time and index, and its body, when its promise verifies by
/// `key_der`.
pub(super) fn promise(entry: &serde_json::Value, key_der: &[u8]) -> Result<(Logged, serde_json::Value), String> {
    let (_, key) = SubjectPublicKeyInfo::from_der(key_der).map_err(|e| format!("Rekor's key: {e}"))?;
    let log_id = b64(text(entry, &["logId", "keyId"])?, "the entry's log id")?;
    if log_id != crate::sha256(key_der) {
        return Err("the entry is of a log whose key is not compiled in".into());
    }
    let number = |name: &str| -> Result<i64, String> {
        let v = entry.get(name).ok_or(format!("the entry has no {name}"))?;
        v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).ok_or(format!("the entry's {name} is no number"))
    };
    let (time, index) = (number("integratedTime")?, number("logIndex")?);
    let body = text(entry, &["canonicalizedBody"])?;
    let promise = b64(text(entry, &["inclusionPromise", "signedEntryTimestamp"])?, "the entry's promise")?;
    chain::ecdsa(&key, chain::ECDSA_SHA256, canonical(body, time, &crate::hex(&log_id), index).as_bytes(), &promise)
        .map_err(|why| format!("Rekor's promise of the entry: {why}"))?;
    let body = serde_json::from_slice(&b64(body, "the entry's body")?).map_err(|e| format!("the entry's body: {e}"))?;
    Ok((Logged { time, index }, body))
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
