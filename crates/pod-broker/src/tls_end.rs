//! The TLS end of the broker (T-100): a [`TunnelEnd`] that ends the TLS of
//! the job with a leaf of the CA of the job, and opens its own TLS to the
//! far end with the trust of the host. It reads each request of HTTP/1.1,
//! checks it ([`crate::inner::check`]), swaps each placeholder for the value
//! of its secret, and scrubs each value out of the answer.
//!
//! It reads the tunnel of a host that a credential of the job names, or of
//! a host whose rules each have a path. Each other tunnel passes as it is
//! ([`Splice`]). The ALPN is `http/1.1` on both sides: no HTTP/2.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use pod_core::{BoxFuture, BoxedIo, Class, Failure, Stage};
use pod_proxy::http::request::refusal;
use pod_proxy::http::{Body, Request};
use rustls::crypto::CryptoProvider;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zeroize::Zeroizing;

use crate::inner::{Out, Outgoing, Pairs, Refused, check, encoded, out_of, request_head};
use crate::policy::PATH_NEEDS_TLS_END;
use crate::prefixed::Prefixed;
use crate::replace::Replace;
use crate::target::Host;
use crate::tunnel::{Splice, Tunnel, TunnelEnd};
use crate::vault::Vault;

/// The largest body that the TLS end reads whole: a request body with a
/// length that it swaps, and an answer with a length that it scrubs and
/// sends with its new length. A larger request body passes with no swap; a
/// larger answer is scrubbed as it comes, in chunks.
pub const MAX_BODY: u64 = 1 << 20;

/// The bounds of a TLS end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// [`MAX_BODY`] by default.
    pub max_body: u64,
    /// The longest wait for each handshake.
    pub handshake: Duration,
    /// The longest wait for the next request on a connection.
    pub idle: Duration,
}

impl Default for Bounds {
    fn default() -> Bounds {
        Bounds { max_body: MAX_BODY, handshake: Duration::from_secs(20), idle: Duration::from_secs(120) }
    }
}

/// The TLS end: the provider of the host, and the configuration of its TLS
/// client to the far ends (its trust: [`pod_tls::Context::config_for`]).
pub struct TlsEnd {
    provider: Arc<CryptoProvider>,
    upstream: Arc<rustls::ClientConfig>,
    bounds: Bounds,
}

impl std::fmt::Debug for TlsEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsEnd").field("bounds", &self.bounds).finish_non_exhaustive()
    }
}

impl TlsEnd {
    pub fn new(provider: Arc<CryptoProvider>, upstream: Arc<rustls::ClientConfig>) -> TlsEnd {
        let mut config = (*upstream).clone();
        config.alpn_protocols = vec![pod_tls::server::HTTP1.to_vec()];
        TlsEnd { provider, upstream: Arc::new(config), bounds: Bounds::default() }
    }

    pub fn with_bounds(mut self, bounds: Bounds) -> TlsEnd {
        self.bounds = bounds;
        self
    }
}

impl TunnelEnd for TlsEnd {
    fn reads_requests(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, tunnel: Tunnel<'a>, client: BoxedIo, upstream: BoxedIo) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let paths = tunnel.rule == PATH_NEEDS_TLS_END;
            match tunnel.vault {
                Some(vault) if paths || vault.names(&tunnel.authority.host) => {
                    match first_speaker(client, upstream).await? {
                        First::Tls(client, upstream) => self.end(&tunnel, vault, client, upstream).await,
                        // Not TLS (SSH to a host of a credential): no swap. A
                        // host whose rules have paths needs its paths read.
                        First::Other(..) if paths => {
                            tunnel.report.note("not-tls");
                            Err(Failure::new(
                                Stage::Proxy,
                                Class::Policy,
                                "a tunnel whose paths are checked carries TLS",
                            ))
                        }
                        First::Other(client, upstream) => {
                            tunnel.report.note("not-tls");
                            Splice.run(tunnel, client, upstream).await
                        }
                    }
                }
                None if paths => {
                    Err(Failure::new(Stage::Proxy, Class::Policy, "no CA for a host whose rules have paths"))
                }
                _ => Splice.run(tunnel, client, upstream).await,
            }
        })
    }
}

/// Who spoke first in a tunnel, and the two streams with the bytes that
/// were read put back.
enum First {
    /// The job, with a TLS record of a handshake.
    Tls(BoxedIo, BoxedIo),
    /// The far end, or the job with another protocol.
    Other(BoxedIo, BoxedIo),
}

/// The record type of a TLS handshake (RFC 8446, section 5.1).
const HANDSHAKE: u8 = 0x16;

/// Wait for the first bytes of either side: a TLS end reads a tunnel whose
/// job starts with a handshake only; a protocol whose server speaks first
/// (SSH, SMTP) passes as it is.
async fn first_speaker(mut client: BoxedIo, mut upstream: BoxedIo) -> Result<First, Failure> {
    let (mut a, mut b) = (vec![0u8; 1], vec![0u8; 4096]);
    tokio::select! {
        n = client.read(&mut a) => {
            let n = n.map_err(|e| Failure::io(Stage::Proxy, "reading the job", e))?;
            a.truncate(n);
            let client: BoxedIo = Box::new(Prefixed::new(a.clone(), client));
            Ok(if a.first() == Some(&HANDSHAKE) { First::Tls(client, upstream) } else { First::Other(client, upstream) })
        }
        n = upstream.read(&mut b) => {
            let n = n.map_err(|e| Failure::io(Stage::Origin, "reading the upstream", e))?;
            b.truncate(n);
            Ok(First::Other(client, Box::new(Prefixed::new(b, upstream))))
        }
    }
}

/// A refusal inside the TLS of the job: the answer, then the close.
pub(crate) async fn answer_refused<W: AsyncWrite + Unpin>(tunnel: &Tunnel<'_>, client: &mut W, refused: &Refused) {
    tunnel.report.note(refused.note);
    let bytes = refusal(refused.status, &refused.words, &[]);
    tunnel.counts.down.fetch_add(bytes.len() as u64, Ordering::Relaxed);
    let _ = client.write_all(&bytes).await;
    let _ = client.shutdown().await;
}

/// The two ends of one tunnel after both handshakes.
pub(crate) struct Ends {
    pub client: Prefixed<BoxedIo>,
    pub upstream: BoxedIo,
    /// The bytes of the far end after the last head that was read.
    pub up_buf: Vec<u8>,
}

impl TlsEnd {
    async fn end(&self, tunnel: &Tunnel<'_>, vault: &Vault, client: BoxedIo, upstream: BoxedIo) -> Result<(), Failure> {
        let host = &tunnel.authority.host;
        let leaf = vault.ca().leaf(host).map_err(|why| Failure::new(Stage::Host, Class::Unavailable, why))?;
        let served = pod_tls::server::Served::new(Some(leaf));
        let config = pod_tls::server::server_config_for(self.provider.clone(), served, &[pod_tls::server::HTTP1])
            .map_err(|e| Failure::new(Stage::Host, Class::Unavailable, e.to_string()))?;
        let accept = pod_tls::server::TlsAcceptor::from(config).accept(client);
        let tls = match tokio::time::timeout(self.bounds.handshake, accept).await {
            Ok(Ok(tls)) => tls,
            Ok(Err(e)) => return Err(Failure::io(Stage::Tls, "the TLS handshake of the job", e)),
            Err(_) => {
                return Err(Failure::new(Stage::Tls, Class::Timeout, "the job did not end its handshake in time"));
            }
        };
        tunnel.report.tls.store(true, Ordering::Relaxed);
        let sni_fits = match (tls.get_ref().1.server_name(), host) {
            (None, _) => true,
            (Some(sni), Host::Name(name)) => sni.trim_end_matches('.').eq_ignore_ascii_case(name),
            (Some(_), Host::Ip(_)) => false,
        };
        let mut client: BoxedIo = Box::new(tls);
        if !sni_fits {
            let words = "the name of the TLS handshake (SNI) is not the target of the CONNECT".into();
            answer_refused(tunnel, &mut client, &Refused { status: 421, note: "sni-mismatch", words }).await;
            return Err(Failure::new(Stage::Tls, Class::Usage, "the SNI is not the CONNECT target"));
        }
        let name = match host {
            Host::Name(name) => name.clone(),
            Host::Ip(ip) => ip.to_string(),
        };
        let words = pod_tls::Words::new("pod-broker");
        let upstream =
            match pod_tls::tls_over(upstream, &name, self.upstream.clone(), self.bounds.handshake, &words).await {
                Ok(tls) => Box::new(tls) as BoxedIo,
                Err(f) => {
                    let refused = Refused { status: 502, note: "upstream-tls", words: f.text().to_string() };
                    answer_refused(tunnel, &mut client, &refused).await;
                    return Err(f);
                }
            };
        let out = Outgoing::of(vault, tunnel.authority);
        let scrub = if out.scrub { vault.scrub() } else { Pairs::new() };
        let mut ends = Ends { client: Prefixed::new(Vec::new(), client), upstream, up_buf: Vec::new() };
        loop {
            let read =
                tokio::time::timeout(self.bounds.idle, pod_proxy::http::read_request(&mut ends.client, Vec::new()));
            let (request, rest) = match read.await {
                Ok(Ok(read)) => read,
                // The job ended its connection, or let it rest past the idle time.
                Ok(Err(pod_proxy::http::RequestError::Gone(_))) | Err(_) => return Ok(()),
                Ok(Err(e)) => {
                    let refused = Refused { status: e.status().unwrap_or(400), note: "head-bad", words: e.to_string() };
                    answer_refused(tunnel, &mut ends.client, &refused).await;
                    return Ok(());
                }
            };
            ends.client.put_back(rest);
            tunnel.report.requests.fetch_add(1, Ordering::Relaxed);
            let inner = match check(&request, tunnel.authority, tunnel.policy) {
                Ok(inner) => inner,
                Err(refused) => {
                    answer_refused(tunnel, &mut ends.client, &refused).await;
                    return Ok(());
                }
            };
            let (head, swaps, basic_back) = request_head(&request, &inner, &out);
            tunnel.report.swaps.fetch_add(swaps, Ordering::Relaxed);
            let head = Zeroizing::new(head);
            let mut scrub = scrub.clone();
            scrub.extend(basic_back);
            let exchange = Exchange { tunnel, bounds: self.bounds, out: &out, scrub: &scrub };
            match exchange.run(&mut ends, &request, inner.framing, &head).await? {
                Next::Keep => continue,
                Next::Close => return Ok(()),
            }
        }
    }
}

/// What follows an exchange.
enum Next {
    Keep,
    Close,
}

/// One request and its answer.
pub(crate) struct Exchange<'a, 'b> {
    pub tunnel: &'a Tunnel<'b>,
    pub bounds: Bounds,
    pub out: &'a Outgoing,
    pub scrub: &'a Pairs,
}

impl Exchange<'_, '_> {
    async fn run(
        &self,
        ends: &mut Ends,
        request: &Request,
        framing: pod_proxy::http::Framing,
        head: &str,
    ) -> Result<Next, Failure> {
        let up = |e: std::io::Error| Failure::io(Stage::Origin, "the exchange with the upstream", e);
        let job = |e: std::io::Error| Failure::io(Stage::Proxy, "the exchange with the job", e);
        if request.minor == 1
            && request.has_token("expect", "100-continue")
            && framing != pod_proxy::http::Framing::Length(0)
        {
            ends.client.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await.map_err(job)?;
        }
        let reusable = crate::tls_body::send_body(self, ends, request, framing, head).await?;
        let (answer, http11) =
            pod_proxy::http::read_head_in(&mut ends.upstream, &mut ends.up_buf, pod_proxy::http::MAX_HEAD)
                .await
                .map_err(up)?;
        let framing = pod_proxy::http::Framing::of(&answer, request.method == "HEAD");
        if self.out.scrub && framing != pod_proxy::http::Framing::Empty && encoded(&answer.headers) {
            let words = "the answer has a Content-Encoding: the broker cannot scrub it, and asked for none";
            answer_refused(
                self.tunnel,
                &mut ends.client,
                &Refused { status: 502, note: "answer-encoded", words: words.into() },
            )
            .await;
            return Ok(Next::Close);
        }
        if answer.status == 101 {
            return crate::tls_body::switch(self, ends, &answer).await.map(|()| Next::Close);
        }
        let out = out_of(framing, self.out.scrub, self.bounds.max_body, request.minor);
        let core = pod_core::http1::Framing::of(answer.status, &answer.headers);
        let keep = reusable
            && request.minor == 1
            && !request.has_token("connection", "close")
            && pod_core::http1::keeps(http11, &answer.headers, core)
            && out != Out::ToClose;
        let kept = crate::tls_body::send_answer(self, ends, &answer, framing, out, keep).await?;
        Ok(if keep && kept { Next::Keep } else { Next::Close })
    }
}

/// Read `n` bytes of a body whole from `body`.
pub(crate) async fn read_whole<R: tokio::io::AsyncRead + Unpin>(body: &mut R, n: u64) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::with_capacity(usize::try_from(n).unwrap_or(0));
    body.take(n).read_to_end(&mut data).await?;
    if (data.len() as u64) < n {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "a body ended before its length"));
    }
    Ok(data)
}

/// The body `Body` of `framing` over `stream`.
pub(crate) fn body_of<S: tokio::io::AsyncRead + Unpin>(
    stream: S,
    rest: Vec<u8>,
    framing: pod_proxy::http::Framing,
) -> std::io::Result<Body<S>> {
    Body::new(stream, rest, framing, None)
}

/// A replace of `pairs`.
pub(crate) fn replace_of(pairs: &Pairs) -> Replace {
    Replace::new(pairs.iter().cloned())
}
