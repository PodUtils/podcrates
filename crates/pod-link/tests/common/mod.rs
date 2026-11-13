//! The helpers of the tests: two ends of a link over each carrier, an echo
//! of each stream, a raw peer that writes frames by hand, and scratch
//! directories.

#![allow(dead_code)]

#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "ws")]
pub mod ws;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use pod_link::{Carrier, Decoder, Frame, Link, Role, Settings, memory::Memory};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

/// A fresh, empty scratch directory unique to one test. Short, so that a
/// socket path in it fits in `sun_path`.
pub fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("pl-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The two ends of a link over the memory carrier.
pub async fn over_memory(dialer: Settings, listener: Settings) -> (Link, Link) {
    let carrier = Memory::default();
    let mut door = carrier.listen("peer").await.unwrap();
    let (near, far) = tokio::join!(carrier.dial("peer"), door.accept());
    (
        Link::new(near.unwrap(), Role::Dialer, dialer).unwrap(),
        Link::new(far.unwrap(), Role::Listener, listener).unwrap(),
    )
}

/// The two ends of a link over the unix carrier, at a path in a scratch
/// directory.
pub async fn over_unix(dialer: Settings, listener: Settings) -> (Link, Link) {
    let dir = scratch("link");
    let path = dir.join("l.sock");
    let carrier = pod_link::unix::Unix::default();
    let mut door = carrier.listen(path.to_str().unwrap()).await.unwrap();
    let (near, far) = tokio::join!(carrier.dial(path.to_str().unwrap()), door.accept());
    drop(door);
    let _ = std::fs::remove_dir_all(&dir);
    (
        Link::new(near.unwrap(), Role::Dialer, dialer).unwrap(),
        Link::new(far.unwrap(), Role::Listener, listener).unwrap(),
    )
}

/// Run `body` on a link over the memory carrier and on one over the unix
/// carrier, each with these settings at both ends.
#[macro_export]
macro_rules! on_both {
    ($memory:ident, $unix:ident, $settings:expr, $body:path) => {
        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn $memory() {
            let (a, b) = common::over_memory($settings, $settings).await;
            $body(a, b).await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn $unix() {
            let (a, b) = common::over_unix($settings, $settings).await;
            $body(a, b).await;
        }
    };
}

/// Answer each stream that `link` accepts with its own bytes, and close
/// it at its end of stream.
pub fn echo(link: Link) -> Arc<Link> {
    let link = Arc::new(link);
    let accepting = link.clone();
    tokio::spawn(async move {
        while let Ok(mut stream) = accepting.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0; 32 << 10];
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if stream.write_all(&buf[..n]).await.is_err() {
                                return;
                            }
                        }
                    }
                }
                let _ = stream.shutdown().await;
            });
        }
    });
    link
}

/// Bytes that tell their own place: a stream that mixes or loses bytes
/// fails the compare.
pub fn pattern(seed: u32, len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u32).wrapping_mul(2_654_435_761).wrapping_add(seed).to_le_bytes()[1]).collect()
}

/// A time limit for each wait of a test.
pub const LIMIT: Duration = Duration::from_secs(30);

/// A peer that speaks frames by hand over the memory carrier: the test
/// writes each frame and reads what the link sent.
pub struct RawPeer {
    pub io: DuplexStream,
    pub decoder: Decoder,
}

impl RawPeer {
    /// A link of `role` with `settings` against a raw peer, which sent its
    /// preface.
    pub async fn start(role: Role, settings: Settings) -> (Link, RawPeer) {
        let (near, mut far) = pod_link::memory::pair(1 << 20);
        far.write_all(pod_link::frame::PREFACE).await.unwrap();
        (Link::new(near, role, settings).unwrap(), RawPeer { io: far, decoder: Decoder::with_preface() })
    }

    pub async fn send(&mut self, frame: Frame) {
        let mut out = Vec::new();
        frame.encode(&mut out).unwrap();
        self.io.write_all(&out).await.unwrap();
    }

    /// The next frame of the link, `None` at the end of the carrier.
    pub async fn next(&mut self) -> Option<Frame> {
        let mut buf = vec![0; 64 << 10];
        loop {
            if let Some(frame) = self.decoder.next().expect("the link sends valid frames") {
                return Some(frame);
            }
            let n = tokio::time::timeout(LIMIT, self.io.read(&mut buf)).await.expect("a frame in time").ok()?;
            if n == 0 {
                return None;
            }
            self.decoder.push(&buf[..n]);
        }
    }

    /// The frames of the link until one matches `want`.
    pub async fn until(&mut self, want: impl Fn(&Frame) -> bool) -> Frame {
        loop {
            let frame = self.next().await.expect("the link ended before the frame");
            if want(&frame) {
                return frame;
            }
        }
    }
}
