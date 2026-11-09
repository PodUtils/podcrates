use super::*;
use std::io::Write;

const CANARY_USER: &str = "canary-user-09f2";
const CANARY_PASS: &str = "canary-pass-7b3e";
/// `printf %s 'canary-user-09f2:canary-pass-7b3e' | base64 -w0`, made
/// outside the crate: the oracle does not depend on the code.
const CANARY_HEADER: &str = "Basic Y2FuYXJ5LXVzZXItMDlmMjpjYW5hcnktcGFzcy03YjNl";
const HOST: &str = "127.0.0.1:5443";

struct ScratchDir(PathBuf);
impl ScratchDir {
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn scratch_dir() -> ScratchDir {
    let d = std::env::temp_dir().join(format!(
        "pod-oci-cred-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").subsec_nanos()
    ));
    std::fs::create_dir_all(&d).expect("scratch dir");
    ScratchDir(d)
}

/// An `auths` document with one base64 `auth` entry, the docker spelling.
fn scratch_auth(user: &str, pass: &str, host: &str) -> (ScratchDir, PathBuf) {
    let dir = scratch_dir();
    let p = dir.path().join("auth.json");
    let joined = format!("{user}:{pass}");
    let enc = base64_encode_for_test(joined.as_bytes());
    let mut f = std::fs::File::create(&p).expect("scratch file");
    write!(f, "{{\"auths\":{{\"{host}\":{{\"auth\":\"{enc}\"}}}}}}").expect("scratch write");
    (dir, p)
}

/// Test-only encoder, so the fixture never depends on the module's path.
fn base64_encode_for_test(bytes: &[u8]) -> String {
    const ALPH: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut enc = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        enc.push(ALPH[((n >> 18) & 63) as usize] as char);
        enc.push(ALPH[((n >> 12) & 63) as usize] as char);
        enc.push(if i + 1 < bytes.len() { ALPH[((n >> 6) & 63) as usize] as char } else { '=' });
        enc.push(if i + 2 < bytes.len() { ALPH[(n & 63) as usize] as char } else { '=' });
        i += 3;
    }
    enc
}

#[test]
fn an_auth_entry_decodes_to_its_user_and_password() {
    let (_dir, p) = scratch_auth(CANARY_USER, CANARY_PASS, HOST);
    let got =
        lookup_in(Path::new("/nonexistent"), &p, HOST).expect("lookup runs").expect("the seeded host has a login");
    assert_eq!(got.username, CANARY_USER);
    assert_eq!(got.password, CANARY_PASS);
}

#[test]
fn an_unknown_host_is_anonymous_not_an_error() {
    let (_dir, p) = scratch_auth(CANARY_USER, CANARY_PASS, HOST);
    let got = lookup_in(Path::new("/nonexistent"), &p, "example.com").expect("lookup runs");
    assert!(got.is_none());
}

#[test]
fn a_malformed_file_is_absence_not_a_panic() {
    let dir = scratch_dir();
    let p = dir.path().join("auth.json");
    std::fs::write(&p, "not json at all").expect("scratch write");
    let got = lookup_in(Path::new("/nonexistent"), &p, HOST).expect("lookup runs");
    assert!(got.is_none());
}

#[test]
fn the_basic_header_is_the_precomputed_value() {
    let cred = Credential { username: CANARY_USER.into(), password: CANARY_PASS.into() };
    assert_eq!(basic_header(&cred), CANARY_HEADER);
}

#[test]
fn a_missing_helper_is_a_named_refusal() {
    let err = helper_login("pod-oci-no-such-helper-xyz", "https://x/v2/").expect_err("a missing helper fails");
    assert!(err.to_string().contains("pod-oci-no-such-helper-xyz"), "names it: {err}");
}

#[test]
fn a_named_but_failing_helper_is_a_named_refusal() {
    let dir = scratch_dir();
    let cfg = dir.path().join("config.json");
    std::fs::write(&cfg, r#"{"auths":{},"credHelpers":{"127.0.0.1:5443":"pod-oci-no-such-helper-xyz"}}"#)
        .expect("scratch write");
    let err = lookup_in(&cfg, Path::new("/nonexistent"), HOST).expect_err("helper fails");
    assert!(err.to_string().contains("pod-oci-no-such-helper-xyz"), "names it: {err}");
}

#[test]
fn the_password_reaches_no_header_or_error_string() {
    let cred = Credential { username: CANARY_USER.into(), password: CANARY_PASS.into() };
    assert!(!basic_header(&cred).contains(CANARY_PASS), "canary in header");
    let err = helper_login("pod-oci-no-such-helper-xyz", "https://x/v2/").expect_err("a missing helper fails");
    assert!(!err.to_string().contains(CANARY_PASS), "canary in refusal");
}

#[test]
fn upsert_creates_auths_where_none_exist() {
    let out = upsert_auth_json("{}", HOST, CANARY_USER, CANARY_PASS);
    let back = parse_auth_json(&out);
    let got = back.get(HOST).expect("written entry reads back");
    assert_eq!(got.username, CANARY_USER);
    assert_eq!(got.password, CANARY_PASS);
}

#[test]
fn upsert_replaces_one_host_and_keeps_the_other() {
    let (_dir, p) = scratch_auth("other-user", "other-pass", "example.com:5000");
    let text = std::fs::read_to_string(&p).expect("scratch read");
    let out = upsert_auth_json(&text, HOST, CANARY_USER, CANARY_PASS);
    let back = parse_auth_json(&out);
    assert_eq!(back.get(HOST).expect("new host").username, CANARY_USER);
    assert_eq!(back.get("example.com:5000").expect("old host").username, "other-user");
}

#[test]
fn upsert_keeps_non_auth_keys() {
    let out = upsert_auth_json(r#"{"credsStore":"desktop","auths":{}}"#, HOST, CANARY_USER, CANARY_PASS);
    let doc: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(doc["credsStore"], "desktop");
    assert!(doc["auths"][HOST]["auth"].is_string());
}

#[test]
fn upsert_starts_fresh_on_a_broken_document() {
    let out = upsert_auth_json("not json", HOST, CANARY_USER, CANARY_PASS);
    let back = parse_auth_json(&out);
    assert!(back.contains_key(HOST));
}

#[test]
fn a_store_through_a_missing_helper_is_a_named_refusal() {
    let err = helper_store("pod-oci-no-such-helper-xyz", "https://x/v2/", "u", "p")
        .expect_err("a missing helper fails the store");
    assert!(err.to_string().contains("pod-oci-no-such-helper-xyz"), "names it: {err}");
}

#[test]
fn server_names_reduce_to_their_host() {
    assert_eq!(server_host("https://index.docker.io/v1/"), "index.docker.io");
    assert_eq!(server_host("https://127.0.0.1:5443/v2/"), "127.0.0.1:5443");
    assert_eq!(server_host("127.0.0.1:5443"), "127.0.0.1:5443");
    assert_eq!(server_host("example.com"), "example.com");
}

#[test]
fn store_writes_a_readable_entry() {
    let dir = scratch_dir();
    let docker = dir.path().join("config.json");
    let containers = dir.path().join("auth.json");
    store_login_in(&docker, &containers, HOST, CANARY_USER, CANARY_PASS).expect("store runs");
    let text = std::fs::read_to_string(&docker).expect("file written");
    let back = parse_auth_json(&text);
    let got = back.get(HOST).expect("entry reads back");
    assert_eq!(got.username, CANARY_USER);
    assert_eq!(got.password, CANARY_PASS);
}

#[test]
fn store_updates_the_file_that_holds_the_host() {
    let (_dir, containers) = scratch_auth("old-user", "old-pass", HOST);
    let dir = scratch_dir();
    let docker = dir.path().join("config.json");
    store_login_in(&docker, &containers, HOST, CANARY_USER, CANARY_PASS).expect("store runs");
    assert!(!docker.exists(), "one home: the containers file wins");
    let text = std::fs::read_to_string(&containers).expect("file kept");
    let back = parse_auth_json(&text);
    assert_eq!(back.get(HOST).expect("entry").username, CANARY_USER);
}

#[test]
fn store_sets_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch_dir();
    let docker = dir.path().join("config.json");
    let containers = dir.path().join("auth.json");
    store_login_in(&docker, &containers, HOST, CANARY_USER, CANARY_PASS).expect("store runs");
    let mode = std::fs::metadata(&docker).expect("file written").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "no group or other access");
}

/// A deletion removes only the named host and keeps the rest.
#[test]
fn delete_removes_only_the_named_host() {
    // Pure over strings: no scratch file is touched.
    let other = "other.example:5000";
    let text = format!("{{\"auths\":{{\"{HOST}\":{{\"auth\":\"eA==\"}},\"{other}\":{{\"auth\":\"eQ==\"}}}}}}");
    let (out, removed) = delete_auth_json(&text, HOST);
    assert!(removed);
    let doc: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert!(doc["auths"].get(HOST).is_none());
    assert_eq!(doc["auths"][other]["auth"], "eQ==");
    let (_, kept) = delete_auth_json(&out, HOST);
    assert!(!kept, "a second logout finds nothing");
}

/// A broken document holds nothing to remove, not an error.
#[test]
fn delete_on_a_broken_document_removes_nothing() {
    let (_, removed) = delete_auth_json("not json", HOST);
    assert!(!removed);
    let (_, removed) = delete_auth_json("{\"auths\":{}}", HOST);
    assert!(!removed);
}

/// A removal writes again only the file holding the host, and the
/// unknown host is still logged out rather than an error.
#[test]
fn remove_deletes_from_the_file_holding_the_host() {
    let (_cdir, containers) = scratch_auth("old-user", "old-pass", HOST);
    let dir = scratch_dir();
    let docker = dir.path().join("config.json");
    assert!(remove_login_in(&docker, &containers, HOST).expect("remove runs"));
    assert!(!docker.exists(), "one home: only the holding file is rewritten");
    let back = parse_auth_json(&std::fs::read_to_string(&containers).expect("file kept"));
    assert!(!back.contains_key(HOST));
    assert!(!remove_login_in(&docker, &containers, "unknown.example").expect("runs"));
}

/// Where the file names a helper for the host, the file
/// beside it must not decide the logout: a missing helper is a named
/// refusal and the file entry stays. (`dTpw` is `u:p`: the read path
/// skips entries with no colon, so the canary carries one.)
#[test]
fn remove_through_a_missing_helper_is_a_named_refusal() {
    let dir = scratch_dir();
    let docker = dir.path().join("config.json");
    std::fs::write(
            &docker,
            "{\"credHelpers\":{\"example.com\":\"pod-oci-no-such-helper-xyz\"},\"auths\":{\"example.com\":{\"auth\":\"dTpw\"}}}",
        )
        .expect("scratch write");
    let containers = dir.path().join("auth.json");
    let err = remove_login_in(&docker, &containers, "example.com").expect_err("a missing helper fails the logout");
    assert!(err.to_string().contains("pod-oci-no-such-helper-xyz"), "names it: {err}");
    let back = parse_auth_json(&std::fs::read_to_string(&docker).expect("file kept"));
    assert!(back.contains_key("example.com"), "nothing erased beside a failed helper");
}

/// An erase through a missing helper names it, like the store.
#[test]
fn an_erase_through_a_missing_helper_is_a_named_refusal() {
    let err =
        helper_erase("pod-oci-no-such-helper-xyz", "https://x/v2/").expect_err("a missing helper fails the erase");
    assert!(err.to_string().contains("pod-oci-no-such-helper-xyz"), "names it: {err}");
}

#[test]
fn store_through_a_missing_helper_is_a_named_refusal() {
    let dir = scratch_dir();
    let docker = dir.path().join("config.json");
    std::fs::write(&docker, r#"{"auths":{},"credHelpers":{"127.0.0.1:5443":"pod-oci-no-such-helper-xyz"}}"#)
        .expect("scratch write");
    let containers = dir.path().join("auth.json");
    let err = store_login_in(&docker, &containers, HOST, CANARY_USER, CANARY_PASS).expect_err("helper fails");
    assert!(err.to_string().contains("pod-oci-no-such-helper-xyz"), "names it: {err}");
    let written = std::fs::read_to_string(&docker).expect("read");
    assert!(!written.contains(CANARY_USER), "nothing stored beside a failed helper");
}

/// The paths come from the directories that the caller gives, and the
/// files read through the `Logins` trait as through `lookup_in`.
#[test]
fn the_paths_come_from_the_given_directories() {
    let home = Path::new("/home/u");
    let both = Files::new(Some(home), Some(Path::new("/run/user/1")));
    assert_eq!(both.docker, Path::new("/home/u/.docker/config.json"));
    assert_eq!(both.containers, Path::new("/run/user/1/containers/auth.json"));
    let home_only = Files::new(Some(home), None);
    assert_eq!(home_only.containers, Path::new("/home/u/.config/containers/auth.json"));
    assert_eq!(Files::new(None, None), Files::default());

    let (_dir, p) = scratch_auth(CANARY_USER, CANARY_PASS, HOST);
    let files = Files { docker: PathBuf::from("/nonexistent"), containers: p };
    let logins: &dyn Logins = &files;
    assert_eq!(logins.lookup(HOST).expect("lookup runs").expect("a login").username, CANARY_USER);
    assert!(NoLogins.lookup(HOST).expect("lookup runs").is_none());
}

/// A refusal of a helper has the stage of the host and a class.
#[test]
fn a_refusal_of_a_helper_has_its_stage_and_class() {
    let err = helper_login("pod-oci-no-such-helper-xyz", "https://x/v2/").expect_err("a missing helper fails");
    assert_eq!((err.stage, err.class), (pod_core::Stage::Host, pod_core::Class::Unavailable));
}
