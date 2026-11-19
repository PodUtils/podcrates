//! The binary `pod-release` as a process, over the files that the releases
//! v0.76.0 and v0.77.0 of podcrates published: it accepts each release, and
//! it refuses one changed byte of a file, of `SHA256SUMS`, of the bundle and
//! of the index, and another signer, each with its exit code and one line.

use pod_testkit::tamper::flip;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const EXE: &str = env!("CARGO_BIN_EXE_pod-release");
const OK: i32 = 0;
const USAGE: i32 = 64;
const NOT_FOUND: i32 = 66;
const REFUSED: i32 = 77;
const INDEX: &str = "release-index.json";
const SUMS: &str = "SHA256SUMS";
const BUNDLE: &str = "SHA256SUMS.sigstore.json";
const SCRIPT: &str = "podlab-install.sh";

fn fixtures(tag: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(tag)
}

/// A copy of the files of the release `tag` in a new directory, with
/// `change` applied. The installer is kept as `.bytes`, so that no check of
/// the scripts reads it.
fn release(tag: &str, change: impl Fn(&Path)) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(fixtures(tag)).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().trim_end_matches(".bytes").to_string();
        std::fs::copy(&path, dir.path().join(name)).unwrap();
    }
    change(dir.path());
    dir
}

fn run(args: &[&str], dir: &Path) -> Output {
    let mut cmd = Command::new(EXE);
    cmd.args(args).arg(dir).env_remove("PAGER");
    cmd.output().unwrap()
}

fn podcrates(tag: &str) -> Vec<String> {
    ["verify", "--repo", "PodUtils/podcrates", "--tag", tag].iter().map(|s| s.to_string()).collect()
}

fn verify(tag: &str, extra: &[&str], dir: &Path) -> Output {
    let mut args = podcrates(tag);
    args.extend(extra.iter().map(|s| s.to_string()));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&args, dir)
}

fn verify_files(tag: &str, dir: &Path, files: &[&str]) -> Output {
    let mut cmd = Command::new(EXE);
    cmd.args(podcrates(tag)).arg(dir).args(files);
    cmd.output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The exit code is `code`, and the error output is one line that starts
/// with `class` and holds `what`.
#[track_caller]
fn fails(out: &Output, code: i32, class: &str, what: &str) {
    let err = text(&out.stderr);
    assert_eq!(out.status.code(), Some(code), "{err}");
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.starts_with(&format!("pod-release: {class}: ")), "{err}");
    assert!(err.contains(what), "{err}");
    assert!(out.stdout.is_empty(), "{}", text(&out.stdout));
}

#[test]
fn the_published_release_with_its_index_verifies() {
    let dir = release("v0.77.0", |_| {});
    let out = verify("v0.77.0", &[], dir.path());
    let said = text(&out.stdout);
    assert_eq!(out.status.code(), Some(OK), "{said}{}", text(&out.stderr));
    assert!(said.contains("ok podlab-install.sh\n"), "{said}");
    assert!(said.contains("ok release-index.json (PodUtils/podcrates v0.77.0, 8 files)\n"), "{said}");
    assert!(said.contains("missing podlab-x86_64-unknown-linux-musl\n"), "{said}");
    let last = said.lines().last().unwrap();
    assert!(last.starts_with("verified: SHA256SUMS dd774c78c2e70e15ed93a2fbaac81878b5eea1ef9b58b0508a631633fe7a835e: logged by Rekor at 2026-11-19T"), "{last}");
    assert!(last.contains("2 files checked, 7 missing; signed by https://github.com/PodUtils/podcrates/"), "{last}");
    // The same signer by its URI and issuer.
    let uri = "https://github.com/PodUtils/podcrates/.github/workflows/release.yml@refs/tags/v0.77.0";
    let out =
        run(&["verify", "--identity", uri, "--issuer", "https://token.actions.githubusercontent.com"], dir.path());
    assert_eq!(out.status.code(), Some(OK), "{}", text(&out.stderr));
}

#[test]
fn the_release_before_the_index_verifies() {
    let dir = release("v0.76.0", |_| {});
    let out = verify("v0.76.0", &[], dir.path());
    assert_eq!(out.status.code(), Some(OK), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("1 files checked, 7 missing"), "{}", text(&out.stdout));
}

#[test]
fn one_changed_byte_of_a_file_is_refused() {
    for at in [0, 15000, usize::MAX] {
        let dir = release("v0.77.0", |d| flip(&d.join(SCRIPT), at));
        fails(&verify("v0.77.0", &[], dir.path()), REFUSED, "refused", "the SHA-256 of podlab-install.sh is");
    }
}

#[test]
fn one_changed_byte_of_the_sums_is_refused() {
    for at in [0, 400, usize::MAX] {
        let dir = release("v0.77.0", |d| flip(&d.join(SUMS), at));
        fails(&verify("v0.77.0", &[], dir.path()), REFUSED, "refused", "the signature of SHA256SUMS");
    }
}

/// A byte of the signature's value in the bundle: the first character of its
/// base64 text, changed.
#[test]
fn one_changed_byte_of_the_bundle_is_refused() {
    let bundle = std::fs::read_to_string(fixtures("v0.77.0").join(BUNDLE)).unwrap();
    let at = bundle.find("\"signature\":\"").unwrap() + "\"signature\":\"".len();
    let dir = release("v0.77.0", |d| flip(&d.join(BUNDLE), at));
    fails(&verify("v0.77.0", &[], dir.path()), REFUSED, "refused", "the signature of SHA256SUMS");
}

#[test]
fn one_changed_byte_of_the_index_is_refused() {
    for at in [0, 120, usize::MAX] {
        let dir = release("v0.77.0", |d| flip(&d.join(INDEX), at));
        fails(&verify("v0.77.0", &[], dir.path()), REFUSED, "refused", "the SHA-256 of release-index.json is");
    }
}

#[test]
fn another_signer_is_refused() {
    let dir = release("v0.77.0", |_| {});
    let path = dir.path();
    for out in [
        verify("v0.76.0", &[], path),
        run(&["verify", "--repo", "PodUtils/podnet", "--tag", "v0.77.0"], path),
        verify("v0.77.0", &["--workflow", "ci.yml"], path),
        run(
            &[
                "verify",
                "--identity",
                "https://github.com/PodUtils/podcrates/.github/workflows/release.yml@refs/tags/v0.77.0",
                "--issuer",
                "https://gitlab.com",
            ],
            path,
        ),
    ] {
        fails(&out, REFUSED, "refused", "the signature of SHA256SUMS");
    }
}

/// With `--all`, each file of `SHA256SUMS` must be there; a file that the
/// caller names must be there and in `SHA256SUMS`.
#[test]
fn a_missing_file_fails_only_when_the_caller_asks_for_it() {
    let dir = release("v0.77.0", |_| {});
    fails(&verify("v0.77.0", &["--all"], dir.path()), NOT_FOUND, "notfound", "podlab-0.77.0.tar.gz");
    let out = verify_files("v0.77.0", dir.path(), &[SCRIPT]);
    assert_eq!(out.status.code(), Some(OK), "{}", text(&out.stderr));
    let said = text(&out.stdout);
    assert!(
        said.contains("ok podlab-install.sh\nok release-index.json") && said.contains("2 files checked, 0 missing"),
        "{said}"
    );
    let out = verify_files("v0.77.0", dir.path(), &["podlab-x86_64-unknown-linux-musl"]);
    fails(&out, NOT_FOUND, "notfound", "podlab-x86_64-unknown-linux-musl");
    fails(
        &verify_files("v0.77.0", dir.path(), &["podlab-sparc64"]),
        REFUSED,
        "refused",
        "SHA256SUMS names no podlab-sparc64",
    );
    fails(&verify_files("v0.77.0", dir.path(), &["../SHA256SUMS"]), REFUSED, "refused", "is not a file name");
    let dir = release("v0.77.0", |d| std::fs::remove_file(d.join(BUNDLE)).unwrap());
    fails(&verify("v0.77.0", &[], dir.path()), NOT_FOUND, "notfound", BUNDLE);
}

/// An index that `SHA256SUMS` does not name is not signed.
#[test]
fn an_index_beside_the_sums_that_do_not_name_it_is_refused() {
    let index = fixtures("v0.77.0").join(INDEX);
    let dir = release("v0.76.0", |d| {
        std::fs::copy(&index, d.join(INDEX)).unwrap();
    });
    fails(&verify("v0.76.0", &[], dir.path()), REFUSED, "refused", "which SHA256SUMS does not name");
}

/// `sums`: the check of a dry run. It takes `SHA256SUMS` as it is, and still
/// checks each file and the index against it and against the schema.
#[test]
fn the_check_with_no_signature_reads_the_files_and_the_index() {
    let dir = release("v0.77.0", |_| {});
    let out = run(&["sums"], dir.path());
    assert_eq!(out.status.code(), Some(OK), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("checked with no signature: SHA256SUMS dd774c78c2e70e15ed93a2fbaac81878b5eea1ef9b58b0508a631633fe7a835e; 2 files checked, 7 missing"), "{}", text(&out.stdout));
    let dir = release("v0.77.0", |d| flip(&d.join(INDEX), 0));
    fails(&run(&["sums"], dir.path()), REFUSED, "refused", "the SHA-256 of release-index.json is");
    // An index with a field that the schema does not know, with its line of
    // SHA256SUMS made again: the schema refuses it.
    let dir = release("v0.77.0", |d| {
        let index = std::fs::read_to_string(d.join(INDEX)).unwrap().replacen("{", "{\n  \"extra\": 1,", 1);
        std::fs::write(d.join(INDEX), &index).unwrap();
        let sums = std::fs::read_to_string(d.join(SUMS)).unwrap();
        let old = sums.lines().find(|l| l.ends_with(INDEX)).unwrap().split(' ').next().unwrap().to_string();
        let new = pod_release::hex(&pod_release::sha256(index.as_bytes()));
        std::fs::write(d.join(SUMS), sums.replace(&old, &new)).unwrap();
    });
    fails(&run(&["sums"], dir.path()), REFUSED, "refused", "the release index: unknown field `extra`");
    fails(&verify("v0.77.0", &[], dir.path()), REFUSED, "refused", "the signature of SHA256SUMS");
    let dir = release("v0.77.0", |d| std::fs::remove_file(d.join(SUMS)).unwrap());
    fails(&run(&["sums"], dir.path()), NOT_FOUND, "notfound", SUMS);
}

#[test]
fn the_index_command_writes_one_line_for_each_file() {
    let index = fixtures("v0.77.0").join(INDEX);
    let out = run(&["index"], &index);
    assert_eq!(out.status.code(), Some(OK), "{}", text(&out.stderr));
    let said = text(&out.stdout);
    assert_eq!(said.lines().count(), 8, "{said}");
    assert!(
        said.contains("podlab-install.sh - 30323 9ab4756a93abba21d25cdffc67c1912bf370c0f826b1ba9a9d59fc5e7588d8e0\n")
    );
    let out = run(&["index", "--target", "aarch64-unknown-linux-musl"], &index);
    assert_eq!(text(&out.stdout).lines().count(), 3, "{}", text(&out.stdout));
    fails(&run(&["index", "--target", "sparc64-unknown-linux-gnu"], &index), NOT_FOUND, "notfound", "sparc64");
    fails(&run(&["index"], &fixtures("v0.77.0").join(SUMS)), REFUSED, "refused", "the release index");
    fails(&run(&["index"], &fixtures("v0.77.0").join("none.json")), NOT_FOUND, "notfound", "none.json");
}

#[test]
fn a_usage_error_gives_64() {
    let dir = release("v0.77.0", |_| {});
    for args in [
        &["verify"][..],
        &["verify", "--repo", "PodUtils/podcrates"],
        &["verify", "--repo", "podcrates", "--tag", "v0.77.0"],
        &["verify", "--tag", "v0.77.0"],
        &["verify", "--repo", "PodUtils/podcrates", "--tag", "v0.77.0", "--identity", "https://x.example"],
        &["verify", "--identity", "http://x.example"],
        &["verify", "--repo", "PodUtils/podcrates", "--tag", "v0.77.0", "--workflow", "a/b.yml"],
        &["verify", "--repo", "PodUtils/podcrates", "--tag", "v0.77.0", "--issuer", "https://gitlab.com"],
        &["verify", "--repo", "PodUtils/podcrates", "--tag", "v0.77.0/x"],
        &["sums", "--all", "x"],
        &["nothing"],
    ] {
        let out = run(args, dir.path());
        assert_eq!(out.status.code(), Some(USAGE), "{args:?}: {}", text(&out.stderr));
        assert!(out.stdout.is_empty(), "{args:?}");
    }
    let out = run(&["verify", "--repo", "podcrates", "--tag", "v0.77.0"], dir.path());
    fails(&out, USAGE, "usage", "--repo: \"podcrates\" is not OWNER/REPO; fix: give the repository as OWNER/REPO");
}

#[test]
fn the_help_the_version_and_the_manual_come_from_the_model() {
    let out = Command::new(EXE).arg("--help").output().unwrap();
    assert_eq!(out.status.code(), Some(OK));
    let help = text(&out.stdout);
    assert!(help.contains("Verify a release") && help.contains("sums") && help.contains("index"), "{help}");
    let out = Command::new(EXE).args(["verify", "--help"]).output().unwrap();
    assert!(text(&out.stdout).contains("--identity"), "{}", text(&out.stdout));
    let out = Command::new(EXE).arg("--version").output().unwrap();
    assert_eq!(text(&out.stdout).trim(), format!("pod-release {}", env!("CARGO_PKG_VERSION")));
    let out = Command::new(EXE).args(["man", "--no-pager"]).output().unwrap();
    assert_eq!(out.status.code(), Some(OK));
    let page = text(&out.stdout);
    assert!(page.contains("EXIT STATUS") && page.contains("77"), "{page}");
    let out = Command::new(EXE).args(["man", "--json"]).output().unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["name"], "pod-release");
}
