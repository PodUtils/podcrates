//! The store through its public API: the bounds of a name, the signature,
//! the random source, the ids, the wrappers of protobuf, the runtime token
//! and the download as a file.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use pod_actions_results::{ArtifactStore, BLOBS, CONTENT_BLOB, LIMIT, Payload, SERVICE, backend_ids, results_scope};
use pod_core::entropy::{Entropy, NoRandom};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const BASE: &str = "http://h/";

/// A directory of one test, removed at the end.
struct Scratch(PathBuf);

impl Scratch {
    fn new(what: &str) -> Scratch {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("pod-actions-results-{what}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sha256(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    format!("sha256:{}", d.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

fn call(s: &ArtifactStore, method: &str, v: Value) -> (u16, Value) {
    let (status, body) = s.route("POST", &format!("{SERVICE}{method}"), v.to_string().as_bytes(), BASE).unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn local(url: &str) -> String {
    format!("/{}", url.strip_prefix(BASE).unwrap())
}

/// The path of the upload URL of a new artifact `name`.
fn create(s: &ArtifactStore, name: &str) -> String {
    let (status, v) = call(s, "CreateArtifact", json!({ "name": name, "version": 4 }));
    assert_eq!(status, 200, "{v}");
    local(v["signed_upload_url"].as_str().unwrap())
}

/// An artifact `name` with `bytes`, in one whole blob.
fn upload(s: &ArtifactStore, name: &str, bytes: &[u8]) {
    let url = create(s, name);
    assert_eq!(s.route("PUT", &url, bytes, BASE).unwrap().0, 201);
    let fin = json!({ "name": name, "size": bytes.len().to_string(), "hash": sha256(bytes) });
    assert_eq!(call(s, "FinalizeArtifact", fin).0, 200);
}

/// The path of the download URL of the artifact `name`.
fn download_url(s: &ArtifactStore, name: &str) -> String {
    let (status, v) = call(s, "GetSignedArtifactURL", json!({ "name": name }));
    assert_eq!(status, 200, "{v}");
    local(v["signed_url"].as_str().unwrap())
}

/// The URL with the last character of its signature changed.
fn flip_last(url: &str) -> String {
    let mut s = url.to_string();
    let last = s.pop().unwrap();
    s.push(if last == '0' { '1' } else { '0' });
    s
}

#[test]
fn a_name_outside_the_store_is_refused() {
    let t = Scratch::new("outside");
    let dir = t.0.join("store");
    let s = ArtifactStore::new(&dir, LIMIT).unwrap();
    // A finished artifact beside the store, where `..` points.
    let victim = t.0.join("victim");
    fs::create_dir_all(&victim).unwrap();
    fs::write(victim.join("artifact.json"), json!({ "name": "../victim", "database_id": "1", "sig": "s" }).to_string())
        .unwrap();
    for name in ["..", "../victim", "a/../../victim", "/tmp", "a\\b", "C:x", "."] {
        for method in ["CreateArtifact", "FinalizeArtifact", "GetSignedArtifactURL", "DeleteArtifact"] {
            let (status, v) = call(&s, method, json!({ "name": name, "size": "0" }));
            assert_eq!((status, v["code"].as_str()), (400, Some("invalid_argument")), "{method} {name:?}");
        }
    }
    assert!(victim.join("artifact.json").exists(), "nothing outside the store is removed");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0, "nothing is made in the store");
}

#[test]
fn the_signature_is_compared_in_full() {
    let t = Scratch::new("sig");
    let s = ArtifactStore::new(&t.0, LIMIT).unwrap();
    let url = create(&s, "data");
    let (head, sig) = url.split_once("sig=").unwrap();
    assert_eq!(sig.len(), 32, "16 random bytes in hex");
    for bad in [flip_last(&url), format!("{head}sig={}", &sig[..31]), format!("{head}sig="), head.to_string()] {
        assert_eq!(s.route("PUT", &bad, b"x", BASE).unwrap().0, 403, "{bad}");
    }
    assert_eq!(s.used(), 0, "a refused upload counts no byte");
    assert_eq!(s.route("PUT", &url, b"bytes", BASE).unwrap().0, 201);
    let fin = json!({ "name": "data", "size": "5", "hash": sha256(b"bytes") });
    assert_eq!(call(&s, "FinalizeArtifact", fin).0, 200);
    let down = download_url(&s, "data");
    assert_eq!(s.route("GET", &flip_last(&down), b"", BASE).unwrap().0, 403);
    let (head, _) = down.split_once("sig=").unwrap();
    assert_eq!(s.route("GET", &format!("{head}sig="), b"", BASE).unwrap().0, 403);
    assert_eq!(s.route("GET", &down, b"", BASE).unwrap(), (200, b"bytes".to_vec()));
    let other = create(&s, "other");
    assert_ne!(other.split_once("sig=").unwrap().1, sig, "each upload has its own signature");
}

#[test]
fn no_random_bytes_gives_no_upload_url() {
    struct Dry;
    impl Entropy for Dry {
        fn fill(&mut self, _: &mut [u8]) -> Result<(), NoRandom> {
            Err(NoRandom)
        }
    }
    let t = Scratch::new("dry");
    let s = ArtifactStore::with_entropy(&t.0, LIMIT, Dry).unwrap();
    let (status, v) = call(&s, "CreateArtifact", json!({ "name": "data" }));
    assert_eq!((status, v["code"].as_str()), (500, Some("internal")), "{v}");
    assert!(v.get("signed_upload_url").is_none());
}

#[test]
fn a_store_opened_again_keeps_its_ids() {
    let t = Scratch::new("again");
    let first = ArtifactStore::new(&t.0, LIMIT).unwrap();
    upload(&first, "one", b"1");
    upload(&first, "two", b"2");
    drop(first);
    let s = ArtifactStore::new(&t.0, LIMIT).unwrap();
    upload(&s, "three", b"3");
    let ids: Vec<Value> = s.list().iter().map(|a| a["database_id"].clone()).collect();
    assert_eq!(ids, [json!("1"), json!("3"), json!("2")], "by name: one, three, two");
    assert_eq!(s.route("GET", &download_url(&s, "three"), b"", BASE).unwrap(), (200, b"3".to_vec()));
}

#[test]
fn a_wrapper_or_a_plain_string_is_read_in_a_request() {
    let t = Scratch::new("wrapper");
    let s = ArtifactStore::new(&t.0, LIMIT).unwrap();
    let url = create(&s, "a");
    assert_eq!(s.route("PUT", &url, b"aa", BASE).unwrap().0, 201);
    let fin = json!({ "name": "a", "size": 2, "hash": { "value": sha256(b"aa") } });
    assert_eq!(call(&s, "FinalizeArtifact", fin).0, 200, "a number and a wrapper");
    upload(&s, "b", b"bb");
    for filter in [json!({ "name_filter": "b" }), json!({ "name_filter": { "value": "b" } })] {
        let list = call(&s, "ListArtifacts", filter.clone()).1;
        assert_eq!(list["artifacts"].as_array().unwrap().len(), 1, "{filter}");
        assert_eq!(list["artifacts"][0]["name"], "b");
    }
    for filter in [json!({ "id_filter": "1" }), json!({ "id_filter": 1 }), json!({ "id_filter": { "value": "1" } })] {
        let list = call(&s, "ListArtifacts", filter.clone()).1;
        assert_eq!(list["artifacts"][0]["name"], "a", "{filter}");
    }
}

#[test]
fn each_field_of_an_answer_is_a_plain_value() {
    let t = Scratch::new("plain");
    let s = ArtifactStore::new(&t.0, LIMIT).unwrap();
    let url = create(&s, "data");
    assert_eq!(s.route("PUT", &url, b"zip", BASE).unwrap().0, 201);
    let fin = json!({
        "name": "data", "size": "3", "hash": sha256(b"zip"),
        "workflow_run_backend_id": "run-1", "workflow_job_run_backend_id": "job-2",
    });
    assert_eq!(call(&s, "FinalizeArtifact", fin).1["artifact_id"], "1");
    let a = &call(&s, "ListArtifacts", json!({})).1["artifacts"][0];
    // download-artifact@v4 refuses a StringValue in another form.
    let want = json!({
        "workflow_run_backend_id": "run-1", "workflow_job_run_backend_id": "job-2", "database_id": "1",
        "name": "data", "size": "3", "digest": sha256(b"zip"), "created_at": a["created_at"],
    });
    assert_eq!(a, &want);
}

#[test]
fn the_scope_of_the_runtime_token_names_the_run_and_the_job() {
    use base64::Engine as _;
    let b64 = |v: Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
    let token = |scp: &str| format!("{}.{}.c2lnbmF0dXJl", b64(json!({ "alg": "RS256" })), b64(json!({ "scp": scp })));
    let scp = format!("Actions.GenericRead:00000000-0000-0000-0000-000000000000 {}", results_scope("1", "2"));
    let ids = backend_ids(&token(&scp)).unwrap();
    assert_eq!((ids.run.as_str(), ids.job.as_str()), ("1", "2"));
    assert_eq!(backend_ids(&token("Actions.Results:1:2:3")), None, "four parts");
    assert_eq!(backend_ids(&token("Actions.Results:1")), None, "two parts");
    assert_eq!(backend_ids(&token("Actions.ResultsX:1:2 Other:1:2")), None, "no scope of the results API");
    assert_eq!(backend_ids("not a token"), None);
}

#[test]
fn a_download_is_a_file_to_stream() {
    let t = Scratch::new("stream");
    let s = ArtifactStore::new(&t.0, LIMIT).unwrap();
    upload(&s, "data", b"the zip");
    let a = s.answer("GET", &download_url(&s, "data"), b"", BASE).unwrap();
    assert_eq!((a.status, a.content_type), (200, CONTENT_BLOB));
    let Payload::File(mut f, len) = a.body else { panic!("a file") };
    let mut out = Vec::new();
    f.read_to_end(&mut out).unwrap();
    assert_eq!((len, out), (7, b"the zip".to_vec()));
    assert!(s.answer("GET", &format!("{BLOBS}download/x"), b"", BASE).is_none(), "an id is a number");
}

#[test]
fn a_new_upload_of_a_name_drops_the_earlier_one() {
    let t = Scratch::new("again-name");
    let s = ArtifactStore::new(&t.0, LIMIT).unwrap();
    let first = create(&s, "data");
    assert_eq!(s.route("PUT", &first, b"old", BASE).unwrap().0, 201);
    let second = create(&s, "data");
    assert_eq!(s.route("PUT", &first, b"x", BASE).unwrap().0, 403, "the first URL is dead");
    assert!(!t.0.join("data").join(".upload-1").exists(), "its blob is removed");
    assert_eq!(s.route("PUT", &second, b"new", BASE).unwrap().0, 201);
    assert_eq!(call(&s, "FinalizeArtifact", json!({ "name": "data", "size": "3", "hash": sha256(b"new") })).0, 200);
    assert_eq!(fs::read(t.0.join("data").join("artifact.zip")).unwrap(), b"new");
    assert_eq!(fs::read_dir(t.0.join("data")).unwrap().count(), 2, "the zip and its record alone");
}

#[test]
fn a_finalize_refuses_a_name_that_another_writer_took() {
    let t = Scratch::new("taken");
    let s = ArtifactStore::new(&t.0, LIMIT).unwrap();
    let url = create(&s, "data");
    assert_eq!(s.route("PUT", &url, b"mine", BASE).unwrap().0, 201);
    let record = json!({ "name": "data", "database_id": "9", "sig": "s" }).to_string();
    fs::write(t.0.join("data").join("artifact.json"), record).unwrap();
    let (status, v) = call(&s, "FinalizeArtifact", json!({ "name": "data", "size": "4" }));
    assert_eq!((status, v["code"].as_str()), (409, Some("already_exists")), "{v}");
    assert!(!t.0.join("data").join("artifact.zip").exists(), "nothing is replaced");
}

#[test]
fn a_record_of_another_name_is_not_the_artifact() {
    let t = Scratch::new("fold");
    let s = ArtifactStore::new(&t.0, LIMIT).unwrap();
    fs::create_dir_all(t.0.join("Data")).unwrap();
    let record = json!({ "name": "data", "database_id": "1", "sig": "s" }).to_string();
    fs::write(t.0.join("Data").join("artifact.json"), record).unwrap();
    assert_eq!(call(&s, "GetSignedArtifactURL", json!({ "name": "Data" })).0, 404);
    assert_eq!(call(&s, "DeleteArtifact", json!({ "name": "Data" })).0, 404);
    assert!(s.list().is_empty());
}

#[test]
fn a_block_id_has_a_limit_and_a_block_list_its_blocks() {
    let t = Scratch::new("block");
    let s = ArtifactStore::new(&t.0, LIMIT).unwrap();
    let url = create(&s, "data");
    let long = "A".repeat(89);
    assert_eq!(s.route("PUT", &format!("{url}&comp=block&blockid={long}"), b"x", BASE).unwrap().0, 400);
    assert_eq!(s.route("PUT", &format!("{url}&comp=block"), b"x", BASE).unwrap().0, 400, "no id");
    assert_eq!(s.route("PUT", &format!("{url}&comp=other"), b"x", BASE).unwrap().0, 400);
    assert_eq!(s.used(), 0, "a refused call counts no byte");
    let ok = "A".repeat(88);
    assert_eq!(s.route("PUT", &format!("{url}&comp=block&blockid={ok}"), b"x", BASE).unwrap().0, 201);
    let list = b"<BlockList><Latest>QUFB</Latest></BlockList>";
    let (status, body) = s.route("PUT", &format!("{url}&comp=blocklist"), list, BASE).unwrap();
    assert_eq!((status, String::from_utf8_lossy(&body).into_owned()), (400, "no block QUFB".to_string()));
}

#[test]
fn the_limit_counts_each_byte_of_the_run() {
    let t = Scratch::new("limit");
    let s = ArtifactStore::new(&t.0, 10).unwrap();
    upload(&s, "one", b"12345");
    let url = create(&s, "two");
    assert_eq!(s.route("PUT", &format!("{url}&comp=block&blockid=QUFB"), b"1234", BASE).unwrap().0, 201);
    let list = b"<BlockList><Latest>QUFB</Latest></BlockList>";
    assert_eq!(s.route("PUT", &format!("{url}&comp=blocklist"), list, BASE).unwrap().0, 201);
    assert_eq!(s.used(), 9, "the join of a block list counts no byte");
    let (status, _) = s.route("PUT", &format!("{url}&comp=block&blockid=QkJC"), b"12", BASE).unwrap();
    assert_eq!(status, 413, "the limit is of the run, not of one artifact");
    assert_eq!(s.route("PUT", &format!("{url}&comp=block&blockid=QkJC"), b"1", BASE).unwrap().0, 201);
}

#[test]
fn the_finalize_checks_the_sha256_in_a_plain_string() {
    let t = Scratch::new("hash");
    let s = ArtifactStore::new(&t.0, LIMIT).unwrap();
    let url = create(&s, "data");
    assert_eq!(s.route("PUT", &url, b"bytes", BASE).unwrap().0, 201);
    let wrong = json!({ "name": "data", "size": "5", "hash": sha256(b"other") });
    let (status, v) = call(&s, "FinalizeArtifact", wrong);
    assert_eq!((status, v["code"].as_str()), (400, Some("invalid_argument")), "{v}");
    let right = json!({ "name": "data", "size": "5", "hash": sha256(b"bytes") });
    assert_eq!(call(&s, "FinalizeArtifact", right).0, 200, "the upload stays for a second finalize");
    assert!(Path::new(&t.0.join("data").join("artifact.zip")).exists());
}

#[test]
fn a_rerun_replaces_an_artifact_of_the_same_name() {
    let t = Scratch::new("rerun");
    let s = ArtifactStore::new(&t.0, LIMIT).unwrap();
    upload(&s, "logs", b"first");
    let (status, v) = call(&s, "CreateArtifact", json!({ "name": "logs" }));
    assert_eq!(status, 409, "a run keeps its names: {v}");
    drop(s);
    let rerun = ArtifactStore::new(&t.0, LIMIT).unwrap().replacing();
    upload(&rerun, "logs", b"second");
    let (status, body) = rerun.route("GET", &download_url(&rerun, "logs"), b"", BASE).unwrap();
    assert_eq!((status, body.as_slice()), (200, &b"second"[..]), "the rerun replaces the artifact");
    let list = rerun.list();
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["database_id"], "2", "the new artifact has a new id");
    let (status, v) = call(&rerun, "CreateArtifact", json!({ "name": "../x" }));
    assert_eq!(status, 400, "a rerun checks the name: {v}");
}
