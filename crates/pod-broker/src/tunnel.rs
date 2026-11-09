//! The inside of a tunnel after the broker allowed its CONNECT: the bytes
//! go both ways as they are ([`Splice`]), or a [`TunnelEnd`] that the host
//! gives takes them. T-100 adds a TLS end with a CA for each job: it reads
//! the SNI and each request, checks the SNI against the CONNECT target
//! (421), checks each path ([`Tunnel::allows_path`]), and swaps the
//! placeholder credentials.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};

use pod_core::{BoxFuture, BoxedIo, Failure, Stage};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::policy::Compiled;
use crate::target::Authority;

/// The bytes of a tunnel, each way. A [`TunnelEnd`] counts here, so that the
/// audit line has them also when the job ends the tunnel.
#[derive(Debug, Default)]
pub struct Counts {
    /// From the job to the far end.
    pub up: AtomicU64,
    /// From the far end to the job.
    pub down: AtomicU64,
}

impl Counts {
    pub fn up(&self) -> u64 {
        self.up.load(Ordering::Relaxed)
    }

    pub fn down(&self) -> u64 {
        self.down.load(Ordering::Relaxed)
    }
}

/// One allowed tunnel.
pub struct Tunnel<'a> {
    pub job: &'a str,
    /// The CONNECT target.
    pub authority: &'a Authority,
    /// The checked address that the broker dialed.
    pub addr: Option<SocketAddr>,
    /// The policy of the job.
    pub policy: &'a Compiled,
    /// The rule that allowed the CONNECT, or [`crate::policy::PATH_NEEDS_TLS_END`].
    pub rule: &'a str,
    pub counts: &'a Counts,
}

impl Tunnel<'_> {
    /// The rule that allows a request to `path` (in its normal form,
    /// [`crate::target::normal_path`]) in this tunnel.
    pub fn allows_path(&self, path: &str) -> Option<String> {
        self.policy.allows_path(self.authority, path)
    }
}

/// The end of a tunnel that the host gives in place of [`Splice`].
pub trait TunnelEnd: Send + Sync {
    /// True when this end reads each request in the tunnel and checks its
    /// path: the broker then allows a CONNECT to a host whose rules each
    /// have a path, and leaves the paths to this end.
    fn reads_requests(&self) -> bool {
        false
    }

    /// Carry the tunnel between `client` (the job, after the 200) and
    /// `upstream` (the far end) until one ends. The broker ends the future
    /// when the job ends.
    fn run<'a>(&'a self, tunnel: Tunnel<'a>, client: BoxedIo, upstream: BoxedIo) -> BoxFuture<'a, Result<(), Failure>>;
}

/// The bytes both ways, as they are.
#[derive(Debug, Default, Clone, Copy)]
pub struct Splice;

impl TunnelEnd for Splice {
    fn run<'a>(&'a self, tunnel: Tunnel<'a>, client: BoxedIo, upstream: BoxedIo) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let (cr, cw) = tokio::io::split(client);
            let (ur, uw) = tokio::io::split(upstream);
            let (a, b) = tokio::join!(pipe(cr, uw, &tunnel.counts.up), pipe(ur, cw, &tunnel.counts.down));
            a.and(b).map_err(|e| Failure::io(Stage::Connect, e))
        })
    }
}

/// Copy `from` to `to` until the end of `from`, then end the write side of
/// `to`; count each byte.
pub async fn pipe<R, W>(mut from: R, mut to: W, count: &AtomicU64) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = from.read(&mut buf).await?;
        if n == 0 {
            let _ = to.shutdown().await;
            return Ok(());
        }
        to.write_all(&buf[..n]).await?;
        to.flush().await?;
        count.fetch_add(n as u64, Ordering::Relaxed);
    }
}
