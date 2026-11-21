# pod-link

A link between two peers: many streams over one carrier byte stream, with
a credit window for each stream and for the link, a keepalive, and the
carriers behind one trait. podnet asked for it for its link workstream
(PodUtils/podnet#466, row E0-2; PodUtils/podnet#467, rows E1-7 and E1-8;
the start signal PodUtils/podnet#480). T-300 made it, as new code, by
calls C116, C117 and C120 to C124. T-396 added the tcp carrier (feature
`tcp`; PodUtils/podnet#513; calls C177 to C179), T-420 the ws carrier
(feature `ws`; PodUtils/podnet#515; call C253), T-421 the h2 carrier
(feature `h2`; PodUtils/podnet#515; call C254), T-310 the selective-repeat
ARQ for a carrier that loses datagrams (PodUtils/podnet#468, row E2-2;
calls C262 to C265), T-423 the quic carrier (feature `quic`;
PodUtils/podnet#515; call C270), T-424 the udp carrier with KCP (feature
`udp`; PodUtils/podnet#515; calls C271 and C272), T-422 and T-495 the poll carriers over requests of
HTTP/1.1: the long poll, SSE, a chunked stream and the simplex poll
(feature `poll`; PodUtils/podnet#515 and #521; calls C266 to C269),
T-493 the DNS simplex carrier over the ARQ (feature `dns`;
PodUtils/podnet#468 row E2-3; calls C274 and C275), T-497 the ICMP echo
carrier over the ARQ (feature `icmp`; PodUtils/podnet#469 row E3-2; calls
C276 and C277) and T-494 the object-storage carrier over a bucket of the S3
API (feature `s3`; PodUtils/podnet#468 row E2-4; calls C489 to C491) and
T-627 its stand-in for the tests of a consumer (feature `s3-standin`;
PodUtils/podnet#564; call C552), and T-500 the split of one session over an up carrier and a down carrier
(`split`; PodUtils/podnet#473 row E7-1; calls C492 and C493). The crate
reads no environment, keeps no global state and prints nothing.

## Use

```rust
use pod_link::{Carrier, Link, Role, Settings, unix::Unix};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// The listener.
let carrier = Unix::default();
carrier.available()?;                         // a reason when the host refuses AF_UNIX
let mut door = carrier.listen("/run/user/1000/podnet/link.sock").await?; // mode 0600
let listener = Link::new(door.accept().await?, Role::Listener, Settings::default())?;
let mut stream = listener.accept().await?;    // even ids are the listener's

// The dialer.
let io = Unix::default().dial("/run/user/1000/podnet/link.sock").await?;
let dialer = Link::new(io, Role::Dialer, Settings::default())?;
let mut s = dialer.open()?;                   // odd ids; no round trip
s.write_all(b"hello").await?;
s.shutdown().await?;                          // CLOSE: a half close
dialer.close().await;                         // GOAWAY after the queued frames
```

A link that crosses a network runs over the tcp carrier: TLS 1.3, each
end pinning the Ed25519 key of the other (`pod_tls::peer`, T-304):

```rust
use pod_identity::KeyName;
use pod_link::tcp::{Keys, Tcp};
use pod_link::{Carrier, Link, Listener, Role, Settings};

// One carrier for each end: the provider, the identity, the way out.
let tcp = Tcp::new(provider, identity, way)            // way: Arc<dyn pod_core::Dial>
    .with_clients(Keys::new([KeyName::parse("SHA256:...").unwrap()]));
tcp.available()?;                                      // a reason when it cannot be used

// The listener: the accept gives the stream and the key that the client proved.
let mut door = tcp.listen("0.0.0.0:7000").await?;
let accepted = door.accept_peer().await?;              // accepted.peer: Some(PublicKey)
let listener = Link::new(accepted.io, Role::Listener, Settings::default())?;

// The dialer: the key of the listener, then its address.
let io = tcp.dial("SHA256:...@relay.example:7000").await?;
let dialer = Link::new(io, Role::Dialer, Settings::default())?;
```

The ws carrier puts the same TLS under a WebSocket, for a network that
lets only HTTPS out; its listener runs the accept loop of `pod-ws`
(`pod_ws::serve`):

```rust
use pod_link::ws::Ws;

let ws = Ws::new(provider, identity, way).with_clients(keys);
let mut door = ws.listen("0.0.0.0:443").await?;       // any path
let accepted = door.accept_peer().await?;
let io = ws.dial("SHA256:...@wss://relay.example/link").await?;
```

The h2 carrier puts the same TLS under one POST of HTTP/2: the request
body goes one way, the response body the other:

```rust
use pod_link::h2::H2;

let h2 = H2::new(provider, identity, way).with_clients(keys);
let mut door = h2.listen("0.0.0.0:443").await?;
let io = h2.dial("SHA256:...@https://relay.example/link").await?;
```

The quic carrier puts the same TLS under QUIC (`pod-quic`): one
bidirectional stream of a connection, from a UDP socket of the system or
over the datagrams of the caller's way out:

```rust
use pod_link::quic::Quic;

let quic = Quic::new(provider, identity).with_clients(keys);  // a socket of the system
let quic = quic.with_way(way);                                  // or the way's `Dial::datagrams`
let mut door = quic.listen("0.0.0.0:7000").await?;              // UDP
let io = quic.dial("SHA256:...@relay.example:7000").await?;
```

The udp carrier runs KCP over UDP (the wire of KCP, PodUtils/podnet#468
row E2-1) and the same TLS above KCP:

```rust
use pod_link::udp::{KcpSettings, Udp};

let mut udp = Udp::new(provider, identity).with_clients(keys);
udp.kcp = KcpSettings { nodelay: false, ..KcpSettings::default() }; // KCP's congestion window
let mut door = udp.listen("0.0.0.0:7000").await?;
let io = udp.dial("SHA256:...@relay.example:7000").await?;
// The same handshake over any datagrams of one peer (a shim of loss):
let (io, key) = udp.connect_over(datagrams, &target, &key_name).await?;
```

The poll carriers put the same TLS under requests of HTTP/1.1, for a
proxy or a front that closes long connections. One form of the bytes down
is the dialer's; a listener serves each form:

```rust
use pod_link::poll::{Form, Poll};

let poll = Poll::new(provider, identity, way).with_clients(keys);   // the long poll
let mut door = poll.listen("0.0.0.0:443").await?;
let sse = poll.clone().with_form(Form::Sse);                         // or Chunked, Simplex
let io = sse.dial("SHA256:...@https://relay.example/link").await?;
```

The dns carrier takes the ARQ over DNS queries and TXT answers, for a host
where the only egress is a resolver: the frames go up in base32 labels of a
name under the zone and come down in TXT records:

```rust
use pod_link::dns::Dns;

let dns = Dns::new(provider, identity).with_clients(keys).zone("link.example");
let mut door = dns.bind("0.0.0.0:53".parse()?).await?;              // an authoritative server
let dialing = Dns::new(provider, identity).to(resolver_addr);       // KEY@ZONE, queries to the resolver
let io = dialing.dial("SHA256:...@link.example").await?;
```

The icmp carrier takes the ARQ over ICMP echo messages; the real path needs
a raw socket (`CAP_NET_RAW` or uid 0), and the same framing runs over any
datagrams of a test:

```rust
use pod_link::icmp::Icmp;

let icmp = Icmp::new(provider, identity).with_clients(keys);
icmp.available()?;                                                   // a reason when the host has no raw socket
let mut door = icmp.bind("0.0.0.0".parse()?).await?;
let io = Icmp::new(provider, identity).dial("SHA256:...@198.51.100.7").await?;
```

The s3 carrier takes the same TLS over objects of a bucket of the S3 API
that both ends reach; the endpoint, the key of the bucket and the TLS of
an endpoint of `https` are the caller's:

```rust
use pod_link::s3::{Credentials, Endpoint, S3};

let endpoint = Endpoint::parse("https://s3.example.com")?;
let s3 = S3::new(provider, identity, way, endpoint, Credentials::new(id, secret))
    .with_region("eu-west-1")
    .with_tls(tls)                                                    // the roots of the caller
    .with_clients(keys);
let mut door = s3.listen("bucket/links/a").await?;                 // BUCKET[/PREFIX]
let io = s3.dial("SHA256:...@bucket/links/a").await?;
```

The split runs one session over two carriers: the bytes of the dialer go
up on one, the bytes of the listener come down on the other:

```rust
use pod_link::split::{self, Split};

let split = Split::new(Arc::new(dns), Arc::new(poll.with_form(Form::Simplex)));
let mut door = split.bind("0.0.0.0:53", "0.0.0.0:443").await?;       // or listen("UP|DOWN")
let io = split.connect("SHA256:...@link.example", "SHA256:...@https://relay.example/link").await?;
// A read or a write after the loss of a half:
// split::lost(&e).map(|l| l.half) == Some(split::Half::Up), and the text says "up-carrier lost".
```

## API

| Item | What |
| --- | --- |
| `Link` | `new(carrier, role, settings)` starts the task of the link on the current tokio runtime; `open`, `accept`, `close` (a `GOAWAY` after the queued frames), `closed` (wait for the end and its reason), `ended`, `streams`, `rtt` (of the last `PING`), `role`. A drop ends the link at once |
| `Stream` | `AsyncRead` and `AsyncWrite`; `id`, `reset(code)`, `buffered`. A shutdown sends `CLOSE`; a drop sends `CLOSE` and, when the peer did not close its side, `RESET` with `CANCEL` |
| `Role` | `Dialer` (odd ids) or `Listener` (even ids) |
| `Settings` | the windows, the stream limit, the frame size, the keepalive (`WIRE.md`, "Settings"); `check`, `queue_limit` |
| `Error` | each failure by name; `class` (for the exit code), `io_kind`; into `std::io::Error` (a stream's error) and into `pod_core::Failure` (the stage `Session`) |
| `ResetCode`, `GoAwayCode` | `CANCEL`, `REFUSED`, `PROTOCOL`; `NORMAL`, `PROTOCOL`. A caller's own reset codes start at 16 |
| `Carrier`, `Listener`, `Facts`, `Accepted` | the trait of a carrier: `name`, `facts` (ordered, reliable, duplex, unit, cost), `available`, `dial`, `listen`; the accept side: `accept`, and `accept_peer` (the stream and the Ed25519 key that the peer proved, `None` for a carrier that proves no key); `BoxIo` is the byte stream that they give |
| `CarrierError`, `CarrierErrorKind` | `Usage` (64), `InUse` and `Unavailable` (69), `NotFound` (66), `Refused` (77: this end refused the peer, a key that its pin does not name), `KeyRefused` (77: the peer refused the key of this end; the same on each carrier that pins keys, T-542, call C293), `Io`; into `pod_core::Failure` (the stage `Connect`) |
| `memory::Memory`, `memory::pair` | the memory carrier: listeners by name in the value (a clone shares them), a ring of 64 KiB to 64 MiB in each direction (1 MiB by default); a full ring makes the writer wait |
| `unix::Unix`, `unix::pair` | the unix socket carrier (`cfg(unix)`): a path of 1 to 107 bytes, a socket of mode 0600, the claim of the path, the peer's uid; a socket pair for one process |
| `tcp::Tcp` (feature `tcp`) | the tcp carrier: `new(provider, identity, way)`, `with_clients(keys)`, `clients`; the fields `connect_timeout` (20 s), `handshake_timeout` (10 s), `max_handshakes` (64), `alpn` (`podlink/1`; podnet gives `podnet/1`, call C273); `address(text)` (the key and the target of `KEY@HOST:PORT`); `connect(&target, &key)` (the stream and the key of the listener); `bind(addr)`; as a `Carrier`: `dial("KEY@HOST:PORT")`, `listen("IP:PORT")` |
| `tcp::TlsListener` | `local_addr`, `accept_key` (the stream and the key of the client); a `Listener` (`accept_peer` gives the key); a drop ends its handshakes under way |
| `ws::Ws` (feature `ws`) | the ws carrier: `new(provider, identity, way)`, `with_clients(keys)`, `clients`; the fields `connect_timeout` (20 s), `handshake_timeout` (10 s, the TLS and the upgrade of a dial), `serve` (the deadlines and the caps of the accept loop of a listener, `pod_ws::serve::Serve`), `protocol` (the subprotocol of both ends; none by default), `beat` (`Beat`); `with_protocol(name)`; `check`; `address(text)` (the key and the URL of `KEY@wss://HOST[:PORT]/PATH`); `connect(&url, &key)`; `bind(addr)`; as a `Carrier`: `dial("KEY@wss://HOST[:PORT]/PATH")`, `listen("IP:PORT")` |
| `ws::WsListener` | `local_addr`, `accept_key`; a `Listener`; a drop ends its accept loop |
| `ws::Beat` | `ping_every` (20 s; none sends no Ping), `pong_within` (10 s); `check` |
| `ws::carry(session, beat)` | the byte stream of a link over a `pod_ws::RelaySession` that the caller opened, under the rules of the carrier |
| `ws::{NAME, ALPN, USER_AGENT, CONNECT_TIMEOUT, HANDSHAKE_TIMEOUT, PING_EVERY, PONG_WITHIN}` | the constants of the ws carrier |
| `h2::H2` (feature `h2`) | the h2 carrier: `new(provider, identity, way)`, `with_clients(keys)`, `clients`; the fields `connect_timeout` (20 s), `handshake_timeout` (10 s: TLS, the prefaces, the POST and its 200), `max_handshakes` (64); `address(text)` (the key and the `PostUrl` of `KEY@https://HOST[:PORT]/PATH`); `connect(&url, &key)`; `bind(addr)`; as a `Carrier`: `dial`, `listen("IP:PORT")` |
| `h2::H2Listener`, `h2::PostUrl` | `local_addr`, `accept_key`; a `Listener`; a drop ends its handshakes under way. `PostUrl { host, port, path }`: `parse`, `authority`, `target` |
| `h2::{NAME, ALPN, CONNECT_TIMEOUT, HANDSHAKE_TIMEOUT, MAX_HANDSHAKES, WINDOW}` | the constants of the h2 carrier |
| `quic::Quic` (feature `quic`) | the quic carrier: `new(provider, identity)`, `with_clients(keys)`, `with_way(way)` (the dials send over `Dial::datagrams`), `with_resolver(resolver)`, `clients`; the fields `alpn` (`podlink/1`), `transport` (`pod_quic::Settings`: a silence of 30 s, a keepalive each 10 s), `connect_timeout` (20 s: the resolution and the datagrams of the way), `handshake_timeout` (10 s), `max_handshakes` (64), `linger` (10 s); `address(text)`; `connect(&target, &key)`; `bind(addr)`, `bind_over(datagrams)`; as a `Carrier`: `dial("KEY@HOST:PORT")`, `listen("IP:PORT")` |
| `quic::QuicListener` | `local_addr`, `accept_key`; a `Listener`; a drop ends its handshakes under way and takes no new connection |
| `quic::{NAME, ALPN, CONNECT_TIMEOUT, HANDSHAKE_TIMEOUT, MAX_HANDSHAKES, SILENCE, KEEPALIVE, LINGER, transport}` | the constants of the quic carrier, and its transport by default |
| `udp::Udp` (feature `udp`) | the udp carrier: `new(provider, identity)`, `with_clients(keys)`, `with_way(way)`, `with_resolver(resolver)`, `clients`; the fields `alpn` (`podlink/1`), `kcp` (`KcpSettings`), `connect_timeout` (20 s), `handshake_timeout` (10 s), `max_handshakes` (64); `address(text)`; `connect(&target, &key)`; `connect_over(datagrams, &target, &key)` and `accept_over(datagrams, &from)` (the handshake over any `datagram::Datagrams` of one peer); `bind(addr)`, `bind_over(datagrams)`; as a `Carrier`: `dial("KEY@HOST:PORT")`, `listen("IP:PORT")` |
| `udp::UdpListener` | `local_addr`, `accept_key`; a `Listener`; a drop ends its handshakes under way and starts no session; the sessions that it gave live on |
| `udp::KcpSettings`, `udp::KcpSettingsError` | `mtu` (1200), `segment` (1 KiB), `window` (256; 32 to 1024), `nodelay` (off: KCP's congestion window), `interval` (10 ms), `resend` (2), `idle` (20 s); `check` |
| `udp::KcpStream` | KCP in stream mode over a `datagram::Datagrams` as one byte stream: `new(datagrams, conv, settings)` (`None` takes the conversation of the first datagram), `conv`. No end of its own; a shutdown returns when the peer acknowledged each byte; after a drop the task sends the bytes in flight |
| `udp::{NAME, ALPN, CONNECT_TIMEOUT, HANDSHAKE_TIMEOUT, MAX_HANDSHAKES, MTU, SEGMENT, MIN_WINDOW, MAX_WINDOW, IDLE, KCP_HEADER}` | the constants of the udp carrier |
| `dns::Dns` (feature `dns`) | the DNS simplex carrier: `new(provider, identity)`, `with_clients(keys)`, `to(server)` (the UDP address a dial sends its queries to), `zone(name)` (a listener's zone), `with_way(way)`, `clients`; the fields `server`, `zone`, `alpn` (`podlink/1`), `arq` (`arq::ArqSettings`), `dns` (`DnsSettings`), `handshake_timeout` (20 s), `max_handshakes` (64); `address(text)` (the key and the zone of `KEY@ZONE`); `connect(&zone, &key)`; `connect_over(datagrams, &target, &key)` and `accept_over(datagrams, &from)`; `bind(addr)`, `bind_over(socket)`; as a `Carrier`: `dial("KEY@ZONE")`, `listen("IP:PORT")`, the cost `Highest` |
| `dns::DnsListener` | `local_addr`, `accept_key`; a `Listener`; a drop ends its handshakes under way and starts no session; the sessions that it gave live on |
| `dns::DnsSettings`, `dns::DnsSettingsError` | `queries_per_second` (20), `in_flight` (8), `query_timeout` (2 s), `poll_min` (5 ms), `poll_max` (2 s), `answer` (1232: the EDNS0 size of each query of a dialer, and the largest answer of a listener; an answer also keeps to the size of its query, 512 bytes with no EDNS0), `depth` (256); `check` |
| `dns::{NAME, ALPN, HANDSHAKE_TIMEOUT, MAX_HANDSHAKES, settings::EDNS_SIZE}` | the constants of the dns carrier |
| `icmp::Icmp` (feature `icmp`) | the ICMP echo carrier: `new(provider, identity)`, `with_clients(keys)`, `with_resolver(resolver)`, `clients`; the fields `alpn` (`podlink/1`), `arq`, `icmp` (`IcmpSettings`), `connect_timeout` (20 s), `handshake_timeout` (20 s), `max_handshakes` (64); `address(text)` (the key and the target of `KEY@HOST`); `connect(&target, &key)` (unix); `connect_over(datagrams, &target, &key)` and `accept_over(datagrams, &from)`; `bind(ip)` (unix); as a `Carrier`: `dial("KEY@HOST")`, `listen("IPv4")`, the cost `High` |
| `icmp::IcmpListener` (unix) | `local_ip`, `accept_key`; a `Listener`; a drop ends its handshakes under way and starts no session |
| `icmp::EchoFrames` | frames each datagram of the ARQ as an ICMP echo message over an inner carrier of whole ICMP messages (the raw socket, or a memory pair in a test): `new(inner, kind, session, id)`; a `datagram::Datagrams` |
| `icmp::IcmpSettings`, `icmp::wire` | `payload` (1400); the echo codec (`REQUEST`, `REPLY`, `HEADER`, `SESSION`, `Echo`, `checksum`, `encode`, `decode`) |
| `icmp::{NAME, ALPN, PAYLOAD, CONNECT_TIMEOUT, HANDSHAKE_TIMEOUT, MAX_HANDSHAKES}` | the constants of the icmp carrier |
| `poll::Poll` (feature `poll`) | the poll carriers: `new(provider, identity, way)` (the long poll), `with_form(form)`, `with_clients(keys)`, `clients`; the fields `form`, `connect_timeout` (20 s), `handshake_timeout` (10 s: the TLS of each connection, and the open), `answer_timeout` (30 s), `hold` (25 s, 1 s to 60 s), `idle` (60 s: a session with no request ends on the listener, a lane that fails ends on the dialer), `max_handshakes` (64), `max_sessions` (256), `max_connections` (1024: the connections at once; one past them closes at its accept), `max_bytes` (64 MiB: the bytes down that the sessions keep for their dialers; a session waits for room, an open with no room for one batch gets 429; each session holds two pipes of 64 KiB and a buffer of 16 KiB besides, and a connection reads a body up by parts of at most 64 KiB into the link), `push` (a listener answers a push; else 406 and the dialer takes the long poll); `check`; `address(text)` (the key and the `PostUrl` of `KEY@https://HOST[:PORT]/PATH`, no query); `connect(&url, &key)`; `bind(addr)`; as a `Carrier`: `dial`, `listen("IP:PORT")`; the name and the cost of its form |
| `s3::S3` (feature `s3`) | the object-storage carrier: `new(provider, identity, way, endpoint, credentials)`, `with_clients(keys)`, `with_region(region)` (`us-east-1` by default), `with_tls(config)` (needed for an endpoint of `https`), `clients`; the fields `endpoint`, `region`, `credentials`, `style`, `tls`, `alpn` (`podlink/1`), `object` (256 KiB, 1 KiB to 4 MiB), `in_flight` (8), `poll_min` (100 ms), `poll_max` (5 s), `budget` (1 GiB: the bytes of the objects of one end of a session; a warning of `tracing` at 80 %, the end of the session with `StorageFull` when the peer removes nothing for `idle`), `page` (1000), `prune` (off: the reader removes each object it read, a listener each open object it took), `connect_timeout` (20 s), `answer_timeout` (30 s), `handshake_timeout` (30 s), `idle` (60 s: a call that fails for it ends the session), `max_handshakes` (64); `check`; `place(text)` (the bucket and the prefix of `BUCKET[/PREFIX]`), `address(text)` (the key, the bucket and the prefix of `KEY@BUCKET[/PREFIX]`); `connect(bucket, prefix, &key)`; `bind(bucket, prefix)`; as a `Carrier`: `dial`, `listen`, the cost `Highest`. A refused key of the bucket (403) is `Refused`, a missing bucket (404) `NotFound` |
| `s3::S3Listener` | `accept_key`; a `Listener` (`address` is `BUCKET[/PREFIX]`); it takes the sessions that open after its bind; a drop ends its task and its handshakes under way, and the sessions that it gave live on |
| `s3::Endpoint`, `s3::Style`, `s3::Credentials` | `Endpoint::parse("http[s]://HOST[:PORT][/PATH]")` (`secure`, `host`, `port`, `base`); `Style::Path` (`HOST/BUCKET/KEY`, the default) or `VirtualHost` (`BUCKET.HOST/KEY`); `Credentials::new(access_key, secret_key)` and `session_token`, whose Debug hides the secret and the token |
| `s3::sign` | SigV4 of the S3 API: `authorization(&credentials, &Signing)`, `canonical_query`, `uri_encode`, `sha256_hex`, `hex`, `amz_date`, `days_from_civil`, `EMPTY_HASH` (a stand-in of a test checks a request with it) |
| `s3::standin` (feature `s3-standin`) | the stand-in of the S3 API on the loopback, for the tests of this crate and of a consumer: `key()` (the made-up key that it takes); `StandIn::start(&buckets)` (on the runtime of the caller), `StandIn::start_thread(&buckets)` (on a thread with its own runtime, for a test with no runtime or one that blocks it); `addr`, `url` (`http://127.0.0.1:PORT`), `endpoint` (an `Endpoint`), `buckets` (the `Bucket`s of `Object`s, locked), `keys(bucket, prefix)`, `counts` (`Counts`: `puts`, `gets`, `lists`, `deletes`, `exists` (412), `refused` (403), `lost`), `lose_puts(n)` (the fault: drop the answers of the next `n` PUTs that took their object). It serves HTTP/1.1 with keep-alive in path style: the PUT with `If-None-Match: *`, the GET with `If-Match`, the DELETE, `ListObjectsV2` with `prefix`, `max-keys`, `start-after` and the continuation token. It checks the SigV4 of each request with the signer of the crate and the payload hash: no signature or a wrong one is 403. A drop closes its port and each connection |
| `s3::{NAME, ALPN, OBJECT, MIN_OBJECT, MAX_OBJECT, IN_FLIGHT, POLL_MIN, POLL_MAX, BUDGET, PAGE, CONNECT_TIMEOUT, ANSWER_TIMEOUT, HANDSHAKE_TIMEOUT, IDLE, MAX_HANDSHAKES}` | the constants of the s3 carrier |
| `split::Split` | one session over two carriers: `new(up, down)` (each an `Arc<dyn Carrier>`); the fields `beat` (10 s), `lost_after` (30 s, more than `beat`), `join_timeout` (30 s); `check`; `addresses(text)` (the two addresses of `UP|DOWN`); `connect(up, down)` (the dial of both halves and their join); `bind(up, down)`; as a `Carrier` (`split`): `dial("UP\|DOWN")`, `listen("UP\|DOWN")`, the facts of both halves (the cost of the dearer). An error of a carrier names its half |
| `split::SplitListener` | `accept_peer` (the joined session and the key that both halves proved); a `Listener` (`address` is `UP\|DOWN`); each half that waits more than `join_timeout` for the other goes; a drop ends its tasks |
| `split::Half`, `split::Lost`, `split::lost`, `split::lost_in` | `Up`, `Down` (`name`); the error inside the `io::Error` (kind `ConnectionAborted`) of a read or a write after the loss of a half, whose text starts with `up-carrier lost` or `down-carrier lost`; `lost(&io_error)` finds it; `lost_in(text)` reads the half from the text of an error of a link |
| `split::wire` | the hello (`MAGIC`, `VERSION`, `HELLO`, `hello`, `read_hello`, `half_byte`), the frames (`kind::{DATA, BEAT, END, JOINED, LOST}`, `HEAD`, `FRAME`, `write`, `read`) |
| `split::{NAME, BEAT, LOST_AFTER, JOIN_TIMEOUT}` | the constants of the split |
| `poll::Form` | `LongPoll` (`poll`), `Sse` (`sse`), `Chunked` (`chunked`), `Simplex` (`httppoll`); `name`, `batch` (256 KiB; 64 KiB for `Simplex`) |
| `poll::PollListener`, `poll::Stats` | `local_addr`, `accept_key`, `stats` (sessions, 429s, requests, bytes up that came twice, bytes down sent again, long polls, simplex polls, pushes, pushes refused, the bytes down kept now, the connections closed past `max_connections`); a `Listener`; a drop ends its connections and each session that it holds |
| `poll::{ALPN, CONNECT_TIMEOUT, HANDSHAKE_TIMEOUT, ANSWER_TIMEOUT, HOLD, MIN_HOLD, MAX_HOLD, IDLE, MAX_HANDSHAKES, MAX_SESSIONS, MAX_CONNECTIONS, MAX_BYTES, MIN_BYTES, BATCH, SIMPLEX_BATCH, WINDOW, ACK_EVERY, PING_EVERY, BACKOFF_FIRST, BACKOFF_MAX, BUSY_LIMIT}` | the constants of the poll carriers |
| `Cost` | the overhead class of a carrier in `Facts::cost`: `Low` (unix, memory, tcp, ws, h2), `Medium` (the long poll, a chunked stream), `High` (SSE, icmp), `Highest` (the simplex poll, dns, s3) |
| `tcp::{Keys, Refusal, refusal}`, `tcp::{NAME, ALPN, CONNECT_TIMEOUT, HANDSHAKE_TIMEOUT, MAX_HANDSHAKES}` | the shared set of client keys, the refusal of a key with the key seen, its finder in an error (from `pod_tls::peer`); the constants |
| `frame` | `Frame`, `Decoder` (`new`, `with_preface`, `push`, `next`, `next_borrowed`, `buffered`), `Borrowed` (a `DATA` body as a slice of the buffer of the decoder), `FrameError`, `PREFACE`, `HEADER`, `MAX_RECORD`, `kind` |
| `arq::ArqStream` | the ARQ over a datagram carrier as one ordered byte stream (`AsyncRead` and `AsyncWrite`): `new(datagrams, settings)` starts its task on the current tokio runtime; `handle`. A write is one record; a shutdown sends the `FIN` and returns at its acknowledgement; a drop sends the `FIN`; when its frames are acknowledged, the task answers the peer until the peer is silent for two RTO ceilings (at most 64 s), then ends; an error also ends it |
| `arq::ArqHandle` | stays when the stream moves into a `Link`: `error`, `stats`, `srtt`, `in_flight`, `rekey_due`, `rekey_wanted` (waits for the ask), `rekeyed` |
| `arq::Arq` | the ARQ as a state machine with no I/O: `new(settings, mtu)`, `on_datagram`, `poll_transmit`, `poll_timeout`, `on_timeout`, `write`, `read`, `send_record`, `recv_record`, `close`, `closed`, `read_finished`, `idle`, `done`, `rekey_due`, `rekeyed`, `in_flight`, `unacknowledged`, `rto`, `srtt`, `fragment`, `stats`, `error` |
| `arq::ArqSettings`, `arq::ArqSettingsError` | the settings of the ARQ (below); `check` |
| `arq::ArqError` | `Timeout` (`arq-timeout`, 75), `Protocol` (76), `Carrier`, `WriteClosed`, `Settings`; `class`, `io_kind`; into `std::io::Error` (with the typed error inside) and into `pod_core::Failure` (the stage `Session`) |
| `arq::Stats` | the counters: sent, retransmitted, timeouts, `ACK`s, probes, received, duplicates, dropped, records, rekeys |
| `arq::wire` | `Datagram` (`encode`, `decode`, `to_vec`, `wire_len`), `Bitmap`, `WireError`, `kind`, `DATA_HEADER`, `ACK_LEN`, `SACK_BITS`, `MAX_FRAGMENT`, `MIN_FRAGMENT` |
| `datagram::Datagrams` | the trait of a datagram carrier: `mtu`, `send`, `recv`. A datagram that the carrier cannot take is a loss, not an error |
| `datagram::memory::pair` | two ends of a datagram carrier in one process (`MemoryDatagrams`): a unit of 1200 bytes and 1024 datagrams waiting by default; a full queue or a gone peer drops the datagram, as UDP does |
| `datagram::lossy::{Lossy, Faults, LossyStats, SplitMix64}` | a shim that loses, reorders and duplicates the datagrams of a carrier from a seed (`new(inner, faults, seed)`, `set_faults`, `stats`); `Faults::NONE`, `Faults::SILENT` |

## Features

| Feature | What |
| --- | --- |
| none (the default) | the link, the frames, the unix and the memory carriers; no TLS crate in the build |
| `tcp` | the tcp carrier (`tcp`): `pod-tls`, rustls and tokio-rustls, with the provider of the caller, and the `async` feature of `pod-core` for the way out |
| `ws` | the ws carrier (`ws`): `tcp`, and `pod-ws` with its feature `serve` |
| `h2` | the h2 carrier (`h2`): `tcp`, h2, http and bytes, and `pod-ws` for the byte stream of an HTTP/2 stream |
| `udp` | the udp carrier (`udp`): `tcp`, and `kcp` (KCP in pure Rust, with no feature) |
| `quic` | the quic carrier (`quic`): `tcp`, and `pod-quic` (noq with no `ring` and no `aws-lc`; the TLS provider is the caller's) |
| `poll` | the poll carriers (`poll`): `tcp`, `pod-ws` for the URL, and base64 for the events of SSE |
| `dns` | the dns carrier (`dns`): `tcp`, and `pod-dns` for the DNS query and the TXT answer |
| `icmp` | the icmp carrier (`icmp`): `tcp`, and libc on unix for the raw socket |
| `s3` | the s3 carrier (`s3`): `poll` (its HTTP/1.1 and its pipes), and sha2 and hmac for SigV4 |
| `s3-standin` | `s3`, and the stand-in of the S3 API in `s3::standin`, for the tests of this crate and of a consumer (in `[dev-dependencies]`); no binary turns it on |
| `fuzzing` | `poll`, and the reader of HTTP/1.1 and of SSE of the poll carriers in `poll::fuzzing`, for the fuzz target `link_poll_wire`; no API, and no consumer turns it on |

## The wire

`WIRE.md` gives the wire of the link: the preface, the frames, the rules
with their ids (W-01 to W-35), the order of the checks, the timers, the
constants and the settings with their limits. It is normative, and the
test `wire_doc` holds it to the code. This section gives the bytes of
each carrier under the link.

The tcp carrier puts TLS 1.3 under the bytes of the link, with the ALPN
`podlink/1`, no resumption, and the certificate of each end's identity
(self-signed, Ed25519). After its handshake the listener writes the byte
1, its acceptance of the key of the dialer; the dial ends when it reads
it, and the preface of the link follows.

The ws carrier puts TLS 1.3 under a WebSocket, with the ALPN `http/1.1`,
no resumption and the same certificates. The dialer sends the upgrade
`GET PATH` with the `Host` of its URL; the listener answers 101 after it
took the key of the dialer, and takes any path. The dialer then sends an
empty binary message, the first frame that the accept loop waits for.
The bytes of the link go in binary messages of at most 16 KiB each way;
the end of the link sends a Close 1000. A text message is an error of
protocol: the carrier sends a Close 1003, and the link ends with an error
of `InvalidData`. `Ws::protocol` is the subprotocol of both ends
(podnet's `podnet/1`): the dial offers it and refuses a 101 that does not
name it, and the listener refuses a request that does not offer it
(426). Each end sends a Ping each 20 s; when no frame comes within 10 s
of a Ping, and no message waits for the link, the session ends and the
link ends with an error of `TimedOut` (`Ws::beat`).

The h2 carrier puts TLS 1.3 with the ALPN `h2` under one stream of
HTTP/2: the dialer sends `POST https://HOST/PATH` with no end of its
body, and the listener answers 200 with no end of its body. The listener
sends its SETTINGS after it took the key of the dialer, and the dial
reads them before it gives the stream, so a refused key is an error of
the dial. Each window (the stream's, the connection's) is 4 MiB.
END_STREAM is the half close; a second request on the connection gets
`REFUSED_STREAM`, and a request that is not a POST gets 405.

The s3 carrier keeps the objects of a link under its prefix (call C489):
`open/SESSION` (empty: the dialer opens a session), `up/SESSION/SEQ` (the
bytes of the dialer) and `down/SESSION/SEQ` (the bytes of the listener).
SESSION is 128 random bits of the dialer in 32 hex digits, SEQ 16 hex
digits from 0, so a listing (`ListObjectsV2`, path style by default) gives
the objects in order. An object holds 1 byte to `object` bytes; an empty
object is the end of its direction. Each PUT has `If-None-Match: *` (412
is a PUT again after a lost answer), each GET has `If-Match` with the ETag
of the listing, and each request has SigV4 (`x-amz-content-sha256`,
`x-amz-date`, `x-amz-security-token` for a temporary key). The reader lists
the keys after the last one it read and takes the run with no gap. The TLS
of the tcp carrier and the byte of acceptance run over the objects.

The split (call C492) starts each half with a hello of the dialer: `PODSPLIT`,
the version 1, the half (1 up, 2 down) and a session of 16 random bytes.
The listener joins the two halves of a session, refuses two halves that
proved two keys, and writes `JOINED` as the first frame down. Then each
direction of each half carries frames of a type (1 byte) and a length (4
bytes, big endian): `DATA` (1 byte to 64 KiB, on the main direction: up
for the dialer, down for the listener), `BEAT` (none, after a silence of
`beat`), `END` (none: the end of the bytes of its sender), `JOINED`, and
`LOST` (the half that its sender lost: 1 or 2). A direction with no frame
for `lost_after`, a failure, or an end of a carrier before the `END` of
its peer is the loss of that half; the end that sees the loss writes
`LOST` on the other half and ends the session.

A frame is the record that the link gives its carrier: at most 1 MiB of
body. A carrier with a smaller unit splits a record itself.

## The ARQ

A carrier of datagrams that can be lost, come out of order or come two
times carries a link through `arq::ArqStream`. The link sees one ordered
byte stream and knows nothing of the ARQ. Each number is big-endian:

| Type | Datagram | Fields |
| --- | --- | --- |
| 1 | `DATA` | the sequence (u32), the fragment index (u16), the fragment count (u16), the payload (1 byte to 1 KiB) |
| 2 | `FIN` | the sequence (u32): the end of the sender's bytes |
| 3 | `ACK` | the next sequence in order (u32), the window in frames (u16), the bitmap of 32 bytes: bit `i` is the frame `next + i` |
| 4 | `PROBE` | none: the peer answers with an `ACK` |

- A write is one record: its fragments have one sequence each, the index
  0 to `count - 1`, and the count. The receiver gives the bytes in order,
  and a record whole to `recv_record`.
- At most 256 frames are in flight after the oldest frame with no
  acknowledgement, and no frame goes past the window of the peer's last
  `ACK`. A receiver gives a window of the free part of its receive buffer
  (in fragments of 1 KiB); a sender whose peer gives no window probes it
  after each RTO, and the answer keeps the ARQ alive.
- An `ACK` goes after each turn of the receive side that took a frame or
  a `PROBE`, and after a read that opens the window.
- The RTO is of RFC 6298 (Karn: a sample only from a frame sent once),
  1 s before the first sample, within 200 ms and 8 s. Each frame has its
  own timer; each timeout doubles its RTO up to the ceiling and sends it
  again. A frame with 3 or more acknowledged frames above it goes again
  once before its timer. After 8 retransmissions and one more RTO, the ARQ
  ends with `arq-timeout` (`ArqError::Timeout`, class `Timeout`), and the
  link above it ends: a read or a write of the stream fails with
  `TimedOut`.
- The sequence never passes from `u32::MAX` to 0 under the same keys:
  `rekey_ahead` frames before the wrap the ARQ asks for a rekey
  (`ArqHandle::rekey_wanted`), and new frames wait at the wrap until the
  caller calls `rekeyed`. The frames in flight go on. The two ends must
  start at the same `start`. The dns and icmp carriers answer each ask at
  once: TLS above the ARQ has nonces of its own, so the sequence of the ARQ
  is no nonce, and no direction stops at the wrap (T-541, call C290).
- A datagram of another form, a frame out of the window, and an `ACK` of a
  frame not sent are dropped and counted. Fragments in sequence that
  disagree (an index or a count) end the ARQ with `ArqError::Protocol`.
  The ARQ authenticates nothing: put a record layer under it on a carrier
  that strangers can reach.
- There is no congestion control: the window of 256 frames bounds the
  flight (T-515).

| Field | Default | Limits | What |
| --- | --- | --- | --- |
| `window` | 256 | 1 to 256 | the frames in flight |
| `fragment` | 1024 | 32 to 1024 | the largest payload of a fragment; also the carrier's unit less 9 bytes |
| `record` | 64 KiB | 1 KiB to 1 MiB | the largest record (a write); at most 65535 fragments |
| `send_buffer` | 1 MiB | `record` or more | the bytes of the records with no acknowledgement; a write waits over it |
| `receive_buffer` | 4 MiB | two records and 256 KiB or more | the bytes in order that the reader did not take |
| `rto_initial` | 1 s | the floor to the ceiling | the RTO before the first sample |
| `rto_floor`, `rto_ceiling` | 200 ms, 8 s | more than 0 | the bounds of the RTO, also after the backoff |
| `retries` | 8 | 1 to 16 | the retransmissions of a frame before `arq-timeout` |
| `start` | 0 | each | the first sequence of each direction; the same at both ends |
| `rekey_ahead` | 2^20 | `window` or more | the frames before the wrap at which the ARQ asks for a rekey |

The ARQ is one sequence for the whole link: when one frame has no
acknowledgement after its retries, the link ends, not one stream (call
C262). A stream-aware ARQ, in which `arq-timeout` closes one stream while
the control frames live, is T-514.

## A link that survives the loss of its carrier

A link does not resume: a lost carrier ends it (call C120). For a link
that lives across the loss of its carrier, run the link over the
application side of a session of `pod-session`:

1. The dialer starts a session (`pod_session::client::start`) on a
   carrier, then `pod_session::resume::run(client, app_end, connect, ...)`,
   where `connect` gives a new carrier after each loss.
2. The listener accepts the session (`pod_session::far::accept`) and runs
   it with a `Keeper`, which keeps it across each resume.
3. Each end gives its application side of the session (a
   `tokio::io::duplex` pair: one half to the session, one to the link) to
   `Link::new`.

The link sees no loss while the session resumes. Its own clock goes on:
a resume that takes longer than `dead_after` (30 s) ends the link with
`Error::Dead`, though the session would resume until its
`resume_deadline` (600 s by default). To keep a link across a resume as
long as the session allows, give `dead_after` a value of the
`resume_deadline` or more, or keep the default and accept that a link
dies after 30 s with no carrier.

## The parity list

The rules of PodUtils/podnet#466 (E0-2) and #467 (E1-7, E1-8), and the
tests that hold them:

- [x] Dialer odd ids, listener even ids: `tests/streams.rs`
  (`ids_by_role_*`), `tests/limits.rs` (`an_open_of_the_wrong_parity_*`).
- [x] A window of 16 KiB at the start and 8 MiB at most, for each stream
  and for the link: `tests/windows.rs`
  (`a_writer_waits_for_credit_and_splits_its_frames`,
  `a_reader_that_keeps_up_grows_its_window`), `tests/limits.rs`
  (`bytes_past_the_window_*`, `each_setting_out_of_its_limits_is_refused`).
- [x] 256 streams by default and 4096 at most: `tests/limits.rs`
  (`an_open_over_the_own_limit_fails`,
  `the_peer_refuses_an_open_over_its_limit`).
- [x] A record of 1 MiB at most, writes split into frames:
  `tests/frames.rs`, `tests/limits.rs`
  (`a_frame_over_one_mebibyte_ends_the_link`), `tests/windows.rs`
  (`four_mebibytes_*`).
- [x] A stream that stops reading cannot starve another: `tests/windows.rs`
  (`a_stalled_reader_starves_no_other_*`).
- [x] 300 echoes: `tests/streams.rs` (`three_hundred_echoes_*`).
- [x] A keepalive and a dead link: `tests/keepalive.rs`.
- [x] Open, data, credit, close (a half close), reset, the end of a link:
  `tests/streams.rs`.
- [x] The unix carrier: a path of 1 to 107 bytes, mode 0600, a regular
  file refused (64), a live socket in use (69), a stale socket replaced,
  the peer's uid unless allowed, a socket pair: `tests/carriers.rs`.
- [x] The unix carrier on hostile hosts: a listener under Landlock with no
  `REFER` (the move of the staged socket gives `EXDEV`) binds at the path
  with the mode 0600 set before the bind, and a refused `listen` names
  `listen`: `tests/unix_hosts.rs`; the mode of the file of a socket is the
  mode that `fchmod` gave the socket before its bind: `src/unix/tests.rs`.
- [x] The memory carrier: a ring of 64 KiB to 64 MiB (1 MiB by default), a
  full ring makes the writer wait and drops nothing, the end of one side
  is the end of stream of the other: `tests/carriers.rs`.
- [x] The resume of a link over `pod-session` (podnet T-263): the carrier
  of the session is cut twice while two streams move 1 MiB each way, and
  the SHA-256 of each stream is equal at both ends: `tests/resume.rs`
  (`two_streams_cross_two_resumes_of_the_session_whole`).
- [x] The ARQ for a carrier that loses frames (podnet T-277, row E2-2 of
  PodUtils/podnet#468): 8 MiB one way and 1 MiB the other at 3 % loss,
  1 % reorder and 1 % duplicates, whole and in order
  (`tests/arq.rs`, `eight_mib_cross_three_percent_loss_whole_and_in_order`),
  and a link of two streams over it
  (`a_link_runs_over_the_arq_at_three_percent_loss`); the fragment index
  and count (`tests/arq_machine.rs`,
  `a_record_is_cut_into_fragments_with_their_count`); the window of 256
  frames and the peer's window
  (`the_window_bounds_the_frames_in_flight`,
  `the_window_of_the_peer_bounds_the_frames`); the bitmap of the `ACK`
  and the retransmission of each lost fragment
  (`each_lost_frame_goes_again_alone`); the RTO of 200 ms to 8 s and 8
  retries (`the_timer_doubles_and_ends_after_eight_retries`,
  `the_rto_keeps_its_floor_and_its_ceiling`), and a silent peer
  (`tests/arq.rs`, `a_silent_peer_ends_with_arq_timeout`); the wrap that
  asks for a rekey and never uses a sequence again
  (`the_sequence_does_not_wrap_without_a_rekey`,
  `the_bytes_wait_at_the_wrap_until_the_rekey`). The close of one stream
  on `arq-timeout` while the link lives is T-514.

The asks of PodUtils/podnet#513 (PodUtils/podnet@T-268, the TCP carrier
with TLS 1.3) and the tests that hold them (`tests/tcp.rs`,
`tests/tcp_refused.rs`, feature `tcp`):

- [x] A link over TCP with TLS 1.3 carries streams:
  `a_link_over_tcp_with_tls_carries_streams` (3 streams of 256 KiB, each
  echoed whole).
- [x] The dial goes through the caller's way out (`pod_core::Dial`, as a
  road of `pod-relay` takes one; a dialer of `pod-proxy` is one): the
  same test counts the dials of the way; a failure of the way keeps its
  class (`a_failure_of_the_way_out_keeps_its_class`).
- [x] The pin of the key of the listener and of each allowed client key
  (`pod_tls::peer`, T-304); the accept gives the key of the client:
  `the_connect_gives_the_key_of_the_listener_and_the_accept_the_key_of_the_client`,
  `a_key_named_by_its_fingerprint_or_its_display_form_dials`.
- [x] Another key is refused at the handshake on each side, with the key
  seen named (77): `the_dialer_refuses_a_listener_of_another_key_and_names_it`,
  `the_listener_refuses_a_client_of_another_key_and_stays_open`. The end
  whose key was refused gets `KeyRefused` (T-542): the dialer on each
  carrier that pins keys (tcp, ws, h2, quic, udp, poll, dns, icmp), and
  the listener on each of them too (T-557: dns, icmp, udp and ws in
  `the_dialer_refuses_a_listener_of_another_key_and_names_it`). The ARQ
  stream (dns, icmp) and the KCP stream (udp) write each buffer of a
  vectored write, as rustls writes the alert of a failed handshake with
  one vectored write and no second try. The ws listener gives a client
  that refused its key as an error of one accept (`KeyRefused`), through
  `pod_ws::serve::serve_reporting`, and skips each other failed handshake
  (call C390). Plants: `link-key-refused-untyped`,
  `link-key-refused-alert-untyped`, `link-quic-key-refused-untyped`,
  `link-arq-vectored-first-only`, `link-kcp-vectored-first-only`,
  `link-ws-key-refused-skipped`, `ws-serve-tls-failure-unreported`.
- [x] TLS 1.2 is refused on each side:
  `a_client_of_tls_1_2_is_refused_by_the_listener`,
  `a_listener_of_tls_1_2_is_refused_by_the_dialer`.
- [x] `available()` with a reason, the facts of a stream:
  `the_carrier_says_why_a_host_or_a_provider_cannot_use_it`.
- [x] A peer that sends nothing holds no other handshake, and its own
  ends at its time limit: `a_peer_that_sends_nothing_holds_no_other_handshake`.
- [x] A plant for each check: `link-tcp-server-key-unchecked`,
  `link-tcp-client-key-unchecked`, `link-tcp-acceptance-skipped`,
  `link-tcp-tls12-listener`, `link-tcp-tls12-dialer`,
  `link-tcp-handshake-inline` (`scripts/plants/`).
- podnet's hello (`src/hello.rs`) is not offered on this carrier
  (PodUtils/podnet#513); the keys of a link of several hops are
  `pod-e2e`'s (T-301).

The asks of PodUtils/podnet#515 for the WebSocket carrier
(PodUtils/podnet@T-269) and the tests that hold them (`tests/ws.rs`,
feature `ws`):

- [x] A link over a WebSocket with TLS carries streams, and the dial goes
  through the caller's way out: `a_link_over_a_websocket_carries_streams`;
  a failure of the way keeps its class:
  `a_failure_of_the_way_out_keeps_its_class_and_the_carrier_says_why_it_cannot_run`.
- [x] The pin of each key, as `tcp` has it; the accept gives the key of
  the client:
  `the_connect_gives_the_key_of_the_listener_and_the_accept_the_key_of_the_client`.
- [x] Another key is refused on each side, with the key seen named (77):
  `the_dialer_refuses_a_listener_of_another_key_and_names_it`,
  `the_listener_refuses_a_client_of_another_key_and_stays_open`.
- [x] The server half on the accept loop of `pod-ws`: a silent peer holds
  no other one and is cut at its deadline:
  `a_peer_that_sends_nothing_holds_no_other_and_is_cut_at_its_deadline`;
  a silent listener ends the dial at its time limit:
  `a_silent_listener_ends_the_dial_at_its_time_limit`.
- [x] A plant for each check: `link-ws-server-key-unchecked`,
  `link-ws-client-key-unchecked`, `link-ws-first-frame-skipped`,
  `link-ws-refusal-unnamed`, `link-ws-handshake-unbounded`
  (`scripts/plants/`).
- [x] The subprotocol `podnet/1` (`Ws::protocol`), offered and named:
  `the_subprotocol_of_both_ends_is_offered_and_named`; a dialer with no
  subprotocol or another gets 426, and a 101 with none is refused:
  `a_dialer_with_no_subprotocol_or_another_gets_426` (T-506).
- [x] A text message is an error of protocol:
  `a_text_message_ends_the_link_with_an_error_of_protocol` (T-506).
- [x] A Ping each 20 s, a frame within 10 s:
  `a_peer_that_answers_no_ping_ends_the_session`,
  `a_peer_that_answers_each_ping_keeps_the_session`,
  `a_message_that_waits_for_the_link_keeps_the_session` (T-506).
- [x] A plant for each: `link-ws-text-message-carried`,
  `link-ws-silent-peer-kept`, `link-ws-ping-unsent`,
  `link-ws-waiting-message-unseen`, `link-ws-chosen-protocol-unchecked`,
  `ws-serve-protocol-unchecked`, `ws-serve-101-unnamed`.

The asks of PodUtils/podnet#515 for the HTTP/2 carrier
(PodUtils/podnet@T-270) and the tests that hold them (`tests/h2.rs`,
feature `h2`):

- [x] A link over one POST carries streams, and the dial goes through the
  caller's way out: `a_link_over_one_post_of_http2_carries_streams`.
- [x] Through a proxy of the way out (a CONNECT tunnel of `pod-proxy`),
  the bytes move both ways while the request body is open: 50 lines, each
  answered before the next: `a_dial_through_a_proxy_moves_bytes_both_ways_while_the_post_is_open`.
- [x] Another key is refused on each side, with the key seen named (77):
  `the_dialer_refuses_a_listener_of_another_key_and_names_it`,
  `the_listener_refuses_a_client_of_another_key_and_stays_open`.
- [x] A request that is not a POST gets 405:
  `a_request_that_is_not_a_post_is_refused`.
- [x] A silent peer holds no other handshake, and its own ends at its
  time limit: `a_peer_that_sends_nothing_holds_no_other_handshake`; a
  silent listener ends the dial: `a_silent_listener_ends_the_dial_at_its_time_limit`.
- [x] A plant for each check: `link-h2-server-key-unchecked`,
  `link-h2-client-key-unchecked`, `link-h2-settings-unread`,
  `link-h2-request-ended`, `link-h2-handshake-inline`
  (`scripts/plants/`).

The asks of PodUtils/podnet#515 for the QUIC carrier
(PodUtils/podnet@T-273) and the tests that hold them (`tests/quic.rs`,
feature `quic`):

- [x] A link over one stream of QUIC carries streams, and the dial goes
  over the datagrams of the caller's way out:
  `a_link_over_quic_carries_streams`; a name resolves through the
  caller's resolver:
  `the_connect_gives_the_key_of_the_listener_and_a_name_resolves_through_the_resolver`;
  a listener runs over the caller's datagrams:
  `a_listener_runs_over_the_datagrams_of_the_caller`.
- [x] TLS 1.3 with the ALPN of the caller (podnet's `podnet/1`):
  `both_ends_must_take_the_same_alpn`.
- [x] Another key is refused on each side, with the key seen named (77):
  `the_dialer_refuses_a_listener_of_another_key_and_names_it`,
  `the_listener_refuses_a_client_of_another_key_and_stays_open`.
- [x] A peer that stops in its handshake holds no other one, and its own
  ends at its time limit:
  `a_peer_that_stops_in_its_handshake_holds_no_other_one`; a silent
  listener ends the dial: `a_silent_listener_ends_the_dial_at_its_time_limit`.
- [x] The bytes before a drop reach the peer:
  `the_bytes_before_a_drop_reach_the_peer`.
- [x] A plant for each check: `link-quic-refusal-unnamed`,
  `link-quic-handshake-inline`, `link-quic-drop-closes-at-once`,
  `link-quic-way-unused` (`scripts/plants/`).

The asks of PodUtils/podnet#515 for the UDP carrier with KCP
(PodUtils/podnet@T-276, row E2-1 of PodUtils/podnet#468) and the tests
that hold them (`tests/udp.rs`, `tests/udp_loss.rs`, feature `udp`):

- [x] A link over KCP with TLS carries streams, and the dial goes over the
  datagrams of the caller's way out: `a_link_over_kcp_carries_streams`.
- [x] The pin of each key; another key is refused on each side, with the
  key seen named (77): `the_dialer_refuses_a_listener_of_another_key_and_names_it`,
  `the_listener_refuses_a_client_of_another_key_and_stays_open`.
- [x] MTU 1200, segments of 1 KiB, a window of 32 to 1024, 20 s of
  silence: `the_carrier_says_what_it_gives_and_why_it_cannot_run`,
  `an_address_or_a_setting_out_of_its_form_is_a_usage_error`,
  `a_peer_that_goes_silent_ends_the_stream_at_the_idle_limit`.
- [x] Loss, reorder and duplicates on the `Lossy` shim:
  `one_mib_crosses_each_way_with_no_loss`,
  `one_mib_crosses_each_way_at_three_percent_loss_with_reorder_and_duplicates`;
  8 MiB each way at 5 % loss and 1 % reorder with `nodelay`:
  `eight_mib_cross_at_five_percent_loss_and_one_percent_reorder_with_nodelay`.
- [x] podnet's gate of row E2-1 in a release build (T-520, calls C284, C787
  and C878; the job `gate` of `nightly.yml`): 8 MiB each way at 5 % drop
  with the defaults,
  `eight_mib_cross_at_five_percent_loss_and_one_percent_reorder_with_the_defaults`;
  20 % drop with `nodelay` and a window of 1024 over a path of 10 ms each
  way: the median of five runs within 8 times the median of five clean
  runs (4.1 to 6.2 measured; podnet asks for 3), and a clean time of at
  least one round trip for each window,
  `twenty_percent_loss_with_nodelay_stays_within_the_factor_of_the_clean_time`;
  its plant `link-udp-no-fast-resend`.
- [x] `available()` names a host that refuses UDP (69), and the carrier
  then sends nothing: `the_carrier_says_what_it_gives_and_why_it_cannot_run`
  (a provider with no TLS 1.3; a host with no UDP is not made here).
- [x] A peer that stops in its handshake holds no other one:
  `a_peer_that_stops_in_its_handshake_holds_no_other_one`; a silent
  listener ends the dial: `a_silent_listener_ends_the_dial_at_its_time_limit`;
  datagrams of a stranger start no session and change no link:
  `datagrams_of_a_stranger_change_nothing`; the bytes before a drop reach
  the peer: `the_bytes_before_a_drop_reach_the_peer`; a listener over the
  caller's datagrams: `a_listener_runs_over_the_datagrams_of_the_caller`.
- [x] A panic of the task of KCP (a broken carrier, or the code of `kcp`
  with its overflow checks) ends the stream with an error:
  `a_panic_of_the_task_ends_the_stream_with_an_error`; the workspace builds
  `kcp` with no overflow checks, as its clock wraps (T-526); a
  fragment of a message (the stream mode sends none) does not go in, and a
  segment larger than one of this end is read whole (T-521, call C280):
  `a_fragment_of_a_stranger_is_dropped_and_a_large_segment_is_read_whole`;
  the fuzz target `link_kcp` gives the stream datagrams of a stranger
  (call C279).
- [x] A plant for each check: `link-udp-handshake-inline`,
  `link-udp-stranger-starts-a-session`, `link-udp-drop-stops-at-once`,
  `link-udp-idle-unchecked`, `link-udp-panic-hangs-the-stream`,
  `link-udp-fragment-taken`, `link-udp-large-segment-stalls`
  (`scripts/plants/`).

The asks of PodUtils/podnet#515 and #521 for the long poll, SSE and the
chunked stream (PodUtils/podnet@T-271, PodUtils/podnet@T-272) and the
simplex poll (PodUtils/podnet@T-280), and the tests that hold them
(`tests/poll.rs`, `tests/poll_lanes.rs`, feature `poll`):

- [x] A link carries streams in each form through a way out that loses
  the answer of a POST and cuts an answer down; no byte comes twice and
  none is lost: `a_link_over_the_long_poll_carries_streams_across_a_lost_answer`,
  `a_link_over_sse_carries_streams_across_a_lost_answer`,
  `a_link_over_a_chunked_stream_carries_streams_across_a_lost_answer`,
  `a_link_over_the_simplex_poll_carries_streams_across_a_lost_answer`.
- [x] A proxy that cuts idle connections holds no link back, and the end
  of each side reaches the other:
  `a_proxy_that_cuts_idle_connections_holds_no_long_poll_back`,
  `a_proxy_that_cuts_idle_connections_holds_no_push_back`.
- [x] Another key is refused on each side, with the key seen named (77);
  a session answers the key that opened it alone:
  `the_dialer_refuses_a_listener_of_another_key_and_names_it`,
  `the_listener_refuses_a_client_of_another_key_and_stays_open`,
  `a_session_answers_the_key_that_opened_it_alone`.
- [x] A silent peer holds no other handshake; a silent listener ends the
  dial; a session with no request ends on each side with an error:
  `a_peer_that_sends_nothing_holds_no_other_handshake`,
  `a_silent_listener_ends_the_dial_at_its_time_limit`,
  `a_session_with_no_request_ends_on_each_side_with_an_error`.
- [x] The hold of the long poll, the backoff of the simplex poll (100 ms
  to 5 s), 429 with Retry-After up to the third in a row, a push past the
  window with no bytes up, the long poll where the listener refuses a
  push: `the_simplex_poll_backs_off_while_the_link_is_quiet_and_the_long_poll_holds`,
  `the_third_answer_429_in_a_row_ends_the_dial`,
  `a_push_moves_more_than_the_window_with_no_bytes_up`,
  `a_listener_with_no_push_gives_the_long_poll`.
- [x] The sessions of a listener keep no more bytes down than
  `Poll::max_bytes`; an open with no room for one batch gets 429, and an
  acknowledgement gives the bytes back (T-517):
  `the_sessions_keep_no_more_bytes_than_the_cap_and_an_open_with_no_room_gets_429`.
- [x] A listener holds at most `Poll::max_connections` connections, and
  one past them closes at its accept; a body up goes into the link by
  parts, so no connection keeps a whole body, and a cut in a body keeps
  the part that went in (T-525):
  `a_connection_past_the_cap_closes_and_the_others_stay`,
  `a_body_up_goes_into_the_link_by_parts_and_a_cut_keeps_its_part`.
- [x] The name and the cost class of each form (`httppoll` is
  `Cost::Highest`):
  `each_form_has_its_name_and_its_cost_and_a_failure_of_the_way_out_keeps_its_class`.
- [x] A plant for each check: `link-poll-duplicate-taken`,
  `link-poll-resend-dropped`, `link-poll-refusal-unnamed`,
  `link-poll-session-key-unchecked`, `link-poll-ack-unsent`,
  `link-poll-no-backoff`, `link-poll-no-expiry`,
  `link-poll-first-busy-fatal`, `link-poll-hold-unheld`,
  `link-poll-handshake-inline`, `link-poll-push-refusal-fatal`,
  `link-poll-bytes-uncapped`, `link-poll-open-room-unchecked`,
  `link-poll-long-head-taken`, `link-poll-connections-uncapped`,
  `link-poll-body-kept-whole` (`scripts/plants/`).
- [x] The reader of HTTP/1.1 refuses a head of more than 4 KiB also when
  one read brings it whole (T-518): `a_long_head_and_a_large_chunk_are_refused`;
  the fuzz target `link_poll_wire` reads the bytes of connections.

The asks of PodUtils/podnet#521 for the DNS simplex carrier
(PodUtils/podnet@T-278, row E2-3 of PodUtils/podnet#468) and the tests
that hold them (`tests/dns.rs`, `tests/dns_loss.rs`, feature `dns`):

- [x] A link over DNS queries and TXT answers with TLS carries streams:
  `a_link_over_dns_carries_streams`.
- [x] The pin of each key; another key is refused on each side, with the
  key seen named: `the_dialer_refuses_a_listener_of_another_key_and_names_it`,
  `the_listener_refuses_a_client_of_another_key_and_stays_open`.
- [x] A silent server ends the dial: `a_silent_server_ends_the_dial_at_its_time_limit`;
  junk and a query of a stranger change no link:
  `a_query_of_a_stranger_changes_nothing`.
- [x] The address, the settings and the facts:
  `an_address_or_a_setting_out_of_its_form_is_a_usage_error`,
  `the_carrier_says_what_it_gives_and_why_it_cannot_run`.
- [x] Loss, reorder and duplicates on the `Lossy` shim, and the end of a
  silent peer at the timeout of the ARQ: `tests/dns_loss.rs`.
- [x] Each direction crosses the wrap of the sequence of the ARQ at loss
  (T-541): `bytes_cross_the_wrap_of_the_sequence_at_loss`.
- [x] The size of an answer and the TC bit (T-543, calls C291 and C292;
  `tests/dns_resolver.rs`, a stand-in resolver on the loopback): a query
  with no EDNS0 gets 512 bytes or less and no OPT record
  (`a_query_with_no_edns_gets_an_answer_of_512_bytes_or_less`); a query
  of 1232 gets answers within it
  (`a_query_with_edns_gets_an_answer_within_its_size`); answers cut with
  the TC bit do not stall the link (`a_cut_answer_does_not_stall_the_link`),
  and the dialer takes the whole records before the cut
  (`dns::client::tests::a_cut_answer_gives_its_whole_records`).
- [x] A plant for each check: `dns-down-needs-the-txt`,
  `dns-wire-base32-roundtrip`, `link-dgram-wrap-waits`,
  `dns-answer-past-the-query-size`, `dns-cut-answer-dropped`
  (`scripts/plants/`).

The asks of PodUtils/podnet#521 for the ICMP echo carrier
(PodUtils/podnet@T-282, row E3-2 of PodUtils/podnet#469) and the tests
that hold them (`tests/icmp.rs`, `tests/icmp_raw.rs`, feature `icmp`):

- [x] Bytes cross each way over `EchoFrames` on the `Lossy` shim, with no
  loss and at 3 % loss with reorder and duplicates:
  `bytes_cross_each_way_with_no_loss`,
  `bytes_cross_each_way_at_three_percent_loss_with_reorder_and_duplicates`.
- [x] Each direction crosses the wrap of the sequence of the ARQ at loss
  (T-541): `bytes_cross_the_wrap_of_the_sequence_at_loss`.
- [x] The keys are proven on each side, and a refused key is named:
  `the_keys_are_proven_on_each_side`, `a_refused_key_is_named_on_each_side`.
- [x] A silent peer ends the link: `a_peer_that_goes_silent_ends_the_link`;
  an echo of another session or of the wrong mark is dropped:
  `an_echo_of_the_wrong_session_or_mark_is_dropped`.
- [x] The address, the settings and the facts:
  `an_address_or_a_setting_out_of_its_form_is_a_usage_error`,
  `the_carrier_says_what_it_gives`; `available()` needs no tokio runtime:
  `available_needs_no_runtime`.
- [x] The raw socket (unix, `CAP_NET_RAW`): a link carries a stream, or the
  test prints a skip line where the host refuses the socket:
  `a_link_over_a_raw_socket_carries_a_stream_or_skips`.
- [x] A plant for each check: `icmp-echo-session-filter`,
  `icmp-wire-checksum`, `link-dgram-wrap-waits` (`scripts/plants/`).

The asks of PodUtils/podnet#521 for the object-storage simplex carrier
(PodUtils/podnet@T-279, row E2-4 of PodUtils/podnet#468) and the tests that
hold them (`tests/s3.rs`, the stand-in of the S3 API on the loopback that
checks SigV4 and the conditions, `s3::standin`; feature `s3-standin`):

- [x] A link over objects with TLS carries streams through the caller's way
  out, and each end learns the key of the other; the keys of a session:
  `a_link_over_objects_carries_streams_and_each_end_learns_the_key_of_the_other`.
- [x] A conditional PUT (`If-None-Match: *`): a PUT again after a lost
  answer gives no byte twice: `a_put_whose_answer_was_lost_gives_no_byte_twice`.
  A conditional GET on the ETag of the listing.
- [x] Objects of 256 KiB, 8 in flight, a poll of 100 ms to 5 s, a page of
  1000 keys: the settings and their bounds,
  `the_address_the_settings_and_the_facts`.
- [x] The budget of 1 GiB with a warning at 80 %:
  `the_budget_ends_a_session_whose_peer_removes_nothing`.
- [x] The prune is opt-in (`S3::prune`):
  `prune_removes_each_object_read_and_the_open_object`.
- [x] The pin of each key, refused on each side and named:
  `another_key_is_refused_on_each_side_and_the_listener_stays_open`; a
  wrong secret (403, `Refused`, 77) and a missing bucket (404):
  `a_wrong_secret_and_a_missing_bucket_end_the_bind_and_the_dial`; a
  listener takes no session older than its bind:
  `a_listener_takes_no_session_older_than_its_bind`.
- [x] SigV4 signs the examples of the documentation of the S3 API alike:
  `s3::sign::tests::the_examples_of_the_documentation_of_the_s3_api_sign_alike`;
  a listing, its next page and the code of an error
  (`s3::xml::tests`); a run of objects stops at its first gap
  (`s3::stream::tests`).
- [x] A plant for each check: `link-s3-put-unconditional`,
  `link-s3-run-past-a-gap`, `link-s3-budget-unchecked`,
  `link-s3-old-session-taken`, `link-s3-prune-skipped`,
  `s3-sign-query-unsorted` (`scripts/plants/`).
- [x] A real S3-compatible server takes the requests (T-623, call C540):
  `tests/s3_seaweedfs.rs` against the pinned SeaweedFS 4.53 on the
  loopback (`scripts/ci/s3-tests.sh` in `ci.yml`; with no
  `POD_LINK_TEST_S3_ENDPOINT` each test skips, and
  `POD_LINK_TEST_S3=required` makes a skip a failure), with pages of one
  key: `a_link_over_the_objects_of_the_server_carries_streams`,
  `prune_removes_each_object_read_from_the_server`,
  `the_server_holds_the_conditions_and_the_pages_of_the_carrier`
  (`If-None-Match: *` gives 412 on a key that is there, `If-Match` 412 on
  another ETag, `max-keys`, `start-after`),
  `a_wrong_secret_and_a_missing_bucket_end_the_bind`,
  `a_listener_takes_no_session_older_than_its_bind`,
  `the_budget_ends_a_session_whose_peer_removes_nothing`. The plants
  `link-s3-numeric-reference-kept` (SeaweedFS writes the ETag of a
  listing as `&#34;...&#34;`, which the stand-in does not) and
  `link-s3-seaweedfs-skip-fails`.
- [x] The stand-in is public for the tests of a consumer (T-627,
  PodUtils/podnet#564; `tests/s3_standin.rs`): a request with no
  signature gets 403, and a drop closes the port and each connection, on
  its own thread and on the runtime of the caller:
  `a_request_with_no_signature_gets_403_and_a_drop_ends_the_thread_of_the_standin`,
  `a_drop_of_a_standin_on_the_runtime_of_the_caller_closes_its_connections`;
  a path with a bad escape gets 403 and `max-keys=0` lists no key, with no
  panic: `a_bad_escape_and_a_listing_of_no_key_get_an_answer_and_no_panic`.
  The plants `link-s3-standin-unsigned-taken`, `link-s3-standin-escape-panics`,
  `link-s3-standin-signature-unchecked` (a wrong secret passes the
  stand-in: `a_wrong_secret_and_a_missing_bucket_end_the_bind_and_the_dial`
  fails) and `link-s3-standin-runs-after-drop`.
- The credentials from the environment or a profile file, and exit 77 with
  no word of a missing credential, are podnet's (the crate reads no
  environment): `Credentials` comes from the caller.

The asks of PodUtils/podnet#521 for one session over an up carrier and a
down carrier (PodUtils/podnet@T-302, row E7-1 of PodUtils/podnet#473) and the
tests that hold them (`tests/split.rs`, over carriers of the test: byte
pipes under a switch, and the ARQ over the `Lossy` shim at 3 % loss):

- [x] 1 MiB down a fast half and 64 KiB up a slow half that loses
  datagrams, in a link: `one_mib_down_a_fast_half_and_64_kib_up_a_lossy_half_cross_whole`.
  The credits of the link for the bytes up ride down; each direction has
  its own credits.
- [x] The loss of a half within `lost_after` (30 s), named
  `up-carrier lost` or `down-carrier lost` on each side, with no
  promotion of the other half:
  `a_silent_up_half_ends_the_session_with_up_carrier_lost_on_each_side`,
  `a_failed_down_half_ends_the_session_with_down_carrier_lost_on_each_side`.
- [x] A link over each end names the lost half also when the loss races
  the close of the bytes of the other link (T-639, PodUtils/podnet#572):
  an end sends no `END` after a loss, as the link closes its bytes then;
  it sends `LOST`. 240 rounds close the up or the down half, at each end
  or at one end while the other end sees silence, on the multi-thread
  runtime with 8 tasks that load it:
  `a_closed_half_ends_each_link_with_the_loss_of_that_half_in_each_round_under_load`.
  A write to the byte stream of the poll, s3 and split carriers after a
  fault gives the fault (`pipes::tests`).
- [x] The join of the halves of each session, the refusal of halves of two
  keys, the end of the bytes of each side:
  `the_listener_joins_the_halves_of_each_session_and_refuses_halves_of_two_keys`;
  the addresses, the settings and the facts:
  `the_address_the_settings_and_the_facts`; the wire (`split::wire::tests`).
- [x] A plant for each check: `link-split-silence-unseen`,
  `link-split-keys-unchecked`, `link-split-end-ignored`,
  `link-split-end-after-loss` (`scripts/plants/`).
- The control frames ride down with the bytes of the listener; no half
  takes over the other's direction at a loss (call C493). podnet's flags
  `--up-carrier` and `--down-carrier` and the router's split tests are
  podnet's.

## Measurement

`tests/measure.rs` measures the link (T-777). It is an ignored test with
no gate, and it adds no dependency:

```sh
cargo test --release -p pod-link --all-features --test measure -- --ignored --nocapture
```

It writes one line for each result, `MEASURE scen=NAME key=value ...`.
Each scenario runs on the memory carrier and, with the feature `tcp`, on
the tcp carrier over the loopback, with the default settings. A stream
gives its role in its first byte.

| Scenario | What |
| --- | --- |
| `host` | `cpus`: the threads of the host; `debug`: true for a debug build, whose numbers do not count |
| `wake` | the time from `Notify::notify_one` to the run of the task that waits (`p50_us`, `p99_us`): the speed of the host, for a later gate on latency |
| `bulk-1`, `bulk-8` | 256 MiB on 1 stream or on 8 streams, three runs and a `summary` (`median_gb_s`, `min_gb_s`, `max_gb_s`). A byte counts when the far reader took it and sent back the count at the end of the stream |
| `rtt-idle`, `rtt-bulk` | 2000 round trips of 64 bytes on one stream (`p50_us`, `p99_us`), on an idle link and beside a bulk stream (`bulk_bytes`) |

The keys of a bulk run: `bytes`, `secs`, `gb_s` (10^9 bytes each
second), `frames` (the `DATA` frames of `frame_size` that the bytes
need), `allocs` and `alloc_bytes_per_frame` (the allocations of the
process from the start to the end of the run, both ends and the runtime,
counted by the allocator of the test binary alone), `allocs_per_frame`,
and `peak_live` (the peak of the live heap above its level at the start).

The results, with the host, the commit and the spread of each: `RESULTS.md`.
`tests/allocations.rs` holds the result of T-778: a bulk run of 64 MiB over
the memory carrier makes at most 16 allocations after its first 1 MiB.

## The adoption map

| podnet | pod-link |
| --- | --- |
| PodUtils/podnet@T-262: the session of streams, credits and keepalive | `Link`, `Stream`, `Settings`, `frame` |
| PodUtils/podnet@T-264: the registry of carriers with an `available()` reason | stays in podnet (call C117); each row holds a `Box<dyn Carrier>` and adds podnet's facts (privilege, build feature); the cost class is `Facts::cost` |
| PodUtils/podnet@T-274: the unix socket carrier; `bind_private` and `claim_unix_path` of `PodUtils/podnet:crates/podnet-netns/src/listen.rs`; `--allow-other-uid` | `unix::Unix` (`allow_other_uid`), `unix::pair` |
| PodUtils/podnet@T-275: the memory carrier | `memory::Memory`, `memory::pair` |
| PodUtils/podnet@T-268: the TCP carrier with TLS 1.3 and the pin of each key | `tcp::Tcp` (feature `tcp`): one row of the registry holds `Box::new(Tcp::new(provider, identity, way).with_clients(keys))`; podnet's dialer (its proxy and egress) is the `way`; `available()` is the reason of the row; the accept side is `listen("IP:PORT")` and `accept_peer()`, whose `peer` podnet compares with `peer:FINGERPRINT`; a key that this end refused is `CarrierErrorKind::Refused` (77) and `tcp::refusal(&error)` gives the key seen; a key of this end that the peer refused is `CarrierErrorKind::KeyRefused` (77) on each carrier (T-542) |
| PodUtils/podnet@T-269: the WebSocket carrier, its server half | `ws::Ws` (feature `ws`): one row of the registry holds `Box::new(Ws::new(provider, identity, way).with_clients(keys))`; the dial address is `KEY@wss://HOST[:PORT]/PATH`; the listener is `listen("IP:PORT")` on the accept loop of `pod-ws`, with podnet's deadlines and caps in `Ws::serve` |
| PodUtils/podnet@T-270: the HTTP/2 carrier, one bidirectional POST | `h2::H2` (feature `h2`): one row of the registry holds `Box::new(H2::new(provider, identity, way).with_clients(keys))`; the dial address is `KEY@https://HOST[:PORT]/PATH`; the listener is `listen("IP:PORT")`. podnet's `body_reader` and `body_writer` are `pod_ws::h2_road::duplex` here |
| PodUtils/podnet@T-273: the QUIC carrier | `quic::Quic` (feature `quic`): one row of the registry holds `Box::new(Quic::new(provider, identity).with_clients(keys))` with `alpn = b"podnet/1"`, and `with_way(way)` when podnet's way carries UDP; the dial address is `KEY@HOST:PORT`; the listener is `listen("IP:PORT")` on UDP |
| PodUtils/podnet@T-276: the UDP carrier with KCP | `udp::Udp` (feature `udp`): one row of the registry holds `Box::new(Udp::new(provider, identity).with_clients(keys))` with `alpn = b"podnet/1"` and podnet's `KcpSettings` (`--kcp-nodelay 1` is `nodelay: true`); the dial address is `KEY@HOST:PORT`; the listener is `listen("IP:PORT")`; podnet's shim tests take `connect_over` and `accept_over` over `datagram::lossy::Lossy` |
| PodUtils/podnet@T-278: the DNS simplex carrier | `dns::Dns` (feature `dns`): one row of the registry holds `Box::new(Dns::new(provider, identity).with_clients(keys).zone(zone))` with `alpn = b"podnet/1"`; a dialer sets `to(resolver)` or `with_way(way)`; the dial address is `KEY@ZONE`; the listener is `listen("IP:PORT")`. The frames go up in base32 labels of a query name and come down in TXT records; the ARQ of T-310 recovers the loss (calls C274, C275) |
| PodUtils/podnet@T-282: the ICMP echo carrier | `icmp::Icmp` (feature `icmp`): one row of the registry holds `Box::new(Icmp::new(provider, identity).with_clients(keys))` with `alpn = b"podnet/1"`; the dial address is `KEY@HOST`; the listener is `listen("IPv4")`. The real path needs `CAP_NET_RAW` (`available()` says so, and the test skips); `connect_over` and `accept_over` over `datagram::lossy::Lossy` run the same framing with no privilege (calls C276, C277) |
| PodUtils/podnet@T-271, PodUtils/podnet@T-272: the HTTP/1.1 long poll, the SSE and chunked stream | `poll::Poll` (feature `poll`): a row of the registry for each form holds `Box::new(Poll::new(provider, identity, way).with_form(form).with_clients(keys))`; the dial address is `KEY@https://HOST[:PORT]/PATH`; one listener (`listen("IP:PORT")`) serves each form. podnet's `server_push=false` is a listener with `push` false: the dialer of `sse` or `chunked` then takes the long poll. The lanes are `POST PATH/up` and `POST PATH/down?since=N` (podnet's), with `PATH/open` first |
| PodUtils/podnet@T-280: the HTTP poll simplex carrier | `poll::Poll` with `Form::Simplex` (`httppoll`): `POST PATH/up` (202 and `Pod-Ack`), `GET PATH/out?since=N` (204 when empty), 64 KiB, a backoff of 100 ms to 5 s; `facts().cost` is `Cost::Highest`. Each lane is a request of its own, and the answer of each POST acknowledges it, so the split of PodUtils/podnet@T-302 can take one lane |
| PodUtils/podnet@T-279: the object-storage simplex carrier | `s3::S3` (feature `s3`): one row of the registry holds `Box::new(S3::new(provider, identity, way, endpoint, credentials).with_tls(tls).with_clients(keys))` with `alpn = b"podnet/1"`, podnet's `--s3-prune` as `prune`; the dial address is `KEY@BUCKET[/PREFIX]`, the listen address `BUCKET[/PREFIX]`; the credentials come from podnet's environment or profile file. The tests of `link_s3` take `s3::standin` (a dev-dependency with the feature `s3-standin`) in place of podnet's copy in tests/link_s3_standin/mod.rs: `StandIn::start_thread` for a test that starts podnet processes, `url()` for `--s3-endpoint`, `counts().refused` for podnet's `refused`; the profile file of the key is podnet's (from `key()`) |
| PodUtils/podnet@T-302: one session split across an up carrier and a down carrier | `split::Split::new(up, down)` of two carriers of the registry; `--up-carrier` and `--down-carrier` give `connect(up, down)` and `bind(up, down)` (or `dial("UP\|DOWN")`); a loss of a half is `split::Lost` (`lost`, `lost_in`), whose text is podnet's report |
| PodUtils/podnet@T-277: the selective-repeat ARQ for any lossy carrier | `arq::ArqStream` over a `datagram::Datagrams` (each lossy carrier of podnet gives one), then `Link::new` over the stream; `arq::ArqHandle::rekey_wanted` is the ask of the record layer (PodUtils/podnet@T-298) for a rekey; podnet's `tests/link_arq.rs` takes `datagram::lossy::Lossy` over `datagram::memory::pair` as its shim |
| exit 64, 69, 77 of a carrier | `CarrierError::class().exit_code()` |
| exit of a link | `Error::class().exit_code()`: a dead link 75, a broken protocol 76 |
