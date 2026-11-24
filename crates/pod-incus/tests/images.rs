//! The import of an image from a stream: a unified tarball in chunks or by
//! its length, a split image as a multipart body, the aliases after it, and
//! a fingerprint that incus computes otherwise.
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::Stand;
use common::store::fingerprint;
use pod_incus::{AnswerPhase, Client, ErrorKind, ImageData, ImageImport, Payload};
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn a_unified_tarball_is_imported_from_a_stream_of_unknown_length() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    // A stream of 1 MiB and some bytes, written as the import reads it.
    let tarball: Vec<u8> = (0..(1 << 20) + 77).map(|i: u32| (i % 251) as u8).collect();
    let (mut writer, reader) = tokio::io::duplex(4096);
    let source = tarball.clone();
    let feed = tokio::spawn(async move {
        for piece in source.chunks(10_000) {
            writer.write_all(piece).await.unwrap();
        }
    });
    let mut import = ImageImport::new(ImageData::Unified(Payload::new(reader, None)));
    import.filename = "alpine.tar.gz".into();
    import.public = true;
    import.properties.insert("os".into(), "Alpine Linux".into());
    import.properties.insert("release".into(), "3.20".into());
    import.aliases = vec!["podvm/alpine".into(), "podvm/alpine:3.20".into()];
    let image = stand.client().import_image(import).await.expect("imported");
    feed.await.unwrap();
    assert_eq!((image.fingerprint.clone(), image.size), (fingerprint(&tarball), tarball.len() as u64));

    let post = &stand.seen("POST", "/1.0/images")[0];
    assert_eq!(post.header("transfer-encoding"), Some("chunked"));
    assert_eq!(post.header("content-type"), Some("application/octet-stream"));
    assert_eq!(post.header("x-incus-public"), Some("true"));
    assert_eq!(post.header("x-incus-filename"), Some("alpine.tar.gz"));
    assert_eq!(post.header("x-incus-properties"), Some("os=Alpine%20Linux&release=3.20"));
    assert_eq!(post.header("x-incus-aliases"), None, "incus 6.0 has no image_import_alias");
    assert_eq!(stand.state().images[0][0].2, tarball);
    let aliases = stand.state().aliases.clone();
    let want = [
        ("podvm/alpine".to_string(), image.fingerprint.clone()),
        ("podvm/alpine:3.20".into(), image.fingerprint.clone()),
    ];
    assert_eq!(aliases, want);

    // podvm asks for an alias before it imports: a name with a slash goes
    // escaped, and an alias that incus does not have is None.
    let alias = stand.client().alias("podvm/alpine:3.20").await.expect("read").expect("the alias is there");
    assert_eq!((alias.name.as_str(), alias.target.as_str()), ("podvm/alpine:3.20", image.fingerprint.as_str()));
    assert!(stand.seen("GET", "/1.0/images/aliases/podvm%2Falpine%3A3.20").len() == 1);
    assert_eq!(stand.client().alias("podvm/absent").await.expect("a 404 is no error"), None);
}

#[tokio::test]
async fn a_split_image_goes_as_two_parts_of_a_multipart_body() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let (meta, rootfs) = (b"metadata.yaml in a tarball".to_vec(), b"the root file system\r\n--x".to_vec());
    let data =
        ImageData::Split { metadata: Payload::bytes(meta.clone()), rootfs: Payload::bytes(rootfs.clone()), vm: false };
    let mut import = ImageImport::new(data);
    import.filename = "rootfs.tar".into();
    let image = stand.client().import_image(import).await.expect("imported");
    let both: Vec<u8> = meta.iter().chain(rootfs.iter()).copied().collect();
    assert_eq!(image.fingerprint, fingerprint(&both));

    let post = &stand.seen("POST", "/1.0/images")[0];
    let content_type = post.header("content-type").unwrap();
    assert!(content_type.starts_with("multipart/form-data; boundary="), "{content_type}");
    assert_eq!(post.header("content-length").map(|n| n.parse::<usize>().unwrap()), Some(post.body.len()));
    let parts = stand.state().images[0].clone();
    assert_eq!(parts[0], ("metadata".to_string(), "metadata.tar".to_string(), meta.clone()));
    assert_eq!(parts[1], ("rootfs".to_string(), "rootfs.tar".to_string(), rootfs.clone()));

    let data = ImageData::Split { metadata: Payload::bytes(meta), rootfs: Payload::new(&b"disk"[..], None), vm: true };
    stand.client().import_image(ImageImport::new(data)).await.expect("imported");
    let post = &stand.seen("POST", "/1.0/images")[1];
    assert_eq!(post.header("transfer-encoding"), Some("chunked"));
    assert_eq!(stand.state().images[1][1].0, "rootfs.img");
}

#[tokio::test]
async fn a_fingerprint_that_incus_computes_otherwise_fails_the_import() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let mut import = ImageImport::new(ImageData::Unified(Payload::bytes(b"tarball".to_vec())));
    import.fingerprint = Some("00".repeat(32));
    import.aliases = vec!["never".into()];
    let e = stand.client().import_image(import).await.unwrap_err();
    let ErrorKind::Operation { err, .. } = e.kind() else { panic!("{e:?}") };
    assert!(err.starts_with("fingerprints don't match, got "), "{err}");
    assert_eq!(e.call(), "POST /1.0/images");
    assert_eq!(stand.seen("POST", "/1.0/images")[0].header("x-incus-fingerprint"), Some("00".repeat(32).as_str()));
    assert!(stand.state().aliases.is_empty(), "no alias for an image that failed");
}

#[tokio::test]
async fn a_payload_shorter_than_its_length_is_refused() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let import = ImageImport::new(ImageData::Unified(Payload::new(&b"short"[..], Some(10))));
    let e = stand.client().import_image(import).await.unwrap_err();
    assert_eq!(e.class(), pod_core::Class::Usage);
    assert_eq!(e.to_string(), "POST /1.0/images: the payload ended after 5 of its 10 bytes");
}

/// The answer of incusd that cannot store an upload: 500, then the close
/// while the client still writes (PodUtils/podvm#653).
const NO_SPACE: &str = "write /var/lib/incus/images/incus_post_1: no space left on device";

/// An import of 16 MiB, far more than the buffer of the socket.
fn large_import() -> ImageImport {
    ImageImport::new(ImageData::Unified(Payload::bytes(vec![0u8; 16 << 20])))
}

#[tokio::test]
async fn an_upload_that_incus_refuses_and_closes_gives_the_answer_of_incus() {
    pod_testkit::need_loopback!();
    let raw = common::raw::Raw::start(common::error(500, NO_SPACE), common::raw::After::Close);
    for run in 0..10 {
        let call =
            tokio::time::timeout(Duration::from_secs(60), async { raw.client().import_image(large_import()).await });
        let e = call.await.expect("the call ends").unwrap_err();
        assert_eq!((e.status(), e.incus_error()), (Some(500), Some(NO_SPACE)), "run {run}: {e:?}");
        assert_eq!(e.call(), "POST /1.0/images");
    }
}

#[tokio::test]
async fn an_upload_that_incus_closes_with_no_answer_gives_the_error_of_the_write() {
    pod_testkit::need_loopback!();
    let raw = common::raw::Raw::start(Vec::new(), common::raw::After::Close);
    let call = tokio::time::timeout(Duration::from_secs(60), async { raw.client().import_image(large_import()).await });
    let e = call.await.expect("the call ends").unwrap_err();
    let ErrorKind::Io(io) = e.kind() else { panic!("{e:?}") };
    assert!(matches!(io.kind(), std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset), "{io:?}");
    // With no reset, the write gives EPIPE and the read its end: the error
    // is the one of the write.
    let raw = common::raw::Raw::start(Vec::new(), common::raw::After::Shut);
    let call = tokio::time::timeout(Duration::from_secs(60), async { raw.client().import_image(large_import()).await });
    let e = call.await.expect("the call ends").unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::Io(io) if io.kind() == std::io::ErrorKind::BrokenPipe), "{e:?}");
}

/// incus that answers and then reads nothing more, with the connection
/// open: the write stalls until `write_timeout`, and the answer is read
/// within 5 s after it (or `answer_timeout`, where it is shorter). A whole
/// answer is the answer, a stalled one ends at that bound. No peer of a
/// Unix socket on macOS can give EPIPE and keep the read side open: the
/// stall is the cut that each OS gives.
#[tokio::test(start_paused = true)]
async fn an_answer_that_stalls_after_a_cut_ends_at_its_short_bound() {
    pod_testkit::need_loopback!();
    let limit = Duration::from_secs(600);
    let raw = common::raw::Raw::start(common::error(500, NO_SPACE), common::raw::After::Hold);
    let write = raw.settings().write_timeout;
    let started = tokio::time::Instant::now();
    let call = tokio::time::timeout(limit, async { raw.client().import_image(large_import()).await });
    let e = call.await.expect("the call ends at the bound of the write").unwrap_err();
    assert_eq!((e.status(), e.incus_error()), (Some(500), Some(NO_SPACE)), "{e:?}");
    assert_eq!(started.elapsed().as_secs(), write.as_secs(), "the answer is read at the stall");

    let mut stalled = common::error(500, NO_SPACE);
    stalled.truncate(stalled.len() - 10);
    let raw = common::raw::Raw::start(stalled, common::raw::After::Hold);
    let started = tokio::time::Instant::now();
    let call = tokio::time::timeout(limit, async { raw.client().import_image(large_import()).await });
    let e = call.await.expect("the call ends at the bound").unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::AnswerTimeout { phase: AnswerPhase::Body, .. }), "{e:?}");
    assert_eq!(started.elapsed().as_secs(), write.as_secs() + 5, "5 s, under the 30 s of answer_timeout");

    let mut settings = raw.settings();
    settings.answer_timeout = Some(Duration::from_secs(2));
    let started = tokio::time::Instant::now();
    let call = tokio::time::timeout(limit, async { Client::new(settings).import_image(large_import()).await });
    let e = call.await.expect("the call ends at the bound").unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::AnswerTimeout { bound, .. } if bound.as_secs() == 2), "{e:?}");
    assert_eq!(started.elapsed().as_secs(), write.as_secs() + 2);
}

/// incus that reads nothing of the body and does not answer: the call
/// ends at `write_timeout` and the 5 s of the late answer, with the error
/// of the write, not at the limit of the caller.
#[tokio::test(start_paused = true)]
async fn an_upload_that_incus_stops_reading_ends_at_the_bound_of_the_write() {
    pod_testkit::need_loopback!();
    let raw = common::raw::Raw::start(Vec::new(), common::raw::After::Hold);
    let mut settings = raw.settings();
    settings.write_timeout = Duration::from_secs(7);
    let started = tokio::time::Instant::now();
    let call = tokio::time::timeout(Duration::from_secs(600), async {
        Client::new(settings).import_image(large_import()).await
    });
    let e = call.await.expect("the call ends at the bound of the write").unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::Io(io) if io.kind() == std::io::ErrorKind::TimedOut), "{e:?}");
    assert_eq!(e.class(), pod_core::Class::Timeout);
    assert_eq!(started.elapsed().as_secs(), 7 + 5);
}

/// An `error` envelope over `max_body` after the cut is refused by size,
/// with the words of its start: not lost behind the error of the write.
#[tokio::test]
async fn an_error_envelope_over_the_bound_after_a_cut_gives_too_large_with_its_words() {
    pod_testkit::need_loopback!();
    let start = format!(
        r#"{{"type":"error","status":"","status_code":0,"operation":"","error_code":500,"error":"{NO_SPACE}","metadata":"#
    );
    let mut answer = format!(
        "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        2u64 << 30
    )
    .into_bytes();
    answer.extend_from_slice(start.as_bytes());
    let raw = common::raw::Raw::start(answer, common::raw::After::Close);
    let call = tokio::time::timeout(Duration::from_secs(60), async { raw.client().import_image(large_import()).await });
    let e = call.await.expect("the call ends").unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::TooLarge { status: 500, .. }), "{e:?}");
    assert_eq!(e.incus_error(), Some(NO_SPACE));
}
