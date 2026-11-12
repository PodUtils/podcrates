# pod-turn

STUN's codec (RFC 8489) with the long-term credentials of MD5 and of
SHA-256, and a TURN client (RFC 8656) over UDP and over TLS on TCP.
GitHub #100 asked for it, for PodUtils/podnet#485 (rules TURN-1 to TURN-7)
and PodUtils/podssh@T-621. T-377 made it: the codec moved here from
`pod-proxy` (one codec in the tree, TURN-1), and the client is new code
(calls C160 to C164). The crate reads no variable of the environment,
keeps no process state, prints nothing, holds no service and mints no
credential: the server, the credentials, the dialer, the TLS context, the
clock and the UDP socket are the caller's.

## Features

| Feature | What | Dependencies |
| --- | --- | --- |
| `stun` | `pod_turn::stun` (the codec) and `pod_turn::md5`: pure Rust, no async, no TLS | `hmac`, `sha1`, `sha2` |
| `turn` (default) | `pod_turn::Client` and its settings, on tokio | `stun`, `pod-core`, `pod-tls`, `tokio`, `tracing`, `zeroize` |

The workspace entry has no default feature: `pod-proxy` takes
`features = ["stun"]` alone, and `pod_proxy::stun` is `pod_turn::stun`. A
consumer that takes the crate by its tag with no `default-features =
false` gets the client.

## Use

```rust
use std::sync::Arc;
use pod_turn::{Client, Credentials, Event, Settings, SystemUdp};

let server: pod_core::Target = "turn.example:3478".parse()?;
let credentials = Credentials::new(user, password);      // the caller's; never printed
let source = Arc::new(SystemUdp::with_resolver(resolver)); // the caller's resolver
let mut settings = Settings::udp(server, credentials, source);
settings.software = Some(format!("podnet/{VERSION}"));
let mut client = Client::allocate(settings).await?;     // 401, then the Allocate with the key
println!("relayed {}", client.relayed_address());       // the address that peers send to
client.create_permission(peer).await?;
client.send_to(peer, b"query").await?;                  // a Send indication
let channel = client.bind_channel(peer).await?;         // 0x4000 and up
client.send(channel, b"query").await?;                  // ChannelData
loop {
    match client.recv().await? {                        // refreshes on its timer while it waits
        Event::Data { peer, channel, data } => { /* data of a peer */ }
        Event::RefreshFailed { kept, error } => { /* the allocation, a permission or a channel */ }
        Event::ServerError { method, code, reason } => { /* an error that no request waits for */ }
    }
}
client.close().await?;                                  // a Refresh with a lifetime of 0
```

Over TLS: `Settings::tls(server, credentials, dialer, context)` with the
caller's `Arc<dyn pod_core::Dial>` and `Arc<pod_tls::Context>` (the
provider and the roots); `Transport::Tls { trust, server_name, .. }`
changes the roots or the name to check.

## API

| Item | What |
| --- | --- |
| `stun::{parse, decode, DecodeError}` | a whole message, each attribute's length checked before its value is read; `parse` gives the text, `decode` the type of the message (`kind`), the offset where the reading stopped and why |
| `stun::{Message, message, binding_request, binding_success, read_address, write_address}` | the message, its writer, the Binding of a doctor, the XOR addresses of IPv4 and IPv6 |
| `stun::{message_type, class_of, method_of, Class}` | the type of a method and a class, and back |
| `stun::{signed, integrity, IntegrityError, long_term_key}` | MESSAGE-INTEGRITY (HMAC-SHA1) and the MD5 key (RFC 8489, 9.2.2, 14.5) |
| `stun::{signed_sha256, integrity_sha256, sha256_key, PasswordAlgorithm, password_algorithms, write_password_algorithms, integrity_of}` | MESSAGE-INTEGRITY-SHA256 (a HMAC of 16 to 32 bytes is checked), the SHA-256 key, PASSWORD-ALGORITHM(S) (RFC 8489, 14.6, 14.11, 14.12) |
| `stun::{fingerprinted, fingerprint_ok}` | FINGERPRINT (RFC 8489, 14.7) |
| `stun::{nonce_features, NONCE_COOKIE}` | the 24 bits of a nonce that starts with `obMatJos2` (RFC 8489, 9.2), as they are |
| `stun::{ALLOCATE, REFRESH, SEND, DATA_METHOD, CREATE_PERMISSION, CHANNEL_BIND, CONNECT, ..}` and the attributes | the methods and the attributes of RFC 8489, RFC 8656 and RFC 6062 |
| `md5::md5` | MD5 (RFC 1321): the MD5 key, and `pod_proxy::digest::md5` |
| `Client::allocate(settings)` | the transport, the 401 challenge, the Allocate (UDP relay, the lifetime of the settings) |
| `Client::{refresh, create_permission, bind_channel, send, send_to, recv, close}` | Refresh, CreatePermission, ChannelBind (a peer keeps its number), ChannelData, a Send indication, the next event, the end of the allocation |
| `Client::{relayed_address, mapped_address, lifetime, username, next_refresh}` | what the server gave, and the next refresh with what it keeps (`Kept`) |
| `Settings` (`udp`, `tls`, `check`), `Transport` (`Udp`, `Tls`), `Credentials`, `Limits`, `Clock`, `TokioClock`, `UdpSource`, `SystemUdp`, `Opened` | the settings (below) |
| `Event` (`Data`, `RefreshFailed`, `ServerError`), `Kept` | what `recv` gives |
| `Error` (`class`, `code`; into `pod_core::Failure` at the stage `Relay`) | `Usage`, `Connect` (the dial, TLS, the UDP socket), `Io`, `Closed`, `Timeout { method }`, `Decode` (the type and the offset), `Frame` (a TLS stream out of step), `Protocol { kind }`, `Integrity { kind }`, `Server { method, code, reason }`, `StaleNonce { method, tries }`, `Realm`, `NoRandom`, `NoAllocation`, `NoChannel`, `NoPermission`, `NoChannelNumber`, `TooLarge` |

## Settings

| Field | Default | What |
| --- | --- | --- |
| `server` | none | `pod_core::Target` of the server |
| `transport` | none | `Udp(Arc<dyn UdpSource>)`, or `Tls { dialer, context, trust, server_name }` (`Trust::Default` and the host of `server` from `Settings::tls`) |
| `credentials` | none | the user and the password after their preparation (OpaqueString); `Debug` shows the user alone; the password is wiped at the drop |
| `realm` | `None` | the realm that the caller expects; another one fails before a key is made |
| `limits.rto`, `limits.retransmits` | 500 ms, 6 | the first wait over UDP, which doubles, and the sends after the first (RFC 8489, 6.2.1) |
| `limits.request_timeout` | 39.5 s | the longest wait for one answer, each transport |
| `limits.connect_timeout` | 10 s | the dial and the TLS handshake |
| `limits.lifetime` | 600 s | the lifetime that Allocate and Refresh ask for |
| `limits.refresh_before` | 60 s | a refresh comes this long before the end, and not before half of the lifetime |
| `limits.stale_nonces` | 2 | the 438 answers that one request answers with the fresh nonce |
| `limits.queue` | 256 | the events that wait while a request waits; more data is dropped |
| `clock` | `TokioClock` | `now` and `sleep_until` of each deadline and refresh |
| `entropy` | `OsEntropy` | the transaction ids |
| `software` | `None` | SOFTWARE of each request: the consumer's name |

## The rules of the client

- **The credentials.** The first Allocate goes with no credentials; the
  401 gives REALM and NONCE (and PASSWORD-ALGORITHMS). The client takes
  SHA-256 when the challenge offers it, else MD5, sends the offer and its
  choice back with each request, and signs with the integrity of its
  choice (call C163). A 438 gives a fresh nonce: the same request goes
  again with the same user and key, at most `stale_nonces` times. A 401
  to a signed request fails. The user and the password of a live
  allocation never change: a server that answers 441 fails the request,
  and a new credential needs a new `Client::allocate`.
- **The answers.** An answer goes with its request by its transaction id.
  A success to a signed request needs the integrity of the chosen
  algorithm; one with another key or with none fails (`Error::Integrity`).
- **The refresh.** `recv` refreshes the allocation, each permission (300
  s) and each channel (600 s) when it is due, as it waits (TURN-2). A
  refresh that fails is an event: a permission or a channel is then
  dropped; the allocation is tried again at half of its time left, and it
  is gone at its end or at a 437.
- **The wire.** Over UDP, a datagram of another sender is dropped. Over
  TLS, each message is cut by its own length; ChannelData is padded to a
  multiple of 4 each way (RFC 8656, 12.5). Bytes that are neither STUN nor
  ChannelData end the client (`Error::Frame`).

## The parity list

The asks of GitHub #100 and the rules of PodUtils/podnet#485 that touch
this crate, and the tests that hold them:

- [x] One codec, moved with its tests: `src/stun_tests.rs`,
  `src/stun_auth_tests.rs` (RFC 5769, 2.1 to 2.4), the plants
  `proxy-stun-length-trusted` and `proxy-stun-integrity-length-kept`
  (their names kept); `pod-proxy` has no copy; `pod_proxy::stun` and
  `pod_proxy::digest::md5` are re-exports (calls C160, C161), and the
  tests of `pod-proxy` and `pod-services-watch` pass unchanged.
- [x] The features `stun` and `turn`; the codec with no tokio and no TLS:
  `examples/static-probe-lean` links it, and `scripts/check-probe.py`
  refuses a tokio in its tree.
- [x] PASSWORD-ALGORITHM(S), the SHA-256 key and
  MESSAGE-INTEGRITY-SHA256, against a vector computed apart with Python's
  `hashlib` and `hmac`: `src/stun_sha256_tests.rs`; the plant
  `turn-sha256-integrity-length-kept`.
- [x] A decode error names the type and the offset: `decode`,
  `tests/turn_udp.rs` (`a_bad_answer_names_its_type_and_offset_*`); the
  old texts of `parse` stay, with the offset and the type after them.
- [x] Over UDP and over TLS on TCP: Allocate, Refresh, CreatePermission,
  ChannelBind, a Send echo, a ChannelData echo (padded on TLS), the 401,
  the 438, and a 441 of a changed user: `tests/turn_udp.rs` (12 tests),
  `tests/turn_tls.rs` (4 tests), against the mock of `tests/common/`; the
  plants `turn-stale-nonce-not-taken`, `turn-channel-data-not-padded`.
- [x] The client never changes its user; a 441 fails, and its refresh is
  an event: `a_changed_user_gets_441_*`, `a_request_of_another_user_*`.
- [x] A forged answer is refused: `an_answer_with_a_wrong_integrity_*`;
  the plant `turn-answer-integrity-unchecked`.
- [x] The refresh on a timer, not after a failure (TURN-2):
  `the_client_refreshes_on_its_timer`; the plant
  `turn-refresh-not-on-timer`.
- [x] The credential stays out of each line (TURN-5): `Debug` of
  `Credentials`, `Settings` and `Client` shows the user alone; the crate
  prints nothing.
- [x] The lifetime for `status` (TURN-6): `Client::lifetime`,
  `next_refresh`. The relay's rate limit is no part of TURN: the consumer
  measures it.
- [x] A live test with a server of the caller, which skips with no server:
  `tests/live.rs` (`POD_TURN_LIVE_SERVER`, `POD_TURN_LIVE_USER`,
  `POD_TURN_LIVE_PASSWORD`, `POD_TURN_LIVE_PEER`); no public server is
  written here. A workflow that runs it against coturn on the loopback is
  T-380.
- [x] The fuzz target `stun_message` (`fuzz/`), and the static probe line.
- [ ] Plain TURN over TCP (RFC 8656, 3.1, with no TLS): not in this crate;
  GitHub #100 puts it out of this change.
- [ ] TURN over DTLS (RFC 7350): not in this crate; GitHub #100 puts it
  out of this change.
- [ ] The TCP relay of RFC 6062 (`Connect`, `ConnectionBind`,
  `ConnectionAttempt`): the constants are here; the steps are not.
  PodUtils/podnet#485 tracks it, and PodUtils/podssh@T-621 can add them on
  an allocation of this crate.
- [ ] The options of RFC 8489 and RFC 8656 that the client does not use
  (the security feature bits of a nonce, USERHASH, ALTERNATE-SERVER, the
  ALPN `stun.turn` of RFC 7443, EVEN-PORT, RESERVATION-TOKEN,
  DONT-FRAGMENT, ADDITIONAL-ADDRESS-FAMILY): T-381.
- [ ] A TURN server as a fact of another party with a watch (TURN-4): the
  consumer's, in `pod-services`, when it names a server.

## The adoption map

| Consumer | Old | New |
| --- | --- | --- |
| podssh | `podssh_ws::stun` (a re-export of `pod_proxy::stun`), the doctor of `PodUtils/podssh:crates/podssh-cli/src/doctor/nat.rs` | no change: `pod_proxy::stun` keeps each name; a tag bump. `parse` gives the same texts, with the offset and the type after them |
| podnet | no TURN (PodUtils/podnet#485) | `pod-turn = { git = .., tag = .. }` with the default feature `turn`: `Settings::udp` or `Settings::tls` over its own dial, resolver and TLS context; `Client` under its forward of UDP |
| podssh | PodUtils/podssh@T-621, the TURN road | the same; the steps of RFC 6062 on its allocation stay podssh's |

## Tests

`cargo test -p pod-turn --all-features`: the codec (19 unit tests), the
mock over UDP (`tests/turn_udp.rs`) and over TLS (`tests/turn_tls.rs`);
`tests/live.rs` is `#[ignore]`. The mock (`tests/common/mock.rs`) is its
own reading of RFC 8656: a session for each UDP 5-tuple or TLS
connection, a relay socket on the loopback for each allocation, the 401,
the 438 on request of a test, a 441 for a user who is not the owner, and
the users and the realm of the tests (made-up values).
