//! The settings of a TURN client: the server and its transport, the
//! credentials, the limits, and the caller's clock, entropy and source of a
//! UDP socket. The consumer fills them once; the crate reads no variable.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use pod_core::entropy::{Entropy, OsEntropy};
use pod_core::{BoxFuture, Class, Datagrams, Dial, Failure, Resolve, Stage, SystemDatagrams, Target};
use tokio::time::Instant;

use super::Error;

/// The user and the password of the long-term credentials (RFC 8489, 9.2).
/// The caller gives each text after its preparation (OpaqueString); the
/// client does none. The password never leaves this value but as a key, and
/// it is wiped when the value is dropped. `Debug` shows the user alone.
pub struct Credentials {
    username: String,
    password: String,
}

impl Credentials {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Credentials {
        Credentials { username: username.into(), password: password.into() }
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub(super) fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials").field("username", &self.username).field("password", &"(hidden)").finish()
    }
}

impl Drop for Credentials {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.password.zeroize();
    }
}

/// How the client reaches the server. Plain TURN over TCP, DTLS and the TCP
/// relay of RFC 6062 are not here (the crate's README, the parity list).
pub enum Transport {
    /// TURN over UDP: the caller's source gives the socket.
    Udp(Arc<dyn UdpSource>),
    /// TURN over TLS on TCP (RFC 8656, 3.1): the caller's dialer gives the
    /// TCP stream, and the caller's TLS context checks the server's
    /// certificate against `server_name` and the roots of `trust`.
    Tls { dialer: Arc<dyn Dial>, context: Arc<pod_tls::Context>, trust: pod_tls::Trust, server_name: String },
}

impl Transport {
    /// `udp` or `tls`, for a line of a log or of an error.
    pub fn name(&self) -> &'static str {
        match self {
            Transport::Udp(_) => "udp",
            Transport::Tls { .. } => "tls",
        }
    }
}

impl fmt::Debug for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Transport::Udp(_) => f.write_str("Udp"),
            Transport::Tls { dialer, trust, server_name, .. } => f
                .debug_struct("Tls")
                .field("dialer", &dialer.label())
                .field("trust", trust)
                .field("server_name", server_name)
                .finish_non_exhaustive(),
        }
    }
}

/// The limits of a client. The defaults are the values of RFC 8489 and
/// RFC 8656.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// The first wait for an answer over UDP; it doubles at each send again
    /// (RFC 8489, 6.2.1: 500 ms).
    pub rto: Duration,
    /// The sends of a request over UDP after the first (RFC 8489: 6, so 7
    /// sends in all).
    pub retransmits: u32,
    /// The longest wait for the answer to one request, over each transport
    /// (RFC 8489, 6.2.2: 39.5 s over TCP).
    pub request_timeout: Duration,
    /// The longest wait for the dial and the TLS handshake.
    pub connect_timeout: Duration,
    /// The lifetime that an Allocate and a Refresh ask for (RFC 8656, 2.2:
    /// 10 minutes by default); the server can give another.
    pub lifetime: Duration,
    /// How long before its end the client refreshes an allocation, a
    /// permission or a channel; at most half of its lifetime.
    pub refresh_before: Duration,
    /// The 438 answers (Stale Nonce) to one request that the client answers
    /// with the fresh nonce; one more fails the request.
    pub stale_nonces: u32,
    /// The events that wait while a request waits for its answer; data past
    /// it is dropped, as a full socket drops a datagram.
    pub queue: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            rto: Duration::from_millis(500),
            retransmits: 6,
            request_timeout: Duration::from_millis(39_500),
            connect_timeout: Duration::from_secs(10),
            lifetime: Duration::from_secs(600),
            refresh_before: Duration::from_secs(60),
            stale_nonces: 2,
            queue: 256,
        }
    }
}

/// The clock of a client: the deadlines of its requests and of its
/// refreshes. [`TokioClock`] is tokio's, which a test can pause.
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    fn sleep_until(&self, deadline: Instant) -> BoxFuture<'_, ()>;
}

/// tokio's clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioClock;

impl Clock for TokioClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep_until(&self, deadline: Instant) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep_until(deadline))
    }
}

/// A socket for TURN over UDP, and the address of the server on it.
pub type Opened = (Arc<dyn Datagrams>, SocketAddr);

/// Where the socket of TURN over UDP comes from: the system, a way of the
/// caller that carries UDP (`Dial::datagrams`), a socket of a test.
pub trait UdpSource: Send + Sync {
    /// A socket for datagrams to `server`, and the address to send them to.
    fn open<'a>(&'a self, server: &'a Target, deadline: Instant) -> BoxFuture<'a, Result<Opened, Failure>>;
}

/// The UDP socket of the system, bound to any port of the family of the
/// server's address. A name goes through the caller's resolver; with none,
/// a name is refused.
#[derive(Default, Clone)]
pub struct SystemUdp {
    resolve: Option<Arc<dyn Resolve>>,
}

impl SystemUdp {
    pub fn new() -> SystemUdp {
        SystemUdp { resolve: None }
    }

    pub fn with_resolver(resolve: Arc<dyn Resolve>) -> SystemUdp {
        SystemUdp { resolve: Some(resolve) }
    }
}

impl fmt::Debug for SystemUdp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemUdp").field("resolve", &self.resolve.as_ref().map(|r| r.label())).finish()
    }
}

impl UdpSource for SystemUdp {
    fn open<'a>(&'a self, server: &'a Target, deadline: Instant) -> BoxFuture<'a, Result<Opened, Failure>> {
        Box::pin(async move {
            let addr = match (server, &self.resolve) {
                (Target::Addr(addr), _) => *addr,
                (Target::Name { host, port }, Some(resolve)) => {
                    let addrs = resolve.resolve(host, *port, deadline).await?;
                    *addrs.first().ok_or_else(|| {
                        Failure::new(Stage::Dns, Class::NotFound, format!("{host} has no address for TURN"))
                    })?
                }
                (Target::Name { host, .. }, None) => {
                    return Err(Failure::new(
                        Stage::Dns,
                        Class::Usage,
                        format!("{host} is a name, and the UDP source of TURN has no resolver"),
                    ));
                }
            };
            let any = match addr.ip() {
                IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            };
            let socket = SystemDatagrams::bind(SocketAddr::new(any, 0)).await.map_err(|e| {
                Failure::caused(Stage::Connect, Class::of_io(e.kind()), "no UDP socket for TURN".to_string(), e)
            })?;
            Ok((Arc::new(socket) as Arc<dyn Datagrams>, addr))
        })
    }
}

/// What a TURN client needs: the caller fills it once.
pub struct Settings {
    /// The TURN server: `turn.example:3478` over UDP, `turn.example:443`
    /// over TLS.
    pub server: Target,
    pub transport: Transport,
    pub credentials: Credentials,
    /// The realm that the caller expects; a challenge of another realm
    /// fails. `None` takes the server's.
    pub realm: Option<String>,
    pub limits: Limits,
    pub clock: Arc<dyn Clock>,
    /// The source of the transaction ids.
    pub entropy: Box<dyn Entropy + Send>,
    /// The SOFTWARE attribute of each request (the consumer's name and
    /// version), or none.
    pub software: Option<String>,
}

impl Settings {
    /// TURN over UDP to `server`, through the socket of `source`.
    pub fn udp(server: Target, credentials: Credentials, source: Arc<dyn UdpSource>) -> Settings {
        Settings::with(server, Transport::Udp(source), credentials)
    }

    /// TURN over TLS on TCP to `server`, through `dialer`, with the default
    /// roots of `context` and the host of `server` as the name to check.
    pub fn tls(
        server: Target,
        credentials: Credentials,
        dialer: Arc<dyn Dial>,
        context: Arc<pod_tls::Context>,
    ) -> Settings {
        let server_name = server.host();
        let transport = Transport::Tls { dialer, context, trust: pod_tls::Trust::Default, server_name };
        Settings::with(server, transport, credentials)
    }

    fn with(server: Target, transport: Transport, credentials: Credentials) -> Settings {
        Settings {
            server,
            transport,
            credentials,
            realm: None,
            limits: Limits::default(),
            clock: Arc::new(TokioClock),
            entropy: Box::new(OsEntropy),
            software: None,
        }
    }

    /// Refuse what cannot work: an empty user, a limit of zero, a refresh
    /// that comes after its end, a lifetime past what the wire holds.
    pub fn check(&self) -> Result<(), Error> {
        let limits = &self.limits;
        let usage = |why: &str| Err(Error::Usage(why.to_string()));
        if self.credentials.username.is_empty() {
            return usage("the TURN user is empty");
        }
        if limits.rto.is_zero() || limits.request_timeout.is_zero() || limits.connect_timeout.is_zero() {
            return usage("a time limit of TURN is zero");
        }
        if limits.lifetime < Duration::from_secs(1) || limits.lifetime.as_secs() > u64::from(u32::MAX) {
            return usage("the lifetime of a TURN allocation is not 1 s to 2^32 - 1 s");
        }
        if limits.queue == 0 {
            return usage("the queue of TURN events holds nothing");
        }
        if let Transport::Tls { server_name, .. } = &self.transport
            && server_name.is_empty()
        {
            return usage("the TLS name of the TURN server is empty");
        }
        Ok(())
    }
}

impl fmt::Debug for Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Settings")
            .field("server", &self.server)
            .field("transport", &self.transport)
            .field("credentials", &self.credentials)
            .field("realm", &self.realm)
            .field("limits", &self.limits)
            .field("software", &self.software)
            .finish_non_exhaustive()
    }
}
