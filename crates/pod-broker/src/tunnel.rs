//! The inside of a tunnel after the broker allowed its CONNECT: the bytes
//! go both ways as they are ([`Splice`]), or a [`TunnelEnd`] that the host
//! gives takes them: the TLS end of [`crate::tls_end`] (T-100) reads the SNI
//! and each request, checks the SNI and the `Host` against the CONNECT
//! target (421), checks each path ([`Tunnel::allows_path`]), swaps the
//! placeholder credentials and scrubs each answer.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use pod_core::{BoxFuture, BoxedIo, Failure, Stage};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::audit::Line;
use crate::policy::Compiled;
use crate::target::Authority;
use crate::vault::Vault;

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

/// What a TLS end did in one tunnel, for its audit line: no path, no
/// header and no value.
#[derive(Debug, Default)]
pub struct Report {
    /// The TLS end read the tunnel.
    pub tls: AtomicBool,
    /// The requests that it read.
    pub requests: AtomicU64,
    /// The placeholders that it swapped for their values.
    pub swaps: AtomicU64,
    /// The values that it took out of the answers.
    pub scrubs: AtomicU64,
    /// What passed with no swap or was refused, by a word of the TLS end
    /// (`body-large`, `answer-encoded`, `sni-mismatch`).
    notes: Mutex<BTreeSet<&'static str>>,
}

impl Report {
    pub fn note(&self, word: &'static str) {
        self.notes.lock().unwrap_or_else(|e| e.into_inner()).insert(word);
    }

    pub fn notes(&self) -> Vec<&'static str> {
        self.notes.lock().unwrap_or_else(|e| e.into_inner()).iter().copied().collect()
    }

    /// The fields of the report in `line`: the counts when the TLS end read
    /// the tunnel, and the notes.
    pub(crate) fn fill(&self, line: &mut Line) {
        if self.tls.load(Ordering::Relaxed) {
            line.set("tls", "end").set("requests", self.requests.load(Ordering::Relaxed));
            line.set("swaps", self.swaps.load(Ordering::Relaxed)).set("scrubs", self.scrubs.load(Ordering::Relaxed));
        }
        let notes = self.notes();
        if !notes.is_empty() {
            line.set("notes", notes.join(" "));
        }
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
    /// The CA and the secrets of the job, when its tunnels have a TLS end.
    pub vault: Option<&'a Vault>,
    /// What the end did, for the audit line.
    pub report: &'a Report,
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
