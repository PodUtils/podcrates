# pod-link

A link between two peers: many streams over one carrier byte stream, with
a credit window for each stream and for the link, a keepalive, and the
carriers behind one trait. podnet asked for it for its link workstream
(PodUtils/podnet#466, row E0-2; PodUtils/podnet#467, rows E1-7 and E1-8;
the start signal PodUtils/podnet#480). T-300 made it, as new code, by
calls C116, C117 and C120 to C124. T-396 added the tcp carrier (feature
`tcp`; PodUtils/podnet#513; calls C177 to C179), T-420 the ws carrier
(feature `ws`; PodUtils/podnet#515; call C253), T-421 the h2 carrier
(feature `h2`; PodUtils/podnet#515; call C254). The crate reads no
environment, keeps no global state and prints nothing.

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

## API

| Item | What |
| --- | --- |
| `Link` | `new(carrier, role, settings)` starts the task of the link on the current tokio runtime; `open`, `accept`, `close` (a `GOAWAY` after the queued frames), `closed` (wait for the end and its reason), `ended`, `streams`, `rtt` (of the last `PING`), `role`. A drop ends the link at once |
| `Stream` | `AsyncRead` and `AsyncWrite`; `id`, `reset(code)`, `buffered`. A shutdown sends `CLOSE`; a drop sends `CLOSE` and, when the peer did not close its side, `RESET` with `CANCEL` |
| `Role` | `Dialer` (odd ids) or `Listener` (even ids) |
| `Settings` | the windows, the stream limit, the frame size, the keepalive (below); `check` |
| `Error` | each failure by name; `class` (for the exit code), `io_kind`; into `std::io::Error` (a stream's error) and into `pod_core::Failure` (the stage `Session`) |
| `ResetCode`, `GoAwayCode` | `CANCEL`, `REFUSED`, `PROTOCOL`; `NORMAL`, `PROTOCOL`. A caller's own reset codes start at 16 |
| `Carrier`, `Listener`, `Facts`, `Accepted` | the trait of a carrier: `name`, `facts` (ordered, reliable, duplex, unit), `available`, `dial`, `listen`; the accept side: `accept`, and `accept_peer` (the stream and the Ed25519 key that the peer proved, `None` for a carrier that proves no key); `BoxIo` is the byte stream that they give |
| `CarrierError`, `CarrierErrorKind` | `Usage` (64), `InUse` and `Unavailable` (69), `NotFound` (66), `Refused` (77), `Io`; into `pod_core::Failure` (the stage `Connect`) |
| `memory::Memory`, `memory::pair` | the memory carrier: listeners by name in the value (a clone shares them), a ring of 64 KiB to 64 MiB in each direction (1 MiB by default); a full ring makes the writer wait |
| `unix::Unix`, `unix::pair` | the unix socket carrier (`cfg(unix)`): a path of 1 to 107 bytes, a socket of mode 0600, the claim of the path, the peer's uid; a socket pair for one process |
| `tcp::Tcp` (feature `tcp`) | the tcp carrier: `new(provider, identity, way)`, `with_clients(keys)`, `clients`; the fields `connect_timeout` (20 s), `handshake_timeout` (10 s), `max_handshakes` (64); `address(text)` (the key and the target of `KEY@HOST:PORT`); `connect(&target, &key)` (the stream and the key of the listener); `bind(addr)`; as a `Carrier`: `dial("KEY@HOST:PORT")`, `listen("IP:PORT")` |
| `tcp::TlsListener` | `local_addr`, `accept_key` (the stream and the key of the client); a `Listener` (`accept_peer` gives the key); a drop ends its handshakes under way |
| `ws::Ws` (feature `ws`) | the ws carrier: `new(provider, identity, way)`, `with_clients(keys)`, `clients`; the fields `connect_timeout` (20 s), `handshake_timeout` (10 s, the TLS and the upgrade of a dial), `serve` (the deadlines and the caps of the accept loop of a listener, `pod_ws::serve::Serve`); `address(text)` (the key and the URL of `KEY@wss://HOST[:PORT]/PATH`); `connect(&url, &key)`; `bind(addr)`; as a `Carrier`: `dial("KEY@wss://HOST[:PORT]/PATH")`, `listen("IP:PORT")` |
| `ws::WsListener` | `local_addr`, `accept_key`; a `Listener`; a drop ends its accept loop |
| `ws::{NAME, ALPN, USER_AGENT, CONNECT_TIMEOUT, HANDSHAKE_TIMEOUT}` | the constants of the ws carrier |
| `h2::H2` (feature `h2`) | the h2 carrier: `new(provider, identity, way)`, `with_clients(keys)`, `clients`; the fields `connect_timeout` (20 s), `handshake_timeout` (10 s: TLS, the prefaces, the POST and its 200), `max_handshakes` (64); `address(text)` (the key and the `PostUrl` of `KEY@https://HOST[:PORT]/PATH`); `connect(&url, &key)`; `bind(addr)`; as a `Carrier`: `dial`, `listen("IP:PORT")` |
| `h2::H2Listener`, `h2::PostUrl` | `local_addr`, `accept_key`; a `Listener`; a drop ends its handshakes under way. `PostUrl { host, port, path }`: `parse`, `authority`, `target` |
| `h2::{NAME, ALPN, CONNECT_TIMEOUT, HANDSHAKE_TIMEOUT, MAX_HANDSHAKES, WINDOW}` | the constants of the h2 carrier |
| `tcp::{Keys, Refusal, refusal}`, `tcp::{NAME, ALPN, CONNECT_TIMEOUT, HANDSHAKE_TIMEOUT, MAX_HANDSHAKES}` | the shared set of client keys, the refusal of a key with the key seen, its finder in an error (from `pod_tls::peer`); the constants |
| `frame` | `Frame`, `Decoder` (`new`, `with_preface`, `push`, `next`, `buffered`), `FrameError`, `PREFACE`, `HEADER`, `MAX_RECORD`, `kind` |

## Features

| Feature | What |
| --- | --- |
| none (the default) | the link, the frames, the unix and the memory carriers; no TLS crate in the build |
| `tcp` | the tcp carrier (`tcp`): `pod-tls`, rustls and tokio-rustls, with the provider of the caller, and the `async` feature of `pod-core` for the way out |
| `ws` | the ws carrier (`ws`): `tcp`, and `pod-ws` with its feature `serve` |
| `h2` | the h2 carrier (`h2`): `tcp`, h2, http and bytes, and `pod-ws` for the byte stream of an HTTP/2 stream |

## The wire

Each end first writes the preface `PODLINK` and the byte 1. Then each
frame is a header of 8 bytes (the type, a stream id of 32 bits, a body
length of 24 bits; big endian) and its body:

| Type | Frame | Id | Body |
| --- | --- | --- | --- |
| 1 | `OPEN` | the new stream | none |
| 2 | `DATA` | a stream | 1 byte to 1 MiB |
| 3 | `WINDOW` | a stream, or 0 for the link | the credit, 4 bytes, not 0 |
| 4 | `CLOSE` | a stream | none: the sender writes no more |
| 5 | `RESET` | a stream | the code, 4 bytes |
| 6 | `PING` | 0 | 8 bytes |
| 7 | `PONG` | 0 | the 8 bytes of the `PING` |
| 8 | `GOAWAY` | 0 | the code, 4 bytes |

The tcp carrier puts TLS 1.3 under these bytes, with the ALPN
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
the end of the link sends a Close 1000.

The h2 carrier puts TLS 1.3 with the ALPN `h2` under one stream of
HTTP/2: the dialer sends `POST https://HOST/PATH` with no end of its
body, and the listener answers 200 with no end of its body. The listener
sends its SETTINGS after it took the key of the dialer, and the dial
reads them before it gives the stream, so a refused key is an error of
the dial. Each window (the stream's, the connection's) is 4 MiB.
END_STREAM is the half close; a second request on the connection gets
`REFUSED_STREAM`, and a request that is not a POST gets 405.

A frame is the record that the link gives its carrier: at most 1 MiB of
body. A carrier with a smaller unit splits a record itself. A peer that
breaks a rule (an unknown type, a length that does not fit its type, a
body over 1 MiB, bytes past a window, an id of the wrong parity or used
again, a frame of a stream that never existed) ends the link: it gets a
`GOAWAY` with the code `PROTOCOL`. A frame of a stream that existed and is
gone is dropped, and its bytes go back to the link's credit.

## Settings

| Field | Default | Limits | What |
| --- | --- | --- | --- |
| `stream_window` | 1 MiB | 16 KiB to 8 MiB | the largest receive window of a stream. A stream starts at 16 KiB (`INITIAL_WINDOW`, part of the protocol); its window doubles each time its reader empties its buffer when credit goes back |
| `link_window` | 8 MiB | 16 KiB to 8 MiB, not less than `stream_window` | the most bytes of all streams that this end holds unread: the memory of a link. Credit goes back when the reader takes the bytes |
| `max_streams` | 256 | 1 to 4096 | the streams of the link at this end, from both ends; an `OPEN` over it gets `RESET` with `REFUSED` |
| `frame_size` | 16 KiB | 1 KiB to 1 MiB | the largest `DATA` body that this end sends |
| `keepalive` | 10 s | more than 0 | the silence before this end sends a `PING` |
| `dead_after` | 30 s | more than `keepalive` | the silence after which the link ends with `Error::Dead` |

A stream that stops reading holds at most its own window of the link's
window, and the other streams use the rest: with the defaults, seven
stalled streams that grew to 1 MiB leave 1 MiB to the others, and a
stream that stalls before it grows holds 16 KiB. The writer of a link
queues at most 4 frames (at least 64 KiB) of `DATA`, so that a credit and
a `PONG` do not wait behind a long run of bytes. A peer that sends more
than 8192 frames that wait for an answer, and does not read, ends the
link.

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
- [x] The memory carrier: a ring of 64 KiB to 64 MiB (1 MiB by default), a
  full ring makes the writer wait and drops nothing, the end of one side
  is the end of stream of the other: `tests/carriers.rs`.
- [x] The resume of a link over `pod-session` (podnet T-263): the carrier
  of the session is cut twice while two streams move 1 MiB each way, and
  the SHA-256 of each stream is equal at both ends: `tests/resume.rs`
  (`two_streams_cross_two_resumes_of_the_session_whole`).
- [ ] The ARQ for a carrier that loses frames, with 3 % of the frames
  lost (podnet T-277): T-310.

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
  `the_listener_refuses_a_client_of_another_key_and_stays_open`.
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

## The adoption map

| podnet | pod-link |
| --- | --- |
| PodUtils/podnet@T-262: the session of streams, credits and keepalive | `Link`, `Stream`, `Settings`, `frame` |
| PodUtils/podnet@T-264: the registry of carriers with an `available()` reason | stays in podnet (call C117); each row holds a `Box<dyn Carrier>` and adds podnet's facts (cost, privilege, build feature) |
| PodUtils/podnet@T-274: the unix socket carrier; `bind_private` and `claim_unix_path` of `PodUtils/podnet:crates/podnet-netns/src/listen.rs`; `--allow-other-uid` | `unix::Unix` (`allow_other_uid`), `unix::pair` |
| PodUtils/podnet@T-275: the memory carrier | `memory::Memory`, `memory::pair` |
| PodUtils/podnet@T-268: the TCP carrier with TLS 1.3 and the pin of each key | `tcp::Tcp` (feature `tcp`): one row of the registry holds `Box::new(Tcp::new(provider, identity, way).with_clients(keys))`; podnet's dialer (its proxy and egress) is the `way`; `available()` is the reason of the row; the accept side is `listen("IP:PORT")` and `accept_peer()`, whose `peer` podnet compares with `peer:FINGERPRINT`; a refused key is `CarrierErrorKind::Refused` (77) and `tcp::refusal(&error)` gives the key seen |
| PodUtils/podnet@T-269: the WebSocket carrier, its server half | `ws::Ws` (feature `ws`): one row of the registry holds `Box::new(Ws::new(provider, identity, way).with_clients(keys))`; the dial address is `KEY@wss://HOST[:PORT]/PATH`; the listener is `listen("IP:PORT")` on the accept loop of `pod-ws`, with podnet's deadlines and caps in `Ws::serve` |
| PodUtils/podnet@T-270: the HTTP/2 carrier, one bidirectional POST | `h2::H2` (feature `h2`): one row of the registry holds `Box::new(H2::new(provider, identity, way).with_clients(keys))`; the dial address is `KEY@https://HOST[:PORT]/PATH`; the listener is `listen("IP:PORT")`. podnet's `body_reader` and `body_writer` are `pod_ws::h2_road::duplex` here |
| exit 64, 69, 77 of a carrier | `CarrierError::class().exit_code()` |
| exit of a link | `Error::class().exit_code()`: a dead link 75, a broken protocol 76 |
