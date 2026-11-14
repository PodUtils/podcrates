use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Value, json};

use super::*;
use crate::wire::{rfc3339, sha256};

const BASE: &str = "http://h/";

fn store(limit: u64) -> (ArtifactStore, PathBuf) {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "podrunner-artifacts-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    (ArtifactStore::new(&dir, limit).unwrap(), dir)
}

fn call(s: &ArtifactStore, method: &str, v: Value) -> (u16, Value) {
    let (status, body) = s.route("POST", &format!("{SERVICE}{method}"), v.to_string().as_bytes(), BASE).unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

/// The path and query of a signed URL of this broker.
fn local(url: &str) -> String {
    format!("/{}", url.strip_prefix(BASE).unwrap())
}

/// Create, upload in two blocks with a block list, and finalize.
fn upload(s: &ArtifactStore, name: &str, bytes: &[u8]) -> (u16, Value) {
    let (status, v) = call(s, "CreateArtifact", json!({ "name": name, "version": 4 }));
    assert_eq!(status, 200, "{v}");
    let url = local(v["signed_upload_url"].as_str().unwrap());
    let (a, b) = bytes.split_at(bytes.len() / 2);
    // The client of the blobs sends base64 block ids, percent-encoded.
    for (id, part) in [("QUFB", a), ("QkJC%3D%3D", b)] {
        let put = s.route("PUT", &format!("{url}&comp=block&blockid={id}"), part, BASE).unwrap();
        assert_eq!(put.0, 201);
    }
    let list = b"<?xml version=\"1.0\"?><BlockList><Latest>QUFB</Latest><Latest>QkJC==</Latest></BlockList>";
    assert_eq!(s.route("PUT", &format!("{url}&comp=blocklist"), list, BASE).unwrap().0, 201);
    let hash = sha256(bytes);
    call(s, "FinalizeArtifact", json!({ "name": name, "size": bytes.len().to_string(), "hash": { "value": hash } }))
}

#[test]
fn an_artifact_goes_up_in_blocks_and_comes_down_whole() {
    let (s, dir) = store(LIMIT);
    let data = b"the zip of the client, 42".to_vec();
    let (status, v) = upload(&s, "data", &data);
    assert_eq!((status, v["ok"].as_bool()), (200, Some(true)), "{v}");
    let (_, list) = call(&s, "ListArtifacts", json!({ "name_filter": { "value": "data" } }));
    let a = &list["artifacts"][0];
    assert_eq!((a["name"].as_str(), a["size"].as_str()), (Some("data"), Some("25")));
    assert_eq!(a["digest"], sha256(&data).as_str(), "a wrapper is a plain string");
    assert!(a.get("sig").is_none(), "the list gives no signature: {a}");
    assert_eq!(a["created_at"].as_str().unwrap().len(), 20);
    let (_, url) = call(&s, "GetSignedArtifactURL", json!({ "name": "data" }));
    let url = local(url["signed_url"].as_str().unwrap());
    assert_eq!(s.route("GET", &url, b"", BASE).unwrap(), (200, data.clone()));
    let bad = url.replace("sig=", "sig=x");
    assert_eq!(s.route("GET", &bad, b"", BASE).unwrap().0, 403, "a wrong signature");
    assert_eq!(fs::read(dir.join("data").join("artifact.zip")).unwrap(), data, "one directory for each name");
    let (status, _) = call(&s, "DeleteArtifact", json!({ "name": "data" }));
    assert_eq!(status, 200);
    assert_eq!(call(&s, "ListArtifacts", json!({})).1["artifacts"], json!([]));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_second_artifact_of_one_name_is_refused() {
    let (s, dir) = store(LIMIT);
    assert_eq!(upload(&s, "data", b"one").0, 200);
    let (status, v) = call(&s, "CreateArtifact", json!({ "name": "data", "version": 4 }));
    assert_eq!((status, v["code"].as_str()), (409, Some("already_exists")), "{v}");
    for bad in ["", "a/b", "..", "a:b", "a\nb"] {
        assert_eq!(call(&s, "CreateArtifact", json!({ "name": bad })).0, 400, "{bad:?}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_wrong_digest_or_size_fails_the_finalize() {
    let (s, dir) = store(LIMIT);
    let (_, v) = call(&s, "CreateArtifact", json!({ "name": "data" }));
    let url = local(v["signed_upload_url"].as_str().unwrap());
    assert_eq!(s.route("PUT", &url, b"bytes", BASE).unwrap().0, 201, "one whole blob");
    let wrong = sha256(b"other");
    let (status, v) = call(&s, "FinalizeArtifact", json!({ "name": "data", "size": "5", "hash": { "value": wrong } }));
    assert_eq!(status, 400);
    assert!(v["msg"].as_str().unwrap().contains("hash"), "{v}");
    assert_eq!(call(&s, "FinalizeArtifact", json!({ "name": "data", "size": "6" })).0, 400, "a wrong size");
    assert_eq!(call(&s, "ListArtifacts", json!({})).1["artifacts"], json!([]), "nothing is kept");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_upload_needs_its_signature_and_the_run_has_a_limit() {
    let (s, dir) = store(8);
    let (_, v) = call(&s, "CreateArtifact", json!({ "name": "data" }));
    let url = local(v["signed_upload_url"].as_str().unwrap());
    assert_eq!(s.route("PUT", &url.replace("sig=", "sig=x"), b"x", BASE).unwrap().0, 403);
    assert_eq!(s.route("PUT", &url, b"12345678", BASE).unwrap().0, 201);
    assert_eq!(s.route("PUT", &url, b"9", BASE).unwrap().0, 413, "over the limit of the run");
    assert!(s.route("GET", "/message", b"", BASE).is_none(), "another route");
    assert_eq!(call(&s, "NoSuchMethod", json!({})).1["code"], "bad_route");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_time_is_rfc3339_in_utc() {
    assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
    assert_eq!(rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
    assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
}

/// The zip of a finalized artifact, for a view of the run; nothing for an
/// artifact that is not finalized or a name with a path part.
#[test]
fn the_zip_of_a_finalized_artifact_and_of_nothing_else() {
    let (s, dir) = store(LIMIT);
    let data = b"the zip of the client".to_vec();
    assert_eq!(upload(&s, "data", &data).0, 200);
    assert_eq!(s.zip_of("data"), Some(data));
    let (status, _) = call(&s, "CreateArtifact", json!({ "name": "open", "version": 4 }));
    assert_eq!(status, 200);
    // A zip with no record: a finalize that stopped between its two renames.
    fs::create_dir_all(dir.join("half")).unwrap();
    fs::write(dir.join("half").join("artifact.zip"), b"half").unwrap();
    for name in ["half", "open", "nosuch", "", "..", "../data", "data/artifact.zip", "/etc"] {
        assert_eq!(s.zip_of(name), None, "{name:?}");
    }
    let _ = fs::remove_dir_all(&dir);
}
