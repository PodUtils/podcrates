//! An answer over `Settings::max_body` (PodUtils/podvm#651): on a full
//! storage pool, incusd 6.0 answers the wait of the operation with HTTP 200
//! and one chunk of an `error` envelope that holds the whole stderr of
//! `tar`. The crate reads such an envelope past the bound and gives the
//! words of incus; another answer over the bound is a refusal that names
//! its status, its size, the bound and its start. The bound here is 64 KiB
//! and the answers some MB, as the 64 MiB and the 108 MB of the report.
#![cfg(unix)]

mod common;

use common::Stand;
use pod_incus::{BodySize, Client, ErrorKind, NewInstance, Source};
use serde_json::json;

const BOUND: usize = 64 * 1024;

/// The text of incusd: the command line of `tar` and a long path, then one
/// line of stderr for each file, `n` bytes or more.
fn tar_error(n: usize) -> String {
    let path = format!("rootfs/usr/lib/python3.12/site-packages/{}__init__.py", "a-long-package-name/".repeat(8));
    let mut text = format!(
        "Failed creating instance from image: Unpack failed: Failed to run: tar --wildcards --numeric-owner \
         --xattrs-include=* --restrict --force-local -C /var/lib/incus/storage-pools/default/images/ab12/rootfs \
         --exclude=rootfs/dev/* -xf -: exit status 2 (tar: {path}: Cannot open: No space left on device"
    );
    let mut i = 0;
    while text.len() < n {
        text.push_str(&format!("\ntar: rootfs/usr/share/doc/{i}: Cannot mkdir: No such file or directory"));
        i += 1;
    }
    text.push_str("\ntar: Exiting with failure status due to previous errors)");
    text
}

/// HTTP 200 with `body` as one chunk, as the wait of incusd sends it.
fn one_chunk(body: &[u8]) -> Vec<u8> {
    let head = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n";
    let mut out = format!("{head}{:x}\r\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out.extend_from_slice(b"\r\n0\r\n\r\n");
    out
}

/// An envelope in the order of the fields of incusd (`api.Response`):
/// the `type` first.
fn envelope(kind: &str, code: u16, error: &str, metadata: &serde_json::Value) -> String {
    let error = serde_json::to_string(error).unwrap();
    format!(
        r#"{{"type":"{kind}","status":"","status_code":0,"operation":"","error_code":{code},"error":{error},"metadata":{metadata}}}"#
    )
}

fn client(stand: &Stand) -> Client {
    let mut settings = stand.settings();
    settings.max_body = BOUND;
    Client::new(settings)
}

#[tokio::test]
async fn an_error_envelope_over_the_bound_gives_the_words_of_incus() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let words = tar_error(3 << 20);
    let body = envelope("error", 500, &words, &json!(null));
    stand.state().raw = Some(("GET".into(), "/1.0/operations/".into(), one_chunk(body.as_bytes())));
    let e = client(&stand).create_instance(&NewInstance::container("web", Source::None)).await.unwrap_err();
    let ErrorKind::Incus { status, code, error } = e.kind() else { panic!("{e:?}") };
    assert_eq!((*status, *code), (500, 500));
    // The words of incus on one line: the start, the end of the first line
    // of the stderr, and the end of the kept 64 KiB with the size of the cut.
    let first: String = words.split('\n').next().unwrap().chars().take(200).collect();
    assert!(error.starts_with(&first), "{error}");
    assert!(error.contains("Cannot open: No space left on device … "), "{error}");
    assert!(error.ends_with(" bytes more)"), "{error}");
    let text = e.to_string();
    assert!(
        text.starts_with("POST /1.0/instances: incus refused the call (500, error_code 500): Failed creating"),
        "{text}"
    );
    assert!(text.contains("Cannot open: No space left on device"), "{text}");
}

#[tokio::test]
async fn an_answer_of_another_type_over_the_bound_is_refused_by_name() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let instances: Vec<_> = (0..2000).map(|i| json!({"name": format!("web-{i}"), "status": "Running"})).collect();
    let body = envelope("sync", 0, "", &json!(instances));
    stand.state().raw = Some(("GET".into(), "/1.0/instances".into(), one_chunk(body.as_bytes())));
    let e = client(&stand).instances().await.unwrap_err();
    let ErrorKind::TooLarge { status, size, bound, head, error } = e.kind() else { panic!("{e:?}") };
    assert_eq!((*status, *size, *bound), (200, BodySize::Chunk { before: 0, size: body.len() as u64 }, BOUND as u64));
    assert!(head.starts_with(br#"{"type":"sync""#), "{:?}", String::from_utf8_lossy(head));
    assert_eq!(error, &None);
    let text = e.to_string();
    let want = format!(
        "GET /1.0/instances: the answer of incus (HTTP 200) is larger than allowed: a chunk of {} bytes, over the \
         bound of {BOUND} bytes; the body starts with \"{{\\\"type\\\":\\\"sync\\\"",
        body.len()
    );
    assert!(text.starts_with(&want), "{text}");
}

/// An `error` envelope whose structure alone passes the bound stays a
/// refusal, with the `error` of its start.
#[tokio::test]
async fn a_refusal_gives_the_error_of_the_start_of_an_envelope() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let words = "Failed creating instance from image: Unpack failed: No space left on device";
    let body = envelope("error", 500, words, &json!(vec![1; 100_000]));
    stand.state().raw = Some(("GET".into(), "/1.0/operations/".into(), one_chunk(body.as_bytes())));
    let e = client(&stand).create_instance(&NewInstance::container("web", Source::None)).await.unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::TooLarge { .. }), "{e:?}");
    assert_eq!(e.incus_error(), Some(words));
    assert!(e.to_string().ends_with(&format!("; incus said: {words}")), "{e}");
}

#[tokio::test]
async fn a_content_length_over_the_bound_is_refused_with_its_size() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let console = vec![b'x'; 1 << 20];
    let mut raw = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", console.len()).into_bytes();
    raw.extend_from_slice(&console);
    stand.state().raw = Some(("GET".into(), "/1.0/instances/web/console".into(), raw));
    let e = client(&stand).console_log("web").await.unwrap_err();
    let ErrorKind::TooLarge { status, size, head, .. } = e.kind() else { panic!("{e:?}") };
    assert_eq!((*status, *size), (200, BodySize::Length(1 << 20)));
    assert!(!head.is_empty() && head.iter().all(|b| *b == b'x'));
    assert!(e.to_string().contains("a Content-Length of 1048576 bytes, over the bound of 65536 bytes"), "{e}");
}

#[tokio::test]
async fn a_pool_its_room_and_an_image_are_read() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = stand.client();
    let pool = client.storage_pool("default").await.unwrap();
    assert_eq!((pool.driver.as_str(), pool.source()), ("dir", Some("/var/lib/incus/storage-pools/default")));
    let room = client.storage_pool_resources("default").await.unwrap();
    assert_eq!((room.space.total, room.space.used), (376_438_784, 341_835_776));
    assert_eq!((room.inodes.total, room.free_inodes()), (102_400, Some(11)));
    let e = client.storage_pool("other").await.unwrap_err();
    assert_eq!(e.status(), Some(404));
    let image = client.image("ab12cd").await.unwrap().expect("the image");
    assert_eq!(
        (image.size, image.kind.as_str(), image.aliases[0].name.as_str()),
        (3_456_789, "container", "podvm/alpine")
    );
    assert_eq!(client.image("ffff").await.unwrap(), None);
}
