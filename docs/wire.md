# The wire of the relay of PodUtils

Wire version: 2

This page gives the bytes of the relay's roads of PodUtils in one place, with
a version. It is for another implementation: podnet, a Worker, a relay of
another language. Each rule is one short sentence. The prose and the reasons
are in `PodUtils/podssh:docs/relay.md`, `PodUtils/podssh:docs/reverse.md`
and `PodUtils/podssh:docs/design.md` (section 5). The default relay's own contract is pinned at
`crates/pod-relay/tests/spec/relay-spec-2026-10-03-r2.txt`; where this
page and that copy differ, the copy is what the default relay does.

Test vectors are in `crates/pod-relay/tests/vectors/`. Two readers check
them: the test `crates/pod-relay/tests/wire_vectors.rs`, through the codecs
of `pod-session`, `pod-relay` and `pod-ws`, and `scripts/check-wire-vectors.py`,
written from this page alone. The test skips the vectors of `tokens.json`:
the relay role makes and takes these tokens, and podcrates has no relay role
yet (T-036, T-856). The script reads each vector. Section 7 gives the form of
the vectors, and section 8 the versions.

## 1. Conventions

- Numbers on the wire are big-endian.
- "u8", "u16", "u32" and "u64" are unsigned integers of 1, 2, 4 and 8 bytes.
- Text is UTF-8.
- Hex in the vectors is lower-case, two digits a byte.
- base64url is RFC 4648, section 5, with no padding.

## 2. The forward road

- The client opens a WebSocket (RFC 6455) to `/connect/<host>/<port>`.
- The upgrade is HTTP/1.1, over TLS on port 443.
- The token goes in the header `X-Relay-Token`.
- `?token=` and `/t/<token>/...` exist for a client with no headers; podssh never uses them.
- An IPv6 address is written bare in the path: `/connect/2001:db8::1/22`.
- The relay dials the target before the upgrade; a target it cannot reach is a plain HTTP `502`.
- After the `101`, binary frames carry the raw TCP stream, both ways.
- There is no prefix, no control message and no subprotocol.
- Frame boundaries mean nothing; the receiver joins the bytes.
- A client masks each frame it sends, with a fresh key (RFC 6455, section 5.3).
- A server masks no frame.
- A length takes the shortest of its three forms (7 bits, 16 bits, 64 bits).
- No reserved bit is set; no extension is negotiated.
- A zero-length binary frame is a keepalive; the receiver skips it.
- The relay's largest frame is 262144 bytes; podssh sends 32 KiB at most in one.
- A Close payload is a u16 code, then a reason of 123 bytes at most.
- A normal end is `1000`; the other forward codes are in `PodUtils/podssh:docs/relay.md`, "Errors and close codes".
- The relay role also serves `/v1/layer/<host>/<port>`: the same upgrade, and each link carries the layer of section 5.

## 3. Tokens: `/v1/mint`

- `POST /v1/mint` with the body `{}` answers `200` and `{token, expires, scope}`.
- `expires` is milliseconds since the epoch; `scope` is `"forward"`.
- Other answers: `405` method, `400` body, `413` body too large, `429` with `Retry-After`, `503` no minting.
- A forward token is `ephm1.` EXPIRY `.forward.` MAC.
- EXPIRY is the decimal milliseconds since the epoch at which it ends.
- At podssh's relay role, MAC is base64url of HMAC-SHA256 under the relay's 32-byte secret.
- The message of the MAC is the ASCII text `ephm1.` EXPIRY `.forward`.
- A token lives 72 hours (259200000 ms) at most; a longer life is cut to 72 hours.
- A relay takes a token while `now < EXPIRY <= now + 259200000` and its MAC is right.
- Each upgrade checks the token once; no frame is checked.
- `POST /v1/mint?once=1` at the relay role gives a token of one upgrade, `once1.` EXPIRY `.` NONCE `.` MAC, and `"once": true`.
- Its NONCE is 16 random bytes in base64url; its MAC is over `once1.` EXPIRY `.` NONCE; it lives 5 minutes.
- Another relay may make its MAC another way; a client treats a token as opaque text of 8 to 4096 printable ASCII bytes.
- Never write a whole token in a log, a URL or a document.

## 4. The reverse road

### Pairs

- `POST /v1/pair` with `{}` answers `{name, node_token, connect_token, stop_token, expires}`.
- `expires` is milliseconds since the epoch, 72 hours ahead at most.
- A client refuses a pair with under 10 minutes left.
- The relay role adds `"once": true` to a pair whose connect token takes one upgrade.
- `GET /v1/status/<name>` with the connect token answers `{"online": BOOL, "sessions": N}`; the relay role adds `"expires": MS`.
- `POST /v1/stop/<name>` with the stop token answers `{"stopped": BOOL, "sessions": N}`.
- A failed authentication is `403 reverse: forbidden`, whichever token failed.

### Sockets

- The node opens `/v1/node/<name>` with `node_token` in `X-Relay-Token`.
- The operator opens `/v1/connect/<name>` with `connect_token`.
- Each upgrade may state the road's dialect in `X-Podssh-Proto`; none means dialect 1.
- A relay that does not read the stated dialect answers `426`, its own dialect in `X-Podssh-Proto`.
- One node socket holds a name; a second gets `409`.
- A request with no upgrade gets `426`.

### Frames of the node

- A binary frame of the node is the session id, then the payload, in ONE frame.
- The session id is 32 ASCII characters of `0-9a-f`.
- The payload is 0 to 65536 bytes; the frame is 32 to 65568 bytes.
- A frame under 32 bytes or over 65568 is closed `1009 bad multiplex frame`.
- An id with a byte outside `0-9a-f` (an upper-case letter too) is closed `1003 bad multiplex id`.
- A stream is cut into payloads of 65536 bytes at most; each frame carries the whole id.
- Bytes of none are one frame of the id alone.

### Frames of the operator

- A binary frame of the operator is the payload alone, 65536 bytes at most.
- A longer one is closed `1009 frame byte cap`.
- The relay reads no id in it: 32 hex characters at its start are payload.
- The operator never sends a text frame; one is closed `1003 binary frames required`.

### Control messages

- Control is a text frame of JSON, 4096 bytes at most, on the node's socket.
- Its member `type` names it; a reader keeps the members it does not know.
- The relay sends the node `hello`, `open {id}` and `close {id}`; the relay role also sends `spent {digest}`.
- `hello` may carry `version` (the dialect), `maxFrameBytes` and `maxSessions`; each is optional.
- A node with no `hello` takes 16 sessions and 65536 bytes.
- A `type` that a node does not know is read as unknown, never as a fault.
- The node sends `ready {id}`, `reject {id, reason}` or `close {id}`, with an optional `reason` in `close`.
- The node writes members in this order: `type`, `id`, then `reason`.
- The JSON has no space; a `"` in a reason is `\"`; other text is UTF-8 as it is.
- A `reject` or `close` whose frame would pass 4096 bytes has its reason halved, at a character boundary, until it fits.
- The node sends `ready` only once its local side has accepted.
- The operator receives `ready`, `reject` and `close` as text frames.

### Close codes

- A Close payload is a u16 code, then the reason in UTF-8.
- The relay cuts its own reason to 100 characters and 123 bytes.
- A client matches a row by the code and the reason, trimmed and in lower case.
- Else, for code 1000 or 1011, a reason that is `node-supplied`, `node supplied`, or exactly one of the node's refusals, is the row "node-supplied" of that code.
- The node's refusals, matched as written (case counts): `connection refused`, `refused`, `rejected`, `closed`, `target closed`, `no route to host`, `not listening`, `eof`, `session closed`.
- Else, codes 1008, 1009 and 1013 alone take their first row of the table.
- Else no row matches: the client does not retry.
- "Retry" says what may come again; "Resumes" says whether the layer carries its session on.

| Code | Reason | Retry | Resumes |
| --- | --- | --- | --- |
| 1001 | operator stopped reverse relay | never | no |
| 1001 | pair expired | new-pair | no |
| 1003 | unknown session id | new-session | yes |
| 1003 | invalid control json | never | no |
| 1003 | invalid session id | never | no |
| 1003 | unknown control type | never | no |
| 1003 | bad multiplex id | never | no |
| 1003 | data before ready | never | no |
| 1003 | binary frames required | never | no |
| 1008 | unknown role | reconnect | yes |
| 1008 | wait for ready | new-session | no |
| 1009 | control frame byte cap | never | no |
| 1009 | bad multiplex frame | never | no |
| 1009 | session byte cap | new-session | yes |
| 1009 | frame byte cap | never | no |
| 1011 | relay backpressure | never | yes |
| 1011 | node unavailable | reconnect | yes |
| 1011 | node offline | reconnect | yes |
| 1011 | node disconnected | reconnect | yes |
| 1011 | socket error | reconnect | yes |
| 1013 | node open timeout | new-session | yes |
| 1013 | reverse message rate cap | reconnect | yes |
| 1000 | node-supplied | never | yes |
| 1011 | node-supplied | never | yes |
| 1000 | session ended | never | yes |

- A close that matches no row resumes, but for codes 1001 to 1003, 1007 to 1010, and 4000 and up.

## 5. The resumable layer

### Records

- A record is a u8 type, a u32 length, then a body of that length.
- The length is 65536 at most; a larger one is refused.
- A type outside `0x01` to `0x0c` is refused at its first byte.
- A body with bytes after its last field, or cut short, is refused.
- A decoder waits for more bytes while a header or a body is incomplete.
- An error ends the link; the layer resumes the session on a new one.
- No type byte is printable, so an SSH server's first line is no record.

| Type | Record | Body |
| --- | --- | --- |
| 0x01 | GREETING | magic, version u8, role u8, nonce, features |
| 0x02 | OPEN | magic, version u8, role u8, nonce, then 0x00 and a public key (new) or 0x01 and a session id (resume), then features |
| 0x03 | ACCEPT | 0x00, a session id and a public key (new); or 0x01, an offset u64 and a proof (resume) |
| 0x04 | PROOF | an offset u64 and a proof |
| 0x05 | REFUSE | a code u8, then a reason |
| 0x06 | DATA | an offset u64, then the bytes (none or more) |
| 0x07 | ACK | an offset u64 |
| 0x08 | PING | a value u64 |
| 0x09 | PONG | a value u64, then an offset u64 |
| 0x0a | CLOSE | a reason |
| 0x0b | RETIRE | nothing |
| 0x0c | END | an offset u64 |

- The magic is the 14 ASCII bytes `podssh-session`; any other is refused.
- The version is 1; each side names the highest it speaks.
- The roles are 1 a client, 2 a node, 3 a relay role or `podssh serve`.
- A nonce, a public key and a proof are 32 bytes; a session id is 16.
- Features are a count u8 (16 at most), then each name as a length u8 and the name.
- A feature name is 1 to 32 bytes of `a-z`, `0-9`, `.`, `-` and `_`; another is refused.
- A side ignores a feature name that it does not know.
- Names of this version: `replay.v1`, `heartbeat.v1`, `move.v1`, `keep.v1`, `once.v1`, `end.v1`, `deadline.SECONDS`.
- A reason is the rest of the body, UTF-8, 1024 bytes at most.
- The REFUSE codes: 0 other, 1 unknown session, 2 wrong proof, 3 offset no longer kept, 4 version, 5 role, 6 busy, 7 protocol.
- A code or a role of a later version still decodes.

### The handshake

- The far end sends GREETING; the client sends OPEN; neither waits for the other.
- A new session: OPEN carries the client's X25519 public key; ACCEPT gives the id and the far end's key.
- The secret is HKDF-SHA256 (RFC 5869) of 32 bytes.
- Its salt is the far end's nonce, then the client's nonce.
- Its input is the X25519 shared value.
- Its info is `podssh-session v1 secret`, a 0x00 byte, the id, the client's public key, then the far end's.
- A public key of low order is refused at both ends.
- A resume: OPEN names the session id; the client then sends PROOF.
- A proof is HMAC-SHA256 under the secret.
- Its message is a label, a 0x00 byte, the id, the far end's nonce, the client's nonce, then the prover's received offset as u64.
- The client's label is `podssh-session v1 client proof`; the far end's is `podssh-session v1 far proof`.
- The nonces are those of the link that carries the proof.
- The far end answers a good PROOF with ACCEPT (resume): its received offset and its own proof.
- Each side then sends again from the other's offset.

### After the handshake

- Each side numbers its bytes from offset 0, across each link.
- DATA below the next expected offset loses its overlap; DATA above it ends the link.
- ACK, PONG and END carry offsets as described in the table.
- CLOSE ends the session; the peer answers with its own CLOSE.
- RETIRE ends this link alone.
- END ends one direction; the session goes on for the other.

## 6. Grants

- A grant is an OpenSSH user certificate of an Ed25519 key, signed by an Ed25519 key (OpenSSH's PROTOCOL.certkeys).
- A "string" is a u32 length, then its bytes.
- The certificate is, in order: the string `ssh-ed25519-cert-v01@openssh.com`, the nonce string, the key string (32 bytes), the serial u64, the type u32, the key id string, the principals string, valid-after u64, valid-before u64, the critical options string, the extensions string, the reserved string, the issuer's key string, then the signature string.
- The type of a grant is 1, a user certificate.
- The principals string holds a string for each principal.
- The options string holds, for each option, its name string, then its data string, in the order of their names.
- A grant has the critical option `once@podssh`, with empty data.
- A grant has no extension, and the reserved string is empty.
- The issuer's key string holds the string `ssh-ed25519`, then the 32-byte key string.
- The signature string holds the string `ssh-ed25519`, then the 64-byte signature string.
- The signature is Ed25519 (RFC 8032) of each byte before the signature string.
- Nothing follows the signature.
- A grant's digest is SHA-256 of the signed bytes; it names the grant once used.
- A principal is `node:` and the node's fingerprint, with `/ROUTE` for a route.
- A fingerprint is `SHA256:` and the base64 (standard, no padding) of SHA-256 of the key's SSH encoding.
- A key's SSH encoding is the string `ssh-ed25519`, then the 32-byte key string.
- An Ed25519 key in the vectors is made from its 32-byte seed, as RFC 8032 makes the public key of a private key.
- The grant travels inside the end-to-end channel, never in a URL.

### Bearers

- A bearer is the other form of a grant: a token that a key of the user signs, bound to one end-to-end channel.
- A bearer is 221 bytes, in order: the magic `pod-bearer/1` and a newline (13 bytes), the signer's key (32), the holder's key (32), `iat` u64, `exp` u64, the nonce (32), the `jti` (32), then the signature (64).
- A certificate starts with a zero byte, so the first byte tells the two forms apart.
- `iat` and `exp` are seconds since the epoch; `exp` is after `iat`, by at most 30 s.
- The holder is the operator's key that the end-to-end channel proves.
- The binding is SHA-256 of `pod-e2e grant binding`, a zero byte, and the Noise handshake hash after the second message of the handshake.
- The `jti` is HMAC-SHA-256 of the nonce, with the binding as the key.
- The signature is Ed25519 (RFC 8032), by the signer's key, of each byte before the signature.
- A node refuses a bearer whose holder is not the channel's operator key, whose signer it does not take, whose signature fails, whose `jti` is not the one of its channel, or whose `jti` it took already.
- A node refuses a bearer whose `iat` is more than 10 s after its clock, or whose `exp` and 10 s passed.
- A node's refusal names the form that it refused: a bearer in the words of a bearer, a certificate in the words of a grant. The reason stays in the node's log.

## 7. The vectors

Each file of `crates/pod-relay/tests/vectors/` is JSON: `wire` (this page's version), `part`, `about`, and `vectors`.
Each vector has a `name` and a `kind`. Integers in the vectors are under 2^53.

### `forward.json`

- `frame`: `writer` (`server` or `client`), `fin`, `opcode`, `payload`, `mask` (a client's), and `hex`, the frame.
- `frame-refused`: `writer` and `hex`; a reader of that writer's frames refuses it.

### `tokens.json`

- `mint`: `secret`, `now_ms`, `lifetime_ms`; the token's `expiry` and `mac`.
- `check`: `secret`, `expiry`, `mac`, `now_ms`, and `valid`, whether the relay takes it.
- The token is `ephm1.` EXPIRY `.forward.` MAC; the files hold its parts alone.

### `reverse.json`

- `node-frame`: `id`, `payload`, and `hex`, the frame.
- `operator-frame`: `payload` and `hex`.
- `node-frame-refused`: `hex`, the `error` (`frame-too-short` or `id-not-lowercase-hex`) and the relay's `close` code.
- `too-long`: `leg`, `payload_len` (of zero bytes), the `error` and the `close` code.
- `chunks`: `id`, `bytes_len` (of zero bytes), and the length of each node frame, `frame_lens`.
- `control-out`: `message`, the members; `text`, the exact JSON the node sends.
- `control-in`: `text` as received, and `message`, the members read; `{"type": "unknown"}` for a type not known.
- `control-cut`: `id`, a reason of `reason_len` bytes of `x` in a `reject`, and the `cut_len` that is sent.

### `closes.json`

- `close`: `code`, `reason`, the Close `payload`, whether a row is `matched`, its `retry` and whether it `resumes`.

### `layer.json`

- `record`: `record` (its name), `fields`, and `hex`, the record.
- The fields are the table's: `version`, `role`, `nonce`, `features`, `opening` (`new` or `resume`), `public`, `id`, `acceptance` (`new` or `resume`), `offset`, `proof`, `code`, `reason`, `bytes`, `value`.
- `refused`: `hex` and the `error`: `unknown-type`, `too-long` or `malformed`.
- `partial`: `hex`, which gives no record and no error.
- `secret`: the `fields` of the exchange, the `secret`, and for each offset of `proofs` the `client` and `far` proofs.

### `grants.json`

- `grant`: the seeds of the issuer, the key and the node; `route`; the fields of section 6; the `digest`; and `hex`, the certificate.
- `critical` is a list of `[name, data in hex]`, in the order of their names.
- `grant-forged`: `hex`, a certificate whose `serial` changed; it reads, and its signature fails.
- `grant-refused`: `hex`, which does not read.
- `bearer`: the seeds of the signer and the holder, the keys `signer` and `holder`, the `handshake_hash`, the `binding`, `iat`, `exp`, `nonce`, `jti`, and `hex`, the bearer.
- `bearer-refused`: `hex`, a `binding`, and the `reason` of the first check that fails, at `iat`, with a signer that the node takes: `form`, `signature`, `life` or `binding`.

## 8. Versions

A change of a vector needs a new version of this page. Add a row; never edit one.
The digest is SHA-256, in hex, of the vector files in the order of their names: for each, its name, a newline, then its bytes with each carriage return removed.

| Version | Date | Digest of the vectors |
| --- | --- | --- |
| 1 | 2026-11-17 | 35d1886cabad3c2027526ac909b285cee92bdbb148d0f3655f7acb6488eb106d |
| 2 | 2026-11-25 | 746b4c220aefa8080f5eeb61475ae02eeef87c0e57dd7ea92ddf5d6c61edd3b2 |
