//! The files of an instance: a file with its owner and mode, an append, a
//! directory with its entries, a link, and a path that does not exist.
#![cfg(unix)]

mod common;

use common::Stand;
use pod_core::Class;
use pod_incus::{Client, FileContent, FileKind, FileWrite, NewInstance, Payload, Source};

async fn instance(stand: &Stand) -> Client {
    let client = stand.client();
    client.create_instance(&NewInstance::container("web", Source::None)).await.expect("an instance");
    client
}

#[tokio::test]
async fn a_file_is_written_and_read_with_its_owner_and_mode() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = instance(&stand).await;
    let mut write = FileWrite::file(Payload::bytes(b"#!/bin/sh\nexec /sbin/init\n".to_vec()));
    (write.uid, write.gid, write.mode) = (Some(1000), Some(100), Some(0o100755));
    client.put_file("web", "/podvm/init script", write).await.expect("written");
    let post = &stand.seen("POST", "/1.0/instances/web/files")[0];
    assert_eq!(post.query.get("path").map(String::as_str), Some("/podvm/init script"));
    assert_eq!(post.header("x-incus-type"), Some("file"));
    assert_eq!((post.header("x-incus-uid"), post.header("x-incus-gid")), (Some("1000"), Some("100")));
    assert_eq!(post.header("x-incus-mode"), Some("0755"), "the permission bits alone");
    assert_eq!(post.header("x-incus-write"), None);

    let file = client.get_file("web", "/podvm/init script").await.expect("read");
    assert_eq!(file.content, FileContent::Bytes(b"#!/bin/sh\nexec /sbin/init\n".to_vec()));
    assert_eq!(
        (file.meta.uid, file.meta.gid, file.meta.mode, file.meta.kind),
        (Some(1000), Some(100), Some(0o755), FileKind::File)
    );
}

#[tokio::test]
async fn an_append_adds_to_the_end_of_the_file() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = instance(&stand).await;
    client.put_file("web", "/log", FileWrite::file(Payload::bytes(b"one\n".to_vec()))).await.unwrap();
    let mut more = FileWrite::file(Payload::new(&b"two\n"[..], None));
    more.append = true;
    client.put_file("web", "/log", more).await.unwrap();
    assert_eq!(stand.seen("POST", "/1.0/instances/web/files")[1].header("x-incus-write"), Some("append"));
    assert_eq!(client.get_file("web", "/log").await.unwrap().content, FileContent::Bytes(b"one\ntwo\n".to_vec()));
}

#[tokio::test]
async fn a_directory_lists_its_entries_and_a_link_gives_its_target() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = instance(&stand).await;
    client.put_file("web", "/etc", FileWrite::directory()).await.unwrap();
    client
        .put_file("web", "/etc/hosts", FileWrite::file(Payload::bytes(b"127.0.0.1 localhost\n".to_vec())))
        .await
        .unwrap();
    client.put_file("web", "/etc/mtab", FileWrite::symlink("/proc/self/mounts")).await.unwrap();
    assert_eq!(stand.seen("POST", "/1.0/instances/web/files")[0].header("x-incus-type"), Some("directory"));

    let etc = client.get_file("web", "/etc").await.expect("the directory");
    assert_eq!(etc.meta.kind, FileKind::Directory);
    assert_eq!(etc.content, FileContent::Entries(vec!["hosts".into(), "mtab".into()]));
    let mtab = client.get_file("web", "/etc/mtab").await.unwrap();
    assert_eq!((mtab.meta.kind, mtab.content), (FileKind::Symlink, FileContent::Bytes(b"/proc/self/mounts".to_vec())));
}

#[tokio::test]
async fn a_path_that_does_not_exist_is_not_found_with_the_words_of_incus() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = instance(&stand).await;
    let e = client.get_file("web", "/nope").await.unwrap_err();
    assert_eq!((e.status(), e.incus_error(), e.class()), (Some(404), Some("Path /nope not found"), Class::NotFound));
    assert_eq!(e.call(), "GET /1.0/instances/web/files");
}
