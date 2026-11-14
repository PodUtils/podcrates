//! The cache store through its public API: the signature, the random
//! source and the ids, the limit of the uploads, the blocks, a `Range` and
//! the answer as a file.

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use pod_actions_results::cache::{BLOBS, CacheStore, LIMIT, SERVICE};
use pod_actions_results::{CONTENT_BLOB, Payload};
use pod_core::entropy::{Entropy, NoRandom};
use serde_json::{Value, json};

const BASE: &str = "http://h/";

/// A directory of one test, removed at the end.
struct Scratch(PathBuf);

impl Scratch {
    fn new(what: &str) -> Scratch {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("pod-actions-results-cache-{what}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn call(s: &CacheStore, method: &str, v: Value) -> Value {
    let (status, body, _) =
        s.route("POST", &format!("{SERVICE}{method}"), v.to_string().as_bytes(), None, BASE).unwrap();
    assert_eq!(status, 200);
    serde_json::from_slice(&body).unwrap()
}

fn local(url: &str) -> String {
    format!("/{}", url.strip_prefix(BASE).unwrap())
}

/// The path of the upload URL of a new entry.
fn create(s: &CacheStore, key: &str) -> String {
    let v = call(s, "CreateCacheEntry", json!({ "key": key, "version": "v1" }));
    assert_eq!(v["ok"], true, "{v}");
    local(v["signed_upload_url"].as_str().unwrap())
}

fn finalize(s: &CacheStore, key: &str, size: usize) -> Value {
    call(s, "FinalizeCacheEntryUpload", json!({ "key": key, "version": "v1", "size_bytes": size.to_string() }))
}

/// An entry `key` with `bytes`, in one whole blob.
fn save(s: &CacheStore, key: &str, bytes: &[u8]) -> Value {
    let url = create(s, key);
    assert_eq!(s.route("PUT", &url, bytes, None, BASE).unwrap().0, 201);
    finalize(s, key, bytes.len())
}

/// The path of the download URL of the entry `key`.
fn download_url(s: &CacheStore, key: &str) -> String {
    let v = call(s, "GetCacheEntryDownloadURL", json!({ "key": key, "version": "v1" }));
    assert_eq!(v["ok"], true, "{v}");
    local(v["signed_download_url"].as_str().unwrap())
}

/// The URL with the last character of its signature changed.
fn flip_last(url: &str) -> String {
    let mut s = url.to_string();
    let last = s.pop().unwrap();
    s.push(if last == '0' { '1' } else { '0' });
    s
}

#[test]
fn the_signature_of_the_cache_is_compared_in_full() {
    let t = Scratch::new("sig");
    let s = CacheStore::new(&t.0, LIMIT).unwrap();
    let up = create(&s, "k");
    assert!(up.starts_with(&format!("{BLOBS}upload/")), "{up}");
    assert_eq!(s.route("PUT", &flip_last(&up), b"x", None, BASE).unwrap().0, 403, "the last character counts");
    assert_eq!(s.route("PUT", &up, b"abc", None, BASE).unwrap().0, 201);
    assert_eq!(finalize(&s, "k", 3)["ok"], true);
    let down = download_url(&s, "k");
    assert_eq!(s.route("GET", &flip_last(&down), b"", None, BASE).unwrap().0, 403);
    let cut = &down[..down.len() - 1];
    assert_eq!(s.route("GET", cut, b"", None, BASE).unwrap().0, 403, "a shorter signature fails");
}

#[test]
fn each_entry_has_a_random_id_and_no_random_bytes_gives_no_url() {
    let t = Scratch::new("ids");
    let a = CacheStore::new(&t.0, LIMIT).unwrap();
    let first = save(&a, "a", b"1");
    let b = CacheStore::new(&t.0, LIMIT).unwrap();
    let second = save(&b, "b", b"2");
    let (x, y) = (first["entry_id"].as_str().unwrap(), second["entry_id"].as_str().unwrap());
    assert_ne!(x, y, "two runs over one store give two ids");
    assert!(x.parse::<i64>().is_ok() && y.parse::<i64>().is_ok(), "an int64 of protobuf: {x}, {y}");
    struct Dry;
    impl Entropy for Dry {
        fn fill(&mut self, _: &mut [u8]) -> Result<(), NoRandom> {
            Err(NoRandom)
        }
    }
    let dry = CacheStore::with_entropy(&t.0, LIMIT, Dry).unwrap();
    let v = call(&dry, "CreateCacheEntry", json!({ "key": "c", "version": "v1" }));
    assert_eq!(v["ok"], false, "{v}");
    assert!(v.get("signed_upload_url").is_none());
}

#[test]
fn the_uploads_hold_the_limit_and_a_block_list_joins_its_blocks() {
    let t = Scratch::new("blocks");
    let s = CacheStore::new(&t.0, 10).unwrap();
    let up = create(&s, "k");
    for (id, part) in [("YQ==", &b"01234"[..]), ("Yg==", b"56789")] {
        let url = format!("{up}&comp=block&blockid={id}");
        assert_eq!(s.route("PUT", &url, part, None, BASE).unwrap().0, 201);
    }
    let other = create(&s, "other");
    assert_eq!(s.route("PUT", &other, b"x", None, BASE).unwrap().0, 413, "the blocks of an upload hold the limit");
    let long = "a".repeat(89);
    assert_eq!(s.route("PUT", &format!("{up}&comp=block&blockid={long}"), b"", None, BASE).unwrap().0, 400);
    assert_eq!(s.route("PUT", &format!("{up}&comp=block"), b"", None, BASE).unwrap().0, 400);
    assert_eq!(s.route("PUT", &format!("{up}&comp=page"), b"", None, BASE).unwrap().0, 400);
    let list = "<BlockList><Latest>YQ==</Latest><Latest>Zg==</Latest></BlockList>";
    assert_eq!(s.route("PUT", &format!("{up}&comp=blocklist"), list.as_bytes(), None, BASE).unwrap().0, 400);
    let list = "<BlockList><Latest>Yg==</Latest><Latest>YQ==</Latest></BlockList>";
    assert_eq!(s.route("PUT", &format!("{up}&comp=blocklist"), list.as_bytes(), None, BASE).unwrap().0, 201);
    assert_eq!(finalize(&s, "k", 10)["ok"], true);
    let (status, body, _) = s.route("GET", &download_url(&s, "k"), b"", None, BASE).unwrap();
    assert_eq!((status, body.as_slice()), (200, &b"5678901234"[..]), "the blocks in the order of the list");
    // A finalized upload gives its bytes back.
    assert_eq!(s.route("PUT", &other, b"x", None, BASE).unwrap().0, 201);
}

#[test]
fn a_range_of_the_cache_is_a_part_of_a_file_to_stream() {
    let t = Scratch::new("range");
    let s = CacheStore::new(&t.0, LIMIT).unwrap();
    save(&s, "k", b"0123456789");
    let url = download_url(&s, "k");
    let a = s.answer("GET", &url, b"", Some("bytes=3-5"), BASE).unwrap();
    assert_eq!((a.status, a.content_type), (206, CONTENT_BLOB));
    assert!(a.headers.contains(&("Content-Range", "bytes 3-5/10".to_string())), "{:?}", a.headers);
    let Payload::File(f, len) = a.body else { panic!("a file") };
    assert_eq!(len, 3);
    let mut rest = String::new();
    f.take(len).read_to_string(&mut rest).unwrap();
    assert_eq!(rest, "345", "the file is at the first byte of the range");
    let (status, _, headers) = s.route("GET", &url, b"", Some("bytes=5-2"), BASE).unwrap();
    assert_eq!(status, 416);
    assert!(headers.contains(&("Content-Range", "bytes */10".to_string())), "{headers:?}");
    assert_eq!(s.route("GET", &url, b"", Some("bytes=9-9"), BASE).unwrap().1, b"9");
}

#[test]
fn a_key_that_holds_a_newline_is_not_another_entry() {
    let t = Scratch::new("names");
    let s = CacheStore::new(&t.0, LIMIT).unwrap();
    let v = call(&s, "CreateCacheEntry", json!({ "key": "x\ny", "version": "v" }));
    let up = local(v["signed_upload_url"].as_str().unwrap());
    assert_eq!(s.route("PUT", &up, b"1", None, BASE).unwrap().0, 201);
    let f = call(&s, "FinalizeCacheEntryUpload", json!({ "key": "x\ny", "version": "v", "size_bytes": "1" }));
    assert_eq!(f["ok"], true, "{f}");
    let v = call(&s, "CreateCacheEntry", json!({ "key": "x", "version": "y\nv" }));
    assert_eq!(v["ok"], true, "the key and the version are not joined: {v}");
    let long = "k".repeat(513);
    assert_eq!(call(&s, "CreateCacheEntry", json!({ "key": long, "version": "v" }))["ok"], false);
    assert_eq!(call(&s, "CreateCacheEntry", json!({ "key": "k", "version": "" }))["ok"], false);
    let (status, _, _) = s.route("GET", &format!("{SERVICE}CreateCacheEntry"), b"", None, BASE).unwrap();
    assert_eq!(status, 405);
}
