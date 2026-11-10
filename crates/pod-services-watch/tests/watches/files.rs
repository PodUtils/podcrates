//! `http-sha256` (a whole file, a range, a length) and `tls-name`,
//! `tls-names` against an HTTPS stand-in.

use pod_services_watch::Finding;
use pod_testkit::stand::{Answer, Request};
use sha2::{Digest, Sha256};

use crate::common::{NAME, endpoint, fact, file, https, one, text};

/// The file of the stand-in: 4096 bytes.
fn body() -> Vec<u8> {
    (0..4096u32).map(|i| (i % 251) as u8).collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The whole file, or the range that the request asks.
fn answer(request: &Request) -> Answer {
    let all = body();
    match request.header("range").and_then(|r| r.strip_prefix("bytes=")) {
        Some(range) => {
            let (a, b) = range.split_once('-').unwrap();
            let (a, b): (usize, usize) = (a.parse().unwrap(), b.parse().unwrap());
            Answer::new(206, all[a..=b].to_vec()).header("Content-Range", format!("bytes {a}-{b}/{}", all.len()))
        }
        None => Answer::new(200, all),
    }
}

#[tokio::test]
async fn http_sha256_sums_the_file_and_checks_its_length() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let sum = hex(&Sha256::digest(body()));
    let ep = endpoint("file", "https", NAME, stand.port(), "path = \"/a.bin\"");
    let w = "[[watch]]\nkind = \"http-sha256\"\nendpoint = \"file\"\nfact = \"sum\"\nlength = \"bytes\"\n";
    let good = ep.clone() + &fact("sum", &format!("\"{sum}\"")) + &fact("bytes", "4096") + w;
    assert!(one(&file("h1", &good)).await.holds());
    let bad = ep + &fact("sum", &format!("\"{}\"", "0".repeat(64))) + &fact("bytes", "4000") + w;
    let report = one(&file("h2", &bad)).await;
    assert_eq!(
        report.outcome.findings,
        vec![Finding::moved("\"sum\"", "0".repeat(64), sum), Finding::moved("\"bytes\"", "4000", "4096")]
    );
}

#[tokio::test]
async fn http_sha256_sums_a_range_of_sectors() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let sum = hex(&Sha256::digest(&body()[1024..1024 + 600]));
    let ep = endpoint("iso", "https", NAME, stand.port(), "path = \"/a.iso\"");
    let facts = fact("lba", "2") + &fact("sector", "512") + &fact("count", "600") + &fact("total", "4096");
    let w = "[[watch]]\nkind = \"http-sha256\"\nendpoint = \"iso\"\nfact = \"sum\"\nlength = \"total\"\nstart = \"lba\"\nsector = \"sector\"\nbytes = \"count\"\n";
    let good = ep.clone() + &facts + &fact("sum", &format!("\"{sum}\"")) + w;
    assert!(one(&file("h3", &good)).await.holds());
    let whole = hex(&Sha256::digest(body()));
    let bad = ep + &facts + &fact("sum", &format!("\"{whole}\"")) + w;
    let report = one(&file("h4", &bad)).await;
    assert_eq!(report.outcome.findings, vec![Finding::moved("\"sum\"", whole, sum)], "{}", text(&report));
}

#[tokio::test]
async fn tls_name_verifies_the_server_name() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let w = "[[watch]]\nkind = \"tls-name\"\nendpoint = \"api\"\n";
    let good = endpoint("api", "https", NAME, stand.port(), "") + w;
    assert!(one(&file("n1", &good)).await.holds());
    let bad = endpoint("api", "https", "other.test", stand.port(), "") + w;
    let report = one(&file("n2", &bad)).await;
    assert!(!report.holds() && text(&report).contains("TLS for other.test"), "{}", text(&report));
}

/// The test certificate names the host alone: the address of the endpoint
/// is not in it.
#[tokio::test]
async fn tls_names_wants_the_address_in_the_certificate() {
    pod_testkit::need_loopback!();
    let stand = https(answer);
    let w = "[[watch]]\nkind = \"tls-names\"\nendpoint = \"doh\"\n";
    let ep = endpoint("doh", "https", NAME, stand.port(), "addresses = [\"127.0.0.1\"]");
    let report = one(&file("n3", &(ep + w))).await;
    let all = text(&report);
    assert_eq!(report.outcome.findings.len(), 1, "{all}");
    assert!(all.contains("no handshake for 127.0.0.1"), "{all}");
}
