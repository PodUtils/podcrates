//! Stand-ins of the far ends of two roads, on the loopback, for the tests
//! of this crate and of a consumer (feature `standin`); no binary needs
//! them:
//!
//! * [`via`]: a forward proxy over TLS that takes HTTP/1.1's CONNECT, as
//!   squid's `https_port` does, and HTTP/2's CONNECT with h2's own server,
//!   as Caddy's forwardproxy does; Basic, and a `407` without it.
//! * [`wstunnel`]: a server of erebe/wstunnel that reads a client as its
//!   server of v11.1.0 does (the path prefix, the JWT, the restrictions),
//!   with the answers of 9 and of another server.
//! * [`echo`]: a target that sends a version line, then echoes.
//!
//! The TLS of a stand-in is the caller's `rustls::ServerConfig`: the crate
//! mints no certificate. Each stand-in keeps what it saw. A stand-in stops
//! taking connections when its value drops.

pub mod via;
pub mod wstunnel;

use std::io;
use std::sync::{Mutex, MutexGuard, PoisonError};

use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// The version line that [`echo`] sends first.
pub const BANNER: &[u8] = b"SSH-2.0-stand-in\r\n";

/// A task of a stand-in that stops when the value drops.
#[derive(Debug)]
pub struct Task(JoinHandle<()>);

impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
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
    Ok(Echo { port, _task: Task(task) })
}

/// What a stand-in saw, also after a task of it panicked.
fn locked<T>(seen: &Mutex<T>) -> MutexGuard<'_, T> {
    seen.lock().unwrap_or_else(PoisonError::into_inner)
}
