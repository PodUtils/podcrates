//! The TURN client (RFC 8656): one allocation on one server, over UDP or
//! over TLS on TCP. The client refreshes the allocation, each permission and
//! each channel on its timer while [`Client::recv`] waits (PodUtils/podnet#485,
//! TURN-2), and never changes the user or the password of a live
//! allocation: a new credential needs a new client and a new Allocate.

mod error;
mod exchange;
mod settings;
mod wire;

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use pod_core::entropy::Entropy;
use pod_core::{BoxedIo, Target};
use tokio::time::Instant;

pub use error::{Error, Event, Kept};
pub use settings::{Clock, Credentials, Limits, Opened, Settings, SystemUdp, TokioClock, Transport, UdpSource};

use crate::stun::{
    self, ALLOCATE, CHANNEL_BIND, CHANNEL_NUMBER, CREATE_PERMISSION, Class, DATA, LIFETIME, REFRESH,
    REQUESTED_TRANSPORT, SEND, TRANSPORT_UDP, TransactionId, XOR_MAPPED_ADDRESS, XOR_PEER_ADDRESS, XOR_RELAYED_ADDRESS,
};
use exchange::{Auth, Value};
use wire::{Conn, FIRST_CHANNEL, LAST_CHANNEL};

/// The lifetime of a permission and of a channel (RFC 8656, 9 and 12).
const PERMISSION: Duration = Duration::from_secs(300);
const CHANNEL: Duration = Duration::from_secs(600);

/// A live allocation on a TURN server.
pub struct Client {
    conn: Conn,
    server: Target,
    credentials: Credentials,
    /// The realm that the settings expect.
    realm: Option<String>,
    limits: Limits,
    clock: Arc<dyn Clock>,
    entropy: Box<dyn Entropy + Send>,
    software: Option<String>,
    auth: Option<Auth>,
    relayed: SocketAddr,
    mapped: Option<SocketAddr>,
    lifetime: Duration,
    /// When the server last gave the allocation its lifetime.
    granted: Instant,
    /// After a refresh of the allocation failed: the time of the next try.
    retry: Option<Instant>,
    /// Each permission, by the peer's address, with the time it was given.
    permissions: HashMap<IpAddr, Instant>,
    /// Each channel, with its peer and the time it was bound.
    channels: HashMap<u16, (SocketAddr, Instant)>,
    queue: VecDeque<Event>,
    recent: VecDeque<TransactionId>,
    open: bool,
}

impl Client {
    /// Connect to the server of `settings` and allocate a relayed address
    /// for UDP: the first Allocate goes with no credentials, the 401 gives
    /// the realm and the nonce, and the Allocate goes again with them.
    pub async fn allocate(settings: Settings) -> Result<Client, Error> {
        settings.check()?;
        let Settings { server, transport, credentials, realm, limits, clock, entropy, software } = settings;
        let deadline = clock.now() + limits.connect_timeout;
        let conn = connect(&server, &transport, deadline, limits.connect_timeout).await?;
        let now = clock.now();
        let mut client = Client {
            conn,
            server,
            credentials,
            realm,
            limits,
            clock,
            entropy,
            software,
            auth: None,
            relayed: SocketAddr::from(([0, 0, 0, 0], 0)),
            mapped: None,
            lifetime: Duration::ZERO,
            granted: now,
            retry: None,
            permissions: HashMap::new(),
            channels: HashMap::new(),
            queue: VecDeque::new(),
            recent: VecDeque::new(),
            open: false,
        };
        let attributes =
            [(REQUESTED_TRANSPORT, Value::Bytes(vec![TRANSPORT_UDP, 0, 0, 0])), client.lifetime_attribute()];
        let reply = client.transact(ALLOCATE, &attributes).await?;
        let read = stun::decode(&reply).map_err(Error::Decode)?;
        let relayed = read.attribute(XOR_RELAYED_ADDRESS).ok_or_else(|| Error::Protocol {
            kind: read.kind,
            why: "an Allocate success with no XOR-RELAYED-ADDRESS".into(),
        })?;
        client.relayed = stun::read_address(relayed, Some(&read.id))
            .map_err(|why| Error::Protocol { kind: read.kind, why: format!("XOR-RELAYED-ADDRESS: {why}") })?;
        client.mapped = read.attribute(XOR_MAPPED_ADDRESS).and_then(|v| stun::read_address(v, Some(&read.id)).ok());
        client.lifetime = lifetime_of(&read)?;
        client.granted = client.clock.now();
        client.open = true;
        tracing::debug!(server = %client.server, relayed = %client.relayed, "TURN: allocated");
        Ok(client)
    }

    /// The relayed transport address: the address that peers send to.
    pub fn relayed_address(&self) -> SocketAddr {
        self.relayed
    }

    /// The address of this client that the server saw, when it said so.
    pub fn mapped_address(&self) -> Option<SocketAddr> {
        self.mapped
    }

    /// The lifetime that the server gave at the last Allocate or Refresh.
    pub fn lifetime(&self) -> Duration {
        self.lifetime
    }

    /// The user of the allocation: the same for each request.
    pub fn username(&self) -> &str {
        self.credentials.username()
    }

    /// When the next refresh is due, and what it keeps alive.
    pub fn next_refresh(&self) -> (Instant, Kept) {
        let mut next = (self.retry.unwrap_or_else(|| self.refresh_at(self.granted, self.lifetime)), Kept::Allocation);
        for (ip, given) in &self.permissions {
            next = next.min((self.refresh_at(*given, PERMISSION), Kept::Permission(*ip)));
        }
        for (number, (_, bound)) in &self.channels {
            next = next.min((self.refresh_at(*bound, CHANNEL), Kept::Channel(*number)));
        }
        next
    }

    /// Refresh the allocation with the lifetime of the settings.
    pub async fn refresh(&mut self) -> Result<(), Error> {
        self.live()?;
        let reply = self.transact(REFRESH, &[self.lifetime_attribute()]).await?;
        let read = stun::decode(&reply).map_err(Error::Decode)?;
        self.lifetime = lifetime_of(&read)?;
        self.granted = self.clock.now();
        self.retry = None;
        Ok(())
    }

    /// Ask the server to relay the data of `peer`'s address (a permission
    /// is for an address; the port is not part of it). The client refreshes
    /// it before its 300 s end.
    pub async fn create_permission(&mut self, peer: SocketAddr) -> Result<(), Error> {
        self.live()?;
        self.permit(peer.ip()).await
    }

    /// Bind a channel to `peer`, or bind its channel again, and give its
    /// number. The bind gives a permission for the peer's address too.
    pub async fn bind_channel(&mut self, peer: SocketAddr) -> Result<u16, Error> {
        self.live()?;
        let number = match self.channels.iter().find(|(_, (p, _))| *p == peer) {
            Some((number, _)) => *number,
            None => {
                (FIRST_CHANNEL..=LAST_CHANNEL).find(|n| !self.channels.contains_key(n)).ok_or(Error::NoChannelNumber)?
            }
        };
        self.bind(number, peer).await?;
        Ok(number)
    }

    /// Send `data` to the peer of `channel` in ChannelData.
    pub async fn send(&mut self, channel: u16, data: &[u8]) -> Result<(), Error> {
        self.live()?;
        if !self.channels.contains_key(&channel) {
            return Err(Error::NoChannel(channel));
        }
        let bytes = wire::channel_data(channel, data, self.conn.padded())?;
        self.conn.send(&bytes).await
    }

    /// Send `data` to `peer` in a Send indication, with no channel. The
    /// peer's address needs a permission.
    pub async fn send_to(&mut self, peer: SocketAddr, data: &[u8]) -> Result<(), Error> {
        self.live()?;
        if data.len() > usize::from(u16::MAX) {
            return Err(Error::TooLarge(data.len()));
        }
        if !self.permissions.contains_key(&peer.ip()) {
            return Err(Error::NoPermission(peer.ip()));
        }
        let id = self.new_id()?;
        let address = stun::write_address(peer, Some(&id));
        let attributes = [(XOR_PEER_ADDRESS, address.as_slice()), (DATA, data)];
        let bytes = stun::message(stun::message_type(SEND, Class::Indication), &id, &attributes);
        self.conn.send(&bytes).await
    }

    /// The next event: data of a peer, a refresh that failed, or an error
    /// of the server. While it waits, the client refreshes what is due. A
    /// failure of the transport ends the client. Cancel safe for the data:
    /// a refresh that a cancel cut goes again at the next call.
    pub async fn recv(&mut self) -> Result<Event, Error> {
        loop {
            if let Some(event) = self.queue.pop_front() {
                return Ok(event);
            }
            self.live()?;
            let (due, kept) = self.next_refresh();
            if self.clock.now() >= due {
                match self.keep(kept).await {
                    Ok(()) => continue,
                    Err(error @ (Error::Io(_) | Error::Closed | Error::Frame { .. })) => return Err(error),
                    Err(error) => return Ok(self.failed(kept, error)),
                }
            }
            let frame = tokio::select! {
                frame = self.conn.recv() => frame?,
                () = self.clock.sleep_until(due) => continue,
            };
            self.incoming(frame);
        }
    }

    /// End the allocation: a Refresh with a lifetime of 0. An allocation
    /// that the server no longer holds (437) is no failure.
    pub async fn close(mut self) -> Result<(), Error> {
        self.live()?;
        let result = match self.transact(REFRESH, &[(LIFETIME, Value::Bytes(vec![0; 4]))]).await {
            Err(Error::Server { code: 437, .. }) | Ok(_) => Ok(()),
            Err(e) => Err(e),
        };
        self.open = false;
        self.conn.shutdown().await;
        result
    }

    /// Refresh one thing that is due.
    async fn keep(&mut self, kept: Kept) -> Result<(), Error> {
        match kept {
            Kept::Allocation => self.refresh().await,
            Kept::Permission(ip) => self.permit(ip).await,
            Kept::Channel(number) => match self.channels.get(&number) {
                Some((peer, _)) => self.bind(number, *peer).await,
                None => Ok(()),
            },
        }
    }

    /// A refresh failed: a permission or a channel is dropped; the
    /// allocation is tried again at half of its time left, and is lost at
    /// its end or when the server no longer holds it (437).
    fn failed(&mut self, kept: Kept, error: Error) -> Event {
        let now = self.clock.now();
        match kept {
            Kept::Permission(ip) => {
                self.permissions.remove(&ip);
            }
            Kept::Channel(number) => {
                self.channels.remove(&number);
            }
            Kept::Allocation => {
                let end = self.granted + self.lifetime;
                let left = end.saturating_duration_since(now);
                if error.code() == Some(437) || left < Duration::from_secs(1) {
                    self.open = false;
                } else {
                    self.retry = Some(now + (left / 2).max(Duration::from_secs(1)));
                }
            }
        }
        tracing::debug!(?kept, %error, "TURN: a refresh failed");
        Event::RefreshFailed { kept, error }
    }

    async fn permit(&mut self, ip: IpAddr) -> Result<(), Error> {
        let attributes = [(XOR_PEER_ADDRESS, Value::Peer(SocketAddr::new(ip, 0)))];
        self.transact(CREATE_PERMISSION, &attributes).await?;
        self.permissions.insert(ip, self.clock.now());
        Ok(())
    }

    async fn bind(&mut self, number: u16, peer: SocketAddr) -> Result<(), Error> {
        let channel = [number.to_be_bytes(), [0, 0]].concat();
        let attributes = [(CHANNEL_NUMBER, Value::Bytes(channel)), (XOR_PEER_ADDRESS, Value::Peer(peer))];
        self.transact(CHANNEL_BIND, &attributes).await?;
        let now = self.clock.now();
        self.channels.insert(number, (peer, now));
        self.permissions.insert(peer.ip(), now);
        Ok(())
    }

    /// The allocation is open, and its lifetime has not ended.
    fn live(&self) -> Result<(), Error> {
        let expired = self.clock.now() >= self.granted + self.lifetime;
        if self.open && !expired { Ok(()) } else { Err(Error::NoAllocation) }
    }

    /// The time to refresh a thing given at `given` for `lifetime`:
    /// `refresh_before` its end, and not before half of its lifetime.
    fn refresh_at(&self, given: Instant, lifetime: Duration) -> Instant {
        given + lifetime.saturating_sub(self.limits.refresh_before).max(lifetime / 2)
    }

    fn lifetime_attribute(&self) -> (u16, Value) {
        let secs = u32::try_from(self.limits.lifetime.as_secs()).unwrap_or(u32::MAX);
        (LIFETIME, Value::Bytes(secs.to_be_bytes().to_vec()))
    }
}

/// The server, the user, the addresses and what the client keeps; never the
/// password, the key or the nonce.
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("server", &self.server)
            .field("username", &self.credentials.username())
            .field("relayed", &self.relayed)
            .field("mapped", &self.mapped)
            .field("lifetime", &self.lifetime)
            .field("permissions", &self.permissions.len())
            .field("channels", &self.channels.len())
            .field("open", &self.open)
            .finish_non_exhaustive()
    }
}

/// The LIFETIME of a success answer.
fn lifetime_of(read: &stun::Message<'_>) -> Result<Duration, Error> {
    let value =
        read.attribute(LIFETIME).ok_or_else(|| Error::Protocol { kind: read.kind, why: "no LIFETIME".into() })?;
    let secs = <[u8; 4]>::try_from(value)
        .map_err(|_| Error::Protocol { kind: read.kind, why: format!("a LIFETIME of {} bytes", value.len()) })?;
    Ok(Duration::from_secs(u64::from(u32::from_be_bytes(secs))))
}

/// Open the transport: the caller's UDP socket, or the caller's dial and
/// the TLS handshake of the caller's context.
async fn connect(server: &Target, transport: &Transport, deadline: Instant, limit: Duration) -> Result<Conn, Error> {
    match transport {
        Transport::Udp(source) => {
            let (socket, addr) = source.open(server, deadline).await.map_err(Error::Connect)?;
            Ok(Conn::udp(socket, addr))
        }
        Transport::Tls { dialer, context, trust, server_name } => {
            let connected = dialer.dial(server, deadline).await.map_err(Error::Connect)?;
            let tls = context.connect_over(connected.io, server_name, trust, limit).await.map_err(Error::Connect)?;
            Ok(Conn::stream(Box::new(tls) as BoxedIo))
        }
    }
}
