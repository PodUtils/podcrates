# pod-egress

The roads out that are not the relay. A proxy, a forwarder or a server of
another product carries the bytes to a target, or to a bridge of Tor, or
to a tunnel of Cloudflare:

* `via`: a second forward proxy over TLS (Caddy's forwardproxy, squid's
  `https_port`, h2o), by HTTP/1.1's or HTTP/2's CONNECT, with Basic from a
  private file;
* `forwarder`: a WebSocket forwarder of another product that takes its
  target from the client: in its URL (`{host}`, `{port}`), in a first frame
  of JSON, or by Wisp (`wisp+wss://`); the built-in ones are the services
  of the kind `forwarder` of `pod-services`;
* `wisp`: many TCP streams over one WebSocket, by Mercury Workshop's Wisp
  protocol, versions 1 and 2, with the password extension of version 2,
  and a road that opens a new connection on the next dial after the old
  one ended;
* `wstunnel`: a server of erebe/wstunnel, 9.1 to 11, with its token, its
  path prefix and its `Authorization`;
* `meek` and `webtunnel`: Tor's pluggable transports meek_lite and
  WebTunnel, as lyrebird speaks them, with what they share (`pt`);
* `cloudflare`: the road of `cloudflared access tcp`, with the credentials
  of Cloudflare Access;
* `vless`: a VLESS server (Xray-core, sing-box, shoes) over a WebSocket or
  XHTTP (HTTP/2, or HTTP/1.1 on two connections);
* `vmess`: a VMess server in the AEAD form over a WebSocket;
* `trojan`: a Trojan server over TLS or a WebSocket over TLS;
* `shadowsocks`: a Shadowsocks server in the AEAD form or Shadowsocks 2022,
  over TCP or a WebSocket (an Outline access key), with the `ss://` URIs,
  the subscriptions, the access keys that name one, a private key file,
  and Outline's salt prefix;
* `eice`: AWS's EC2 Instance Connect Endpoint, as `aws ec2-instance-connect
  open-tunnel` reaches it, its URL presigned with SigV4;
* `iap`: Google Cloud's Identity-Aware Proxy for TCP, as `gcloud compute
  start-iap-tunnel` reaches it: IAP's own messages, their
  acknowledgements, and a session that a new link takes up.

This is the whole crate (T-035, parts 1 to 3).

The base is podssh's `crates/podssh-ws/src/` at `2ffb7cd7`: `via/`,
`forwarder/`, `wstunnel/`, `wisp/`, `meek.rs`, `meek/`, `webtunnel.rs`,
`pt.rs` and `cloudflare.rs` (part 1), `vless/`, `vmess/`, `trojan/` and
`shadowsocks/` (part 2), `eice/` and `iap/` (part 3), with their tests.

## API

| Module | What |
| --- | --- |
| crate root | `Note` (`Arc<dyn pod_core::Events>`: a peer's own words), `Duplex` (the stream of a tunnel), `AtEnd`, `Carried`, `Ended`, `Status`, `Form` |
| `coalesce` | the first write of a session as one write of `TARGET` (3,333) bytes: `first_write(head, data) -> (frame, short)`, `padding(head, data) -> (filler, short)`, `First` (`write(head, data) -> (bytes, coalesced)`, `report`, `coalesced`, `short`), `Report { bytes, short, coalesced }`, `PAD_HEADER`, `FIELD_OVERHEAD` |
| `replay` | the window of early-data nonces: `ReplayWindow` (`new`, `with_window`, `window`, `admit`, `admit_at`, `len`, `is_empty`), `Admitted { Fresh, Repeat }`, `WINDOW` (10 minutes), `MOST` |
| `carry` | `spawn(session, form, at_end, note, peer)` (a WebSocket session as a byte stream, and the peer's first word), `spawn_bytes(session, at_end, peer)` (each data frame is bytes), `Carried { stream, status }` (`of`, `abort`), `Status` (`get`, `set_once`), `Ended`, `First`, `Form` (`Query`, `Frame`, `Wisp`), `answer(payload)`, `EARLY_WAIT`, `NOTE` (the kind `egress.note`) |
| `end` | `AtEnd` (`Now`, `AfterPeer`), `end_of_input(at, peer_ended, close)` |
| `via` (feature `via`) | `Via` (`parse`, `with_credentials`, `shown`, `names_user`, `has_credentials`), `Dial { way, trust, timeout, offer, auth_file }`, `Offer`, `Version`, `Client` (`new`, `via`, `open(host, port)`; a `pod_core::Dial`), `Opened { stream, version, shared, fell_back }`, `Stream` (`pod_ws::h2_road::Stream`), `DEFAULT_PORT` |
| `forwarder` (feature `forwarder`) | `Template` (`parse`, `form`, `shown`, `fill`, `first_frame`), `Dial { way, trust, timeout, at_end, note }`, `open(template, host, port, dial)`, `Forwarder { template, dial }` (a `pod_core::Dial`), `ForwarderError` (`line`, `stage_and_class`; a `pod_core::Failure`), `builtin` (`builtin()`, `lookup`, `words`, `Builtin`), `HOST`, `PORT`, `WISP`, `RECEIVE_CAP`, and the shared parts again |
| `wisp` (feature `wisp`) | `Wisp` (`connect(url, connect)`, `opened`, `confirms`, `is_open`, `open(host, port, at)`), `Connect { way, trust, timeout, note, login }`, `Login` (`new(user, password)`, `parse("USER:PASSWORD")`, `user`; no `Debug` or line shows the password, zeroed on drop; over `wss://` alone, or to the loopback), `MAX_USER`, `WispStream` (`id`, `opened(wait)`, `end`, `close`), `Road { wisp, at_end, shown }` (a `pod_core::Dial` on one connection), `Reopening` (`new(url, connect, at_end)`, `connect(..)`, `with_tries`, `shown`, `is_open`, `opened`; a `pod_core::Dial` that opens a new connection on the next dial after the old one ended: `TRIES` connections at most in one dial, `pod_core::backoff` between two, and a dial in the backoff after a failed one fails at once; a refused credential is not tried again), `TRIES`, `WispError` (`stage_and_class`; a `pod_core::Failure`; a CLOSE 0xc0 to 0xc2 on stream 0 is of the stage relay and the class refused), `End`, `Opened`, `answer(info, login)`, `packet` (`Packet`, `Info`, `Extension`, `Reason` with `WRONG_PASSWORD`, `WRONG_SIGNATURE`, `NO_CREDENTIAL`, `is_credential`, `encode`, `decode`, `data`), `PROTOCOL`, `VERSION`, `RECEIVE_CAP`, `NOTE` (the kind `wisp.note`) |
| `wstunnel` (feature `wstunnel`) | `Server` (`parse`, `shown`, `tls`, `host`, `port`, `is_loopback`), `Secret` (`parse`, `is_default`), `check(server, secret)`, `Dial { way, trust, timeout, at_end }`, `open(server, secret, host, port, dial)`, `Tunnel { server, secret, dial }` (a `pod_core::Dial`), `WstunnelError` (`line`, `stage_and_class`; a `pod_core::Failure`), `upgrade` (`request`, `upgrade`, `words`, `SUBPROTOCOL`, `VERSIONS`, `REFUSED_9`, `REFUSED_10`), `claims`, `host_claim`, `sign`, `uuid_text`, `KEY`, `PREFIX`, `DEFAULT_PREFIX`, `RECEIVE_CAP`, `PEER` |
| `pt` (feature `meek` or `webtunnel`) | `Reach { way, trust, wait }`, `HttpUrl` (`parse`, `path`, `shown`), `host_port`, `get` |
| `meek` (feature `meek`) | `Args` (`parse`), `Front`, `open(args, reach)`, `Session` (`shown`, `carry(local)`), `Ended`, `next_poll`, `session_id`, `MAX_PAYLOAD`, `INIT_POLL`, `MAX_POLL`, `MAX_TRIES`, `RETRY_DELAY`, `EXCHANGE_WAIT` |
| `webtunnel` (feature `webtunnel`) | `Args` (`parse`, `shown`, `dial`, `server_name`), `open(args, reach)`, `request`, `check_answer`, `chain_hash` |
| `cloudflare` (feature `cloudflare`) | `Access`, `connect(host, access, way, trust, timeout, idle)`, `connect_loopback(host, port, access, user_agent, timeout, idle)` (feature `plain-ws`), `request`, `refused_by_access`, `PORT`, `PATH`, `MAX_MESSAGE`, `LOGIN_PATH`, `CLIENT_ID`, `CLIENT_SECRET`, `TOKEN` |
| `vless` (feature `vless`) | `Server` (`parse`, `shown`, `with_early_data`), `Id`, `Transport`, `Mode`, `Extra`, `link`, `Dial { way, trust, timeout, answer, silence_is_ready, at_end, early }`, `open(server, host, port, dial)`, `open_as(server, transport, ..)`, `modes(transport)`, `Opened { stream, how, answered, first_write, early }`, `Road { server, dial }` (a `pod_core::Dial`), `VlessError` (`line`, `stage_and_class`; a `pod_core::Failure`), `early` (`Early::new`, `admit`, `Offer { ZeroRtt, OneRtt, Absent }` with `of`, `zero_rtt`, `words`), `ReplayWindow`, `Admitted`, `WINDOW`, `coalesce::TARGET`, `ENDED`, `not_vless`, `ANSWER_WAIT`, `EARLY_WAIT` |
| `vmess` (feature `vmess`) | `Server` (`parse`, `shown`), `Cipher`, `link`, `Dial` (VLESS's), `open`, `open_at(.., unix)`, `Opened`, `Road { server, dial }` (a `pod_core::Dial`), `VmessError` (`line`, `stage_and_class`; a `pod_core::Failure`), `ANSWER_WAIT`, `EARLY_WAIT` |
| `trojan` (feature `trojan`) | `Server` (`parse`, `shown`), `Password`, `Transport`, `link`, `request(password, host, port)` (the bytes of the published layout), `accept::parse(buf, &Password)` (the accept side: a `Request { command, host, port, used }` of `Command` (`Connect`, `UdpAssociate`), `None` while the bytes are too short, `RequestError::WrongPassword` or `RequestError::NotTrojan(Malformed)` so that a server gives its fallback; `MAX_REQUEST`), `Password::matches` (constant time), `capability()` and `Capability` (`Tcp`, `UdpAssociate`; `ALL`, `carried`, `command`, `words`) for the doctor's line, `CONNECT` and `UDP_ASSOCIATE`, `Dial` (VLESS's), `open`, `Opened`, `Road { server, dial }` (a `pod_core::Dial`), `TrojanError` (`line`, `stage_and_class`; a `pod_core::Failure`), `ANSWER_WAIT`, `EARLY_WAIT` |
| `shadowsocks` (feature `shadowsocks`) | `Node` (`parse`, `new`, `shown`, `Display` as `shown`, `with_prefix`, `prefix`), `Method`, `codec::Decoder` (the accept side with `new` and `salt()`, the request's salt for a replay window) and `parse_address` (the accept side's reader of the target in the first chunk), `Dial { way, trust, timeout, answer, silence_is_ready }`, `open(node, host, port, dial)`, `Opened { carried, answered_in }`, `Carried`, `Status`, `Ended`, `Road { node, dial }` (a `pod_core::Dial`), `OpenError` (`line`, `stage_and_class`; a `pod_core::Failure`), `access_key`, `key_file(path)` (an access key in a private file of 64 KiB at most: an `ss://` link, SIP008's JSON or Outline's YAML, its anchors and merge keys), `prefix` (Outline's salt prefix: `from_text`, `to_url_text`, `check`, `salt`, `MAX_PREFIX` 16, `MIN_RANDOM` 8), `subscription`, `Listed`, `codec` (`Encoder`, `Decoder`, `ChunkError`, `address`, `parse_address`, `MAX_PAYLOAD`), `sip022`, `master_key`, `ANSWER_WAIT`, `TAG_LEN` |
| `eice` (feature `eice`) | `Endpoint` (`parse`), `Target`, `Credentials` (`check`; no value in `Debug`), `connect(endpoint, target, credentials, way, trust, timeout)`, `presigned_path`, `request`, `sigv4` (`Presign` with `canonical_request`, `string_to_sign`, `signature`, `query`; `escape`, `amz_date`, `ALGORITHM`, `EMPTY_SHA256`), `PORT`, `PATH`, `SERVICE`, `EXPIRES`, `MAX_DURATION`, `MAX_FRAME` |
| `iap` (feature `iap`) | `Target` (`new`), `Resume`, `connect(target, resume, token, way, trust, timeout)`, `path`, `request`, `check_token`, `carry` (`open(dial, note)`, `Dial`, `Dialing`, `Carried` (`get`, `ended`), `NotOpen` and `Ended` (each `stage_and_class`; a `pod_core::Failure`), `LOST`, `RESUMED`, `STEP`, `FIRST_MESSAGE`, `RECONNECTS`, `RECONNECT_WAIT`, `ACK_WAIT`, `DRAIN`), `close` (`fate`, `Fate`, `Why`, `stage_and_class`, the codes), `flow` (`Flow`), `frame` (`Message`, `decode`, `data`, `ack`, `MAX_DATA`, the tags), `HOST`, `PORT`, `CONNECT_PATH`, `RECONNECT_PATH`, `SUBPROTOCOL`, `ORIGIN`, `INTERFACE`, `MAX_TOKEN` |
| `standin` (feature `standin`) | `standin::echo()` (an `Echo { port }`: `BANNER`, then each byte back); `standin::via::start(server_config, Plan)` (a `Stand { port, seen }`: TLS with the caller's `rustls::ServerConfig` and the `Plan`'s ALPN, HTTP/1.1's CONNECT or h2's for `h2`, `Plan { alpn, mode, credentials }` with `Mode` (`Carry`, `Forbid`: 403, `ResetHttp2`: the reset of a stream of HTTP/2), `Plan::basic`, and `407` with `Proxy-Authenticate: Basic` without the Basic of the credentials; `Seen` (`accepted()`, `connects()`: each `Connect { version, target, authorization }`)); `standin::wstunnel::start(Wst, Option<server_config>)` (a `Stand { port, seen }`: wstunnel 11's reading of a client, `Wst { restrict, authorization, checks_signature, unmask, answer, answers_close }`, `Wst::v11()`, `Answer` (`Tunnel`, `Refuse9`, `NoSubprotocol`); `Seen { requests: Vec<Asked { path, protocol, authorization }>, targets, keys }`). `standin::vless` (`ws_server(config)`, `xhttp_server(config, alpn)`, `xhttp1_server(Option<config>)`: Xray-core's VLESS over a WebSocket at `/ws` and over XHTTP at `/xh/` on HTTP/2 or HTTP/1.1, the users' `ID`, `CLOSE_AFTER`, `parse_request`, `Seen`); `standin::trojan` (`listen(config, serve)` with `serve_tls` or `serve_ws`: trojan-gfw's Trojan in TLS alone or in a WebSocket at `/tr`, `PASSWORD`, `FALLBACK`, `hash_of`, `Seen`); `standin::wisp` (`Stand::start(Speaks, buffer)`: a Wisp server of version 1 or 2 that counts the credit of each stream, `Speaks::Password { user, password, required }` a server that asks for a password and refuses a wrong one with 0xc0, `data_of`, `overruns`, `grant`, `end_all` (each WebSocket closed), `packet`, `Seen` with `logins`); `standin::shadowsocks` (with the feature `shadowsocks` too: `aead(method, password, banner)` and `sip022(method, psk, banner, Answer)`, one connection each, from the specification's own ciphers `Spec` (`aead`, `sip022`, `seal`, `open`), `evp`, `RIGHT`, `Served`). Each `Stand<S>` (`port`, `seen`) stops taking connections when it drops; `detach` keeps it until the runtime ends. The IDs, passwords and keys are made-up values or the caller's. The crate mints no certificate: the caller gives the server configuration |
| `fuzzing` (feature `fuzzing`) | `wisp_packet`, `wisp_answer`, `iap_message`, `iap_data`, `iap_ack`, `trojan_parse`, `trojan_request` |

## Each road as a dialer

A road that takes its target from the client is a `pod_core::Dial`
(`via::Client`, `forwarder::Forwarder`, `wisp::Road`, `wstunnel::Tunnel`,
`vless::Road`, `vmess::Road`, `trojan::Road`, `shadowsocks::Road`):
a dial of a target opens the road to it before the deadline, and gives the
stream. Each says `names_inside()` (the far end resolves the name), its
`label()` and its `proxy_label()`. A failure is a `pod_core::Failure` with
the stage and the class of the road's error, in the line that names the
road. meek_lite, WebTunnel, the Access client, EICE and IAP reach one far
end that the bridge, the tunnel or the cloud names: they are not dialers.

## How a new road joins the crate

1. Put the road in `src/ROAD/` (or `src/ROAD.rs`), behind a feature of its
   name in `Cargo.toml`, with its optional dependencies. `src/lib.rs`
   declares the module under `#[cfg(feature = "ROAD")]`.
2. The road dials through a `pod_ws::Way` and a `pod_tls::Trust` that its
   `Dial` (or its settings struct) holds by value; no `ProxyChoice`, no
   process state, no variable of the environment.
3. A WebSocket road whose server carries the bytes in frames takes
   `carry::spawn` or `carry::spawn_bytes`, and `AtEnd` from `end`; a peer's
   own words go to a `Note`. A tunnel's stream is a `Duplex`.
4. Its error gives `stage_and_class()` and `From<Error> for
   pod_core::Failure`. A road that takes its target from the client is a
   `pod_core::Dial` (a struct of the road and its `Dial`); `road::left`
   and `road::late` bound a dial by its deadline.
5. Its tests go to `tests/ROAD.rs` with `required-features`, its way from
   `tests/common/way.rs`; its plants to `scripts/plants/egress-*.plant`;
   its files and blobs to `SOURCES`; its rows to the tables of this README.
6. Do not change the shared parts (`carry`, `end`, `Note`, `Duplex`,
   `road`) for one road: a change there is its own entry.

## Features

| Feature | What |
| --- | --- |
| `via` | the second proxy (h2, http, bytes, base64, zeroize, tokio-rustls) |
| `forwarder` | the forwarders and the built-in ones (pod-services); turns on `wisp` |
| `wisp` | the Wisp client (zeroize) |
| `wstunnel` | wstunnel (HMAC-SHA-256 of its token) |
| `standin` | (turns on `trojan`) the stand-ins of a forward proxy over TLS, of a wstunnel server and of VLESS, Trojan, Wisp and (with `shadowsocks`) Shadowsocks servers on the loopback, for the tests of this crate and of a consumer (h2, http, bytes, base64, hmac, sha2, tokio-rustls, aes-gcm, chacha20poly1305, md5, hkdf, sha1, blake3); no binary needs it |
| `meek`, `webtunnel` | Tor's pluggable transports (SHA-256) |
| `cloudflare` | the Access road |
| `vless` | VLESS (h2, http, bytes, SHA-1, tokio-rustls) |
| `vmess` | VMess's AEAD form (AES, HMAC-SHA-256, MD5, the AEADs of `pod-tls`'s feature `pure`); turns on `vless` |
| `trojan` | Trojan (SHA-224), its client and its accept side's parser; turns on `vless` |
| `shadowsocks` | Shadowsocks (MD5, HKDF-SHA1, BLAKE3 in its portable code, the AEADs of `pod-tls`'s feature `pure`) |
| `eice` | the road of EC2 Instance Connect Endpoint (HMAC-SHA-256 and SHA-256 of SigV4, zeroize) |
| `iap` | the road of IAP for TCP (zeroize) |
| `plain-ws` | `cloudflare::connect_loopback`, for the tests; no binary turns it on |
| `fuzzing` | the readers of the fuzz targets `wisp_packet`, `iap_message` and `trojan_request_accept` (turns on `wisp`, `iap` and `trojan`); no consumer turns it on |

No feature is on by default. Each one is pure Rust.

## Settings

| Setting | Where | What |
| --- | --- | --- |
| the way out | `Dial::way`, `Connect::way`, `Reach::way`, the `way` of `cloudflare::connect`, `eice::connect` and `iap::connect` | the dialer (direct, the host's proxy), the TLS context and its words, the user agent |
| the trust | `trust` of each | the roots of each TLS connection |
| the bounds | `timeout`, `wait` | each step of a dial |
| the end of input | `at_end` | a Close at once, or after the peer's end |
| the wait for an answer | `answer`, `silence_is_ready` of VLESS's and Shadowsocks's `Dial` | how long a road waits for the server's first bytes, and whether a silence is a stream (a pipe) |
| early data | `Dial::early` of VLESS's, VMess's and Trojan's `Dial` | `Some(early::Early)` keeps the nonce in the session's `ReplayWindow` and announces the link's `ed` size at 0-RTT, or keeps 1-RTT where the nonce is a repeat; `None` sends none |
| the notes | `note` | the sink of a peer's own words (`egress.note`, `wisp.note`), and of the steps of an IAP session (`iap.lost`, a warning; `iap.resumed`; `iap.step`, for a verbose log) |
| the credentials | `eice::Credentials`, the `token` of `iap::connect` | AWS's key (and its session token) that signs each URL; Google's OAuth token of each link, which the `Dial` of `iap::carry` asks for again, as one may expire |
| the credentials file | `via::Dial::auth_file` | how the user names it in the words of a `407` |

The crate reads no variable of the environment. The app's name in the
words of a refusal is the `app` of the TLS words of the way
(`pod_tls::Words`); the user agent is the way's.

## Failures

`via` returns `pod_proxy::DialError` (its stage and class are pod-proxy's).
Its TLS to the second proxy is `pod_proxy::hop::tls`, the hop of an
`https://` proxy of the host: a refused handshake is `ProxyUnreachable`
(`TLS: ` and the words of pod-tls); a proxy that chose no protocol of the
offer (the alert `no_application_protocol`) is `ProxyProtocol` (`it did not
choose HTTP/2 (h2) by ALPN, which was asked for: ...`).
`ForwarderError`: `Target` is `Connect`/`Usage`; `Connect` is the stage and
the class of `pod_ws::ConnectError`; `Answer` is `WebSocket`/`Protocol`;
`Closed` (the forwarder's Close before the target's bytes) is
`Origin`/`Unavailable`. `WispError`: `Opening` is `WebSocket`/`Protocol`;
`Refused` is `Origin`/`Refused`, and `Relay`/`Refused` with 0xc0 to 0xc2
(a credential); `Closed` with 0x48 is `Origin`/`Policy`,
another `Closed` is `Origin`/`Unavailable`; `Gone` and `Failed` are
`WebSocket`/`Unavailable`. `WstunnelError`: `Target` is `Connect`/`Usage`;
`Refused` (each 400 of wstunnel) is `WebSocket`/`Refused`; `NotWstunnel` is
`WebSocket`/`Protocol`. The server of VLESS, VMess, Trojan and
Shadowsocks is the relay of its road: `Target` is `Connect`/`Usage`; `Dial`
is the stage and the class of `pod_proxy::DialError`; `Carry` and
Shadowsocks's `WebSocket` (TLS, the upgrade, HTTP/2) are
`WebSocket`/`Unavailable`; `Refused` (an HTTP status) is
`WebSocket`/`Refused`; VLESS's `Http1` is `WebSocket`/`Unsupported`; an end
before the answer (`Ended`, Trojan's `Fallback`, Shadowsocks's `Silent` with
an end) is `Relay`/`Refused`; an answer of another protocol (`NotVless`,
VMess's `Wrong`, Shadowsocks's `Chunk`) is `Relay`/`Protocol`; no answer in
the wait (`Silent`) is `Relay`/`Timeout`; Shadowsocks's `Send` and `Cut` are
`Relay`/`Unavailable`. meek_lite, WebTunnel and `pt` keep podssh's
`String`. `eice::connect` and `iap::connect` return pod-ws's
`ConnectError`, whose stage and class are pod-ws's; a refusal's body shows
no part of a credential. `iap::carry::NotOpen`: `Dial` is the dial's own
failure; `Closed` has the class of its code (`close::stage_and_class`:
4033 and 4004 are `WebSocket`/`Refused`, 4003 and 1000 `Origin`/
`Unavailable`, 4047 and 4051 `Origin`/`NotFound`, 4001, 4002 and 4074
`Session`/`Unavailable`, 4005 to 4008 and 4013 `WebSocket`/`Protocol`,
another `WebSocket`/`Unavailable`); `Silent` is `WebSocket`/`Timeout`;
`Fault` is `WebSocket`/`Protocol`. `iap::carry::Ended`: `Normal` is no
failure; `Closed` as above; `Lost` is `Session`/`Unavailable`; `Fault` is
`WebSocket`/`Protocol`.

## Parity

Each line of the source, as it is here:

- [x] via: the URL, the credentials file, the versions offered, HTTP/1.1's
  and HTTP/2's CONNECT, one HTTP/2 connection for each tunnel that follows,
  HTTP/1.1 after a reset of HTTP/2, the words of a `407` and of a proxy
  with probe resistance;
- [x] the forwarders: the template's forms, the target checked before it
  goes in, the first frame and its answer, the early wait of the query
  form, the notes, the Close before the target's bytes, the built-in ones;
- [x] the carrier of a session: frames of 32 KiB, the notes, the first word,
  the status, the liveness, text frames as bytes (frps);
- [x] the end of input: `Now` and `AfterPeer`;
- [x] Wisp: versions 1 and 2, the credit, the streams, the reasons, the
  message of the day, stream open confirmation, the password extension,
  the forwarder's road, a road that opens a new connection after the end;
- [x] wstunnel: the server's URL, the secret file, the token and its
  claims, the upgrade and the words of each version;
- [x] meek_lite: the fronts, the polls, the session id, the exchanges and
  their tries;
- [x] WebTunnel: the arguments, the pin of the certificates, the upgrade;
- [x] the Access client: the upgrade of cloudflared, the credentials in
  their headers, a refusal of Access with no credential in it;
- [x] VLESS: the share link and its ID, `extra`, the request and the
  answer, the WebSocket, XHTTP's stream-one, stream-up and packet-up on
  HTTP/2, packet-up and stream-up on HTTP/1.1 and two connections, the
  padding, the wait for the answer and a pipe's silence;
- [x] VMess: v2rayN's line, the AEAD header and its auth ID of the clock,
  the body's ciphers, the answer, the WebSocket;
- [x] Trojan: the share link and its password, the request, TLS alone and
  a WebSocket over TLS, the fallback told from the target;
- [x] Shadowsocks: the AEAD form and SIP022, `ss://` of SIP002, the
  subscriptions (SIP008 and lists), Outline's access keys and their YAML,
  a key file, Outline's salt prefix, a server in a WebSocket, a silence
  that names a proxy, the half-close.
- [x] EICE: the endpoint by its DNS name, SigV4 in a query string as
  botocore signs it, the URL signed once TLS is up and kept in memory that
  is wiped, awscrt's upgrade, the largest frame, a refusal with no part of
  the credential or of the URL;
- [x] IAP: the target's names as GCP writes them, gcloud's connect and
  reconnect, the token in its header alone, IAP's messages, the
  acknowledgements, a session that a new link takes up (three tries, from
  1.5 s), the fate of each Close, the end of input with the server's last
  bytes, a refusal with no part of the token.

The differences: the way out is a `pod_ws::Way` (podssh: a `ProxyChoice`,
the process's TLS context and `podssh/VERSION`); a peer's words go to
`pod_core::Events` (podssh: `Arc<dyn Fn(&str)>`); the structs of a road's
settings own their values (podssh: references); the built-in forwarders are
a function (podssh: a static); the names of the caller are parameters (the
app of the TLS words, `auth_file`); the entropy is pod-core's (podssh:
pod-tls's `OsRandom`); via's stream of HTTP/2 is pod-ws's; each road that
takes a target is a `pod_core::Dial` and its error a `pod_core::Failure`
(new). In part 2: the user agent of a request of VLESS, VMess and Trojan is
the way's (podssh: `url::USER_AGENT`); the proxy that a silence of
Shadowsocks names is the way's dialer's `proxy_label` (podssh: the
environment's proxy); the words that named podssh name "this client";
VLESS's, VMess's and Trojan's carriers share one dial of TCP and TLS
(`vless::ws::connect`). In part 3: the user agent of EICE and IAP is the way's (podssh: the constants
`USER_AGENT`); the IAP carrier's lines are events of the caller's `Note`
(podssh: `Say`, a `Fn(Line)` of `Info` and `Verbose`); `NotOpen` and
`Ended` of IAP have words, a stage and a class (new).

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podssh | `podssh_ws::{via, forwarder, wstunnel, wisp, meek, webtunnel, pt, cloudflare}` | the same modules of podssh-ws, each `pub use pod_egress::ROAD::*` with podssh's glue: `via::dial(trust, &proxy, timeout, offer)` and `AUTH_FILE`; `forwarder::dial(&trust, &proxy, timeout, at_end, note)`, `forwarder::note(say)`, `forwarder::silent()`; `wstunnel::dial(&trust, &proxy, timeout, at_end)`; `wisp::connect_with(&trust, &proxy, timeout, note)`; `pt::reach(&proxy, trust, wait)`; `cloudflare::connect(host, access, trust, proxy, timeout, idle)` and `USER_AGENT` |
| podssh | `forwarder::carry`, `forwarder::end` (crate-private) | `pod_egress::carry`, `pod_egress::end`, re-exported in podssh's `forwarder` for chisel, upterm, frp, shadowsocks, vless |
| podssh | `via::Dial { trust, proxy, timeout, offer }`, `forwarder::Dial { trust, proxy, .. }`, `wstunnel::Dial { trust, proxy, .. }`, `wisp::Connect { trust, proxy, .. }`, `pt::Reach { proxy, trust, wait }` | the glue functions above, which build each with `podssh_ws::client::way(&proxy)` |
| podssh | `forwarder::Note` of `Arc::new(\|text: &str\| ..)` | `forwarder::note(\|text\| ..)`, or `forwarder::silent()` |
| podssh | `forwarder::builtin::BUILTIN` | `forwarder::builtin::builtin()`; `lookup` gives a `Builtin` by value |
| podssh | `via::Stream::from_parts(send, recv)` (vless) | `pod_ws::h2_road::duplex(send, recv)` |
| podssh | `cloudflare::connect_loopback(host, port, access, timeout, idle)` | `.., access, user_agent, timeout, idle` (feature `plain-ws`) |
| podssh | `podssh_ws::{eice, iap}` | the same modules of podssh-ws, each `pub use pod_egress::ROAD::*` with podssh's glue: `eice::connect(endpoint, target, credentials, trust, &proxy, timeout)` and `iap::connect(target, resume, token, trust, &proxy, timeout)` over `podssh_ws::client::way(&proxy)`, each `USER_AGENT` |
| podssh | `iap::carry::{Say, Line}` and `carry::open(dial, say)` | podssh's glue keeps `Say` and `Line`: `iap::carry::open(dial, say)` gives the crate a `Note` that turns `iap.lost` and `iap.resumed` into `Line::Info`, and `iap.step` into `Line::Verbose` |
| podssh | `wstunnel::upgrade::request(host, secret, token, key)`, `upgrade(stream, request, key)` | `.., key, user_agent)`, `.., key, app)` |
| podssh | `tests/via.rs`, `forwarder.rs`, `live_forwarder.rs`, `meek.rs`, `webtunnel.rs`, `wisp.rs`, `wisp_support/`, `wstunnel.rs`, `cloudflare_access.rs` | the tests of this crate; `tests/common/talker.rs` stays in podssh for its other roads |
| podssh | the plants `t438-wisp-credit`, `t439-via-no-credential`, `t441-wstunnel-wrong-secret` | `egress-wisp-credit`, `egress-via-no-credential`, `egress-wstunnel-wrong-secret` |
| podssh | the unit tests of `eice/` and `iap/` (`tests.rs`, `sigv4/tests.rs`, `carry/tests.rs`, and those of `close.rs`, `flow.rs`, `frame.rs`) | the same tests of this crate; podssh-cli's `eice_cli.rs`, `iap_cli.rs` and `cloud_support/` drive its binary and stay |
| podssh | `PODSSH_TEST_WISP_PYTHON`, `PODSSH_TEST_WISP_REQUIRED`, `PODSSH_TEST_FORWARDER_PLANT` | `POD_EGRESS_TEST_WISP_PYTHON`, `POD_EGRESS_TEST_WISP_REQUIRED`, `POD_EGRESS_TEST_FORWARDER_PLANT` |
| podssh | PodUtils/podssh:.github/workflows/forwarder-live.yml | `.github/workflows/forwarder-live.yml` of podcrates; podssh's `via-interop.yml`, `wstunnel-interop.yml` and `wisp-rate.yml` drive its binary and stay |
| podssh | `podssh_ws::{vless, vmess, trojan, shadowsocks}` (part 2) | the same modules of podssh-ws, each `pub use pod_egress::ROAD::*` with podssh's glue: `vless::dial(&trust, &proxy, timeout, answer, silence_is_ready, at_end)` (for VLESS, VMess and Trojan), `shadowsocks::dial(&trust, &proxy, timeout, answer, silence_is_ready)` |
| podssh | `vless::Dial { trust: &trust, proxy: &proxy, .. }`, `shadowsocks::Dial { proxy, trust, .. }` (podssh-cli, `proxy_nodes`) | `Dial { way: podssh_ws::client::way(&proxy), trust: trust.clone(), .. }`, or the glue functions above |
| podssh | `proxy_nodes::carry(nodes, host, port, &Dial<'_>, ..)` | `&Dial`: the same, with no lifetime |
| podssh | `tests/vless.rs`, `vmess.rs`, `trojan.rs`, `shadowsocks.rs`, `shadowsocks_2022.rs` | the tests of this crate; `tests/vless_support/` and `trojan_support/` stay in podssh for `proxy_nodes.rs`, `hysteria.rs` and `tuic.rs` |
| podssh | the plant `t440-vless-refusal-unsaid` | `egress-vless-refusal-unsaid` |
| podssh | `PODSSH_XRAY_TEST_BIN` of `tests/vmess.rs` | `POD_EGRESS_XRAY_TEST_BIN` |
| podssh | PodUtils/podssh:.github/workflows/vmess-interop.yml | `.github/workflows/vmess-interop.yml` of podcrates; podssh's `vless-interop.yml`, `trojan-interop.yml`, `outline-interop.yml` and `proxy-nodes-live.yml` drive its binary and stay |
| podssh | the dependencies `hkdf`, `blake3`, `aes-gcm` and `chacha20poly1305` (dev) of podssh-ws | gone from podssh-ws: they serve this crate's roads |

Part 1 is in `v0.51.0`; parts 2 and 3 are in `v0.52.0`.

## Tests

`cargo test -p pod-egress --all-features`. The unit tests: via's URL, its
credentials file, its offers, the words of a `407` and of a refusal, the
requests of HTTP/1.1 and HTTP/2; the coalescer's first write, its padding
and its once-only rule; the replay window's 10 minutes; the forwarder's
templates, the lines of its failures, the built-in ones; the carrier's
notes, its text frames, its status, the answer of the frame form; the end
of input; Wisp's packets, its opening, its errors and the forwarder's road;
wstunnel's server, secret, token (against Python's vectors), request and
words; meek_lite's polls, session id, arguments and carry; WebTunnel's
arguments, pin, request and answer; the Access client's request; VLESS's
link, ID, `extra`, header, answer, upgrade, XHTTP's requests and HTTP/1.1;
VMess's line, KDF, header (against Xray-core's vectors) and body, and the
fixture of `pod_testkit::vectors::VMESS` with its `alterId` rule; Trojan's
link, request, the UDP associate of `capability` and the fallback's answer;
Shadowsocks's methods, keys, codec, SIP022, `ss://`, subscriptions, access
keys and their YAML; EICE's endpoint, its presigned path
against aws-cli's, SigV4 against the vectors of aws-c-auth and aws-cli,
its upgrade, and no credential in a refusal or in `Debug`; IAP's upgrade
and reconnect against gcloud's, the target's names, the token's checks,
a refusal with no part of the token, the fate and the class of each
Close, the flow, the messages, and the carrier against a stand-in in
memory (both ways, a lost link taken up, no session, a bad
acknowledgement, the end of input).

The tests of each road against stand-ins on the loopback (the tests of
`via`, `wstunnel`, `vless`, `vmess`, `trojan`, `wisp` and `shadowsocks`
take the exported ones of the feature `standin`; `tests/*_support` keep the
certificates of the tests, the way out and the targets):

- `tests/via.rs`: `the_credential_goes_with_each_connect_and_a_407_says_what_is_missing`,
  `two_tunnels_share_one_http2_connection`,
  `the_host_s_proxy_carries_the_tls_to_the_second_proxy`,
  `a_connect_that_http2_resets_goes_over_http1`,
  `a_refusal_an_unreachable_target_and_another_name_fail_with_their_words`,
  `the_client_is_a_dialer_and_a_refusal_is_a_typed_failure`;
- `tests/forwarder.rs`: the query and the frame forms, a Close before the
  target's bytes, a failed answer, a pipe's end of input, a target that
  would change the URL, and
  `a_forwarder_is_a_dialer_and_its_close_is_a_typed_failure`;
- `tests/wisp.rs` (with `tests/wisp_support/`): two streams at once, the
  credit, a closed stream, version 2, a credential refused, the
  forwarder's road, a pipe's end of input,
  `a_wisp_road_is_a_dialer_and_a_blocked_target_is_a_typed_failure`, and
  two tests against wisp-server-python where it is installed
  (`POD_EGRESS_TEST_WISP_PYTHON`; `POD_EGRESS_TEST_WISP_REQUIRED` makes a
  missing one a failure);
- `tests/wisp_more.rs`: the road that opens a new connection after the end
  of the old one, the bound on the tries and the backoff, a refused
  credential tried once, the password extension and a wrong password, a
  login over `ws://` refused;
- `tests/wstunnel.rs`: each server of 9.1 to 11, the secret, the words of
  another version and of another server, a pipe's end of input, `wss://`,
  `a_tunnel_is_a_dialer_and_a_refusal_is_a_typed_failure`;
- `tests/meek.rs`, `tests/webtunnel.rs`: the stand-ins of meek-server and
  of the WebTunnel server;
- `tests/cloudflare_access.rs` (feature `plain-ws`): the upgrade of
  cloudflared line by line, messages of 16 KiB, no Ping, a refusal of
  Access;
- `tests/vless.rs` (with `tests/vless_support/`, the stand-ins of
  Xray-core's server over a WebSocket, XHTTP on h2's HTTP/2 server and
  HTTP/1.1 alone): a WebSocket and each mode of XHTTP, a wrong ID, a
  refused upgrade, a wrong path and HTTP/1.1, packet-up and stream-up on
  two connections, a pipe, a pipe's end of input, the host's proxy, the first
  write of 3,333 bytes over the WebSocket and each XHTTP path, early data
  announced once and then at 1-RTT on one connection,
  `a_vless_road_is_a_dialer_and_a_wrong_id_is_a_typed_failure`;
- `tests/trojan.rs` (with `tests/trojan_support/`, the stand-ins of
  trojan-gfw's server): TLS alone and a WebSocket, a wrong password and a
  wrong path, a pipe's end of input, a pipe, the published requests of
  `pod_testkit::vectors::TROJAN`,
  `a_trojan_road_is_a_dialer_and_a_wrong_password_is_a_typed_failure`;
- `tests/shadowsocks.rs`, `tests/shadowsocks_2022.rs`: the published
  SIP004 answers of `pod_testkit::vectors` run against this crate's codec,
  the master key and the stream key of Python and OpenSSL, a codec of the
  specification on the loopback, a wrong password or key, the half-close, a
  pipe, the URIs and the lists, a server 60 s off,
  `a_shadowsocks_road_is_a_dialer_and_a_silence_is_a_typed_failure`;
- `tests/shadowsocks_outline.rs`: a key file that builds the whole node and
  shows no path, a key file that others can read, the salt prefix on the
  wire and read by the stand-in server, a prefix that leaves too few random
  bytes;
- `tests/vmess.rs`, against Xray-core (`vmess-interop.yml`, each day;
  `POD_EGRESS_XRAY_TEST_BIN` names it, and each test says "did not run"
  with none): each cipher, a pipe, a clock 5 minutes off, another ID and
  another path;
- `tests/eice.rs`: `an_eice_tunnel_goes_through_the_way_with_a_url_signed_for_it`,
  `an_eice_refusal_is_a_typed_failure_and_shows_no_credential`,
  `an_eice_credential_that_cannot_go_in_a_url_is_refused_before_a_dial`,
  against a stand-in of an endpoint with TLS for its name; the way's
  dialer takes port 443 to the stand-in (`tests/common/way.rs`, `aimed`);
- `tests/iap.rs`: `an_iap_session_goes_through_the_way_with_its_token_in_a_header`,
  `an_iap_refusal_of_the_account_is_a_typed_failure`, against a stand-in
  of IAP's relay, the same way;
- live (`#[ignore]`, `forwarder-live.yml`, each day):
  `tests/live_forwarder.rs`, GitHub's version line through each built-in
  forwarder; `POD_EGRESS_TEST_FORWARDER_PLANT` makes each fail.

The plants (`scripts/plants/`, group `egress`): `egress-wisp-credit`,
`egress-via-no-credential`, `egress-wstunnel-wrong-secret`,
`egress-via-dial-untyped`, `egress-forwarder-close-untyped`,
`egress-wstunnel-refusal-untyped`, `egress-wisp-blocked-untyped`,
`egress-vless-refusal-unsaid`, `egress-vless-dial-untyped`,
`egress-trojan-dial-untyped`, `egress-shadowsocks-dial-untyped`.
`egress-eice-refusal-shows-credential`, `egress-eice-credential-unchecked`,
`egress-iap-refusal-untyped`, `egress-iap-refusal-shows-token`.

The fuzz targets `wisp_packet`, `iap_message` and `trojan_request_accept` (`fuzz/`, feature
`fuzzing`).

## Sources

`SOURCES` lists each file of podssh at `2ffb7cd7` and the file that it
became.
