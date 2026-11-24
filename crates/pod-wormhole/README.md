# pod-wormhole

The client of a magic-wormhole mailbox, for a rendezvous by a short code: a
person reads a code to another person, and the two ends meet at a mailbox (a
WebSocket that needs no account) and move what they must with no secret that
the mailbox sees. The ends run SPAKE2 keyed by the whole code over the
mailbox, then seal each later message in NaCl's SecretBox under a key of the
shared key, as magic-wormhole's own client does, so that a peer of its
Python library and this crate meet. When no mailbox answers, the ends can
meet at the topic of the code at a public ntfy server (`ntfy`, plain
HTTPS, for a way out that refuses the WebSocket upgrade), at the tag of
the code at a public Nostr relay (`nostr`), in the
swarm of the code at a public WebTorrent tracker (`tracker`), or at the
topic of the code at a public MQTT broker (`mqtt`), on port 443, with the
same messages.

The base is podssh's `crates/podssh-relay/src/code/` (the mailbox client,
the wormhole, the code and its word list). What the ends say once they hold
a key (the pairing exchange of podssh, the transfer of podnet) is the
caller's protocol over a `Wormhole`; the exchange of podssh goes to
`pod-relay` (T-024).

## API

| Module | What |
| --- | --- |
| `config` | `Config { appid, client_name, client_version, way, trust, connect_timeout, answer_limit, limits, words, events }`, `Config::new(appid, client_name, client_version, way)`, `CONNECT_TIMEOUT`, `ANSWER_LIMIT`, `LIMITS` |
| `mailbox` | `Mailbox`: `connect(&url, &config, side)`, `list`, `allocate`, `claim`, `open`, `add`, `release`, `close`, `end`, `message(wait)`, `appid`, `shown`, `motd`; `Mood` (`Happy`, `Lonely`, `Scary`, `Errory`); `Message`; `crowded(&Failed)`; `ANSWER_LIMIT`, `KEPT_MOST` |
| `wormhole` | `Wormhole`: `open(place, side, nameplate, id, &code, versions)` (a `Mailbox`, an `ntfy::Topic`, a `nostr::Relay`, a `tracker::Tracker` or a `mqtt::Broker`), `agree(wait)`, `confirm(wait)`, `send`, `receive(wait)`, `close(mood)`, `fail(mood, failed)`, `shown`, `derive_key(purpose, len)` (magic-wormhole's `derive_key` of the agreed key: the transit key is `derive_key("APPID/transit-key", 32)`); `derive_key(&key, purpose, len)`, `phase_key`, `seal`, `open_sealed`, `NONCE`, `TAG`, `HELD_MOST`, `DERIVED_MOST` |
| `meet` | `Ends { mailboxes, config }`, `opened(&ends, versions)` (a new code at the first mailbox that answers), `joined(&ends, &code, versions, giver_wait)` (the giver's wormhole, the key made), `or_close` |
| `mqtt` | `Brokers { points, config, resend }`, `Brokers::new(points, config)`, `Point { url, login }`, `Login { user, password }` (no `Debug` shows the password), `opened(&brokers, versions)` (a new code at the first broker that answers, the giver subscribed), `joined(&brokers, &code, versions, giver_wait)` (the taker's wormhole, the key made), `topic(appid, nameplate)`, `nameplate(&mut dyn Entropy)`, `Role` (`Giver`, `Taker`; `topics`), `PROTOCOL`, `KEEP_ALIVE`, `RESEND`, `NAMEPLATE_DIGITS`, `PAYLOAD_MOST`, `TO_GIVER`, `TO_TAKER` |
| `mqtt::board` | `Board { point, config }` (also `mqtt::Board`): `new(point, config)`, `put(topic, payload)`, `take(topic, wait)` (`Ok(None)`: no note within the wait), `clear(topic)`, `shown`; `check_topic`, `check_payload`, `TOPIC_MOST` |
| `mqtt::broker` | `Broker`: `connect(&point, &config, side, topic, role, resend)`, `add`, `message(wait)`, `disconnect`, `end`, `appid`, `inbox`, `shown`; `client_id`, `refusal`, `PING_EVERY`, `KEPT_MOST` |
| `mqtt::packet` | the pure codec of MQTT 3.1.1: `Packet` (`Connect`, `ConnAck`, `Publish` with its `retain` flag, `Subscribe`, `SubAck`, `PingReq`, `PingResp`, `Disconnect`), `Connect`, `encode`, `decode(bytes, most)`, `encode_length`, `kind_name`, `Malformed`, `LENGTH_MOST`, `REFUSED` |
| `nostr` | `Relays { points, config, resend }`, `Relays::new(points, config)`, `opened(&relays, versions)` (a new code at the first relay that answers, the giver subscribed), `joined(&relays, &code, versions, giver_wait)` (the taker's wormhole, the key made), `KIND` (25431), `TAG` (`t`), `RESEND`, `SINCE_SLACK`, `CONTENT_MOST` |
| `nostr::relay` | `Relay` (also `nostr::Relay`): `connect(&url, &config, side, tag, role, resend)`, `add`, `message(wait)`, `close` (after the relay answered each event of this end, within the answer limit), `end`, `appid`, `inbox`, `pubkey`, `shown`; `KEPT_MOST`, `UNANSWERED_MOST` |
| `nostr::event` | the pure events of NIP-01: `Key` (`new`, `drawn`, `from_secret`, `pubkey`, `sign(created_at, kind, tags, content)`), `id_of`, `checked(&event)` (the id and the BIP-340 signature checked), `Checked` (`tag`), `said(text)`, `Said` (`Event`, `Ok`, `Eose`, `Closed`, `Notice`, `Auth`, `Other`), `harmless(message)` |
| `tracker` | `Trackers { points, config, resend }`, `Trackers::new(points, config)`, `opened(&trackers, versions)` (a new code at the first tracker that answers, the giver announced), `joined(&trackers, &code, versions, giver_wait)` (the taker's wormhole, the key made), `info_hash(tag)`, `RESEND`, `SDP_MOST`, `LINGER`, `ANNOUNCE_LEAST`, `ANNOUNCE_MOST` |
| `tracker::link` | `Tracker` (also `tracker::Tracker`): `connect(&url, &config, side, tag, role, resend)`, `add`, `message(wait)`, `close(mood)`, `end`, `appid`, `info_hash`, `peer_id`, `shown`; `KEPT_MOST` |
| `tracker::wire` | the pure messages of a WebTorrent tracker: `said(text)`, `Said` (`Announced`, `Offer`, `Answer`, `Failure`, `Warning`, `Other`), `Note { tag, side, from, got, messages }` (`to_sdp`, `indexed`), `note_of(sdp)`, `ID_CHARS` |
| `ntfy` | `Servers { points, config, pace, h2 }`, `Servers::new(points, config)`, `Pace { poll, waiting, retry }` (`Default`: `POLL`, `WAITING`, `RETRY`), `Point { tls, host, port, base }` (`parse("https://HOST[:PORT][/PATH]")`, `shown`, `host_header`, `target`), `opened(&servers, versions)` (a new code at the first server that answers, the giver reading), `joined(&servers, &code, versions, giver_wait)` (the taker's wormhole, the key made), `MESSAGE_MOST` (4,095), `POLL` (1 s), `WAITING` (5 s), `RETRY` (5 s), `SINCE_SLACK` (60 s) |
| `ntfy::link` | `Topic` (also `ntfy::Topic`): `connect(&point, &servers, side, tag, role)`, `add`, `message(wait)`, `close`, `end`, `appid`, `inbox`, `is_h2`, `shown`; `KEPT_MOST`, `SEEN_MOST` |
| `ntfy::conn` | one HTTP connection to a server: `dial(&point, &config, h2)`, `Conn` (`Http1`, `Http2`; `is_h2`, `request`, `stream`, `end`), `Answer { status, body }` (`is_success`, `refusal`), `Body` (`part`), `BODY_MOST` |
| `ntfy::wire` | the pure lines of an ntfy server: `said(line)`, `Said` (`Open`, `Keepalive`, `Message { id, time, topic, text }`, `Other`), `is_id`, `content(side, phase, body)` (`Err(len)` over `MESSAGE_MOST`), `refusal(status, body)`, `Lines` (`feed`, `rest`), `ID_CHARS`, `LINE_MOST` |
| `place` | `Place` (`Mailbox`, `Ntfy`, `Relay`, `Tracker`, `Broker`; `From` each): `shown`, `noun`, `appid`, and the steps of a wormhole at either |
| `phrase` | `Code`: `make(nameplate, words, &mut dyn Entropy)` (`Failed::Mine` for a count out of `check_words`), `parse(typed)`, `nameplate`, `password`, `for_terminal`, `words`; `check_words` (2 to 8), `nameplate_ok`, `WORDS` (3, the default), `WORDS_LEAST`, `WORDS_MOST`, `NAMEPLATE_MOST`, `TYPED_MOST` |
| `words` | the PGP word list (`EVEN`, `ODD`, `list`, `word`, `find`) |
| `message` | the pure reading of a mailbox's messages: `object`, `message_of`, `pake_of`, `words`, `hex`, `unhex`, `Message`, `WORDS_MOST` |
| `failed` | `Failed` (`Unreachable`, `Refused`, `Expired`, `Nobody`, `WrongCode`, `Peer`, `Mine`, `Broken`, `NoRandom`), `stage_and_class()`, `From<Failed> for pod_core::Failure`; `Missed` (the words of `Unreachable` as a `str`, and the failure of each meeting point with its stage: the first is the source) |
| `standin` (feature `standin`) | a mailbox on the loopback that speaks the server protocol of version 1: `start()` gives a `Stand` (`url`, `moods`, `tamper(phase)`, `nameplates(appid)`; dropped, it stops); `scripted(script)` gives one upgraded connection to a script; `welcome()` |

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

## The Nostr meeting point

`nostr::opened` and `nostr::joined` are the same two steps at a list of
Nostr relays (PodUtils/podssh#235, PodUtils/podssh@T-545;
`docs/decisions.md`, C546 to C549). The order of the meeting points is
the caller's; podssh's ruling Q77 puts the relays after the mailboxes and
before the brokers, as `pod-relay`'s runs do.

* The giver draws a nameplate of 4 digits. The tag of the code is the
  topic of `mqtt` (`pw` and 40 hex digits of SHA-256 over the AppID and
  the nameplate), never the words, with the letter of its direction.
* NIP-01 in text messages of a WebSocket with no subprotocol. One `REQ`
  with the filter `{"kinds": [25431], "#t": [TAG], "since": START - 60}`;
  then each message of the mailbox as the content of an event of the kind
  25431 (of the ephemeral range, which a relay does not store for long),
  with the one tag `["t", TAG]` of the other direction. Each event is
  signed (BIP-340 Schnorr over secp256k1, `k256`) by a key drawn for the
  pairing alone: no public key links two pairings.
* Before a code is shown or typed, the connection proves the relay: the
  `REQ` gets its `EOSE`, and an empty event to this end's own tag comes
  back with `OK` true. A relay that is not there, closes the `REQ`
  (`CLOSED`), refuses an event (`OK` false, but `mute:` and `duplicate:`),
  sends a `NOTICE` and no `EOSE`, or keeps the event from its subscriber,
  is passed over with its words, as the event `wormhole.nostr`. A
  `NOTICE` is said as an event `wormhole.nostr` (info).
* A received event is checked (its id and its signature), of the kind, of
  the tag, and of another key; else it is dropped.
* A relay keeps no ephemeral event, or keeps it a short time. The giver
  holds its messages until it hears the taker; the taker sends its
  messages again each `Relays::resend` (2 s) until it hears the giver,
  each time as a new event (a later `created_at`), so that no relay drops
  it as a duplicate.
* A public relay checks an event in its own time, and drops one that it
  had not passed on when the connection of its sender ends. So an end
  closes the subscription only after the relay answered (`OK`) each of
  its events, within the answer limit (T-634, PodUtils/podssh#247): the
  giver's last word reaches the taker. At most 64 events wait for an
  answer (`UNANSWERED_MOST`).
* A content is at most 64,000 bytes (`CONTENT_MOST`): the event stays
  under the 65,536 bytes that strfry takes.

## The ntfy meeting point

`ntfy::opened` and `ntfy::joined` are the same two steps at a list of
ntfy servers (PodUtils/podssh#237, PodUtils/podssh@T-496;
`docs/decisions.md`, C568 to C573), for a way out that refuses the
WebSocket upgrade: each request is of plain HTTPS. `pod-relay`'s runs put
the servers after the mailboxes, or first where the caller's probe found
the upgrade refused.

* The topic of the code is the topic of `mqtt` of the AppID and the
  nameplate with the letter of each direction, never the words: 43
  letters and digits, which ntfy takes (`[-_A-Za-z0-9]{1,64}`).
* Each message of the mailbox is one POST to the other end's topic whose
  body is its JSON (`{"side", "phase", "body"}`, the body in hex), with
  `X-Firebase: no`. A body is at most 4,095 bytes (`MESSAGE_MOST`): ntfy
  keeps a body of its whole limit (4,096) as an attachment, as a cloud
  session measured at ntfy.sh, ntfy.envs.net and ntfy.adminforge.de; a
  longer message fails before the send.
* A server keeps each message (12 h on ntfy.sh), so nothing is sent
  again. The giver holds its messages until the taker is heard; each end
  reads from its start less `SINCE_SLACK` (60 s), then from the id of the
  last message read (`since=`), and drops a message that comes twice by
  its id.
* TLS offers `h2` then `http/1.1`. On HTTP/2 one connection holds the
  stream of `TOPIC/json?since=` and the POSTs. On HTTP/1.1 the polls
  (`TOPIC/json?poll=1&since=`) and the POSTs take turns on one connection,
  which is dialed again once where the server closed it between two
  requests. An end polls each `Pace::poll` (1 s); the giver polls each
  `Pace::waiting` (5 s) until the taker is heard: a visitor of ntfy gets
  60 requests at once and one more each 5 s.
* Before a code is shown or typed, the connection proves the server: the
  stream gives its `open` line, or a poll gives `200`. A server that is
  not there or refuses is passed over with its words (ntfy's `error` and
  `code`), as the event `wormhole.ntfy`. An answer `429` is the event
  `wormhole.ntfy` (info), and the request goes again after `Pace::retry`
  (5 s), within `Config::answer_limit`.
* ntfy.sh takes 250 messages a day from one address; a pairing posts
  seven.

## The WebTorrent meeting point

`tracker::opened` and `tracker::joined` are the same two steps at a list
of WebTorrent trackers (PodUtils/podssh#235, PodUtils/podssh@T-695;
`docs/decisions.md`, C556 to C561). podssh's ruling Q77 puts the trackers
after the Nostr relays and before the brokers, as `pod-relay`'s runs do.

* The JSON of webtorrent/bittorrent-tracker (11.2.3) in text messages of
  a WebSocket. The `info_hash` of the code is `info_hash(tag)`: 20 hex
  digits of SHA-256 over a label and the topic of `mqtt` of the AppID and
  the nameplate, never the words. Each end draws a `peer_id` of 20 hex
  digits for each pairing.
* Before a code is shown or typed, the connection proves the tracker: an
  announce (`event` `started`, no offer) gets the tracker's answer for the
  `info_hash`. A tracker that is not there, refuses (`failure reason`) or
  does not answer within `Config::answer_limit` is passed over with its
  words, as the event `wormhole.tracker`.
* A tracker passes an offer to one other peer of the swarm and keeps no
  copy, and it passes an answer to the peer of its `to_peer_id`. It reads
  the `sdp` alone (tracker.openwebtorrent.com and tracker.webtorrent.dev
  refuse an offer with no `type` and `sdp`): the ends put a `Note` there,
  `{"tag", "side", "from", "got", "m": [{"phase", "body"}]}`: this end's
  messages that the other end does not have, and the count of the
  other end's messages that this end has. A message that comes twice is
  dropped by its index; a note of another tag, of this side, or of a peer
  that is not the first one heard is dropped.
* The taker offers its note at each message and again each
  `Trackers::resend` (2 s) while it waits: a lost offer is sent again, and
  the giver gets an offer to answer. After an answer that brought a
  message, the taker offers at once.
* The giver speaks only in an answer to an offer, and a tracker passes one
  answer to each offer (tracker.webtorrent.dev refuses a second one). It
  answers at once an offer that brought a message or that lacks one of its
  messages; it keeps the last other offer and answers it at its next
  message. Its close after a happy exchange answers the taker's offers
  until the taker has each of its messages, at most `LINGER` (10 s); the
  taker's close offers the count of the giver's messages that it has. So
  the giver's messages go while it reads or closes.
* The giver announces again each interval of the tracker, within
  `ANNOUNCE_LEAST` and `ANNOUNCE_MOST` (1 s to 120 s; bittorrent-tracker
  says 120 s). Both ends leave the swarm (`event` `stopped`) at their
  close.
* An `sdp` is at most 10,000 bytes (`SDP_MOST`): tracker.webtorrent.dev
  carried 12,000 bytes and not 14,000.

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

| Feature | What |
| --- | --- |
| none (default) | the client |
| `standin` | `standin`: the stand-in mailbox on the loopback, for the tests of this crate and of a consumer (`pod-relay`'s, podnet's); a consumer turns it on in `[dev-dependencies]`; no binary needs it (`scripts/check-probe.py`, ALLOWED) |

The crate is pure Rust (SPAKE2 of the RustCrypto PAKEs on
curve25519-dalek 4, `crypto_secretbox`, HKDF, SHA-2, and the Schnorr
signatures of `k256`): no C, and it links
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
| The words of a new code (3; 2 to 8), as magic-wormhole's `--code-length` | `Config::words`; `opened` refuses another count before a dial |
| The largest frame (256 KiB) and message (1 MiB) of a mailbox | `Config::limits` |
| The lines for the user | `Config::events` (`pod_core::Events`): `wormhole.mailbox` (warn; a mailbox that failed, why), `wormhole.ntfy` (warn; an ntfy server that failed, why; info: an answer `429`), `wormhole.nostr` (warn; a Nostr relay that failed, why; info: its `NOTICE`), `wormhole.tracker` (warn; a tracker that failed, why; info: its words after the other end was heard), `wormhole.broker` (warn; a broker that failed, why) and `wormhole.motd` (info; the mailbox's words). No event holds the code or a password |
| The mailbox URLs, their order, `--pair-mailbox`, `PODSSH_PAIR_MAILBOX`, the public ones | the caller: `Ends::mailboxes` (`pod-services` has the public ones) |
| The MQTT brokers, their logins, their order, whether to take them (`PODSSH_MQTT`) | the caller: `Brokers::points` (`pod-services` has the public ones: the kind `mqtt-broker`, the endpoint `broker`, the facts `user` and `password`) |
| How often the taker sends again at a broker (2 s) | `Brokers::resend` |
| The ntfy servers, their order, whether to take them (`PODSSH_PAIR_NTFY`) | the caller: `Servers::points` (`pod-services` has the public ones: the kind `ntfy-server`, the endpoint `server`; the facts `message-bytes-most`, `requests-burst`, `request-refill-seconds`, `messages-per-day`, `account-required`) |
| How often an end polls an ntfy server on HTTP/1.1 (1 s; the waiting giver 5 s), the wait after `429` (5 s), HTTP/2 or not | `Servers::pace`, `Servers::h2` |
| The Nostr relays, their order, whether to take them | the caller: `Relays::points` (`pod-services` has the public ones: the kind `nostr-relay`, the endpoint `relay`) |
| How often the taker sends again at a Nostr relay (2 s) | `Relays::resend` |
| The WebTorrent trackers, their order, whether to take them | the caller: `Trackers::points` (`pod-services` has the public ones: the kind `webtorrent-tracker`, the endpoint `tracker`; the facts `filter-list`, `filter-list-part` and `filter-category` name the filter list that refuses each one) |
| How often the taker offers again at a tracker (2 s) | `Trackers::resend` |
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

The source of `Unreachable` is the failure of the first meeting point,
with its own stage: `Proxy` for a proxy that does not answer, `Dns` for a
name that does not resolve. `Missed::causes` gives the failure of each
point. `Expired` says the exact life of the code: "5 seconds", "1 minute",
"90 seconds".

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
| [x] `tests/mailbox_harness`, `scripts/wormhole-peer.py` | `tests/harness/` (the Python server and peer), with a stand-in mailbox in Rust (`src/standin.rs`, feature `standin`) |
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
| podssh | none (PodUtils/podssh@T-545, new) | `pod_wormhole::nostr::{opened, joined}` after the mailboxes, with `Relays` of the services of the kind `nostr-relay` (`endpoint("relay")`), or `pod_relay::code::run::Ends::relays`; the switch and the line that names the third party are podssh's |
| podssh | none (PodUtils/podssh@T-695, new) | `pod_wormhole::tracker::{opened, joined}` after the Nostr relays, with `Trackers` of the services of the kind `webtorrent-tracker` (`endpoint("tracker")`), or `pod_relay::code::run::Ends::trackers`; the switch, the line that names the third party and the filter list of each tracker (its facts) are podssh's |
| podssh | none (PodUtils/podssh@T-496, new) | `pod_wormhole::ntfy::{opened, joined}` after the mailboxes, or first where its probe found the upgrade refused, with `Servers` of the services of the kind `ntfy-server` (`Point::parse(&endpoint("server")?.url())`), or `pod_relay::code::run::Ends::{ntfy, upgrade_refused}`; `PODSSH_PAIR_NTFY` and the line that names the third party are podssh's |
| podnet | none: podnet has no wormhole code | its transfer (T-102) takes `pod-wormhole` with its own `Config` (`appid`, `"podnet"`, its `Way`) |
| podnet | `tests/transfer_harness/mod.rs`, its own stand-in mailbox | `pod_wormhole::standin` (`[dev-dependencies]`, feature `standin`) |
| podnet | a random transit key in its sealed `transit` message | `Wormhole::derive_key("APPID/transit-key", 32)`, as magic-wormhole's transfer |
| podnet | a code of 3 words | `Config::words` for `--words N` |

The adoption trial is `adoption/podssh/pod-wormhole.patch` with its `.run`,
`.commit` and `.needs` (it applies after `pod-quic`'s patch).

## Tests

`cargo test -p pod-wormhole --features standin` (each test file but the
unit tests needs the feature). The tests run on the loopback
(`need_loopback!`): the wormhole over the stand-in mailbox in Rust and, where
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
login. A code of 5 words over each mailbox and of 6 words over each
broker meets; a count out of bounds fails before a dial.
`tests/nostr.rs` runs the Nostr meeting point over a stand-in relay in
Rust (`tests/harness/nostr.rs`), which keeps no event and passes each
event to the subscriptions of the moment that hold it; as a public relay,
it takes each message 20 ms after it came and drops the messages of a
connection that ended before then: two ends pair and
move a message each way, each with a key of its own, two new keys at a
second pairing; an event with the tag of another code is not delivered;
the taker sends its first message again, each time as a new event, until
the giver answers; the last word of an end that closes at once reaches
the other end; a relay that closes the `REQ`, refuses the events, or
says a `NOTICE` and no `EOSE` is passed over with its words; an event that
does not check is refused by the stand-in and dropped by an end; each
public relay takes the largest event. No Nostr relay of another party
publishes a static release (C549): the stand-in is the relay of CI.
`tests/tracker.rs` runs the WebTorrent meeting point over a stand-in
tracker in Rust (`tests/harness/tracker.rs`), which keeps no offer and
passes each one to another peer of the swarm, and over
webtorrent/bittorrent-tracker where `POD_WORMHOLE_TEST_TRACKER` names it
(`ws://127.0.0.1:PORT/`, as `scripts/ci/tracker-tests.sh` runs the release
of its hashed npm lock file; `POD_WORMHOLE_TEST_WEBTORRENT=required`
makes a skip a failure): two ends pair and move messages each way, each
with a `peer_id` of its own, two new ones at a second pairing, the giver's
last word delivered at its close; the swarm of another code is another
one; a lost offer is offered again; the giver announces again each
interval; a stranger in the swarm after the taker, with the taker's
side, is not heard; a
tracker that refuses or says nothing is passed over with its words; each
public tracker carries the largest `sdp`.
`tests/ntfy.rs` runs the ntfy meeting point over a stand-in server in Rust
(`tests/harness/ntfy.rs`, HTTP/1.1; `tests/harness/ntfy_tls.rs`, behind TLS
with HTTP/2 or HTTP/1.1 alone), which keeps each message as ntfy's cache
does, and over ntfy's own server where `POD_WORMHOLE_TEST_NTFY` names it
(`http://127.0.0.1:PORT/`, as `scripts/ci/ntfy-tests.sh` runs the pinned
release 2.37.2; `POD_WORMHOLE_TEST_NTFY_SERVER=required` makes a skip a
failure): two ends pair by polls that take turns with the POSTs on one
connection of each end (HTTP/1.1), and on one connection of each end with
a stream (HTTP/2), and a server behind TLS with HTTP/1.1 alone is polled,
with no word of the code in a request; a message of 4,096 bytes or more
fails before the send; a read takes what came since its start, then
since the last id; a POST answered `429` goes again; a server that
refuses is passed over with its words; each public server carries the
largest message.
`tests/live.rs` is `#[ignore]`: two ends meet at
each public mailbox and pair at each public ntfy server (on HTTP/2 and
on HTTP/1.1), each public Nostr relay, each public
WebTorrent tracker and each public broker of `pod-services` (`live.yml`). `tests/board_live.rs` is `#[ignore]`: a board puts, takes
and clears a note at each public broker (`live.yml`, each Thursday).

```sh
python3 -m venv .venv && .venv/bin/pip install magic-wormhole-mailbox-server magic-wormhole
POD_WORMHOLE_TEST_PYTHON=.venv/bin/python POD_WORMHOLE_TEST_SERVER=required cargo test -p pod-wormhole --features standin
```

The plants `wormhole-version-unchecked`, `wormhole-held-unbounded`,
`wormhole-mqtt-topic-constant`, `wormhole-mqtt-no-resend`,
`wormhole-board-clear-not-retained`, `wormhole-board-clear-not-empty`,
`wormhole-derived-key-salted`, `wormhole-code-words-fixed`,
`wormhole-standin-not-crowded`, `wormhole-nostr-tag-constant`,
`wormhole-nostr-no-resend`, `wormhole-nostr-refusal-ignored`,
`wormhole-nostr-signature-unchecked`, `wormhole-nostr-close-unanswered`,
`wormhole-tracker-empty-answer`,
`wormhole-tracker-no-resend`, `wormhole-tracker-info-hash-constant`,
`wormhole-tracker-refusal-ignored`, `wormhole-tracker-stranger-heard`,
`wormhole-ntfy-topic-of-code`, `wormhole-ntfy-size-unchecked`,
`wormhole-ntfy-since-start`, `wormhole-ntfy-limit-fails`,
`wormhole-ntfy-polls-on-h2` and `wormhole-ntfy-connection-each-request`
(`scripts/plants/`) prove twenty-five of the tests. The fuzz target
`mailbox_message` runs the pure reading of a mailbox's messages and the
parser of a typed code; `mqtt_packet` runs the codec of MQTT and the topic; `nostr_message` runs
the reading of a Nostr relay's messages and the check of an event;
`tracker_message` runs the reading of a tracker's messages and of a note;
`ntfy_message` runs the reading of the lines of an ntfy server.

## Sources

`SOURCES` lists each imported file with its blob: podssh at `c5636c9`.
