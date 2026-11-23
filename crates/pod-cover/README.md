# pod-cover

The primitives of a listener that answers as a static site until a client
proves that it holds the key: the cover site and its name routing, the
cover script, the authenticated switch after a real TLS handshake, the
multiplexer of a page load and a session, and the application-layer
delegated credential. New code for PodUtils/podnet#491 (SWITCH-1 to
SWITCH-4, SCRIPT-1 to SCRIPT-3, MUX-1 and MUX-2) and PodUtils/podssh#225
(COVER-1 to COVER-3, DELEG-1 to DELEG-3).

The crate is sans-IO. It opens no socket, reads no clock and keeps no
global state. The caller gives the bytes, the instant and the key. The
only read of the file system is the load of a site tree and of a script,
which a binary does once at start.

## API

| Item | What |
| --- | --- |
| `Site::load(root)` | read `index.html`, the assets, `robots.txt` and `404.html` from a directory. A tree with no `index.html`, a link outside the root, more than 4096 files or more than 64 MiB is refused by name |
| `Site::respond(request)` | answer one HTTP/1.1 request head. GET and HEAD only. A traversal, an unknown path or a bad path gets the site's own 404 page. A HEAD has the same head and no body |
| `Site::set_server(value)` | set the `Server` header. The default is `nginx`. A value that names this project is refused |
| `Sites { names, default }` | the sites of one listener by name, and the site of an unknown name. `Sites::respond(name, request)` sends an unknown name to the default site's own 404 page |
| `Script::load(path)`, `Script::parse(text)` | the cover script as TOML. A missing file, date, cover version or split policy is a named error at load |
| `Script::replay(len)` | a determined `Vec<Record>` of sizes and delays for a payload of `len` bytes. The sizes add up to `len`. The same script and length give the same sequence |
| `Script::split_payload(bytes)` | split a payload by the split policy. The parts add up to the payload |
| `Switch::new(key)` | the HMAC-SHA-256 key that both ends hold |
| `Switch::bind(exporter)` | bind the proof to the exporter of one handshake. Both ends give the same bytes |
| `Switch::prove(nonce, now)` | the proof of a client: the timestamp, the nonce and the tag |
| `Switch::authenticate(bytes, now)` | `Outcome::Session` for a valid proof, `Outcome::Cover` for each other byte. A repeat, an old proof, a future proof, a wrong tag and a short frame are all `Cover` |
| `Mux::new(budget)` | one budget for a page load and a session over one connection |
| `Mux::push(frame)`, `Mux::next(limit)` | add a frame of the lane `Cover` or `Session`, and take the next bytes to write. A cover frame comes before a session frame (MUX-2) |
| `Credential::issue(...)` | sign a delegated credential with the key of the name's certificate |
| `Credential::verify(chain_name, now, exporter, chain_key)` | check the same chain, the same name, the window and the exporter binding |

## The cover script

```toml
date = "2026-10-30"
cover_version = "1.7.2"
seed = 42

[[record_size]]
min = 512
max = 640
count = 50

[[timing]]
micros = 1200
jitter_micros = 80
count = 70

[split]
min = 512
max = 1600
```

`date` and `cover_version` are required. A script with no date is deleted
(SCRIPT-3), so the loader refuses it. `record_size` is the measured
histogram and `timing` is the measured delay after each record. Each
bucket holds a range, a delay and a count. `split` is the record split
policy of the cover: the smallest and the largest TLS record, 16 to
16384 bytes. `seed` makes the replay repeatable. A missing script is an
error at load, never a fall back to a generic stream (SCRIPT-2).

## The switch proof

The proof is 56 bytes: an 8-byte timestamp, a 16-byte nonce and a
32-byte HMAC-SHA-256 tag. The tag covers the timestamp, the nonce, the
length and bytes of the TLS exporter, and the domain `pod-cover switch
v1`. The server accepts a proof from 30 seconds in its future to 240
seconds in its past, keeps each nonce for the window, and refuses a
repeat. Compare of the tag is constant time. A refusal is not a close:
the same bytes go to the cover site (SWITCH-2).

The server must complete a real TLS handshake with the cover site first
(SWITCH-1). The cover site must terminate TLS on the same address, and
the operator must own its name (SWITCH-4). This crate does not open the
listener and does not configure the certificate.

## The delegated credential

The credential is a signed record of six fields: `issuer`, `name`,
`not_before`, `not_after`, `subject_key` and `signature`. It has an
optional `exporter` field, the SHA-256 of a TLS exporter. The server
issues it with the Ed25519 key of the certificate for the name. The
client verifies it against the same name and key, the same window and
the same exporter.

**What it proves.** The holder of `subject_key` received this name, this
window and this binding from the holder of the certificate key. The
signature covers each field, so a changed issuer, name, window, subject
key or binding fails.

**What it does not prove.**

* It is not RFC 9345 and it is not a TLS delegated credential. There is
  no CertificateVerify, no certificate extension and no wire format of
  a TLS handshake. The TLS stack still authenticates the certificate of
  the name.
* It does not prove that the issuer is honest. It proves that the issuer
  key signed these fields.
* It is not single use. A replay inside the window succeeds unless the
  caller also binds the exporter or uses its own nonce.
* It runs after the TLS handshake. A client with no credential still has
  a verified connection to the cover site.

## Name routing

`Sites` routes the name of the connection, from the SNI of the handshake
or from the `Host` header, to one site. An unknown name gets the default
site's own 404 page, never a page of this crate (COVER-1). The bodies
and the headers hold no name of this project, no version and no contract
(COVER-3). A test scans each body for those words.

## Tests

```sh
cargo test -p pod-cover
```

The suite holds a site from a directory, the routing of a known and an
unknown name, the refusal of a traversal and of a link outside the root,
the scan of each body for the forbidden words, a script that reproduces
its histogram within 0.3 percent, the refusal of a missing script, a
valid and a replayed and a wrong proof with the fall-through to the site,
the exporter binding, the cover-first schedule of the multiplexer, and
each named refusal of a credential.

## Dependencies

`pod-core` for the HTTP/1.1 head, `serde` and `toml` for the script,
`hmac` and `sha2` for the proof tag, `ed25519-dalek` for the credential,
and `zeroize` for the switch key. No C, no tokio, no async runtime.

## Not covered

* The TLS acceptor, the certificate chain and the exporter are the
  caller's. A listener and its integration belong to `pod-relay-serve`
  and to podssh.
* The credential has no wire encoding. The caller encodes it for its own
  protocol.
* The multiplexer schedules frames. It does not pace them on a clock and
  it does not implement a congestion controller.
