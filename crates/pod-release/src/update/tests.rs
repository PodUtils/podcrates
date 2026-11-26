//! The steps of an update against a stand-in of GitHub on the loopback
//! that serves the files of podcrates v0.76.0 (its asset through a
//! redirect to a signed URL, as GitHub does), and the plan of a tag.

use std::path::PathBuf;
use std::sync::Arc;

use pod_testkit::stand::{self, Answer, Request, Stand};

use super::*;

/// The asset `name` of v0.76.0; the installer is kept as `.bytes`, so that
/// no check of the scripts reads it.
fn fixture(name: &str) -> Vec<u8> {
    let file = if name == "podlab-install.sh" { "podlab-install.sh.bytes" } else { name };
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0.76.0").join(file)).unwrap()
}

fn client() -> pod_proxy::Client {
    use pod_proxy::{Dialer, ProxyChoice, Settings};
    let dialer =
        Dialer::new(Settings::new("pod-release", ProxyChoice::Direct), Arc::new(pod_proxy::resolve::System::new()));
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let words = pod_tls::Words::new("pod-release");
    let context = Arc::new(pod_tls::Context::new(provider, pod_tls::Settings::new(words)));
    pod_proxy::Client::new(context, Arc::new(dialer))
}

const DOWNLOAD: &str = "/PodUtils/podcrates/releases/download/v0.76.0/";

/// The stand-in: the API answers `api` (403 with none), the feed lists
/// v0.76.0, and each file of the release is the fixture through `file`.
fn github(api: Option<&'static str>, file: fn(&str) -> Option<Vec<u8>>) -> Stand {
    stand::http(move |req: &Request| {
        let path = req.path();
        if path == "/repos/PodUtils/podcrates/releases" {
            return match api {
                Some(json) => Answer::new(200, json),
                None => Answer::new(403, "API rate limit exceeded"),
            };
        }
        if path == "/PodUtils/podcrates/releases.atom" {
            return Answer::new(200, "<feed><entry><link href=\"https://github.com/PodUtils/podcrates/releases/tag/v0.76.0\"/></entry></feed>");
        }
        if let Some(name) = path.strip_prefix("/assets/") {
            return match file(name) {
                Some(body) => Answer::new(200, body),
                None => Answer::new(404, "Not Found"),
            };
        }
        match path.strip_prefix(DOWNLOAD) {
            Some(name) if file(name).is_some() => {
                Answer::new(302, "").header("Location", format!("/assets/{name}?sig=SECRET&exp=1"))
            }
            _ => Answer::new(404, "Not Found"),
        }
    })
    .expect("a stand-in")
}

fn published(name: &str) -> Option<Vec<u8>> {
    ["SHA256SUMS", "SHA256SUMS.sigstore.json", "podlab-install.sh"].contains(&name).then(|| fixture(name))
}

fn source(stand: &Stand) -> Source {
    let at = format!("http://127.0.0.1:{}", stand.port());
    Source { api: at.clone(), web: at, ..Source::github("PodUtils/podcrates", "release.yml") }
}

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

#[tokio::test]
async fn a_signed_release_and_its_file_come_through_a_redirect() {
    pod_testkit::need_loopback!();
    let stand = github(None, published);
    let fetcher = Fetcher::new(client()).allow_plain_http();
    let source = source(&stand);
    let signed = signed(&fetcher, &source, &v("0.76.0")).await.expect("the release verifies");
    assert_eq!(signed.verified.logged.index, 3992748924);
    assert_eq!(
        signed.identity.uri,
        "https://github.com/PodUtils/podcrates/.github/workflows/release.yml@refs/tags/v0.76.0"
    );
    let body = binary(&fetcher, &source, &signed, "podlab-install.sh").await.expect("the file");
    assert_eq!(body, fixture("podlab-install.sh"));
    let missing = binary(&fetcher, &source, &signed, "podlab-sparc64-unknown-linux-musl").await;
    assert!(matches!(missing, Err(Error::NotFound(_))), "{missing:?}");
}

#[tokio::test]
async fn a_changed_file_or_changed_sums_from_the_server_is_refused() {
    pod_testkit::need_loopback!();
    let fetcher = Fetcher::new(client()).allow_plain_http();
    let changed_file = |name: &str| {
        let mut body = published(name)?;
        if name == "podlab-install.sh" {
            body[100] ^= 1;
        }
        Some(body)
    };
    let stand = github(None, changed_file);
    let src = source(&stand);
    let got = signed(&fetcher, &src, &v("0.76.0")).await.unwrap();
    match binary(&fetcher, &src, &got, "podlab-install.sh").await {
        Err(Error::Refused(why)) => {
            assert!(why.contains("its SHA-256 is") && why.contains("nothing was replaced"), "{why}")
        }
        other => panic!("{other:?}"),
    }
    let changed_sums = |name: &str| {
        let mut body = published(name)?;
        if name == "SHA256SUMS" {
            body[0] ^= 1;
        }
        Some(body)
    };
    let stand = github(None, changed_sums);
    match signed(&fetcher, &source(&stand), &v("0.76.0")).await {
        Err(Error::Refused(why)) => assert!(why.contains("the signature of SHA256SUMS"), "{why}"),
        other => panic!("{other:?}"),
    }
}

/// Another workflow of the same repository is not the signer.
#[tokio::test]
async fn another_workflow_is_refused() {
    pod_testkit::need_loopback!();
    let stand = github(None, published);
    let fetcher = Fetcher::new(client()).allow_plain_http();
    let source = Source { workflow: "ci.yml".into(), ..source(&stand) };
    assert!(matches!(signed(&fetcher, &source, &v("0.76.0")).await, Err(Error::Refused(_))));
}

#[tokio::test]
async fn a_tag_with_no_release_is_not_found_and_no_query_is_shown() {
    pod_testkit::need_loopback!();
    let stand = github(None, published);
    let fetcher = Fetcher::new(client()).allow_plain_http();
    match signed(&fetcher, &source(&stand), &v("9.9.9")).await {
        Err(Error::NotFound(why)) => assert!(why.contains("no release has the tag v9.9.9"), "{why}"),
        other => panic!("{other:?}"),
    }
    // An asset whose signed URL answers 404: the message holds no query.
    let gone = |name: &str| (name == "SHA256SUMS").then(Vec::new);
    let stand = github(None, gone);
    let got = fetcher.get(&source(&stand).file("v0.76.0", "SHA256SUMS"), "*/*", 10, SMALL_LIMIT).await;
    assert_eq!(got, Ok(Vec::new()));
    let stand = github(None, |_| None);
    let url = format!("http://127.0.0.1:{}/assets/x?sig=SECRET", stand.port());
    let why = fetcher.get(&url, "*/*", 10, SMALL_LIMIT).await.unwrap_err();
    assert!(matches!(why, Error::NotFound(_)) && !why.why().contains("SECRET"), "{why:?}");
}

#[tokio::test]
async fn the_newest_comes_from_the_api_else_from_the_feed() {
    pod_testkit::need_loopback!();
    let fetcher = Fetcher::new(client()).allow_plain_http();
    let api = r#"[{"tag_name":"v0.77.0-beta.1","prerelease":true},{"tag_name":"v0.76.0"},{"tag_name":"v0.78.0","draft":true}]"#;
    let stand = github(Some(api), published);
    let got = newest(&fetcher, &source(&stand), &v("0.75.0")).await.unwrap();
    assert_eq!(got, Some((v("0.76.0"), "GitHub's API")));
    let got = newest(&fetcher, &source(&stand), &v("0.75.0-beta.1")).await.unwrap();
    assert_eq!(got, Some((v("0.77.0-beta.1"), "GitHub's API")));
    let stand = github(None, published);
    let got = newest(&fetcher, &source(&stand), &v("0.75.0")).await.unwrap();
    assert_eq!(got, Some((v("0.76.0"), "the releases' feed")));
    let stand = github(Some("[]"), published);
    let src = Source { web: "http://127.0.0.1:9/none".into(), ..source(&stand) };
    assert_eq!(newest(&fetcher, &src, &v("0.75.0")).await, Ok(None));
}

#[tokio::test]
async fn a_url_that_is_not_https_is_refused_by_default() {
    let fetcher = Fetcher::new(client());
    let got = fetcher.get("http://127.0.0.1:9/x", "*/*", 10, SMALL_LIMIT).await;
    assert!(matches!(got, Err(Error::Config(_))), "{got:?}");
}

/// podnet's `the_same_tag_is_not_installed_again` (its issues 197, 275).
#[test]
fn the_same_tag_is_not_installed_again() {
    let p = |tag: &str, current: &str, pinned, check, force| plan(&v(tag), &v(current), pinned, check, force);
    assert_eq!(p("1.0.0", "1.0.0", false, false, false), Plan::UpToDate);
    assert_eq!(p("1.0.0", "1.0.0", true, false, false), Plan::Same);
    assert_eq!(p("1.0.0", "1.0.0", true, true, false), Plan::Same);
    assert_eq!(p("1.0.0", "1.0.0", true, false, true), Plan::Install);
    // --force with no pinned tag installs the same version again, and
    // --check says that it is available. No downgrade.
    assert_eq!(p("1.0.0", "1.0.0", false, false, true), Plan::Install);
    assert_eq!(p("1.0.0", "1.0.0", false, true, true), Plan::Available);
    assert_eq!(p("0.9.0", "1.0.0", false, false, true), Plan::Newer);
    assert_eq!(p("2.0.0", "1.0.0", true, true, false), Plan::Available);
    assert_eq!(p("2.0.0", "1.0.0", false, false, false), Plan::Install);
    // No downgrade with no pinned tag; on purpose with one.
    assert_eq!(p("0.3.0-beta.9", "0.3.0-beta.10", false, true, false), Plan::Newer);
    assert_eq!(p("0.3.0-beta.9", "0.3.0-beta.10", true, true, false), Plan::Available);
}

/// A binary that does not run here is refused, and the one in place stays.
#[cfg(unix)]
#[test]
fn install_refuses_a_binary_that_does_not_run_here() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("podssh");
    std::fs::write(&exe, b"#!/bin/sh\necho 'podssh 1.0.0'\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let limit = trial::LIMIT;
    match install(&exe, b"#!/bin/sh\nexit 4\n", "podssh", &v("1.1.0"), limit) {
        Err(Error::Refused(why)) => assert!(why.contains("podssh 1.1.0 did not run here"), "{why}"),
        other => panic!("{other:?}"),
    }
    let done = install(&exe, b"#!/bin/sh\necho 'podssh 1.1.0 (abc)'\n", "podssh", &v("1.1.0"), limit).unwrap();
    assert_eq!(done.said, "podssh 1.1.0 (abc)");
    assert_eq!(done.prev.as_deref().ok(), Some(dir.path().join("podssh.prev").as_path()));
}

/// With no pinned tag, an equal release is `UpToDate` and an older one is
/// `Newer`, so that a caller says "the newest release" apart from "this
/// build is newer" (PodUtils/podssh#329, T-971).
#[test]
fn plan_tells_an_equal_release_from_an_older_one() {
    assert_eq!(plan(&v("1.2.0"), &v("1.2.0"), false, false, false), Plan::UpToDate);
    assert_eq!(plan(&v("1.1.9"), &v("1.2.0"), false, false, false), Plan::Newer);
    assert_eq!(plan(&v("1.1.9"), &v("1.2.0"), false, true, false), Plan::Newer);
    assert_eq!(plan(&v("1.2.0"), &v("1.2.0-beta.3"), false, true, false), Plan::Available);
}

mod former;
