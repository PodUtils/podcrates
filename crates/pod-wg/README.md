# pod-wg

The WireGuard initiator of WARP, sans I/O: the code of podnet's
`src/warp/wg.rs` that makes and reads the bytes of WireGuard, as
PodUtils/podnet#447 cut it.

The crate is this side of a WireGuard tunnel as the initiator: the handshake
(Noise IKpsk2, with a preshared key or the zero key), the cookie of a
responder under load, the transport messages with the window of their
counters, and the timers of a tunnel (the rekey, its resend, a dead path, the keepalive). It owns no
socket, no task and no clock. The caller sends and receives the bytes, and
gives the time `now` of its monotonic clock to each call that reads a timer.
podnet keeps its driver: the UDP socket of the host or of its userspace
stack, the write half, the read half and the liveness task, with the lock
of the tunnel.

The crate reads no environment variable, prints nothing and keeps no
process state. With no feature, it has no tokio. ChaCha20-Poly1305 is
ring's (call C8), X25519 is curve25519-dalek's, and BLAKE2s and HChaCha20
are the crate's: ring has neither.

## API

| Item | Contents |
| --- | --- |
| `Keys` | `private`, `peer_public`, `reserved` (bytes 1 to 3 of each message: WARP's `client_id`; zero for WireGuard), `preshared` (`PresharedKey` of a wg-quick file; `None` is the zero key); `public()`. `Debug` shows no key. |
| `initiate(keys)` | An `Initiation` with random bytes of the system and the timestamp of the wall clock now. |
| `initiate_from(keys, entropy, timestamp)` | The ephemeral key and the index from the caller's `pod_core::entropy::Entropy`; the timestamp from `Stamps`. |
| `initiate_with(keys, ephemeral, sender, timestamp)` | The same values give the same message. |
| `Initiation` | `message` (148 bytes), `sender()`, `mac1()`, `finish(keys, response, now) -> Session`, `add_mac2(cookie, now)`. Its keys are wiped on drop. |
| `Cookie` | `open(keys, init, reply, now)`: the cookie of a reply to `init`; `fresh(now)`. |
| `Session` | `send_key`, `recv_key`, `local`, `remote`, `send_counter`, `created`; `seal(reserved, packet)` (padded to 16 bytes; an empty packet is a keepalive), `open(message)` (the IP packet, cut at the length of its IP header; empty for a keepalive), `receiver_of(message)`. Its keys are wiped on drop. |
| `Tunnel` | `new(keys, session, stamps, now)`; `rekey(now)`, `send(packets, now) -> Outgoing`, `receive(message, now) -> Received`, `tick(now)`; `keys()`, `current()`, `rekeying()`. |
| `Outgoing` | `handshake` (the initiation of a rekey that is due), `messages` (one for each packet). |
| `Received` | `Packet`, `Keepalive`, `Rekeyed`, `Cookie`, `Ignored`, `Dropped(Error)`. |
| `Stamps` | `new(wall, start)`, `next(now)`: the TAI64N timestamp of an initiation. |
| `MSG_INITIATION`, `MSG_RESPONSE`, `MSG_COOKIE`, `MSG_DATA`; `INITIATION_LEN`, `RESPONSE_LEN`, `COOKIE_REPLY_LEN` | The types and the lengths of the messages. |
| `REKEY_AFTER` (120 s), `REKEY_TIMEOUT` (5 s), `HANDSHAKE_DEAD` (20 s), `KEEPALIVE` (25 s), `COOKIE_AGE` (115 s) | The timers. |
| `Error` | Each failure, with podnet's text; `class()`; `Failure::from`. |
| `standin` (feature `standin`) | The responder of the tests: `respond`, `respond_with`, `respond_preshared` (with a preshared key), `cookie_reply`, `session`, `keys`, `start(reserved, path) -> (Keys, SocketAddr)`, `start_preshared` (with a new preshared key in the `Keys`), `start_under_load` (a cookie reply to each initiation with no mac2 of its cookie), `serve`, `DataPath` (`All`, `NeedsReserved`, `Dead`). On the loopback it answers each handshake, sends each packet back, and answers a TCP SYN with its SYN-ACK. |
| `fuzzing` (feature `fuzzing`) | The readers of a response, a cookie reply and a transport message on fixed keys, for the fuzz targets of `fuzz/`. |

## The timers

`Tunnel` is podnet's `State` with the clock of the caller:

* `rekey(now)`: an initiation when the current session is older than
  `REKEY_AFTER`; the resend of a waiting rekey after `REKEY_TIMEOUT`;
  `Error::Dead` when the first initiation of the rekey had no response in
  `HANDSHAKE_DEAD`.
* `send` seals each packet with the current session, after the rekey that is
  due. `tick` (each second in podnet) gives the rekey that is due and a
  keepalive when nothing went out for `KEEPALIVE`.
* `receive`: a transport message opens with the current or the previous
  session; the response of a rekey makes a new current session and keeps
  the old one as the previous; a cookie reply to a rekey gives mac2 to its
  next resend. A message that fails its check is `Dropped`, and changes
  nothing.

`Stamps` gives each initiation of one tunnel a timestamp past the last: the
wall clock is read once, and the monotonic time since then is added. A step
back of the wall clock does not move a timestamp back, and two initiations
at one instant do not share one; the responder drops an initiation whose
timestamp is not past the last that it took.

## The preshared key

`Keys::preshared` is the psk step of Noise IKpsk2 (the WireGuard paper,
section 5.4.3): `(C, τ, κ) = Kdf3(C, Q)` and `H = Hash(H || τ)` in the
response, after its DH steps. `None` is the zero key of WireGuard. The
initiation does not depend on the key: `Initiation::finish` mixes it in, and
a response of a peer with another key, or with none, fails as
`Error::ResponseNotAuthentic`. No published WireGuard vector has a preshared
key on fixed keys: the vector of the test
(`the_vector_of_a_handshake_with_a_preshared_key`) is the output of an
independent program, the Noise steps written out on Python's hashlib, hmac
and `cryptography` (call C314).

## The cookie

A responder under load answers an initiation with a cookie reply (the
WireGuard paper, section 5.4.7). `Cookie::open` opens it with the mac1 of
the initiation (XChaCha20-Poly1305 under the hash of `cookie--` and the
static key of the responder). While it is fresh (`COOKIE_AGE`, 120 s less
5 s, as the Linux kernel has it), `Initiation::add_mac2` puts its MAC in
each initiation; a resend is not sent at once, it waits for its time. As for
mac1, mac2 is the MAC of the bytes with zero reserved bytes.

## Failures

Each `Error` converts to a `pod_core::Failure` at the stage `Warp`; a caller
of another tunnel moves it with `Failure::at`.

| Error | Class |
| --- | --- |
| `NotResponse`, `OtherHandshake`, `Mac1`, `ResponseNotAuthentic`, `NotCookie`, `CookieNotAuthentic`, `NotTransport`, `OtherSession`, `NotAuthentic`, `Replayed` | `Protocol` |
| `Dead` | `Timeout` |
| `NoRandom` | `Unavailable` |

## Features and settings

* No default feature. `standin` adds the responder of the tests (with
  tokio); `fuzzing` adds the readers of the fuzz targets (with `standin`).
  No binary needs either.
* No setting: the keys, the reserved bytes, the clock, the entropy and the
  wall time are arguments.

## Parity

With podnet `src/warp/wg.rs` and `src/warp/wg/standin.rs` at `54d7ed4`:

* The same bytes on the wire: the initiation, the response, the transport
  messages, mac1 over zero reserved bytes, the padding to 16 bytes, the cut
  at the IP length, the window of 2048 counters (`the_replay_window`).
* The same timers and texts of each failure.
* The tests `blake2s_vectors`, `hmac_and_kdf_vectors`,
  `a_handshake_and_transport_messages` and `the_replay_window` keep their
  names. `no_answer_is_an_error_that_names_the_door`,
  `a_tunnel_over_udp_carries_packets_both_ways` and
  `wireguard_probe_renews_a_dead_leg` stay in podnet: they run its driver.

The differences, each a fix or a new part:

* A response that fails its check keeps the times of the waiting rekey.
  podnet's `WgRx` set them to now: a peer that answered with bad responses
  held a dead path open, and each one delayed the resend
  (`a_bad_response_keeps_the_dead_time`; plant
  `wg-bad-response-resets-the-dead-time`).
* The age of the session reads the given `now`; podnet's `State::rekey`
  read the clock of the system for it (`the_timers_of_a_tunnel`).
* The timestamps never go back (`Stamps`); podnet read the wall clock at
  each initiation (`stamps_only_go_forward`; plant `wg-timestamp-goes-back`).
* The cookie reply and mac2 are new; podnet took a cookie reply as the error
  "under load" and sent no mac2, so a responder under load refused each
  handshake (`a_cookie_reply_puts_mac2_in_the_resend`; plant
  `wg-cookie-mac2-missing`).
* No random bytes of the system is `Error::NoRandom`, not a panic. The keys
  of an initiation, a session and a cookie are wiped on drop.
* The stand-in waits on after an empty datagram; podnet's read its type
  byte, and a panic ended its task
  (`an_empty_datagram_does_not_stop_the_standin`).
* The anyhow errors are `Error`, with the same texts.

## Adoption map: podnet

| podnet at `54d7ed4` | With pod-wg |
| --- | --- |
| `src/warp/wg.rs`: BLAKE2s, HMAC, the KDF, X25519, `seal`, `open`, `tai64n`, `random` | `pod-wg` (private) |
| `wg::Keys`, `wg::Initiation`, `wg::initiate`, `wg::Session`, `MSG_*`, `INITIATION_LEN`, `RESPONSE_LEN`, `REKEY_AFTER`, `REKEY_TIMEOUT`, `HANDSHAKE_DEAD`, `KEEPALIVE` | the same names of `pod_wg`, which `src/warp/wg.rs` re-exports |
| `Initiation::finish(keys, response)` | `finish(keys, response, now)` |
| `State` and `State::rekey` | `pod_wg::Tunnel` in the `Arc<Mutex<..>>` of podnet's halves: `send`, `receive`, `tick` |
| the cookie reply as an error | `Cookie::open` and `Initiation::add_mac2` in `connect_on`; `Received::Cookie` in `WgRx` |
| `Peer`, `Peer::from_exported`, `Sock`, `connect`, `connect_inside`, `connect_on`, `WgTx`, `WgRx`, `WgKeepalive` | stay in `src/warp/wg.rs` |
| `src/warp/wg/standin.rs` | `pod_wg::standin` (dev-dependency with the feature `standin`); podnet's `standin.rs` keeps `start` (it makes the `Peer` of `pod_wg::standin::start`) and its two tests of the driver, and gets a third: `connect` passes a front door under load (`start_under_load`) |
| `ring`, `curve25519-dalek` of `wg.rs` | in pod-wg; podnet keeps both for its other code |

## Tests

`cargo test -p pod-wg --all-features`: 22 tests (the vectors of BLAKE2s, its
HMAC and the KDF, of HChaCha20, XChaCha20-Poly1305 against RustCrypto's;
the handshake and the transport messages; the mac1 and the payload of a
response; an initiation from known values; the preshared key on both sides,
on one side and two keys, and its vector; the cookie; the timers, a rekey
with the previous session, a bad response, a cookie in a rekey; the
stand-in on the loopback, also under load and with a preshared key; the
classes of the errors). The fuzz targets
`wg_response`, `wg_cookie` and `wg_transport` (`fuzz/`). The plants
`wg-mac1-unchecked`, `wg-replayed-counter-taken`, `wg-timestamp-goes-back`,
`wg-bad-response-resets-the-dead-time`, `wg-cookie-mac2-missing`,
`wg-psk-step-skipped`.
