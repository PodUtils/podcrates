//! The import of an image from a stream: a unified tarball in chunks or by
//! its length, a split image as a multipart body, the aliases after it, and
//! a fingerprint that incus computes otherwise.
#![cfg(unix)]

mod common;

use common::Stand;
use common::store::fingerprint;
use pod_incus::{ErrorKind, ImageData, ImageImport, Payload};
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
