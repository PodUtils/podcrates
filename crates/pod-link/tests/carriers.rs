//! The carriers: the memory carrier's names and rings, and the unix
//! carrier's path, mode, claim and peer check.

mod common;

use common::LIMIT;
#[cfg(unix)]
use common::scratch;
use pod_link::memory::{self, Memory};
use pod_link::{Carrier, CarrierErrorKind, Facts};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

#[tokio::test]
async fn the_memory_carrier_connects_by_name() {
    let carrier = Memory::default();
    assert_eq!(carrier.name(), "memory");
    assert_eq!(carrier.facts(), Facts::STREAM);
    assert!(carrier.available().is_ok());
    let mut door = carrier.listen("a").await.unwrap();
    assert_eq!(door.address(), "a");
    let e = carrier.listen("a").await.err().unwrap();
    assert_eq!(e.kind, CarrierErrorKind::InUse);
    assert_eq!(e.class().exit_code(), 69);
    let e = carrier.dial("b").await.err().unwrap();
    assert_eq!(e.kind, CarrierErrorKind::NotFound);
    assert_eq!(carrier.dial("").await.err().unwrap().kind, CarrierErrorKind::Usage);
    let (near, far) = tokio::join!(carrier.dial("a"), door.accept());
    let (mut near, mut far) = (near.unwrap(), far.unwrap());
    near.write_all(b"ping").await.unwrap();
    drop(near);
    let mut got = Vec::new();
    far.read_to_end(&mut got).await.unwrap();
    assert_eq!(got, b"ping", "the end of one side is the end of stream of the other");
    // A listener that is gone frees its name.
    drop(door);
    assert_eq!(carrier.dial("a").await.err().unwrap().kind, CarrierErrorKind::NotFound);
    carrier.listen("a").await.unwrap();
}

#[tokio::test]
async fn a_full_ring_makes_the_writer_wait_and_drops_nothing() {
    assert_eq!(Memory::new(1000).err().unwrap().kind, CarrierErrorKind::Usage);
    assert_eq!(Memory::new(memory::MAX_RING + 1).err().unwrap().kind, CarrierErrorKind::Usage);
    let ring = memory::MIN_RING;
    let (mut a, mut b) = memory::pair(ring);
    let data = common::pattern(7, 4 * ring + 3);
    let send = data.clone();
    let writer = tokio::spawn(async move {
        a.write_all(&send).await.unwrap();
        a
    });
    // The writer waits on the full ring; nothing is lost.
    tokio::task::yield_now().await;
    assert!(!writer.is_finished());
    let mut got = vec![0; data.len()];
    timeout(LIMIT, b.read_exact(&mut got)).await.unwrap().unwrap();
    assert!(got == data);
    drop(writer.await.unwrap());
}

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::PermissionsExt as _;

    use pod_link::unix::{MAX_PATH, Unix};

    use super::*;

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[tokio::test]
    async fn a_listener_has_the_mode_0600_and_leaves_no_staging_directory() {
        let dir = scratch("mode");
        let path = dir.join("s.sock");
        let carrier = Unix::default();
        assert_eq!(carrier.name(), "unix");
        assert!(carrier.available().is_ok());
        let mut door = carrier.listen(path.to_str().unwrap()).await.unwrap();
        assert_eq!(mode(&path), 0o600);
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, ["s.sock"]);
        let (near, far) = tokio::join!(carrier.dial(path.to_str().unwrap()), door.accept());
        let (mut near, mut far) = (near.unwrap(), far.unwrap());
        near.write_all(b"over unix").await.unwrap();
        near.shutdown().await.unwrap();
        let mut got = Vec::new();
        far.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, b"over unix");
        drop(door);
        assert!(!path.exists(), "the listener removes its socket");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_regular_file_is_never_removed() {
        let dir = scratch("file");
        let path = dir.join("f");
        std::fs::write(&path, b"keep").unwrap();
        let e = Unix::default().listen(path.to_str().unwrap()).await.err().unwrap();
        assert_eq!(e.kind, CarrierErrorKind::Usage);
        assert_eq!(e.class().exit_code(), 64);
        assert_eq!(std::fs::read(&path).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_live_socket_is_in_use_and_a_stale_one_is_replaced() {
        let dir = scratch("claim");
        let path = dir.join("c.sock");
        let at = path.to_str().unwrap();
        let carrier = Unix::default();
        let first = carrier.listen(at).await.unwrap();
        let e = carrier.listen(at).await.err().unwrap();
        assert_eq!(e.kind, CarrierErrorKind::InUse);
        assert_eq!(e.class().exit_code(), 69);
        drop(first);
        // A socket file whose listener is gone: a std listener that is
        // dropped leaves its file.
        drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
        assert!(path.exists());
        let mut again = carrier.listen(at).await.unwrap();
        let (near, far) = tokio::join!(carrier.dial(at), again.accept());
        near.unwrap();
        far.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_path_out_of_its_limits_is_a_usage_error() {
        let carrier = Unix::default();
        let long = format!("/tmp/{}", "x".repeat(MAX_PATH));
        for at in ["", long.as_str()] {
            assert_eq!(carrier.listen(at).await.err().unwrap().kind, CarrierErrorKind::Usage);
            assert_eq!(carrier.dial(at).await.err().unwrap().kind, CarrierErrorKind::Usage);
        }
    }

    #[tokio::test]
    async fn a_dial_with_no_listener_is_not_found() {
        let dir = scratch("none");
        let path = dir.join("n.sock");
        let e = Unix::default().dial(path.to_str().unwrap()).await.err().unwrap();
        assert_eq!(e.kind, CarrierErrorKind::NotFound);
        assert_eq!(e.class().exit_code(), 66);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_socket_pair_carries_a_link() {
        let (a, b) = pod_link::unix::pair().unwrap();
        let s = pod_link::Settings::default();
        let dialer = pod_link::Link::new(a, pod_link::Role::Dialer, s).unwrap();
        let listener = pod_link::Link::new(b, pod_link::Role::Listener, s).unwrap();
        let mut x = dialer.open().unwrap();
        x.write_all(b"pair").await.unwrap();
        x.shutdown().await.unwrap();
        let mut y = timeout(LIMIT, listener.accept()).await.unwrap().unwrap();
        let mut got = Vec::new();
        y.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, b"pair");
    }
}
