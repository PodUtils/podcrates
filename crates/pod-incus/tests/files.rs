//! The files of an instance: a file with its owner and mode, an append, a
//! directory with its entries, a link, and a path that does not exist; the
//! read of a file in pieces with a range and a bound.
#![cfg(unix)]

mod common;

use common::Stand;
use pod_core::Class;
use pod_incus::files::PIECE;
use pod_incus::{ByteRange, Client, ErrorKind, FileContent, FileKind, FileWrite, NewInstance, Payload, Source};

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

/// A file of `len` bytes that no slice repeats at a short period.
fn text(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

async fn with_file(stand: &Stand, bytes: &[u8]) -> Client {
    let client = instance(stand).await;
    let mut write = FileWrite::file(Payload::bytes(bytes.to_vec()));
    (write.uid, write.mode) = (Some(1000), Some(0o640));
    client.put_file("web", "/log", write).await.expect("written");
    client
}

#[tokio::test]
async fn a_read_in_pieces_gives_the_whole_file_with_its_meta_and_size() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let bytes = text(2 * PIECE + 7);
    let client = with_file(&stand, &bytes).await;
    let mut out = Vec::new();
    let read = client.get_file_to("web", "/log", None, None, &mut out).await.expect("read");
    assert_eq!(out, bytes);
    assert_eq!(
        (read.start, read.written, read.size, read.ended),
        (0, bytes.len() as u64, Some(bytes.len() as u64), true)
    );
    assert_eq!((read.meta.uid, read.meta.mode, read.meta.kind), (Some(1000), Some(0o640), FileKind::File));
    let get = stand.seen("GET", "/1.0/instances/web/files");
    assert_eq!(get[0].header("range"), None, "a read of the whole file sends no Range");
}

#[tokio::test]
async fn a_range_and_a_bound_give_the_slice_of_the_whole_read() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let bytes = text(PIECE + 100);
    let len = bytes.len() as u64;
    let client = with_file(&stand, &bytes).await;
    for ignore in [false, true] {
        stand.state().ignore_range = ignore;
        let past = format!("bytes={}-{}", len - 1, len + 49);
        let cases = [
            (5, None, None, "bytes=5-", true),
            (5, Some(20), None, "bytes=5-19", true),
            (5, Some(20), Some(3), "bytes=5-19", false),
            (5, None, Some(len - 5), "bytes=5-", true),
            (5, None, Some(10), "bytes=5-", false),
            (0, None, Some(PIECE as u64 + 1), "", false),
            (len - 1, Some(len + 50), None, past.as_str(), true),
        ];
        for (start, end, limit, header, ended) in cases {
            let mut out = Vec::new();
            let range = ByteRange { start, end };
            let read = client.get_file_to("web", "/log", Some(range), limit, &mut out).await.expect("read");
            let lo = start as usize;
            let mut hi = end.unwrap_or(len).min(len) as usize;
            if let Some(l) = limit {
                hi = hi.min(lo + l as usize);
            }
            let what = format!("{start} {end:?} {limit:?} ignore={ignore}");
            assert_eq!(out, bytes[lo..hi], "{what}");
            assert_eq!((read.start, read.written, read.ended), (start, out.len() as u64, ended), "{what}");
            assert_eq!(read.size, Some(len), "{what}");
            let sent = stand.seen("GET", "/1.0/instances/web/files").last().cloned().unwrap();
            assert_eq!(sent.header("range").unwrap_or(""), header, "{what}");
        }
    }
}

#[tokio::test]
async fn a_start_at_the_end_is_an_empty_read_with_the_size() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = with_file(&stand, b"0123456789").await;
    for (start, ignore) in [(10, false), (25, false), (10, true), (25, true)] {
        stand.state().ignore_range = ignore;
        let mut out = Vec::new();
        let read = client.get_file_to("web", "/log", Some(ByteRange::from(start)), None, &mut out).await.unwrap();
        assert!(out.is_empty(), "{start} {ignore}");
        assert_eq!((read.start, read.written, read.size, read.ended), (start, 0, Some(10), true), "{start} {ignore}");
        assert_eq!(read.meta.kind, FileKind::File);
    }
}

/// A writer that keeps the count and the largest write, not the bytes.
#[derive(Default)]
struct Counted {
    written: u64,
    largest: usize,
}

impl std::io::Write for Counted {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.largest = self.largest.max(buf.len());
        self.written += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The bound of the memory: with `max_body` at 4 KiB, `get_file` refuses a
/// file of 1 MiB, and `get_file_to` gives each byte of it in writes of one
/// piece at most.
#[tokio::test]
async fn a_file_larger_than_max_body_is_read_in_pieces() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let bytes = text(1 << 20);
    with_file(&stand, &bytes).await;
    let mut settings = stand.settings();
    settings.max_body = 4096;
    let client = Client::new(settings);
    let e = client.get_file("web", "/log").await.unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::Io(_)), "{e}");
    let mut out = Counted::default();
    let read = client.get_file_to("web", "/log", None, None, &mut out).await.expect("read");
    assert_eq!((read.written, out.written, read.ended), (1 << 20, 1 << 20, true));
    assert!(out.largest <= PIECE, "a write of {} bytes", out.largest);
}

struct Closed;

impl std::io::Write for Closed {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn a_writer_that_fails_a_directory_and_a_missing_path_are_errors_by_name() {
    pod_testkit::need_loopback!();
    let stand = Stand::start();
    let client = with_file(&stand, b"abc").await;
    let e = client.get_file_to("web", "/log", None, None, &mut Closed).await.unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::Output(o) if o.kind() == std::io::ErrorKind::BrokenPipe), "{e}");
    client.put_file("web", "/etc", FileWrite::directory()).await.unwrap();
    let e = client.get_file_to("web", "/etc", None, None, &mut Vec::new()).await.unwrap_err();
    assert_eq!(e.class(), Class::Usage, "{e}");
    let e = client.get_file_to("web", "/nope", None, None, &mut Vec::new()).await.unwrap_err();
    assert_eq!((e.status(), e.incus_error(), e.class()), (Some(404), Some("Path /nope not found"), Class::NotFound));
}
