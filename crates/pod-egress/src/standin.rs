//! Stand-ins of the far ends of the roads, on the loopback, for the tests
//! of this crate and of a consumer (feature `standin`); no binary needs
//! them:
//!
//! * [`via`]: a forward proxy over TLS that takes HTTP/1.1's CONNECT, as
//!   squid's `https_port` does, and HTTP/2's CONNECT with h2's own server,
//!   as Caddy's forwardproxy does; Basic, and a `407` without it.
//! * [`wstunnel`]: a server of erebe/wstunnel that reads a client as its
//!   server of v11.1.0 does (the path prefix, the JWT, the restrictions),
//!   with the answers of 9 and of another server.
//! * [`vless`]: a VLESS server of Xray-core over a WebSocket, and over XHTTP
//!   on HTTP/2 or HTTP/1.1.
//! * [`trojan`]: a Trojan server of trojan-gfw in TLS alone or in a
//!   WebSocket, with its fallback.
//! * [`wisp`]: a Wisp server of version 1 or 2, with the credit of each
//!   stream counted.
//! * `shadowsocks` (with the feature `shadowsocks` too): a server of the
//!   AEAD form and of Shadowsocks 2022, from the specification's own
//!   ciphers.
//! * [`echo`]: a target that sends a version line, then echoes.
//!
//! The TLS of a stand-in is the caller's `rustls::ServerConfig`: the crate
//! mints no certificate. Its IDs, passwords and keys are made-up values or
//! the caller's. Each stand-in keeps what it saw, and stops taking
//! connections when its value drops ([`Stand::detach`] keeps it until the
//! runtime ends).

#[cfg(feature = "shadowsocks")]
pub mod shadowsocks;
pub mod trojan;
pub mod via;
pub mod vless;
pub mod wisp;
pub mod wstunnel;

use std::io;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// The version line that [`echo`] sends first.
pub const BANNER: &[u8] = b"SSH-2.0-stand-in\r\n";

/// A task of a stand-in that stops when the value drops.
#[derive(Debug)]
pub struct Task(Option<JoinHandle<()>>);

impl Task {
    pub(crate) fn of(task: JoinHandle<()>) -> Task {
        Task(Some(task))
    }

    /// The task runs on until its runtime ends.
    fn detach(mut self) {
        self.0 = None;
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}

/// A stand-in that runs: its port on the loopback, and what it saw.
#[derive(Debug)]
pub struct Stand<S> {
    pub port: u16,
    pub seen: Arc<S>,
    task: Task,
}

impl<S> Stand<S> {
    /// The port and what the stand-in sees; the stand-in runs on until its
    /// runtime ends.
    pub fn detach(self) -> (u16, Arc<S>) {
        self.task.detach();
        (self.port, self.seen)
    }
}

/// A target on the loopback: its port.
#[derive(Debug)]
pub struct Echo {
    pub port: u16,
    _task: Task,
}

/// A target on a free port of the loopback: [`BANNER`], then each byte
/// back, on each connection.
pub async fn echo() -> io::Result<Echo> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let task = tokio::spawn(async move {
        while let Ok((mut tcp, _)) = listener.accept().await {
            tokio::spawn(async move {
                if tcp.write_all(BANNER).await.is_ok() {
                    let (mut r, mut w) = tcp.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                }
            });
        }
    });
    Ok(Echo { port, _task: Task::of(task) })
}

/// What a stand-in saw, also after a task of it panicked.
fn locked<T>(seen: &Mutex<T>) -> MutexGuard<'_, T> {
    seen.lock().unwrap_or_else(PoisonError::into_inner)
}
