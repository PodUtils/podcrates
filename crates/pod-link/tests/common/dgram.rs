//! The helpers of the tests of the carriers of datagrams (quic, udp): a
//! way out that carries UDP and counts its sockets, datagrams that go
//! silent after the first sends, and a resolver of one name.

use std::io::{self, IoSliceMut};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use pod_core::datagrams::{Datagrams, RecvMeta, SystemDatagrams, Transmit};
use pod_core::{BoxFuture, Class, Connected, Dial, Failure, Found, Resolve, Source, Stage, Target};

/// A way out that carries UDP alone: each `datagrams` is a socket of the
/// loopback, wrapped in [`Mute`] after `passes` sends when it is given.
#[derive(Default)]
pub struct UdpWay {
    pub sockets: AtomicUsize,
    pub passes: Option<usize>,
}

impl Dial for UdpWay {
    fn label(&self) -> String {
        "test-udp".into()
    }

    fn dial<'a>(&'a self, target: &'a Target, _: tokio::time::Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move { Err(Failure::new(Stage::Connect, Class::Unavailable, format!("no TCP to {target}"))) })
    }

    fn datagrams<'a>(&'a self) -> BoxFuture<'a, Result<Arc<dyn Datagrams>, Failure>> {
        Box::pin(async move {
            self.sockets.fetch_add(1, Ordering::SeqCst);
            let socket = SystemDatagrams::bind("127.0.0.1:0".parse().unwrap())
                .await
                .map_err(|e| Failure::new(Stage::Connect, Class::of_io(e.kind()), e.to_string()))?;
            Ok(match self.passes {
                Some(passes) => {
                    Arc::new(Mute { inner: socket, passes, sent: AtomicUsize::new(0) }) as Arc<dyn Datagrams>
                }
                None => Arc::new(socket),
            })
        })
    }
}

/// Datagrams that send the first `passes` transmits and lose each one
/// after them: a peer that stops in its handshake.
#[derive(Debug)]
pub struct Mute {
    inner: SystemDatagrams,
    passes: usize,
    sent: AtomicUsize,
}

impl Datagrams for Mute {
    fn poll_writable(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.poll_writable(cx)
    }

    fn try_send(&self, transmit: &Transmit<'_>) -> io::Result<()> {
        if self.sent.fetch_add(1, Ordering::SeqCst) < self.passes { self.inner.try_send(transmit) } else { Ok(()) }
    }

    fn poll_recv(
        &self,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Poll<io::Result<usize>> {
        self.inner.poll_recv(cx, bufs, meta)
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }
}

/// A resolver that knows one name.
pub struct OneName {
    pub name: &'static str,
    pub addr: SocketAddr,
    pub asked: AtomicUsize,
}

impl Resolve for OneName {
    fn label(&self) -> String {
        "test-one-name".into()
    }

    fn resolve_from<'a>(
        &'a self,
        host: &'a str,
        _port: u16,
        _deadline: tokio::time::Instant,
        _after: Option<Source>,
    ) -> BoxFuture<'a, Result<Found, Failure>> {
        Box::pin(async move {
            self.asked.fetch_add(1, Ordering::SeqCst);
            match host == self.name {
                true => Ok(Found { addrs: vec![self.addr], source: Source { rank: 0, name: "test" }, more: false }),
                false => Err(Failure::new(Stage::Dns, Class::NotFound, format!("{host}: no such name"))),
            }
        })
    }
}
