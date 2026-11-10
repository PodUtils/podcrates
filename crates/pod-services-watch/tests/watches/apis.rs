//! The watches of public APIs against one HTTPS stand-in at the bases of
//! the test: `github-release`, `github-files`, `github-file`, `tls-roots`,
//! `crate-file`, `rdap`, `oci-ping` and `oci-digest`.

use pod_services_watch::{Bases, Finding, Net};
use pod_testkit::stand::{Answer, Request, Stand};
use sha2::{Digest, Sha256};

use crate::common::{NAME, endpoint, fact, file, https, net, one_in, text};

const PEM: &str = "-----BEGIN CERTIFICATE-----\nQUJD\nREVG\n-----END CERTIFICATE-----\n";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The archive of the crate `demo` 1.0.0: one source file.
fn archive() -> Vec<u8> {
    let text = b"pub const RELAY: &str = \"https://relay.one.test\";\n";
    let mut tar = tar::Builder::new(Vec::new());
    let mut head = tar::Header::new_gnu();
    head.set_size(text.len() as u64);
    head.set_mode(0o644);
    head.set_cksum();
    tar.append_data(&mut head, "demo-1.0.0/src/defaults.rs", &text[..]).unwrap();
    let raw = tar.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut gz, &raw).unwrap();
    gz.finish().unwrap()
}

fn answer(request: &Request, origin: &str) -> Answer {
    let json = |status, body: &str| Answer::new(status, body.to_string()).header("Content-Type", "application/json");
    match request.path() {
        "/api/repos/o/r/releases/latest" => json(200, r#"{"tag_name":"v2","published_at":"2026-11-01T00:00:00Z"}"#),
        "/api/repos/o/r/releases/tags/v1" => json(200, r#"{"tag_name":"v1","published_at":"2026-01-01T00:00:00Z"}"#),
        "/raw/o/r/v1/a.txt" | "/raw/o/r/v2/a.txt" => Answer::new(200, "one"),
        "/raw/o/r/v2/b.txt" => Answer::new(200, "two"),
        "/raw/o/r/v1/b.txt" => Answer::new(200, "one"),
        "/raw/o/r/HEAD/consts.go" => Answer::new(200, "const V = \"x1\"\nconst N = 26\nconst H = \"relay.one.test\"\n"),
        "/raw/o/r/v1/ca.go" => Answer::new(200, format!("var roots = `{PEM}`\n")),
        "/api/v1/crates/demo" => json(200, r#"{"crate":{"max_stable_version":"1.0.0"}}"#),
        "/static/crates/demo/demo-1.0.0.crate" => Answer::new(200, archive()),
        "/rdap/dns.json" => json(200, &format!(r#"{{"services":[[["test"],["{origin}/registry/"]]]}}"#)),
        "/registry/domain/ok.test" => {
            json(200, r#"{"events":[{"eventAction":"expiration","eventDate":"2027-11-10T00:00:00Z"}]}"#)
        }
        "/registry/domain/soon.test" => {
            json(200, r#"{"events":[{"eventAction":"expiration","eventDate":"2026-12-01T00:00:00Z"}]}"#)
        }
        "/v2/" => {
            Answer::new(401, "").header("WWW-Authenticate", format!("Bearer realm=\"{origin}/token\",service=\"reg\""))
        }
        "/token" => json(200, r#"{"token":"a-stand-in"}"#),
        "/v2/lib/a/manifests/sha256:aaa" if request.header("authorization") == Some("Bearer a-stand-in") => {
            Answer::new(200, "").header("Docker-Content-Digest", "sha256:aaa")
        }
        "/v2/lib/a/manifests/1" => Answer::new(200, "").header("Docker-Content-Digest", "sha256:ccc"),
        _ => Answer::new(404, "no"),
    }
}

/// The stand-in, and a way out whose bases are it.
fn stand() -> (Stand, Net) {
    let port = std::sync::Arc::new(std::sync::OnceLock::<String>::new());
    let seen = port.clone();
    let stand = https(move |request| answer(request, seen.get().map(String::as_str).unwrap_or_default()));
    let origin = format!("https://{NAME}:{}", stand.port());
    port.set(origin.clone()).unwrap();
    let mut net = net();
    net.bases = Bases::at(&origin);
    (stand, net)
}

#[tokio::test]
async fn github_release_wants_a_young_pin() {
    pod_testkit::need_loopback!();
    let (_stand, mut net) = stand();
    let w = |max: u32| {
        format!("[[watch]]\nkind = \"github-release\"\nrepo = \"o/r\"\nfact = \"tag\"\nmax-age-days = {max}\n")
    };
    let good = fact("tag", "\"v1\"") + &w(400);
    let report = one_in(&mut net, &file("r1", &good)).await;
    assert!(report.holds() && text(&report).contains("the newest release of o/r is v2"), "{}", text(&report));
    let report = one_in(&mut net, &file("r2", &(fact("tag", "\"v1\"") + &w(274)))).await;
    assert!(text(&report).contains("a release of 313 days, past 274"), "{}", text(&report));
}

#[tokio::test]
async fn github_files_sums_each_file_at_the_pin_and_at_the_newest() {
    pod_testkit::need_loopback!();
    let (_stand, mut net) = stand();
    let one = hex(&Sha256::digest(b"one"));
    let w = "[[watch]]\nkind = \"github-files\"\nrepo = \"o/r\"\ntag = \"tag\"\nfact = \"files\"\n";
    let good = fact("tag", "\"v1\"") + &fact("files", &format!("[\"copy a.txt {one}\"]")) + w;
    assert!(one_in(&mut net, &file("r3", &good)).await.holds());
    let bad = fact("tag", "\"v1\"") + &fact("files", &format!("[\"copy b.txt {one}\"]")) + w;
    let report = one_in(&mut net, &file("r4", &bad)).await;
    let two = hex(&Sha256::digest(b"two"));
    assert_eq!(report.outcome.findings, vec![Finding::moved("\"files\" b.txt", one, format!("{two} at v2"))]);
}

#[tokio::test]
async fn github_file_holds_each_value() {
    pod_testkit::need_loopback!();
    let (_stand, mut net) = stand();
    let w = "[[watch]]\nkind = \"github-file\"\nrepo = \"o/r\"\npath = \"consts.go\"\nfacts = [\"v\", \"n\"]\n";
    let good = fact("v", "\"x1\"") + &fact("n", "26") + w;
    assert!(one_in(&mut net, &file("r5", &good)).await.holds());
    let bad = fact("v", "\"x2\"") + &fact("n", "26") + w;
    let report = one_in(&mut net, &file("r6", &bad)).await;
    assert_eq!(report.outcome.findings, vec![Finding::moved("\"v\"", "x2", "no \"x2\" in o/r/consts.go")]);
    let hosts = endpoint("api", "https", "relay.one.test", 443, "")
        + "[[watch]]\nkind = \"github-file\"\nrepo = \"o/r\"\npath = \"consts.go\"\nendpoint = \"api\"\n";
    assert!(one_in(&mut net, &file("r7", &hosts)).await.holds());
}

#[tokio::test]
async fn tls_roots_are_the_certificates_of_the_file_at_the_pin() {
    pod_testkit::need_loopback!();
    let (_stand, mut net) = stand();
    let w = "[[watch]]\nkind = \"tls-roots\"\nrepo = \"o/r\"\npath = \"ca.go\"\ntag = \"tag\"\nfact = \"roots\"\n";
    let good = fact("tag", "\"v1\"") + &fact("roots", &format!("\"\"\"\n{PEM}\"\"\"")) + w;
    assert!(one_in(&mut net, &file("c1", &good)).await.holds());
    let other = PEM.replace("REVG", "REVH");
    let bad = fact("tag", "\"v1\"") + &fact("roots", &format!("\"\"\"\n{other}\"\"\"")) + w;
    let report = one_in(&mut net, &file("c2", &bad)).await;
    assert!(matches!(&report.outcome.findings[..], [Finding::Moved { .. }]), "{}", text(&report));
}

#[tokio::test]
async fn crate_file_holds_the_hosts_in_the_newest_release() {
    pod_testkit::need_loopback!();
    let (_stand, mut net) = stand();
    let w = "[[watch]]\nkind = \"crate-file\"\ncrate = \"demo\"\npath = \"src/defaults.rs\"\nrole = \"relay\"\n";
    let good = endpoint("one", "https", "relay.one.test", 443, "role = \"relay\"") + w;
    assert!(one_in(&mut net, &file("c3", &good)).await.holds());
    let bad = endpoint("two", "https", "relay.two.test", 443, "role = \"relay\"") + w;
    let report = one_in(&mut net, &file("c4", &bad)).await;
    assert!(
        matches!(&report.outcome.findings[..], [Finding::Moved { was, .. }] if was == "relay.two.test"),
        "{}",
        text(&report)
    );
}

#[tokio::test]
async fn rdap_wants_a_registration_that_does_not_end_soon() {
    pod_testkit::need_loopback!();
    let (_stand, mut net) = stand();
    let good = "[[watch]]\nkind = \"rdap\"\ndomain = \"ok.test\"\n".to_string();
    let report = one_in(&mut net, &file("a1", &(fact("x", "1") + &good))).await;
    assert!(report.holds() && text(&report).contains("until 2027-11-10"), "{}", text(&report));
    let bad = "[[watch]]\nkind = \"rdap\"\ndomain = \"soon.test\"\n";
    let report = one_in(&mut net, &file("a2", &(fact("x", "1") + bad))).await;
    assert!(text(&report).contains("ends on 2026-12-01, in 21 days"), "{}", text(&report));
}

#[tokio::test]
async fn oci_ping_and_digest_follow_the_challenge() {
    pod_testkit::need_loopback!();
    let (stand, mut net) = stand();
    let origin = format!("https://{NAME}:{}", stand.port());
    let ep = endpoint("registry", "https", NAME, stand.port(), "");
    let ping = "[[watch]]\nkind = \"oci-ping\"\nendpoint = \"registry\"\nfact = \"realm\"\n";
    let good = ep.clone() + &fact("realm", &format!("\"{origin}/token\"")) + ping;
    assert!(one_in(&mut net, &file("o1", &good)).await.holds());
    let bad = ep.clone() + &fact("realm", "\"https://auth.example/token\"") + ping;
    let report = one_in(&mut net, &file("o2", &bad)).await;
    assert_eq!(
        report.outcome.findings,
        vec![Finding::moved("\"realm\"", "https://auth.example/token", format!("{origin}/token"))]
    );

    let digest = "[[watch]]\nkind = \"oci-digest\"\nendpoint = \"registry\"\nfacts = [\"images\"]\n";
    let good = ep.clone() + &fact("images", "[\"lib/a:1@sha256:aaa\"]") + digest;
    let report = one_in(&mut net, &file("o3", &good)).await;
    assert!(report.holds() && text(&report).contains("the tag lib/a:1 moved to sha256:ccc"), "{}", text(&report));
    let bad = ep + &fact("images", "[\"lib/a@sha256:bbb\"]") + digest;
    let report = one_in(&mut net, &file("o4", &bad)).await;
    assert_eq!(
        report.outcome.findings,
        vec![Finding::moved("\"images\"", "lib/a@sha256:bbb", "HTTP 404 for the digest")]
    );
}
