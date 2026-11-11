//! The images and the files of the stand-in.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use serde_json::{Value, json};

use super::model::run;
use super::{Seen, Shared, error, http, sync};

/// A made-up fingerprint of `bytes`: 64 hexadecimal digits (incus gives
/// the SHA-256; the tests compare it with the one that the stand-in gives).
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish()).repeat(4)
}

/// The parts of a `multipart/form-data` body: the form name, the file name
/// and the bytes of each.
pub fn multipart(content_type: &str, body: &[u8]) -> Option<Vec<(String, String, Vec<u8>)>> {
    let boundary = content_type.split_once("boundary=")?.1;
    let delimiter = format!("--{boundary}").into_bytes();
    let mut parts = Vec::new();
    let mut at = find(body, &delimiter)? + delimiter.len();
    loop {
        if body[at..].starts_with(b"--") {
            return Some(parts);
        }
        at += 2; // CRLF after the delimiter
        let head_end = at + find(&body[at..], b"\r\n\r\n")?;
        let head = String::from_utf8_lossy(&body[at..head_end]).into_owned();
        let data_start = head_end + 4;
        let next = format!("\r\n--{boundary}").into_bytes();
        let data_end = data_start + find(&body[data_start..], &next)?;
        let param = |key: &str| {
            let marker = format!("{key}=\"");
            head.split_once(&marker).and_then(|(_, rest)| rest.split_once('"')).map(|(v, _)| v.to_string())
        };
        parts.push((param("name")?, param("filename").unwrap_or_default(), body[data_start..data_end].to_vec()));
        at = data_end + next.len();
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// `POST /1.0/images`: the bytes of an image, read in the operation.
pub fn image(shared: &Arc<Shared>, seen: &Seen) -> Vec<u8> {
    let content_type = seen.header("content-type").unwrap_or("application/octet-stream");
    let parts = if content_type.starts_with("multipart/form-data") {
        match multipart(content_type, &seen.body) {
            Some(parts) if parts.len() == 2 && parts[0].0 == "metadata" => parts,
            _ => return error(400, "Invalid multipart image"),
        }
    } else {
        vec![(String::new(), seen.header("x-incus-filename").unwrap_or_default().to_string(), seen.body.clone())]
    };
    let bytes: Vec<u8> = parts.iter().flat_map(|(_, _, b)| b.clone()).collect();
    let made = fingerprint(&bytes);
    let mut state = shared.state.lock().unwrap();
    state.images.push(parts);
    let expected = seen.header("x-incus-fingerprint").map(str::to_string);
    let fail = match expected {
        Some(want) if want != made => Some(format!("fingerprints don't match, got {made} expected {want}")),
        _ => None,
    };
    let kept = state.fail.clone();
    if fail.is_some() {
        state.fail = fail;
    }
    drop(state);
    let answer = run(shared, "task", json!({}), json!({"fingerprint": made, "size": bytes.len().to_string()}));
    shared.state.lock().unwrap().fail = kept;
    answer
}

/// `GET` and `POST /1.0/instances/NAME/files?path=PATH`.
pub fn file(shared: &Arc<Shared>, name: &str, seen: &Seen) -> Vec<u8> {
    let Some(path) = seen.query.get("path").cloned() else { return error(400, "Missing path argument") };
    let mut state = shared.state.lock().unwrap();
    if !state.instances.contains_key(name) {
        return error(404, "Instance not found");
    }
    let key = (name.to_string(), path.clone());
    if seen.method == "POST" {
        let kind = seen.header("x-incus-type").unwrap_or("file").to_string();
        let mode = seen.header("x-incus-mode").unwrap_or("0644").to_string();
        let owner =
            format!("{}:{}", seen.header("x-incus-uid").unwrap_or("0"), seen.header("x-incus-gid").unwrap_or("0"));
        let append = seen.header("x-incus-write") == Some("append");
        let entry = state.files.entry(key).or_insert_with(|| (Vec::new(), kind.clone(), String::new()));
        if !append {
            entry.0.clear();
        }
        entry.0.extend_from_slice(&seen.body);
        entry.1 = kind;
        entry.2 = format!("{mode} {owner}");
        return sync(json!({}));
    }
    let Some((bytes, kind, meta)) = state.files.get(&key).cloned() else {
        return error(404, &format!("Path {path} not found"));
    };
    let (mode, owner) = meta.split_once(' ').unwrap_or(("0644", "0:0"));
    let (uid, gid) = owner.split_once(':').unwrap_or(("0", "0"));
    let headers = [
        ("X-Incus-uid", uid.to_string()),
        ("X-Incus-gid", gid.to_string()),
        ("X-Incus-mode", mode.to_string()),
        ("X-Incus-type", kind.clone()),
    ];
    if kind == "directory" {
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let entries: Vec<Value> = state
            .files
            .keys()
            .filter(|(n, p)| n == name && p.starts_with(&prefix) && !p[prefix.len()..].contains('/'))
            .map(|(_, p)| Value::from(&p[prefix.len()..]))
            .collect();
        let mut named = vec![("Content-Type", "application/json".to_string())];
        named.extend(headers);
        return http(200, &named, super::sync_body(Value::Array(entries)).as_bytes());
    }
    let mut named = vec![("Content-Type", "application/octet-stream".to_string())];
    named.extend(headers);
    http(200, &named, &bytes)
}
