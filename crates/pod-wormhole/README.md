# pod-wormhole

The client of a magic-wormhole mailbox, for a rendezvous by a short code: a
person reads a code to another person, and the two ends meet at a mailbox (a
WebSocket that needs no account) and move what they must with no secret that
the mailbox sees. The ends run SPAKE2 keyed by the whole code over the
mailbox, then seal each later message in NaCl's SecretBox under a key of the
shared key, as magic-wormhole's own client does, so that a peer of its
Python library and this crate meet. When no mailbox answers, the ends can
meet at the topic of the code at a public MQTT broker on port 443 instead
(`mqtt`), with the same messages.

The base is podssh's `crates/podssh-relay/src/code/` (the mailbox client,
the wormhole, the code and its word list). What the ends say once they hold
a key (the pairing exchange of podssh, the transfer of podnet) is the
caller's protocol over a `Wormhole`; the exchange of podssh goes to
`pod-relay` (T-024).

## API

| Module | What |
| --- | --- |
| `config` | `Config { appid, client_name, client_version, way, trust, connect_timeout, answer_limit, limits, events }`, `Config::new(appid, client_name, client_version, way)`, `CONNECT_TIMEOUT`, `ANSWER_LIMIT`, `LIMITS` |
| `mailbox` | `Mailbox`: `connect(&url, &config, side)`, `list`, `allocate`, `claim`, `open`, `add`, `release`, `close`, `end`, `message(wait)`, `appid`, `shown`, `motd`; `Mood` (`Happy`, `Lonely`, `Scary`, `Errory`); `Message`; `crowded(&Failed)`; `ANSWER_LIMIT`, `KEPT_MOST` |
| `wormhole` | `Wormhole`: `open(place, side, nameplate, id, &code, versions)` (a `Mailbox` or a `mqtt::Broker`), `agree(wait)`, `confirm(wait)`, `send`, `receive(wait)`, `close(mood)`, `fail(mood, failed)`, `shown`, `derive_key(purpose, len)` (magic-wormhole's `derive_key` of the agreed key: the transit key is `derive_key("APPID/transit-key", 32)`); `derive_key(&key, purpose, len)`, `phase_key`, `seal`, `open_sealed`, `NONCE`, `TAG`, `HELD_MOST`, `DERIVED_MOST` |
| `meet` | `Ends { mailboxes, config }`, `opened(&ends, versions)` (a new code at the first mailbox that answers), `joined(&ends, &code, versions, giver_wait)` (the giver's wormhole, the key made), `or_close` |
| `mqtt` | `Brokers { points, config, resend }`, `Brokers::new(points, config)`, `Point { url, login }`, `Login { user, password }` (no `Debug` shows the password), `opened(&brokers, versions)` (a new code at the first broker that answers, the giver subscribed), `joined(&brokers, &code, versions, giver_wait)` (the taker's wormhole, the key made), `topic(appid, nameplate)`, `nameplate(&mut dyn Entropy)`, `Role` (`Giver`, `Taker`; `topics`), `PROTOCOL`, `KEEP_ALIVE`, `RESEND`, `NAMEPLATE_DIGITS`, `PAYLOAD_MOST`, `TO_GIVER`, `TO_TAKER` |
| `mqtt::board` | `Board { point, config }` (also `mqtt::Board`): `new(point, config)`, `put(topic, payload)`, `take(topic, wait)` (`Ok(None)`: no note within the wait), `clear(topic)`, `shown`; `check_topic`, `check_payload`, `TOPIC_MOST` |
| `mqtt::broker` | `Broker`: `connect(&point, &config, side, topic, role, resend)`, `add`, `message(wait)`, `disconnect`, `end`, `appid`, `inbox`, `shown`; `client_id`, `refusal`, `PING_EVERY`, `KEPT_MOST` |
| `mqtt::packet` | the pure codec of MQTT 3.1.1: `Packet` (`Connect`, `ConnAck`, `Publish` with its `retain` flag, `Subscribe`, `SubAck`, `PingReq`, `PingResp`, `Disconnect`), `Connect`, `encode`, `decode(bytes, most)`, `encode_length`, `kind_name`, `Malformed`, `LENGTH_MOST`, `REFUSED` |
| `place` | `Place` (`Mailbox`, `Broker`; `From` each): `shown`, `noun`, `appid`, and the steps of a wormhole at either |
| `phrase` | `Code`: `make(nameplate, &mut dyn Entropy)`, `parse(typed)`, `nameplate`, `password`, `for_terminal`, `words`; `nameplate_ok`, `WORDS`, `WORDS_MOST`, `NAMEPLATE_MOST`, `TYPED_MOST` |
| `words` | the PGP word list (`EVEN`, `ODD`, `list`, `word`, `find`) |
| `message` | the pure reading of a mailbox's messages: `object`, `message_of`, `pake_of`, `words`, `hex`, `unhex`, `Message`, `WORDS_MOST` |
| `failed` | `Failed` (`Unreachable`, `Refused`, `Expired`, `Nobody`, `WrongCode`, `Peer`, `Mine`, `Broken`, `NoRandom`), `stage_and_class()`, `From<Failed> for pod_core::Failure` |

A giver calls `opened`, shows the code (`Code::for_terminal`), calls
`Wormhole::agree` and `confirm`, then sends and receives phases; a taker
types the code, calls `joined` and `confirm`, then receives and sends. Each
phase is read in order and once, as the mailbox may repeat or reorder them.
The giver ends the code at the first exchange, whatever its end, so that a
wrong code, a guess or a success spends it.

## The MQTT meeting point

`mqtt::opened` and `mqtt::joined` are the same two steps at a list of MQTT
brokers, for a caller whose mailboxes do not answer (PodUtils/podssh#205;
`docs/decisions.md`, C92 and C93). The order of the meeting points, and when
to take this one, are the caller's.

* The giver draws a nameplate of 4 digits (no mailbox allocates one). The
  topic of the code is `pw` and 40 hex digits of SHA-256 over the AppID and
  the nameplate, never the words: each client of a public broker reads each
  topic. Each end reads the topic with the tag of its direction (`g`: to
  the giver, `t`: to the taker), and writes the other.
* MQTT 3.1.1 in binary messages of a WebSocket that offers the subprotocol
  `mqtt`: `CONNECT` (a clean session, a random client id of 22 letters and
  digits, keep-alive 60 s, the login of the `Point`), one `SUBSCRIBE` at
  QoS 0, `PUBLISH` at QoS 0 with no retain, `PINGREQ` after 30 s with
  nothing sent (a second one with no `PINGRESP` ends the connection),
  `DISCONNECT` at the close. A packet may span messages.
* Each message is the JSON of a mailbox's message (`side`, `phase`, `body`
  in hex), so the wormhole reads the same `pake`, `version` and numbered
  phases at a broker as at a mailbox. A stranger on the topic gets one
  guess of the words and spends the code; the broker sees ciphertext.
* A broker keeps no message at QoS 0: one that reaches no subscriber is
  gone. Each end subscribes before it publishes; the giver holds its
  messages until it hears the taker; the taker sends its messages again
  each `Brokers::resend` (2 s) until it hears the giver. So the giver must
  wait (`agree`) while a taker joins, as two processes do.
* A message is at most 64,000 bytes (`PAYLOAD_MOST`, the bound of
  shiftr.io): a phase of about 31 KiB.
* A broker that is not there, refuses the login (`CONNACK` 1 to 5) or the
  subscription, or breaks the protocol (a malformed packet, a text frame,
  a packet that was not asked for) is passed over with the reason, as the
  event `wormhole.broker`.

## The board of retained messages

`mqtt::Board` keeps a note at a topic of one broker, for a caller that
holds one connection at a time (PodUtils/podssh#220, for PodUtils/podssh@T-738). Each
call is a connection of its own: `CONNECT` (a clean session, a random
client id, the login of the `Point`), then

* `put(topic, payload)`: `PUBLISH` at QoS 0 with RETAIN; the broker keeps
  the note for each later subscriber, in place of the one before;
* `take(topic, wait)`: `SUBSCRIBE` at QoS 0, then the first `PUBLISH` of
  the topic within `wait` (a broker sends the retained note after the
  `SUBACK`); `Ok(None)` when none comes; an empty `PUBLISH` is no note;
* `clear(topic)`: `PUBLISH` at QoS 0 with RETAIN and no payload, after
  which the broker keeps no note (MQTT 3.1.1, section 3.3.1.3);

and `DISCONNECT`. After a `PUBLISH`, a `PINGREQ` and its `PINGRESP` show
that the broker read it: QoS 0 has no acknowledgement. A topic is 1 to 42
letters and digits (`TOPIC_MOST`), and a note 1 to 64,000 bytes
(`PAYLOAD_MOST`); others are refused before a connection, with
`Failed::Mine`. A take does not use a note up. Each client of a public
broker reads each topic: the caller seals each note, and draws a topic
that a stranger cannot guess.

## Features

None. The crate is pure Rust (SPAKE2 of the RustCrypto PAKEs on
curve25519-dalek 4, `crypto_secretbox`, HKDF and SHA-2): no C, and it links
static on each target. The WebSocket and its way out are `pod-ws`'s.

## Settings

The crate reads no variable of the environment, keeps no process state and
prints nothing.

| Value | Where |
| --- | --- |
| The AppID (nameplates of another application never meet it; SPAKE2's identity) | `Config::appid` |
| The client's name and version, as the mailbox's `client_version` | `Config::{client_name, client_version}` |
| The way out: the dialer, the proxy, the resolver, the TLS context, the user agent | `Config::way` (`pod_ws::Way`) |
| The roots of a `wss://` mailbox | `Config::trust` |
| The bound of each step of a dial (20 s) | `Config::connect_timeout` |
| The bound of the answer to a command (30 s) | `Config::answer_limit` |
| The largest frame (256 KiB) and message (1 MiB) of a mailbox | `Config::limits` |
| The lines for the user | `Config::events` (`pod_core::Events`): `wormhole.mailbox` (warn; a mailbox that failed, why), `wormhole.broker` (warn; a broker that failed, why) and `wormhole.motd` (info; the mailbox's words). No event holds the code or a password |
| The mailbox URLs, their order, `--pair-mailbox`, `PODSSH_PAIR_MAILBOX`, the public ones | the caller: `Ends::mailboxes` (`pod-services` has the public ones) |
| The MQTT brokers, their logins, their order, whether to take them (`PODSSH_MQTT`) | the caller: `Brokers::points` (`pod-services` has the public ones: the kind `mqtt-broker`, the endpoint `broker`, the facts `user` and `password`) |
| How often the taker sends again at a broker (2 s) | `Brokers::resend` |
| How long a code lives, the bounds of the steps of an exchange | the caller: `Wormhole::agree(wait)`, `confirm`, `receive` |

## Failures

`Failed` keeps its variants, which a caller matches, and no variant holds
the code. Each is also a `pod_core::Failure` (`From`):

| Variant | Stage | Class |
| --- | --- | --- |
| `Unreachable`, `Broken` | `Rendezvous` | `Unavailable` |
| `Refused`, `WrongCode` | `Rendezvous` | `Refused` |
| `Expired` | `Rendezvous` | `Timeout` |
| `Nobody` | `Rendezvous` | `NotFound` |
| `Peer` | `Rendezvous` | `Protocol` |
| `Mine` | `Rendezvous` | `Policy` |
| `NoRandom` | `Host` | `Unavailable` |

## Parity

Against podssh `crates/podssh-relay/src/code/` at `c5636c9`:

| Capability of podssh | Here |
| --- | --- |
| [x] `mailbox.rs`: the server protocol of version 1 (`bind`, `list`, `allocate`, `claim`, `open`, `add`, `release`, `close`), the welcome with an error or a permission, the moods, `crowded` | `mailbox`; the AppID, the client and the way are the `Config` |
| [x] `wormhole.rs`: `pake`, `version`, the numbered phases, SecretBox under the HKDF key of the side and the phase, `scary` for a version that does not open | `wormhole` |
| [x] `phrase.rs`, `words.rs`: the code, the PGP words, typed in any case with hyphens or spaces, a refusal that echoes no word | `phrase`, `words` |
| [x] `mod.rs`: `Failed`, `hex`, `unhex`, `WORDS` | `failed`, `message`, `phrase` |
| [x] `run.rs`: `opened`, `joined` (the choice of the mailbox, the list of nameplates, the giver's `pake` where a mailbox lists none), `or_close`, `Ends` | `meet`; `Ends` holds a `Config` in place of the trust, the proxy and `Say` |
| [ ] `run.rs`: `give`, `take`, `give_with`, `take_with`, `Waits`, `Given`, `Taken`, `Reply`; `exchange.rs` | `pod-relay` (T-024), over `Wormhole` |
| [ ] `mod.rs`: `APPID`, `ENV`, `LIFETIME`, `mailboxes`, `mailboxes_for`, `public_mailboxes`, `Say` | stay in podssh: its names, its variable and flag, its lines |
| [x] `crate::open::CONNECT_TIMEOUT`, `ProxyChoice` | `Config::connect_timeout`, `Config::way` |
| [x] `OsEntropy` | `pod_core::entropy::OsEntropy`; a draw that fails is `Failed::NoRandom`, never a weaker scalar |
| [x] The tests of the five files, each with its name | the unit tests of the modules; the three tests of the Python library lose the word podssh in their names |
| [x] `tests/mailbox_harness`, `scripts/wormhole-peer.py` | `tests/harness/` (the Python server and peer), with a stand-in mailbox in Rust (`tests/harness/stand_in.rs`) |
| [ ] The tests of `tests/pair_code.rs` that run the exchange; the role's mailbox | `pod-relay`, `pod-relay-serve` |
| [x] No fuzz target in podssh | the target `mailbox_message` in `fuzz/` |

Differences from the source, each with a test:

* The limits of a mailbox's frames are 256 KiB and 1 MiB a message
  (`Config::limits`); podssh used the 16 MiB of the relay.
* A mailbox that sends more than 256 messages while a command waits
  (`KEPT_MOST`), or more than 256 that no phase asks for (`HELD_MOST`),
  ends the exchange with `Failed::Broken`; the source kept them with no
  bound (`tests/faults.rs`; the plant `wormhole-held-unbounded`).
* The failure of a command that got no answer names the limit with its
  fraction (`within 300ms`); the source cut it to whole seconds (`0 s`).
* `Failed::Nobody` says that no giver waits at the code (the source said
  node, the word of podssh's pairing).

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podssh | `podssh_relay::code::{mailbox, wormhole, phrase, words}`, `Code`, `Failed`, `WORDS` | the same names, re-exported from `pod_wormhole` by `code/mod.rs` |
| podssh | `Mailbox::connect(url, &trust, &proxy, side)` (`doctor/carriers.rs`) | `Mailbox::connect(url, &code::config(&trust, &proxy, say), side)`; `code::config` builds the `Config` with `APPID`, `"podssh"`, `CARGO_PKG_VERSION`, `podssh_ws::client::way(&proxy)`, the trust, `open::CONNECT_TIMEOUT` and the events as lines |
| podssh | `code::run::Ends { mailboxes, trust, proxy, say }` | the same struct; `Ends::wormhole()` gives `pod_wormhole::Ends` |
| podssh | `code::run::{opened, joined}` | the same functions over `pod_wormhole::{opened, joined}` (`joined` takes `waits.giver`) |
| podssh | `code::run::{give, take, give_with, take_with}`, `code/exchange.rs` | `pod_relay::code::{run, exchange}` (T-025) |
| podssh | `code::{APPID, ENV, LIFETIME, mailboxes, mailboxes_for, public_mailboxes, Say}` | stay |
| podssh | the dependencies `spake2`, `rand_core` 0.6, `crypto_secretbox` of `podssh-relay`, the feature `code` | `pod-wormhole` (git tag), the feature `code = ["pair", "dep:pod-wormhole"]` |
| podssh | the plant `t086-version-unchecked` (`code/wormhole.rs`) | `wormhole-version-unchecked` in podcrates; podssh's `pair-code.yml` runs the others |
| podssh | `scripts/wormhole-peer.py`, `tests/mailbox_harness` (the Python server and peer) | the copies in `tests/` of this crate; podssh's `pair_code.rs` keeps its own for the role and the exchange |
| podssh | none (PodUtils/podssh@T-737, new) | `pod_wormhole::mqtt::{opened, joined}` when no mailbox answers, with `Brokers` of the services of the kind `mqtt-broker` (`endpoint("broker")`, the facts `user` and `password`); `PODSSH_MQTT=no` and the line that names the third party are podssh's |
| podssh | none (PodUtils/podssh@T-738, new) | `pod_wormhole::mqtt::Board` (`put`, `take`, `clear`) at the brokers of the kind `mqtt-broker`, for the URL of each piece of the resumable layer as a sealed note; the host of each piece is a service of the kind `file-host` of `pod-services` |
| podnet | none: podnet has no wormhole code | its transfer (T-102) takes `pod-wormhole` with its own `Config` (`appid`, `"podnet"`, its `Way`) |

The adoption trial is `adoption/podssh/pod-wormhole.patch` with its `.run`,
`.commit` and `.needs` (it applies after `pod-quic`'s patch).

## Tests

`cargo test -p pod-wormhole`. The tests run on the loopback
(`need_loopback!`): the wormhole over a stand-in mailbox in Rust and, where
Python has `magic-wormhole-mailbox-server` and `magic-wormhole`, over
magic-wormhole's own server, with a peer of its own library giving and
taking the code. Both ends derive one key for a purpose; the peer of the
library derives the same transit key (`APPID/transit-key`); the unit test
`a_derived_key_is_magic_wormholes_by_purpose_and_length` holds the vectors of
magic-wormhole's `test_keys.py`. `POD_WORMHOLE_TEST_SERVER=required` turns a skip for want
of Python into a failure; `POD_WORMHOLE_TEST_PYTHON` names the interpreter.
`tests/faults.rs` scripts a mailbox that goes wrong. `tests/broker.rs`
runs the MQTT meeting point over a stand-in broker in Rust
(`tests/harness/broker.rs`) and over Eclipse Mosquitto with a `protocol
websockets` listener: the broker of `POD_WORMHOLE_TEST_BROKER`
(`ws://127.0.0.1:PORT/mqtt`, as `ci.yml` runs the pinned image of
`images/mosquitto/`), else `POD_WORMHOLE_TEST_MOSQUITTO`, `mosquitto` or
`/usr/sbin/mosquitto` on a free port; `POD_WORMHOLE_TEST_MQTT=required`
makes a skip a failure. `tests/board.rs` runs the board on the same
brokers (the stand-in keeps retained messages): a put, a take, a second
put, a clear and a take after the clear; a note put while a take waits; a
topic or a payload out of bounds refused before a connection; a refused
login. `tests/live.rs` is `#[ignore]`: two ends meet at
each public mailbox and pair at each public broker of `pod-services`
(`live.yml`). `tests/board_live.rs` is `#[ignore]`: a board puts, takes
and clears a note at each public broker (`live.yml`, each Thursday).

```sh
python3 -m venv .venv && .venv/bin/pip install magic-wormhole-mailbox-server magic-wormhole
POD_WORMHOLE_TEST_PYTHON=.venv/bin/python POD_WORMHOLE_TEST_SERVER=required cargo test -p pod-wormhole
```

The plants `wormhole-version-unchecked`, `wormhole-held-unbounded`,
`wormhole-mqtt-topic-constant`, `wormhole-mqtt-no-resend`,
`wormhole-board-clear-not-retained`, `wormhole-board-clear-not-empty` and
`wormhole-derived-key-salted` (`scripts/plants/`) prove six of the tests. The fuzz target
`mailbox_message` runs the pure reading of a mailbox's messages and the
parser of a typed code; `mqtt_packet` runs the codec of MQTT and the topic.

## Sources

`SOURCES` lists each imported file with its blob: podssh at `c5636c9`.
