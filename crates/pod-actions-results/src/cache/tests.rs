use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

const BASE: &str = "http://h/";

fn store(limit: u64) -> (CacheStore, PathBuf) {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "pod-actions-results-cache-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    (CacheStore::new(&dir, limit).unwrap(), dir)
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

fn save(s: &CacheStore, key: &str, version: &str, bytes: &[u8]) -> Value {
    let v = call(s, "CreateCacheEntry", json!({ "key": key, "version": version }));
    if v["ok"] != true {
        return v;
    }
    let url = local(v["signed_upload_url"].as_str().unwrap());
    assert_eq!(s.route("PUT", &url, bytes, None, BASE).unwrap().0, 201);
    call(
        s,
        "FinalizeCacheEntryUpload",
        json!({ "key": key, "version": version, "size_bytes": bytes.len().to_string() }),
    )
}

fn restore(s: &CacheStore, key: &str, keys: &[&str], version: &str) -> Option<(String, Vec<u8>)> {
    let v = call(s, "GetCacheEntryDownloadURL", json!({ "key": key, "restore_keys": keys, "version": version }));
    if v["ok"] != true {
        return None;
    }
    let (status, body, _) =
        s.route("GET", &local(v["signed_download_url"].as_str().unwrap()), b"", None, BASE).unwrap();
    assert_eq!(status, 200);
    Some((v["matched_key"].as_str().unwrap().to_string(), body))
}

#[test]
fn the_exact_key_then_the_newest_entry_of_a_restore_key() {
    let (s, dir) = store(LIMIT);
    assert_eq!(save(&s, "deps-linux-aaa", "v1", b"old")["ok"], true);
    assert_eq!(save(&s, "deps-linux-bbb", "v1", b"new")["ok"], true);
    assert_eq!(
        restore(&s, "deps-linux-aaa", &["deps-linux-"], "v1").unwrap(),
        ("deps-linux-aaa".into(), b"old".to_vec())
    );
    assert_eq!(
        restore(&s, "deps-linux-ccc", &["x-", "deps-linux-"], "v1").unwrap().0,
        "deps-linux-bbb",
        "newest first"
    );
    assert!(restore(&s, "deps-linux-aaa", &["deps-"], "v2").is_none(), "another version is another entry");
    assert!(restore(&s, "none", &[], "v1").is_none());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_entry_is_written_once_and_a_wrong_size_is_refused() {
    let (s, dir) = store(LIMIT);
    assert_eq!(save(&s, "k", "v1", b"one")["ok"], true);
    let again = save(&s, "k", "v1", b"two");
    assert_eq!(again["ok"], false);
    assert!(again["message"].as_str().unwrap().contains("exists"), "{again}");
    let v = call(&s, "CreateCacheEntry", json!({ "key": "k2", "version": "v1" }));
    let url = local(v["signed_upload_url"].as_str().unwrap());
    assert_eq!(s.route("PUT", &url.replace("sig=", "sig=x"), b"x", None, BASE).unwrap().0, 403);
    assert_eq!(s.route("PUT", &url, b"abc", None, BASE).unwrap().0, 201);
    let f = call(&s, "FinalizeCacheEntryUpload", json!({ "key": "k2", "version": "v1", "size_bytes": "4" }));
    assert_eq!(f["ok"], false, "{f}");
    assert!(call(&s, "CreateCacheEntry", json!({ "key": "a,b", "version": "v1" }))["ok"] == false);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_download_takes_a_range() {
    let (s, dir) = store(LIMIT);
    save(&s, "k", "v1", b"0123456789");
    let v = call(&s, "GetCacheEntryDownloadURL", json!({ "key": "k", "version": "v1" }));
    let url = local(v["signed_download_url"].as_str().unwrap());
    let (status, body, headers) = s.route("GET", &url, b"", Some("bytes=0-1"), BASE).unwrap();
    assert_eq!((status, body.as_slice()), (206, &b"01"[..]));
    assert!(headers.contains(&("Content-Range", "bytes 0-1/10".to_string())), "{headers:?}");
    assert_eq!(s.route("GET", &url, b"", Some("bytes=8-"), BASE).unwrap().1, b"89");
    assert_eq!(s.route("GET", &url, b"", Some("bytes=-3"), BASE).unwrap().1, b"789");
    assert_eq!(s.route("GET", &url, b"", Some("bytes=4-100"), BASE).unwrap().1, b"456789");
    assert_eq!(s.route("GET", &url, b"", Some("bytes=10-"), BASE).unwrap().0, 416);
    assert_eq!(s.route("GET", &url.replace("sig=", "sig=x"), b"", None, BASE).unwrap().0, 403);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_oldest_entry_goes_first_over_the_limit() {
    let (s, dir) = store(10);
    save(&s, "a", "v", b"123456");
    save(&s, "b", "v", b"789012");
    let keys: Vec<String> = s.entries().iter().map(|e| e["key"].as_str().unwrap().to_string()).collect();
    assert_eq!(keys, ["b"]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_store_stays_for_the_next_run() {
    let (s, dir) = store(LIMIT);
    save(&s, "k", "v1", b"kept");
    let next = CacheStore::new(&dir, LIMIT).unwrap();
    assert_eq!(restore(&next, "k", &[], "v1").unwrap().1, b"kept");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_lookup_reads_the_scope_of_the_ref_then_that_of_the_default_branch() {
    let (main, dir) = store(LIMIT);
    let main = main.with_scopes(vec!["refs/heads/main".into()]);
    save(&main, "deps-a", "v1", b"from-main");
    let feature =
        CacheStore::new(&dir, LIMIT).unwrap().with_scopes(vec!["refs/heads/x".into(), "refs/heads/main".into()]);
    assert_eq!(restore(&feature, "deps-a", &[], "v1").unwrap().1, b"from-main", "the default branch is read");
    assert_eq!(save(&feature, "deps-a", "v1", b"from-x")["ok"], true, "the scope of the ref is its own");
    assert_eq!(restore(&feature, "deps-a", &[], "v1").unwrap().1, b"from-x", "the ref first");
    let other =
        CacheStore::new(&dir, LIMIT).unwrap().with_scopes(vec!["refs/heads/y".into(), "refs/heads/main".into()]);
    assert_eq!(restore(&other, "deps-a", &[], "v1").unwrap().1, b"from-main", "a branch does not read another branch");
    let _ = fs::remove_dir_all(&dir);
}
