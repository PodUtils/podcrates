# pod-gsr

The GSRN relay of gsocket (hackerschoice) as a crate. Two peers that share
one secret meet at an address that the secret gives, and the relay carries
the bytes between them. The session is TLS with SRP: the relay learns no
secret, and neither peer needs a certificate or an account.

The facts of the relay are the data file `hackerschoice-gsr` of
`pod-services` (`crates/pod-services/services/hackerschoice-gsr.toml`):
the domain and the port, the keys of the two derivations, the SRP user,
group and cipher, the shape of the password, the longest secret, the
version of the protocol, the ping interval and the idle limit.
`Settings::of_data()` reads the built-in file; `Settings::of(service)` reads
a `Service` of the caller's own set (an overlay of `pod-services`). The
crate reads no variable, keeps no process state and prints nothing. Each
failure is a `pod_core::Failure` with its stage and its class.

## Use

```rust
use pod_gsr::{Role, Settings};

let settings = Settings::of_data()?;
let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
let mut entropy = pod_core::entropy::OsEntropy;
// The dialer is the caller's: direct, or through a proxy (pod-proxy).
let mut stream = pod_gsr::open(&*dialer, &settings, Role::Listen, secret, &mut entropy, deadline).await?;
// `stream` is AsyncRead and AsyncWrite: the bytes of the peer.
```

A caller with no tokio runtime takes the feature `blocking`:

```rust
let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
let mut stream = pod_gsr::blocking::open(dialer.as_async(), &settings, Role::Connect, secret, &mut entropy, deadline)?;
stream.set_timeout(Some(std::time::Duration::from_secs(30)));
// `stream` is std::io::Read and Write; `stream.end()` after a read of 0 bytes.
```

## API

| Item | What |
| --- | --- |
| `open(dial, settings, role, secret, entropy, deadline)` | dial the shard, send `LISTEN` or `CONNECT`, wait for the peer, send `ACCEPT`, and run the TLS handshake; gives a `Stream` (feature `async`) |
| `Role` | `Listen` registers the address and waits; `Connect` asks for a registered peer |
| `Settings::of_data()`, `Settings::of(service)`, `SERVICE` | the endpoint and the facts of the service as one value; a missing or mistyped value, or a shard count outside 1 to 26, is a `Failure` of the class `Config` |
| `Settings` fields | `domain`, `port`, `sharded`, `shards`, `addr_key`, `srp_key`, `password_chars`, `srp_user`, `srp_group`, `cipher`, `secret_prefix`, `secret_suffix`, `secret_max`, `version`, `ping`, `idle` |
| `MIN_PING` | the longest interval between two `PING` messages (5 s) |
| `Identity`, `ADDR_LEN` | the address, the SRP password and the shard of a secret; `Debug` shows the address alone |
| `Stream` | `AsyncRead` and `AsyncWrite`; `end()` says how the peer's side ended, `peer_finished()` whether it ended (feature `async`) |
| `End` | `Clean`: the peer sent close_notify; `Cut`: the transport ended with no close_notify |
| `blocking::open(dial, settings, role, secret, entropy, deadline)` | `open` on a private current-thread runtime, with a `std::time::Instant`; inside a tokio task, a `Failure` of the class `Usage` (feature `blocking`) |
| `blocking::Stream` | `std::io::Read` and `Write` on that runtime; `end()`, `peer_finished()`, `shutdown()`, `set_timeout(limit)` (a read past the limit is `TimedOut`, and the stream stays usable); a read or a write inside a tokio task fails with `Unsupported` (feature `blocking`) |
| `Version` | the version of the control protocol |
| `check_tls(settings)` | build the SRP context of each side with no connection, and give the version of the linked OpenSSL: a self-test of a binary |

The caller gives the dialer (`pod_core::Dial`), the random source for the
token of a listener (`pod_core::Entropy`), and the deadline of the dial and
of the wait for the peer. A caller that needs a limit on the TLS handshake
wraps the call in its own timeout.

## Features

| Feature | What |
| --- | --- |
| `async` (default) | `open` and `Stream`, on tokio |
| `blocking` | `blocking::open` and `blocking::Stream`: the same code on a private current-thread runtime that shuts down in the background, as `pod_proxy::blocking` (call C20). With no `async`, `open` and `Stream` are not public |
| none | the core with no tokio: `Identity`, `Settings`, `Role`, `End`, `Version`, `check_tls` |
| `fuzzing` | `wire` is public, with `wire::fuzz_messages` for the fuzz target `gsr_message` (`fuzz/`); the `--cfg fuzzing` of cargo-fuzz does the same. No consumer turns it on |

## Settings

Each field of `Settings` is a fact or the endpoint `default` of the
service file. `Settings::of` checks each one: a text is a text, a number
is in its range, `srp-password-hex-chars` is 64 or less (a SHA-256 digest
in hex), `shard-count` is 1 to 26 (the letters `a` to `z`). `ping` is the
shorter of `ping-interval-ms` and `MIN_PING`. `idle` is the measured idle
limit of a registration (`idle-timeout-ms`); the crate does not use it,
and a caller sizes its own keepalive from it. A stand-in relay in a test
sets `domain`, `port` and `sharded = false`.

## The protocol

The address of a secret is the first 16 bytes of
`SHA-256(addr-key ++ secret)`. The SRP password is the lowercase hex of
`SHA-256(srp-key ++ secret)`, cut to 33 characters. The host is the shard
letter (`'a'` + the sum of the address bytes modulo 26) in front of the
domain: `testsecret123` gives `l.gs.thc.org`.

A listener sends `LISTEN` (128 bytes) with a random 16-byte token at byte
16 and the 16-byte address at byte 32. A connector sends `CONNECT` (128
bytes) with the same address and no flag. The relay answers `START` (32
bytes) when it paired the two ends; the flags of `START` name the side of
the TLS handshake. Both peers send `ACCEPT` (32 bytes), then the TLS
handshake starts. The reader of the relay's messages asks for the bytes of
one message only, so the first byte of the TLS handshake stays in the
socket, and a read that a timeout cancels keeps its bytes.

A peer that waits sends `PING` (32 bytes) at each `ping` interval; the
relay answers `PONG`. A fatal `STATUS` ends the wait with its code: 2 means
that no peer listens (`Unavailable`), 3 means that the relay ended the
registration at its idle limit (`Timeout`).

### The ping interval

The service's own interval is 45 s (`ping-interval-ms`). A listener behind
a proxy does not get 45 s: a proxy that ends an idle `CONNECT` tunnel after
15 s drops the registration before a peer arrives. `Settings` thus pings at
the shorter of the service's interval and `MIN_PING` (5 s).

## TLS

rustls has no SRP cipher suite, so the crate links OpenSSL (call C34 of
`docs/decisions.md`). This is an exception to rule 4 of `docs/design.md`
(the binary gives the rustls provider): the SRP context is OpenSSL's, and
no other crate shares it. The workspace dependency `openssl` has the feature `vendored`:
openssl-src builds OpenSSL 3.6.3 from source, and each binary links it
static. The static probe (`examples/static-probe`) calls `check_tls`, so
CI proves the link on each target.

The context is TLS 1.2 with `SRP-AES-256-CBC-SHA` and the group of the
service (`4096`), with no session tickets and no renegotiation. TLS 1.3 has
no SRP suite. Both sides cap the version at TLS 1.2: on OpenSSL 3.6.3 the
loopback tests pair with this cap, with a cap on the server alone, and with
no cap; the live run through the relay used the cap on both sides. The
server sets the verifier with `SSL_set_srp_server_param_pw`; the client
names the user and gives the password. The password is the text
`secret-prefix`, the derived password and `secret-suffix`; the copy that
OpenSSL reads is wiped when the session drops.

## End of stream and truncation

A read of 0 bytes is the end of the peer's side, and `Stream::end()` then
says how it ended: `End::Clean` when the peer sent close_notify (its
shutdown), `End::Cut` when the transport ended with no close_notify (a
peer that dropped its stream, the relay, or a hop between them). The
crate does not decide what a cut means: a caller that counts its bytes, or
that needs the peer's close_notify, builds its own failure from
`End::Cut`. OpenSSL's shutdown state decides the record: a close_notify
read sets `SSL_RECEIVED_SHUTDOWN`.

Four behaviours here are not obvious. A peer that gets them wrong reports
a complete transfer with a short body.

* **A close_notify is read late.** OpenSSL can report the end of the
  session before it hands over the records that arrived in the same read.
  `poll_read` reads the records that OpenSSL still holds before it reports
  the end.
* **The first side to finish must not close the transport.** The relay
  ends the session when one end closes its connection, while the peer may
  still have bytes to deliver. `poll_shutdown` sends the close_notify at
  once, as `SSL_shutdown` of gsocket does (it is a record that the relay
  carries), and closes the transport only once `peer_finished` is true. A
  later `poll_shutdown` closes it. After a cut, no close_notify goes out.
* **A shutdown ends the writes.** After `poll_shutdown`, a write fails;
  the reads go on until the peer's end.
* **A quiet connection is not a finished one.** The relay can go seconds
  with no byte in the middle of a transfer, so a caller waits on a quiet
  socket and does not take it as an end.

## Tests

* The unit tests (25): the identity vector of `testsecret123`, a shard
  letter for each count, the layout of each message, the reader that stops
  at the end of a message and keeps the bytes of a cancelled read, the wait
  that pings and ends at its deadline, the fatal status, the length of a
  secret, the facts of the data file, the configuration failures (no
  endpoint, no facts, a shard count of 0, 27 or 255), the SRP context of
  each side and `check_tls`.
* `tests/loopback.rs` (6, `need_loopback!`): a stand-in relay on the
  loopback pairs a listener and a connector, and 1 MiB crosses each way;
  different secrets fail the handshake (`Tls`, `Refused`); a connector with
  no listener is unavailable; the end of the peer ends the stream; a drop
  of the peer is `End::Cut`; a shutdown of the peer is `End::Clean` with
  each byte, on both sides.
* `tests/unix_loopback.rs` (9): the same stand-in over an `AF_UNIX`
  listener, for a host that denies `bind(AF_INET)`: the byte path, the
  pairing, a counted read of 1 MiB through a close with no close_notify,
  a shutdown before the peer's end, and the two ends of `End`.
* `tests/blocking.rs` (5, feature `blocking`): two peers on threads with
  no runtime move 1 MiB through the stand-in on a Unix socket, end clean
  and answer; a read past its limit is `TimedOut` and the stream goes on; a
  call inside a runtime is `Usage`; a read inside a runtime fails with no
  panic; a dial that fails is a failure of the dial.
* `tests/live.rs` (1, ignored): 1 MiB through the real relay, with the
  proxy of the environment:
  `cargo test -p pod-gsr --test live -- --ignored`.
* The fuzz target `gsr_message` (`fuzz/`) and its plant
  `scripts/plants/gsr-reader-forgets-bytes` (group `fuzz`).

## Parity

The prototype (`pod-gsr` 0.3.0 of `pod-gsr.zip`) against this crate:

| Prototype | Here |
| --- | --- |
| `data/hackerschoice-gsr.toml`, `build.rs`, `src/data.rs` (the data file travels with the crate and goes through `Catalog::lay`) | [x] the file is in the built-in set of `pod-services`; `Settings::of_data` reads `pod_services::get`; `build.rs` and `data.rs` are gone |
| `Settings::of`, `MIN_PING`, the checks of each fact | [x] the same, and a shard count outside 1 to 26 is `Config`; the field `idle` |
| `Identity` (derivations, shard, host, `Debug`) | [x] the same; the shard letter holds to `a` to `z` for each count |
| `wire` (messages, `Reader`) | [x] the same; public under `fuzzing`, with the driver of the fuzz target |
| `open`, `Role`, the wait, the fatal codes, the words of the OpenSSL stack | [x] the same |
| `srp` (the context of each side, TLS 1.2) | [x] the same; the password copy is wiped; `check_tls` |
| `Stream` (the late close_notify, the shutdown that waits, `peer_finished`) | [x] the same; `end()` records `End::Clean` or `End::Cut` (T-087, GitHub #25); the shutdown sends its close_notify at once and holds the transport open; a full read buffer or a buffer with bytes in it is not an end |
| the tests: 21 unit, 4 TCP, 7 over `AF_UNIX`, 1 live | [x] each test with its name, and 4 new unit tests |

## Adoption map

None: the crate is new, and it has no consumer. The consumers that issue
#24 names (a road of the SSH relay of podssh, a transport of podnet, the
blocking face for podvm) take it by a tag.

## Sources

The code is the owner's prototype `pod-gsr` 0.3.0 (`pod-gsr.zip`, attached
to GitHub #24). The prototype is not a git repository: it is the base of
the import (T-086), and no `SOURCES` block names it. The layout of the
messages, the derivations, the SRP setup and the facts of the data file
come from four files of `hackerschoice/gsocket` at `5950647f` (the default
branch `master`): `SOURCES` lists them, and `scripts/check-sources.py`
reports each change. The same four files are equal at `ad546b2e` (the
commit that GitHub #23 names).
