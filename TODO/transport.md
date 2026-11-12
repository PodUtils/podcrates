This file holds the entries of milestone M1: the crates of the transport
(identity, terminal, TLS, proxies, DNS, WebSocket). Each crate follows the
procedure of `docs/design.md`, section 8. The quiet code of podssh goes
first (`docs/design.md`, section 5).

# T-010: `pod-identity`: the Ed25519 identity, the key files and the entropy

**Source:** `docs/design.md`, section 4 (M1).
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podssh's identity of a node (an Ed25519 seed, its public key, its
fingerprint, its key files) is in `PodUtils/podssh:crates/podssh-relay/src/identity/`.
The layer, the e2e channel and podnet need it.

## Premise

- Read: `identity/` (694 lines) uses `crate::session::Entropy` and the
  private files of `crate::cache` (they go to `pod-core`).
- Tests: `PodUtils/podssh:crates/podssh-relay/tests/identity.rs`,
  `PodUtils/podssh:crates/podssh-relay/tests/identity_file.rs`.

## Approach

The procedure of `docs/design.md`, section 8: import `identity/` and
`Entropy`, `OsEntropy` of `session/`; edition; adapt to `pod-core`; the
tests; the README with the adoption map of podssh.

## Decision

The entropy source goes to `pod-core`, not here: `pod-session` and
`pod-e2e` take it too. The generic part of podssh's `cache` (the private
files, the places, the owners and the access lists) goes to
`pod_core::files`, with the name of the app and its variable as an `App`;
the token cache waits for `pod-relay`. A function that read the places of
the process takes the `Places` of the caller.

## Prove

```sh
cargo test -p pod-identity -p pod-core
sh scripts/agent/check.sh
sh scripts/adoption.sh podssh pod-identity
```

## Start condition

T-005 is done.

## Done

2026-11-05, the commits of this entry (import, adaptation). The import
could not build before its adaptation (it named the modules of
podssh-relay), so the edition 2024 came with the adaptation; clippy is
clean on Linux, Windows and macOS. `cargo test -p pod-identity -p
pod-core`: 70 passed (identity 10, key files 10, foreign trees 3; the
files of `pod-core` 9 and its unit tests). The trial in podssh at
`d4e186d` (its patch is in git at `371b447`; podssh took it in `8caf2d1`): `podssh-relay` re-exports
the crate in its module `identity` with podssh's places; `podssh-relay`
builds with each feature, its tests `identity` (7 without the feature
`pair`), `identity_file` (10), `cache` (21), `e2e` (10) and `e2e_faults`
(10) pass, and `podssh-cli` and `podssh-iroh` build with the features
`iroh` and `warp`.

# T-011: `pod-term`: the line discipline and the grammar of a terminal's output

**Source:** `docs/design.md`, section 4 (M1).
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podssh and podvm share the terminal code of
`PodUtils/podssh:crates/podssh-terminal` (3,691 lines).

## Premise

- Read: the crate has no pod dependency; its tests are
  `PodUtils/podssh:crates/podssh-terminal/tests/discipline.rs` and
  `PodUtils/podssh:crates/podssh-terminal/tests/keys.rs` and the unit
  tests. It is the quietest crate of podssh (one change in 50 commits).

## Approach

The procedure of `docs/design.md`, section 8. The crate is the first
whole move: it is the test of the procedure.

## Prove

```sh
cargo test -p pod-term
sh scripts/agent/check.sh
sh scripts/adoption.sh podssh pod-term
```

## Start condition

T-003 is done.

## Done

2026-11-05, the commits of this entry (import, edition 2024, the
adaptation). `cargo test -p pod-term`: 176 passed, the same tests with
their names. The comments name no record of podssh and no line of the
source of podvm; the test switches are `POD_TERM_PLANT` and
`POD_TERM_TEST_TMUX`. The terminal oracle of its tests moved to
`pod-testkit` (feature `screen`), because the tests of podssh-ws,
podssh-ssh and podssh-cli read it too. The plants C, D and E are caught
(`scripts/plants/term-plant-*.plant`). `examples/static-probe` links it.
The trial in podssh at `d4e186d` (its patch is in git at `2ad40ad`; podssh took it in `bbd4038`):
the record agrees (640 entries), `terminal_oracle` of podssh-ws (3) and
of podssh-ssh (5), `silent_notice` of podssh-ssh (4) and of podssh-cli
(1), and the 17 tests of `session::` of podssh-cli pass.

# T-012: `pod-tls`: trust, the TLS configurations, the pure-Rust provider and ECH

**Source:** `docs/design.md`, section 4 (M1).
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** L
**Status:** done

## Problem

podssh's TLS (its own pure-Rust provider, the stores, the TLS wrapper,
the server side, ECH) and podnet's trust (roots, pins, the fragments of
a ClientHello) are two implementations of one layer.

## Premise

- Read: podssh-ws `crypto/` (2,448), `tls.rs` and `tls/` (922),
  `stores/` (326), `tls_server.rs` and `tls_server/` (378), `bundle.rs`,
  `tls_wrap.rs`, `ech/` (1,021), and podssh-relay `tls_groups.rs`.
- Read: podnet `PodUtils/podnet:src/trust.rs` (495; it calls
  `init_crypto` and names the edge roots of Cloudflare) and
  `PodUtils/podnet:src/tls_fragment.rs` (397).
- RUSTSEC-2023-0071 concerns operations with the private key of `rsa`
  0.9; podssh uses it to verify only.

## Approach

1. The base: podssh's files, with the provider behind the feature `pure`,
   the RSA verifier behind `rsa`, ECH behind `ech`.
2. The merge: podnet's roots, pins and `SSL_CERT_FILE` as settings, the
   ClientHello fragments, extra roots from the caller (the edge roots go
   to `pod-cloudflare`), no process default provider.
3. The parity list; the trials of podssh and podnet.

Step 1 is done: the commits `2bc2e06` to `6ec42f5` (import, edition,
adaptation, tests, README, static probe). Step 2 is done: the commits
`6040431` to `7923ab7` (import of podnet, the module `fragment`, the
roots of podnet, the clock hint and the notAfter dates, the verifier
scan, the README). `cargo test -p pod-tls --all-features`: 154 passed, 7
ignored (live and system-store tests). Step 3 is done: the commits
`de95d00` to `97d32f5` (the 6 plants of podssh's TLS as
`scripts/plants/tls-*`, all caught; the trials). The trial in podssh at
`d4e186d` passes its record, `check-repo.py`, the library crates with no
C compiler, the builds of the default features and of `iroh,warp`,
clippy, and the tests of each crate that uses TLS: podssh-ws 405,
podssh-relay 623, podssh-ssh 2, podssh-cli 1111, 0 failed. The trial in
podnet at `b7682c5` passes its record, the build, clippy, `cargo test
--lib` (716) and the tests `tls`, `egress`, `relay_reverse`, `cli`,
`doctor`, `api_surface`. No trial needed a change of `pod-tls`.

## Decision

- One `pod_tls::Context` (the provider, the `Settings`, the kept
  configurations, the groups, the state of ECH) replaces the six process
  statics of the source. The consumer makes it once and shares it.
- `Settings.words` (`Words`: the app name and the words of its options)
  makes the hints of a failed handshake; with podssh's words the text is
  the same as before.
- The feature `ech` turns on `pure`: the HPKE of the sealed hello is in
  the pure provider (rustls has HPKE only from aws-lc). The ECDHE_RSA
  suites of TLS 1.2 follow the feature `rsa`, and a check refuses a suite
  whose signature the provider cannot verify.
- The read of a name's HTTPS record is the trait `ech::Records`; `pod-dns`
  implements it (T-015).
- The file of the TLS groups has the header `pod-tls groups 1`; a file of
  podssh is set aside once, with a note, and written again.
- `deny.toml` ignores RUSTSEC-2023-0071 (rsa 0.9): `pod-tls` verifies
  with RSA public keys only.
- podnet's order of roots (a CA file alone; else the file beside the
  binary, the first system bundle and the compiled roots) is its own
  road: `Context::roots()` with `Settings::ca_file`. The tests of each
  source want other texts of the sources and another answer to a bad
  certificate in a CA file (podssh refuses the file; podnet skips the
  certificate), so one road cannot hold both. The roots of the road are
  read once for each `Context`, and a failure is kept, as in podnet.
- The split of the ClientHello is an argument, `fragment::wrap(io,
  Option<Split>)`: podnet parses it after the `Context` exists. A bad
  value is `Error::Split { part, value }`, from which the consumer writes
  the words of its flag.
- `client_config_with(extra, alpn)` gives an owned `ClientConfig` for each
  call, as podnet changes it for quinn.
- `pod_core::clock::hint` gives a clock before the epoch as it reads, as
  podnet did (`the_hint_gives_a_clock_before_the_epoch`, which fails on
  the code before the fix).

## Prove

```sh
cargo test -p pod-tls --all-features
sh scripts/agent/check.sh
```

## Start condition

T-005 is done.

## Done

2026-11-05. The podssh base `2bc2e06` to `6ec42f5`, the merge of podnet
`6040431` to `7923ab7`, the clock hint of `pod-core` `01c2a8c`, the plants
and the trials `de95d00` to `97d32f5`, and the patches on the newer
commits of the consumers `83b5d03` and `35cc732` (podnet `c7a31f3`, podssh
`85d4e9d`, with Rust 1.99 and the tag `v0.2.0`). `cargo test -p pod-tls
--all-features`: 154 passed, 7 ignored; `sh scripts/agent/check.sh`
passes on Rust 1.99. `scripts/plant-each.sh --group tls`: 6 caught. The
trials: podssh at `85d4e9d` (podssh-ws 405, podssh-relay 623, podssh-ssh
2, podssh-cli 1113, 0 failed; its record, `check-repo.py`, clippy and
rustfmt) and podnet at `c7a31f3` (716 tests of the library and the
targets `tls` 3, `egress` 8, `relay_reverse` 10, `cli` 55, `doctor` 3,
`api_surface` 2) pass here. CI: the adoption runs 42044831717 (podssh at
`d4e186d`, podnet at `b7682c5`) and 42047077773 (podnet at `c493a27`);
the ci run 42048395812 at `00568db` (Rust 1.99).

# T-013: `pod-proxy`: the dial, the proxies, the race of addresses and the HTTP client (the base of podssh)

**Source:** `docs/design.md`, section 4 (M1).
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

Each crate that opens a connection needs one dialer. podssh's dial
(`PodUtils/podssh:crates/podssh-ws/src/dial.rs`), its proxies, its
SOCKS5, its race of addresses and its HTTP GET client are the base.

## Premise

- Read: podssh-ws `dial.rs`, `proxy.rs`, `proxy_auth.rs`, `socks5.rs`,
  `mptcp.rs`, `race.rs`, `cut.rs`, `lowat.rs`, `write_limit.rs`,
  `stun.rs`, `http.rs`, `http/`, the GET client of `client.rs`.
- `dial.rs` calls `resolve::pinned` and `socks5.rs` calls
  `resolve::resolve`: they take `&dyn Resolve` of `pod-core`.

## Approach

The procedure of `docs/design.md`, section 8, with podssh's files only.
The merge of podnet's code is T-014.

Steps 1 to 4 and 6 are done: the commits `614f68a` to `17ff19a` (import at
podssh `6ece2ff`, edition, adaptation, tests, the plants `proxy-*`, the
README, the static probe). `cargo test -p pod-proxy --all-features`: 85
passed, 1 ignored (a live test); `plant-each.sh --group proxy`: 5 caught.
The trial in podssh and the release come with T-014: its change of
`socks5://` (the rule of curl) changes podssh too, so podssh takes the
crate once.

## Decision

- `pod_core::Found` has `more` and `pod_core::SOURCE_WAIT` (4 s): the race
  gives a source of names 4 s, then asks the next one.
- `DialError` stays an enum of `pod-proxy` (the tests match on it); it
  converts into `Failure` with itself as the cause. A refusal of a proxy
  with a 5xx status is `Unavailable`, another refusal `Refused`.
- `resolve::System` is the system resolver with the caller's pins; the
  bypass of a pinned loopback takes the pins of the dialer's resolver.
- The user agent is a setting with no default; the words of a 407 take
  `Settings::app`.
- The feature `ech`: the GET client opens through `pod_tls::ech::open`,
  as podssh's `https_get` did.

## Prove

```sh
cargo test -p pod-proxy
sh scripts/agent/check.sh
```

## Start condition

T-005 and T-012 are done.

## Done

2026-11-05. The podssh base `614f68a` to `17ff19a` and the trial commits
`04848e9` to `fb71265` (with T-014). `cargo test -p pod-proxy
--all-features`: 141 passed, 1 ignored; the plants of the group `proxy`:
9 caught. The trial in podssh at `d3745ab`: its record, `check-repo.py`,
the builds of the features of its gate, clippy, rustfmt, and 2,237 tests
of podssh-ws (342), podssh-relay (669) and podssh-cli (1,226), 0 failed.
CI: the adoption run 42108955836 at `1ca1c6e`, the ci run 42089986177.
# T-014: `pod-proxy`: podnet's proxy chain, Digest and fetch

**Source:** `docs/design.md`, section 8, step 5.
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

podnet's egress has capabilities that podssh's dial does not have: HTTP
CONNECT with Digest, `unix:`, `+exec:`, the detection of the proxy of a
tool, and its HTTP client under DoH and the mint of the relay.

## Premise

- Read: podnet `PodUtils/podnet:src/egress.rs` (1,707),
  `PodUtils/podnet:src/proxy_auth.rs` (615), `PodUtils/podnet:src/fetch.rs`
  (548).
- `socks5://` differs: podnet resolves the name locally and `socks5h://`
  sends it; podssh sends the name for both (`docs/design.md`, section 8,
  step 5: the rule of curl).
- podnet allows `+exec:` only from a value that the user gave on purpose.

## Approach

Each capability of podnet with its tests, behind a feature when it
changes a behaviour; the parity list; the trials of podssh and podnet.

The merge is done: the commits `4b22d84` to `16dd70b` (import at podnet
`a8071b1`; the proxy URLs, the rules of the variables, the proxy of a
tool; the dial with `socks5://` resolved here, Digest, a proxy on a Unix
socket or at a command; the credentials of a served proxy; the HTTP
client; the README; the plants). `cargo test -p pod-proxy --all-features`:
139 passed, 1 ignored, with the 33 tests of podnet by their names;
`plant-each.sh --group proxy`: 9 caught. Next: the trials of podssh and
podnet, then the release.

## Decision

- What differs between the consumers (the order of the variables, an
  unusable URL passed over, the default port of `http://` (80 or 8080),
  the ports of `NO_PROXY`, the rule of the loopback) is a field of
  `env::Rules`, not a cargo feature: features unify in one binary.
  podssh's values are the default; the README gives podnet's.
- `socks5://` resolves the name here, after the greeting; `socks5h://`
  sends it (the rule of curl). podssh's tests that sent a name take
  `socks5h://`.
- The words of a 407 are podnet's (they name Digest), with the program of
  `Settings::app`.
- The Digest client nonce comes from `OsEntropy`, and no random bytes is
  an error (podnet sent zeros); the credentials file is read with
  `pod_core::files::read_own` (no symbolic link, no file of another user).
- `read_response` passes over up to 5 interim (1xx) answers and takes a
  TLS end with no close_notify as the end of the stream, for both
  consumers: it corrects the reading.
- A `+exec:` proxy only from `Origin::User` (podnet's rule); the plant
  `proxy-command-from-a-variable` proves it.

## Prove

```sh
cargo test -p pod-proxy --all-features
sh scripts/agent/check.sh
```

## Start condition

T-013 is done.

## Done

2026-11-05. The merge `4b22d84` to `16dd70b`, the allow list `237d0da`,
and the trial commits `04848e9` to `fb71265`. The 33 tests of podnet in
`pod-proxy` by their names. The trial in podnet at `95d1cf2`: its record,
`check-size.py`, the build, `cargo deny --locked check bans sources`,
clippy, the fuzz workspace, rustfmt, `cargo test --lib` (687) and the
targets `api_surface`, `cli`, `connect`, `docs`, `doctor`, `egress`,
`relay_reverse`, `tls`, `update_umask` (97), 0 failed. CI: the adoption run
42108955836 at `1ca1c6e`.
# T-015: `pod-dns`: the resolver: system, DoH in both forms, pins, HTTPS records

**Source:** `docs/design.md`, section 4 (M1).
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

podssh resolves through RFC 8484 DoH over HTTP/2 with HTTPS and SVCB
records; podnet through DoH JSON, pins and the query by name through a
proxy. Each consumer needs both.

## Premise

- Read: podssh-ws `resolve.rs` and `resolve/` (2,215); podnet
  `PodUtils/podnet:src/resolve.rs` (591).
- Timeouts differ (podssh 5 s and 3 s); they become settings.

## Approach

The base of podssh, then podnet's DoH JSON, pins and query by name; one
`Resolve` implementation; the parity list; the trials.

The crate is in: the commits `f8902e2` to `1276611` (import of podssh at
`a791553`, edition, adaptation, tests, the plant `dns-length-past-end`,
the target `dns_wire`; import of podnet at `380d9a4`, the merge, the
target `doh_json`, the plants `dns-nxdomain-ends-the-lookup` and
`dns-cache-not-off`; README; static probe). `cargo test -p pod-dns
--all-features`: 58 passed, 4 ignored (live), 8 of them podnet's by
their names; 70 runs with no failure. A defect of the merge is fixed in
`1276611`: an NXDOMAIN of AAAA after the addresses of A kept no address
(`an_nxdomain_of_aaaa_after_the_addresses_of_a_keeps_them` fails before).
The trials come with v0.4.0.

## Decision

- The resolvers are data of `pod-services`: the default settings take the
  services of kind `doh` (role `v4`), the operators in turns, the
  wire-only ones last, which gives podssh's list.
- `Resolver::new` asks no DoH (podnet's `resolve_no_doh`): the dial of the
  DoH questions takes such a resolver, so no question recurses.
- The differences of the consumers are settings, not features: `system`,
  `both`, `nxdomain`, `names` (`First` for podssh, `Last` for podnet),
  `connect`, `trust`, `user_agent`, `memory`; the defaults are podssh's.
- A resolver is asked by name when the dial resolves names inside
  (`Dial::names_inside`).
- The feature `ech`: `Resolver` and `Way` read the ECH list of a name for
  `pod-tls`.

## Prove

```sh
cargo test -p pod-dns --all-features
sh scripts/agent/check.sh
```

## Start condition

T-014 is done.

## Done

2026-11-06. The crate is on `main` (`be940fd` and before). The trials of
v0.4.0 pass each step: podssh at `fd261a1` (`8847ba3`: 2,356 tests, the
features `tor` and `ts`, the static binary) and podnet at `57cfc0b`
(`e1c2f15`: 773 tests, cargo-deny, clippy, the fuzz targets, the static
binary). The podnet trial found two faults of its glue, fixed in the
patch: the step of an IP literal is `literal`, and `-v` showed no line of
the pod crates (`report::log_filter`).

# T-016: `pod-ws`: the WebSocket client and server, HTTP/2 and cleartext roads

**Source:** `docs/design.md`, section 4 (M1).
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

podssh has its own WebSocket client and server; podnet uses
tokio-tungstenite and a bridge of its own. One crate serves both.

## Premise

- Read: podssh-ws `client.rs`, `frame.rs`, `handshake.rs`, `session.rs`,
  `url.rs`, `close.rs`, `error.rs`, `plain.rs`, `server.rs`, `h2_road/`,
  `cleartext/`, `wake.rs`; podnet `PodUtils/podnet:src/ws.rs` (596).
- Read podnet at `a026974` or later: its layer cuts changed the origin
  dial of `src/ws.rs` to `origin::dial::dial` (PodUtils/podnet#418).
- The limits differ: podssh 16 MiB a message; podnet the defaults of
  tokio-tungstenite. They become settings.
- `server.rs` calls the WebTunnel road (`pod-egress`): a trait.

## Approach

The base of podssh, then podnet's bridge, close codes and `close_hint`;
the parity list; the trials.

## Prove

```sh
cargo test -p pod-ws --all-features
sh scripts/agent/check.sh
```

## Start condition

T-015 is done.

## Done

2026-11-10, partial: the release (the next tag after `v0.12.0`, so the
trials name `v0.13.0`) and the takes of podssh and podnet remain.
Commits `000eef0` (the import of `PodUtils/podssh:crates/podssh-ws` at `8cd0f2b`
and podnet `src/ws.rs` at `76ffb5e`, with `SOURCES`), `e474bc9` (the
adaptation, podnet's bridge, the tests, the fuzz targets), `028a1eb`
(README, the static probe, the row of `docs/design.md`, C41), `709b008`
(the doc link; `plain::loopback_tcp` public), `27d8510` (the bridge takes
any stream), `623de6d`, `89cb9cc` and `2d8cdbc` (the trials), and the
commit of this record.

- API: each connection goes through the caller's `Way` (the TLS context,
  the dialer, the reader of HTTPS records for ECH, the user agent);
  `h2_road::Pool` is a value of the caller; `Limits::{RELAY,
  TUNGSTENITE}` are the limits of each source; `ConnectError`,
  `SessionError` and `WsError` are each a `pod_core::Failure`. The crate
  depends on `pod-core`, `pod-tls` and `pod-proxy` (C41). It has 25
  source files, each under 500 lines.
- Cut: podssh keeps its glue in `podssh-ws` (its `WsClientConfig` with a
  `ProxyChoice`, the GET client, the provider line of `doctor`,
  `ForwardClose::remedy` as the trait `close::Remedy`, `USER_AGENT`, a
  `static` pool); podnet keeps its words and hops in `ws.rs`,
  `serve_websocket`, `forward_path`, `forward_url`, `LocalSpec` and a
  bridge of tokio-tungstenite for its relay client until `pod-relay`.
- Defects: a first form of the bridge did not end when the WebSocket way
  ended while the raw stream stayed open (podnet's control channel ended
  it); `bridge_moves_bytes_both_ways` hung on it, and passes 30 runs of
  30 after the fix. The bridge took `pod_core::BoxedIo` alone, and
  podnet's `BoxedIo` is a trait of its own: the podnet trial found it.
  The seeds of the fuzz targets were normalized to LF by
  `.gitattributes`; `fuzz/seeds/**` is now `-text`.
- Tests: 122 with 1 measure ignored: the unit tests (28), `rfc6455`
  (27), `session` (16), `control_frames` (10), `handshake_tail` (9),
  `h2_road` (6), `properties` (6), `frame_lengths` (5), `cleartext_road`
  (4), `limits` (3), `plain_loopback` (2), `random_source` (2) keep the
  names of podssh; `bridge` (4, and the measure `throughput`) and
  `accept_key_matches_rfc_example` are podnet's, with new tests for the
  bridge, `Authorization`, a 401 and `close_hint`. The same pass as i686
  musl binaries, and as aarch64 musl binaries under QEMU. The fuzz
  targets `ws_frame`, `ws_upgrade` and `close_reason` pass the stable
  test of `fuzz/`.
- Trials: podnet at `22d99a1` (14 files, 125 lines in, 272 out) and
  podssh at `2f8e0bb` (63 files, 304 lines in, 6,641 out); each step of
  `sh scripts/adoption.sh podnet pod-ws` and `sh scripts/adoption.sh
  podssh pod-ws` passed (the counts are in the commit `2d8cdbc`), with
  the static binary of each for x86_64 musl.
- Checks: `python3 -I scripts/check-sources.py` on `pod-ws` (podssh 33
  files, podnet 2 files, 0 changes); `sh scripts/agent/check.sh` (fmt,
  clippy, doc, record, size, paths, the static probes, the tests).

- Released in `v0.13.0` (`947603a`, release 42802815017; GitHub #49).
  podnet took `pod-ws` at `5eace6e` (PodUtils/podnet#420; `--3way` at
  podnet `99014d5`, each hunk clean; each line of the `.run` passed): the
  `taken` line of the podnet block is in `crates/pod-ws/SOURCES`, and the
  trial of podnet ends. podssh took `pod-ws` at `a083593e`
  (PodUtils/podssh#184; `--3way` at podssh `b61efbdf`, conflicts only in
  the tag lines and the citations; each line of the `.run` passed, and
  `cargo test` of podssh-cli, podssh-relay and podssh-ws: 3,384 passed):
  the `taken` line of the podssh block is in `crates/pod-ws/SOURCES`, and
  the trial of podssh ends.

# T-017: The fuzz workspace and its weekly workflow

**Source:** `docs/design.md`, section 8, step 4.
**Category:** chore
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podssh has 23 fuzz targets and podnet 16. Each moves with its code, and
the targets need a workspace and a run.

## Premise

- Read: podssh `PodUtils/podssh:fuzz/` and `PodUtils/podssh:scripts/fuzz.sh`;
  podnet `PodUtils/podnet:fuzz/` and `PodUtils/podnet:src/fuzzing.rs` (the
  facade of the targets). `no_proxy` and `proxy_url` exist in both.

## Approach

A cargo-fuzz workspace in fuzz/; a `fuzzing` feature in each crate;
the targets of the crates of M1; `fuzz.yml` each week.

## Prove

```sh
cargo check --manifest-path fuzz/Cargo.toml --bins
```

## Start condition

T-013 is done.

## Done

2026-11-05, commits `942372b` to `19e8394`. `fuzz/` is its own workspace
(the root excludes it) with 8 targets: `one_line` (pod-core),
`pem_bundle` (pod-tls), `http_head`, `http_chunked`, `socks5_answer`,
`proxy_url`, `no_proxy`, `connect_status` (pod-proxy; the last three join
the targets of podssh and podnet). Each ran 61 s here on
nightly-2026-11-01 with cargo-fuzz 0.13.2 and the address sanitizer, with
no crash (0.66 to 4.49 million runs). The stable test of the targets runs
in `ci.yml`; `fuzz.yml` runs each target each week; the plant
`fuzz-one-line-escape` is caught. A defect that the stable test of
`proxy_url` found is fixed in `942372b`: a proxy host with a colon is an
IPv6 address (`a_host_with_a_colon_is_an_ipv6_address` fails on the code
before). `fuzz/README.md` names each target that waits, with its crate.


# T-041: `pod-testkit`: the SSH test server of podssh (the feature `ssh`)

**Source:** `docs/design.md`, section 4 (M0); T-006, Decision.
**Category:** chore
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

podssh's tests have an SSH server in the test process (russh). podnet's
git hosting needs one in its tests too.

## Premise

- Read: `PodUtils/podssh:crates/podssh-cli/tests/ssh_harness/` (20 files,
  2,054 lines): `serve.rs`, `shell.rs`, `pass.rs`, `ends.rs`, `eof.rs`,
  `forward.rs`, `sftp.rs` are generic; the rest answers podssh's own
  commands.

## Approach

The generic parts behind the feature `ssh` of `pod-testkit`, on russh
0.65 of crates.io with ring (`docs/decisions.md`, call C9); the commands
of podssh stay in podssh.

## Prove

```sh
cargo test -p pod-testkit --features ssh
```

## Start condition

A crate of podcrates or a consumer needs an SSH server in its tests.


# T-044: The tests of the system stores of `pod-tls` with a root in the store, on Windows and macOS

**Source:** the trial of `pod-tls` in podssh (T-012).
**Category:** chore
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The tests `system_store` of `pod-tls` (the stores of Windows and macOS)
are ignored: they need a root in the store of the host. podssh's
`build.yml` put a root in the store and ran them; its trial of `pod-tls`
drops those steps, as the tests are in `pod-tls` now. No CI of podcrates
runs them.

## Premise

- Read: `crates/pod-tls/tests/system_store.rs` (2 ignored tests, the
  switches `POD_TLS_TEST_STORE_ROOT` and `POD_TLS_TEST_STORE_HOLDS`) and the
  steps of podssh's `build.yml` at `d4e186d` that add a root to the store.

## Approach

A job of `nightly.yml` on Windows and on macOS: add a test root to the
store of the runner, run the ignored tests of `system_store` with the
switches, and remove the root.

## Prove

```sh
cargo test -p pod-tls --test system_store -- --ignored
```

The job of `nightly.yml` passes on Windows and on macOS, and fails on a
planted defect (a store that is not read).

## Start condition

T-012 is done.

## Done

2026-11-05, commit `6939b31` (`nightly.yml`). The nightly run 42057509096:
on windows-2025 and windows-11-arm the test passed with the root out of
the store (the control) and passed with the root in the machine's ROOT
store ("the root comes from the current user's ROOT store", 1 passed in
each step); on macos-15 and macos-15-intel the trust settings were read
and the server that the root signs was refused. The control of the first
step is the planted defect: the same test with the root out of the store
must not find it.

# T-045: The adoption trials check the static binaries of the consumer and build each feature

**Source:** `docs/design.md`, section 8, step 7; the trial of `pod-tls`.
**Category:** chore
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

Step 7 of the procedure asks each trial for "the static check of the
consumer binary on its targets" and "a build with the consumer's
features". The trials of `pod-tls` build the default features and the
features of podssh's gate, but not `tor` and `ts`, and no `.run` checks a
static binary.

## Premise

- Read: the run files of the trials of `pod-tls` (in git at `8dc1fb5`).
- Read: podssh has `scripts/check-static.sh` (PodUtils/podssh@T-685); this repository has
  `scripts/check-static.sh` for the probe.

## Approach

1. Each `.run` builds the consumer's binary for `x86_64-unknown-linux-musl`
   with zig and checks it with the static check of the consumer (else of
   podcrates).
2. The podssh trial builds the features `tor` and `ts` too.
3. Each trial checks the toolchain of each job of the consumer's release
   workflow against the `rust-version` of podcrates (podnet's armv7 job
   built with Alpine's Rust 1.96.1, below 1.99).

The infrastructure is in: `adoption.yml` gives each trial the musl
target, zig and cargo-zigbuild (the versions of `ci.yml`), and
`adoption/README.md` asks each `.run` for the static build and the check.
The lines of the `.run` files come with the next trials (`pod-dns`,
T-015), as the trials of `pod-proxy` leave when the consumers take it.

## Prove

```sh
sh scripts/adoption.sh podssh pod-tls
sh scripts/adoption.sh podnet pod-tls
```

Each trial checks a static binary of the consumer.

## Start condition

T-012 is done.

## Done

2026-11-06. The `.run` files of v0.4.0 build each consumer's binary for
`x86_64-unknown-linux-musl` and check it static (podssh, podnet, and
podvm's `shell`); the podssh trial builds the features `tor` and `ts`;
each trial runs `scripts/check-consumer-toolchains.py` (`794d065`). The
podvm trial raised podvm's `rust-version` from 1.85 to 1.99 on its fault.

# T-048: `pod-services`: the inventory of the relays and services of other parties in podssh and podnet

**Source:** the owner, 2026-11-05 (`docs/decisions.md`, Services crate).
**Category:** research
**Milestone:** M1
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The owner: "they hardcode/measure many relays/third party services, it
risks drift and staleness, so we move that code & related script here as
a new crate". podssh and podnet each hold lists of hosts of other parties
in their code, and scripts and workflows that measure or watch them.

## Premise

- Asked: the sessions of podssh and podnet send an inventory: each list of
  hosts, the code that measures or watches them, the workflows, the
  scripts and the data files, with the file and lines, the source of each
  value and the code that reads it.
- Read: the inventories of podssh (`docs/research/services/podssh.md`, at
  podssh `9d0623d`) and of podnet (`docs/research/services/podnet.md`, at
  podnet `aaf6f4c`).

## Approach

1. Join both inventories; read each item at the consumer's main.
2. Choose, for each item, what moves to `pod-services` (a list with its
   source and date; a watch; a script) and what stays, with why.
3. Write the design of the crate in `docs/design.md` (its API, its
   dependencies, where each watch runs) and the entry of the move (T-049).

## Decision

The owner's answers of 2026-11-05 (`docs/decisions.md`, Services): the
defaults of the own relay and the facts of the probes (GitHub's SSH keys,
the ACME directory) move as data, with watches; the design is pluggable
and agnostic ("we will soon add other relay/services"); the release
identity of `update` and the cloud endpoints of the user's accounts stay
with their code. `docs/design.md`, section 4, `pod-services`, has the
design.

## Prove

```sh
cargo todo check
```

The inventory and the choices are in this entry, and `docs/design.md`
has the row of `pod-services` with its sources.

## Done

2026-11-05. The inventories of podssh (`docs/research/services/podssh.md`)
and of podnet (`docs/research/services/podnet.md`) are in this repository;
the owner ruled each group that the sessions left open; `docs/design.md`
has the design of `pod-services`. The move is T-049 (the library and the
data) and T-050 (the watches, the workflows, the scripts).

# T-049: `pod-services`: the schema, the data of each service and the library

**Source:** the owner, 2026-11-05 (`docs/decisions.md`, Services crate).
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** L
**Status:** done

## Problem

The lists of relays and services of other parties, and their watches,
must have one home, so that they do not drift and do not go stale in two
repositories.

## Premise

- The inventory of T-048.

## Approach

`docs/design.md`, section 4, `pod-services`: the schema and its check in
`build.rs`; one data file for each service of the inventories (A of
podssh, 1 of podnet), each value with its source and its date; the API by
id and by kind, and the overlay of the consumer's files; the tests of the
constants that the consumers had (for example podssh's test that ties
the relay's limits to `relay-limits.toml`, and the test that the n0
relays are iroh's list); the README with the parity list and the
adoption map of each place of the inventories; the trials of podssh and
podnet.

The crate is in: the commits `5b347c4` to `8518632`: 27 services in 19
kinds (`crates/pod-services/services/`), the schema and its check in
`build.rs`, the API by id and by kind, the overlay (feature `overlay`),
the README with the parity list and the adoption maps. `cargo test -p
pod-services --all-features`: 34 passed, 8 of them the tests of the
consumers by their names; the plant `services-no-source` is caught. The
trials of podssh and podnet come with `pod-dns` and the release v0.4.0.

## Decision

- A source has one of `url`, `measured` or `ruling`, a `read` date and an
  optional note; a value read again on 2026-11-06 keeps the consumer's
  read in the note.
- Where the consumers differed: both WARP version pairs are kept
  (`usque-api-version`/`usque-client-version` of podssh,
  `api-version`/`client-version` of podnet; the API refuses a pair that
  does not match); the cloudflared pin is podnet's 2026.10.7 with the
  files of both; both edge addresses are kept (each is an A record of
  `region1.v2.argotunnel.com`); the DoH lists are joined.
- An optional `[rank]` with its source orders the services of a kind.
- An overlay file replaces a whole service by id; a second overlay of the
  same id is an error of the class `Config`.

## Prove

```sh
cargo test -p pod-services
sh scripts/agent/check.sh
```

## Start condition

T-048 is done, and the owner reserved `pod-services` on crates.io.

## Done

2026-11-06. The same trials as T-015: one patch for each consumer takes
`pod-dns` and `pod-services`; each moved constant of the adoption maps is a
read of `pod-services`. The data of podvm is T-066.

# T-050: `pod-services`: the watches, their workflows and the measurement scripts

**Source:** the owner, 2026-11-05 (`docs/decisions.md`, Services crate, Services: the design).
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** L
**Status:** done

## Problem

The consumers watch and measure the services of other parties in their own
workflows and scripts (the inventories, B, C and D of podssh, 2, 3 and 4
of podnet). The watches must run in one place, on the data of
`pod-services`.

## Premise

- Read: `docs/research/services/podssh.md`, `docs/research/services/podnet.md`.

## Approach

The member crate `pod-services-watch` (`docs/design.md`, section 4): the
`Watch` trait and one implementation for each watch kind that the
inventories need, each with a plant; the workflows of podcrates that run
the watches each day and the measurements by hand; the measurement scripts
of the consumers in a directory services of `scripts/`; the trials of podssh and podnet
(their workflows and scripts of watches leave them).

## Decision

Call C42 (`docs/decisions.md`): the runner is the binary of the member
crate; a watch that cannot speak a form of a service is `skipped`;
`http-json` with `member` finds a host of the file that the document
lacks, and counts the hosts of the document that the file lacks.

## Prove

```sh
cargo test -p pod-services-watch
sh scripts/plant-each.sh --group services
python3 -I scripts/check-workflows.py
```

## Start condition

T-049 is done.

## Done

2026-11-10, the commit of this entry. `crates/pod-services-watch`: the
trait `Watch`, `Net` (the dial and the client of `pod-proxy`, the TLS of
`pod-tls`, the DoH of `pod-dns`, the WebSocket of `pod-ws`), `Report`,
and 24 watch kinds, each the kind of at least one watch of the data
(`http-json`, `http-status`, `http-text`, `github-ssh-keys`, `http-sha256`,
`tls-name`, `tls-names`, `dns-srv`, `dns-addresses`, `doh-answer`,
`stun-binding`, `websocket-upgrade`, `forward-banner`, `proxy-banner`,
`ssh-banner`, `ssh-host-key`, `github-release`, `github-files`,
`github-file`, `tls-roots`, `crate-file`, `rdap`, `oci-ping`, `oci-digest`);
the runner prints one line for each watch. `pod-testkit` has the
stand-ins (feature `stand`: HTTP, raw TCP and UDP, TLS from the caller).
`cargo test -p pod-services-watch`: 30 passed (`tests/watches/`: each
kind against a stand-in, and the test that each kind of the data has its
implementation). The plants `scripts/plants/services-*` (24, group
`services`): `sh scripts/plant-each.sh --group services`: 25 caught (with
`services-no-source`), 0 missed. `sh scripts/agent/check.sh` passes (fmt,
clippy, doc, record, size, paths, static); `cargo test --workspace
--all-features`: each test binary passes.
`.github/workflows/services.yml`: the watches each day and by hand (a
service, a kind), and the measurements of `scripts/services/`.
`scripts/services/edge-facts.sh` comes from podnet `tools/edge-facts.sh`
(`scripts/services/SOURCES`, which `scripts/check-sources.py` now reads);
the trial `adoption/podnet/pod-services-watch.*` removes the script and
its step of podnet's `live.yml` and names v0.14.0 (`sh scripts/adoption.sh podnet
pod-services-watch` at podnet `5bf153d`: each step passed). A run from this cloud
session (an allowlist): 62 hold, 2 skipped, 21 failed, each failure a
reset of the proxy, a UDP that does not pass, the limit of GitHub's API
with no token, or Docker Hub's limit (429). Fixed in place from the live
run: the forwarders of the query form have no watch `websocket-upgrade`
(their Worker refuses an upgrade with no host); a text frame of a
forwarder is its own word (Mudlet's `{"type":"game.connected"}`); the
first frame of the first-frame form, also binary (ssheasy), is the
answer; `github-file` looks for the addresses of an endpoint; 429 of a
registry is a failure, not a moved digest; `tls-roots` takes `tag`. The
rest: T-151 (the watch scripts of podssh and podnet that leave them),
T-152 (VLESS over XHTTP).

The dispatch of `services.yml` at `7d17a57` (run 42803032733, GitHub
Actions): 72 hold, 2 skipped (the XHTTP nodes), 9 failed. Fixed in the
next commit: a runner of GitHub has no IPv6 (5 failures of the IPv6
addresses of `cloudflare-doh`, `google-doh` and `cloudflare-dns`: the
runner probes a route, and with none a watch does not check an IPv6
address); Pinggy sends its banner only after the client's (`ssh-banner`
now sends the client's first; `ssh-host-key` held); ECR Public answered
429 to one HEAD (one more try after its `Retry-After`). The outside facts
that moved: four Trojan nodes of T-133 fail (T-157). The run with
`measure` `edge-facts` at `966bd01` (run 42803038882) passed.

The dispatch at `b9bab61`, after the fixes (run 42806121501): 79 hold, 2
skipped, 5 failed: the four Trojan nodes (T-157), and the watch
`websocket-upgrade` of `croc` (T-156), which the bridge refuses with no
relay in the query (`HTTP 403: relay index is not allowed`; T-158).
- The take: podnet took the trial `pod-services-watch` at `0b945a3`
  (PodUtils/podnet#448): `tools/edge-facts.sh` and its step of `live.yml`
  left podnet. `scripts/services/SOURCES` has the `taken` line;
  `check-sources.py` with a clone: 1 file, 0 changes. The trial files are
  removed. podnet's fix `54bd958`: a patch that names a file of podcrates
  in a document of the consumer writes it as
  `PodUtils/podcrates:PATH`, or the consumer's test of the paths of its
  documents fails.

# T-052: The tests of a closed port of `pod-proxy` race with the other tests

**Source:** the run of the targets of T-017 (300 runs of the binary of `tests/egress.rs`).
**Category:** defect
**Milestone:** M1
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`a_failed_dial_keeps_the_kind_of_its_last_error` bound a listener, dropped
it and dialed the freed port; a parallel test of the same binary could
bind the port first, and the dial then reached it. Four other tests of
`pod-proxy` (`dial`, `fetch`, `get`, `dialer`) find a closed port the same
way.

## Premise

- Measured: the binary of `tests/egress.rs` failed 26 of 300 runs.

## Approach

`tests/closed/mod.rs`: a socket bound to the port that does not listen,
held until the test ends; a connection to it is refused, and no other
test takes the port. The five tests use it.

## Prove

```sh
cargo test -p pod-proxy --all-features --test egress --test dial --test fetch --test get --test dialer
```

## Done

2026-11-05, commit `df023da`. The binary of `tests/egress.rs` failed 26 of
300 runs before and 0 of 300 after; the binaries of `dial`, `fetch`, `get`
and `dialer` 0 of 100 each.
# T-054: `pod-services`: the inventory of podvm

**Source:** PodUtils/podvm#194; the owner, 2026-11-05 (`docs/decisions.md`, Services crate).
**Category:** research
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm compiles in the URL of the relay, the endpoints of registries and
the sources of guest images (PodUtils/podvm#194). The services crate holds
the hosts of other parties of each consumer.

## Premise

- podvm sent no inventory. This session read podvm itself, at
  `ecf61c0` of its branch `migrate/podvm`, after its rename `98112cb`.

## Approach

podvm's inventory in a page podvm.md of `docs/research/services/`, as T-048 did for
podssh and podnet; its data files are T-066.

## Prove

```sh
cargo todo check
```

## Done

2026-11-06. `docs/research/services/podvm.md`: the hosts in the Rust
code (A), the guest images and their digests (B), the pinned OCI images of
the test corpus (C), the trust data (D), the code that measures (E), the
workflows (F), the scripts (G), the proposed data files (H) and what stays
in podvm (I). Findings: podvm's code has no default relay (`--relay` is
required); only `tcp.ssh.relay.ajam.dev` is in `pod-services` already. The
new data are five OCI registries and three guest images, with the kinds
`oci-registry` and `guest-image` and the watch kinds `oci-ping`,
`oci-digest` and `http-sha256` (T-050). podvm's facts of the relay
(document `r12`, a hello frame cap of 65504) differ from
`ajam-relay.toml` (`2026-10-03-r2`, 262144): T-047 takes them.

# T-063: The synchronous face for podvm: `pod-tls` with no tokio, a blocking `pod-proxy`

**Source:** PodUtils/podvm#194, question 1; `docs/decisions.md`, call C20.
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podvm is synchronous and measures the size of each dependency. It asks
for a synchronous face of `pod-proxy` and `pod-tls`.

## Premise

- Read: `pod-tls` uses tokio for `tls_over`, the ECH road and the server's
  acceptor; its configurations, roots and stores need none.

## Approach

1. `pod-tls`: tokio behind a default feature `async`; with
   `default-features = false` the configurations, roots, stores and the
   server's configuration build with no tokio.
2. `pod-proxy`: a feature `blocking`: `blocking::Dialer` and
   `blocking::Client` over a private current-thread runtime, the same code
   path; a test of each over a loopback server.
3. The size of each in podvm's binary, measured.

## Prove

```sh
cargo clippy -p pod-core -p pod-tls --no-default-features --features pod-tls/pure,pod-tls/rsa -- -D warnings
cargo test -p pod-proxy --features blocking --test blocking
cargo test -p pod-tls --test blocking
```

## Done

2026-11-06. `pod-core` and `pod-tls` have the default feature `async`; with
no default feature neither has tokio (`cargo tree`: 0 tokio, 4 with the
defaults), and `ci.yml` runs their clippy so. `pod-tls` adds
`tls_over_blocking` (rustls's `StreamOwned` over a `Read + Write` stream,
with the words of a failure) and `server::server_config`. `pod-proxy`'s
feature `blocking` gives `blocking::Dialer` and `blocking::Client` on a
private current-thread runtime: a TCP connection comes back as the
standard `TcpStream`. Tests: 4 of `pod-proxy` (direct, CONNECT, a proxy at
a command, a GET over HTTPS) and 2 of `pod-tls` (a handshake, a wrong name),
on a thread with no runtime; `scripts/agent/check.sh` passes. The size of a
static musl program (release, LTO, stripped) that makes one HTTPS GET:
rustls with ring and webpki-roots 1,310,800 bytes; with `pod-tls` and no
tokio 1,376,128 (+64 KiB); with `pod-proxy`'s `blocking` (tokio, the
proxies, the client) 1,710,144 (+390 KiB). The I/O-free state machines of
C20 wait for a measurement of podvm that asks for no tokio.


# T-065: A vectored write through `pod_proxy::Stream` sends only its first buffer

**Source:** the podssh session, 2026-11-06: the step `resume.cut.up.sftp` of
podssh's release gate after podssh took `pod-proxy` (podssh `a791553`).
**Category:** defect
**Milestone:** M1
**Priority:** P0
**Effort:** S
**Status:** done

## Problem

podssh: "pod_proxy::Stream (stream.rs, AsyncWrite) does not forward
poll_write_vectored / is_write_vectored (WriteLimit and pod_core::Metered
do); rustls writes its records with write_vectored, so each call sends one
buffer, and a write that the far end holds keeps the TLS session from
reading." An upload through a stand-in relay that cuts each 1.5 MB gets no
answer after 0.12 s, and no write is acknowledged. `ChildIo` and
`pod_tls::fragment::Fragmenting` do not forward them either.

## Premise

- Measured by podssh: podssh `d3745ab` (before `pod-proxy`) reads the
  answers of the server at 0.53 s; `a791553` reads nothing after 0.12 s;
  a `Dial` that gives the bare `TcpStream` finishes the upload. The socket
  options are the same in both (`TCP_NODELAY`, `TCP_NOTSENT_LOWAT` 16384).
- Read: the default `AsyncWrite::poll_write_vectored` of tokio writes the
  first buffer that is not empty; `TcpStream`, `UnixStream`, `ChildStdin`
  and `DuplexStream` write each buffer.

## Approach

`Stream`, `ChildIo` and `Fragmenting` (after the first record) forward
`poll_write_vectored` and `is_write_vectored`. A test of each stream kind
makes one vectored write of three buffers and expects each byte in one
call.

## Prove

```sh
cargo test -p pod-proxy --all-features --test stream
cargo test -p pod-tls --all-features --lib fragment
```

## Done

2026-11-06, commit `161a4d0`. The four tests failed before the change
(no stream said that it writes vectored) and pass after it; the checks of
`scripts/agent/check.sh` pass. The fix goes to podssh with v0.4.0, whose
patch removes podssh's workaround
`PodUtils/podssh:crates/podssh-ws/src/dial/bare.rs` (podssh `ccfef9e`,
PodUtils/podssh@T-690).

# T-066: `pod-services`: the data files of podvm

**Source:** T-054 (`docs/research/services/podvm.md`, section H).
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podvm compiles in the endpoints of five OCI registries and the sources and
digests of three guest images (`docs/research/services/podvm.md`). The
services crate holds the hosts of other parties of each consumer.

## Premise

- Read: `docs/research/services/podvm.md`, sections H and I.
- Read: `docs/research/services/podvm-inventory.md`, podvm's own
  inventory (PodUtils/podvm#196, 67 rows), with the rootfs completion
  (mirrors, DNS, CAs) that `podvm.md` does not list.

## Approach

1. The kinds `oci-registry` and `guest-image` in the schema of
   `pod-services`.
2. One data file for each service of section H, with its facts and their
   sources.
3. The adoption map of podvm in the README of `pod-services`; the trial of
   podvm (T-040).

## Prove

```sh
cargo test -p pod-services
```

## Start condition

T-049 is released (v0.4.0).

## Done

2026-11-06, the commit of this entry. Eight files in
`crates/pod-services/services/`: `docker-hub-registry`,
`ecr-public-registry`, `ghcr-registry`, `quay-registry`,
`opensuse-registry` (kind `oci-registry`) and `alpine-netboot`,
`freedos-image`, `microsoft-validationos` (kind `guest-image`). The kinds
are words of the data: the schema did not change. Each value was read in
podvm at `0249527` and measured on 2026-11-06: each of the 27 pinned
digests answered a HEAD of its manifest with the same digest; the token
realms are the challenges of `/v2/`; the FreeDOS zip, its member and the
VHDX extent of the ISO (a range read) have podvm's sums and lengths. The
Alpine pins are the files of 3.22.6; `netboot/` of v3.22 now holds 3.22.8,
with other sums, so podvm's experiments fail on their sum. The watches
name the new kinds `oci-ping`, `oci-digest` and `http-sha256` (T-050).
The README has the services, the watch kinds, podvm's parity and its
adoption map; `SOURCES` has podvm's 23 files (`scripts/check-sources.py
--clone PodUtils/podvm=...`: 0 changes). `cargo test -p pod-services
--all-features`: 40 passed (schema 12, api 9, moved 8, overlay 5, podvm 6).
The trial in podvm is T-040.

# T-067: Two tests fail on macOS and Windows in the nightly

**Source:** the run 42195081977 of `nightly.yml` at `f08ddaf`.
**Category:** defect
**Milestone:** M1
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The nightly at `f08ddaf` failed: `pod-dns`'s
`a_close_a_silence_and_a_refusal_are_kept_apart` on macOS ("127.0.0.1: no
connection within 3 s": `TimedOut`, not `Refused`), and `pod-proxy`'s
`the_proxy_of_a_tool_comes_through_the_readers_of_the_caller` on both
Windows hosts (the origin was `/etc/environment`, not the `.npmrc` of
`HOME`). Both tests came after the last green nightly (`6939b31`).

## Premise

- Read: macOS drops a SYN to a port that a socket holds and does not
  listen on, so the dial times out. `pod-proxy`'s `tests/closed/mod.rs`
  holds a port the same way.
- Read: the code joins `HOME` and the name of a tool's file with the
  separator of the host; the test's reader matched the text
  `/home/u/.npmrc`, which Windows does not make.

## Approach

On macOS the refused port is free (a listener bound and dropped), in the
test of `pod-dns` and in `pod-proxy`'s `closed::port`. The test of the
tool's file builds the path with `Path::join`.

## Prove

```sh
cargo test -p pod-dns --lib cuts
cargo test -p pod-proxy --lib env
cargo test -p pod-proxy --test egress --test dial --test fetch --test get --test dialer
```

The nightly proves macOS and Windows.

## Done

2026-11-06. Linux: the tests pass. macOS and Windows: the next nightly.

# T-068: The hint of an unknown issuer names a CA that replaces the roots, which podvm has not

**Source:** the trial of `pod-tls` in podvm (T-040, `87a1338`).
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`Words::unknown_issuer_hint` always names a place for a CA that replaces
the roots. podvm has only its host bundle, which adds to them: its hint
said "which replace podvm's roots", and that is wrong.

## Premise

- Read: `crates/pod-tls/src/hint.rs`; podssh and podnet set
  `replace_roots` to a flag and a variable.

## Approach

An empty `replace_roots` means that there is no such place: the hint then
names `add_roots` (else "a CA file") as the place that adds to the roots.
The field keeps its type, so the patches of v0.4.0 do not change.

## Prove

```sh
cargo test -p pod-tls --lib hint
```

## Done

2026-11-06. The test
`a_consumer_with_only_a_place_that_adds_gets_no_word_of_replace` passes
with the 6 tests of the hint. podvm sets `replace_roots` empty and
`add_roots` to its host bundle from the release after v0.4.0.

# T-069: `pod-proxy`: a streamed body for `blocking::Client` (and the async one)

**Source:** the trial of `pod-proxy` in podvm (T-040, `27f8688`).
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podvm's registry client streams each blob to disk. `blocking::Client`
reads each body into memory, so podvm keeps ureq for its registry and
image pulls (the trial of `pod-proxy` in podvm, `27f8688`).

## Premise

- Read: `crates/pod-proxy/src/blocking.rs`, `crates/pod-proxy/src/client.rs`.

## Approach

A response whose body is a reader (`Read` for the blocking face, a stream
for the async face), with the same caps and the same errors; the GET of
today stays a convenience over it. A test streams a body larger than the
cap of the buffered GET.

## Prove

```sh
cargo test -p pod-proxy --all-features
```

## Done

2026-11-06. `Client::send_streamed(request) -> Result<client::Streamed,
Failure>` sends the request and reads the head of the answer within
`Request::timeout`. `Streamed` has `status`, `reason`, `headers`, `header`
and `is_success`, and implements `tokio::io::AsyncRead` over the body; it
owns the connection. The body has no bound of its size. Each wait for
bytes has the limit `Request::body_idle` (`client::BODY_IDLE`, 30 s), else
`TimedOut`. A body that ends before its `Content-Length` or its last chunk
is `UnexpectedEof`; a `HEAD`, 1xx, 204 and 304 have no body. The client
follows no redirect, as `send`. The blocking face:
`blocking::Client::send_streamed(request) -> Result<blocking::Streamed,
Failure>`; `blocking::Streamed` has `status()`, `reason()`, `headers()`,
`header()` and `is_success()`, and implements `std::io::Read` with
`block_on` on the runtime of the client.

One code path (call C20): `http::read_head` reads the head and
`http::Body` (`src/http/body.rs`) reads each body, by length, by chunks or
to the close, with an optional bound and an optional idle limit
(`http::Idle`). `Client::send` is `send_streamed` with the bound
`max_body`, no idle limit and the body read whole, under its one time
limit; `http::read_answer` (under `https_get` and `http::exchange`) reads
through `read_head` and `Body`. The words and the caps of `send`,
`https_get` and `read_answer` did not change: their tests pass with no
change.

Tests: `tests/streamed.rs` (5) and `tests/blocking_streamed.rs` (5), with
the TLS server of `tests/served/`: a body of 17 MiB (more than `MAX_BODY`)
by `Content-Length` and by chunks of many sizes, read in reads of 7919
bytes, with its SHA-256 equal to the server's (async and blocking); the
buffered `send` refuses the same answer; a body cut at half its length is
`UnexpectedEof`; a server that stops in the middle of the body gives
`TimedOut` within 5 s for a `body_idle` of 300 ms; a `HEAD` has no body;
the blocking face through a CONNECT stand-in (`ProxyChoice::Via`), and
directly where `NO_PROXY` names the host while the proxy of the
environment fails (the rules of podnet, no loopback rule). Without the
change the new tests do not compile (no `send_streamed`, no `Streamed`).
`cargo test -p pod-proxy --all-features` passes (each test target), clippy
and fmt too.


# T-070: `pod-proxy`: the drop of a `blocking::Dialer` waits for a name lookup that the dial left

**Source:** the trial of `pod-proxy` in podvm (T-040, `27f8688`).
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The runtime of a `blocking::Dialer` waits, when it drops, for a system
name lookup that the time limit of a dial already abandoned (a proxy given
by its name; the trial of `pod-proxy` in podvm, `27f8688`).

## Premise

- Read: `crates/pod-proxy/src/blocking.rs`, `crates/pod-proxy/src/client.rs`.

## Approach

The drop shuts the runtime down in the background
(`Runtime::shutdown_background`). A test drops a dialer whose lookup
hangs and returns at once.

## Prove

```sh
cargo test -p pod-proxy --features blocking --test blocking
```

## Done

2026-11-06. The runtime of the blocking face shuts down in the background
when the last value that holds it drops (`blocking::Background`). The
test found a second fault under the first: the dial of `race.rs` gave the
deadline to the resolver and trusted it, so a resolver that did not keep to
it held the dial past its time limit (30 s for a limit of 200 ms). The dial
now bounds each call of the resolver by the deadline (the async face too).
`the_drop_of_a_dialer_does_not_wait_for_a_lookup_that_the_dial_left`:
without the change of `blocking.rs` the drop waited 29.8 s; with both
changes the test ends in 0.2 s. `cargo test -p pod-proxy --all-features`
passes, clippy too.


# T-071: `pod-proxy`: a read limit for `blocking::Stream::Exec`

**Source:** the trial of `pod-proxy` in podvm (T-040, `27f8688`).
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`blocking::Stream::set_timeouts` sets no limit for a proxy at a command,
and podvm's pump needs one; podvm refuses such a proxy now (the trial of
`pod-proxy` in podvm, `27f8688`).

## Premise

- Read: `crates/pod-proxy/src/blocking.rs`, `crates/pod-proxy/src/client.rs`.

## Approach

The read and the write of `Exec` take the limit with `tokio::time::timeout`
on the runtime of the dialer, and give `TimedOut` at the limit, as a
socket does. A test with a command that writes nothing.

## Prove

```sh
cargo test -p pod-proxy --features blocking --test blocking
```

## Done

2026-11-06. `blocking::Stream::set_timeouts` sets the limit of a proxy at a
command too: each read and each write of `Exec` runs within it, and at the
limit fails with the kind of a socket's limit (`WouldBlock` on Unix,
`TimedOut` elsewhere); a cancelled read or write of a pipe loses no byte.
`a_read_from_a_proxy_at_a_command_ends_at_its_limit`: without the change
the read waited 30 s; with it the test ends in 0.3 s. podvm can take a proxy
at a command for its pump.

# T-073: `pod-dns` names the DoH server that answered in its diagnostics

**Source:** PodUtils/podnet#322 (COORD-004 of podnet).
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podnet: "The live check of podnet (`live.yml`, step 'Quick tunnel with no
system DNS, then with 1.1.1.1 blocked') proves that DoH falls to a second
operator ... before the take it read `server=\"https://8.8.…\"` in the debug
line of `podnet::resolve` ... since the take, the line is `pod_dns::resolver`
`resolved` with `host`, `source` and `addrs`, and no server."

## Premise

- Read: `crates/pod-dns/src/way.rs` (`Way::lookup_with`), `resolver.rs`.

## Approach

Each answer of DNS over HTTPS writes a debug line `doh answered` of the
target `pod_dns::way` with `host`, `server` (the URL with no query:
`https://8.8.8.8/resolve`) and `operator`, before the line `resolved`.

## Prove

```sh
cargo test -p pod-dns --lib way
```

## Done

2026-11-06. `the_server_url_names_the_resolver_that_was_asked` passes;
podnet's check greps `server="https://8\.8\.` in the line `doh answered`
from the release after v0.5.0.

# T-074: `pod-proxy`: the words of a failed dial name the target twice

**Source:** the trial of the registry of podvm on `pod-proxy` (T-040, `44d1bd0`).
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The trial of the registry in podvm (`44d1bd0`): a failed dial reads
`could not connect to H: H: Connection refused`; podvm's glue removes the
repeats, as its tests pin each host once.

## Premise

- Read: `crates/pod-proxy/src/client.rs`, `src/env.rs`, `src/error.rs`.

## Approach

The words of `DialError::Connect` name the target once; a test pins the
text of a refused dial.

## Prove

```sh
cargo test -p pod-proxy --all-features --test dial
```

## Done

2026-11-06. When the one address that was tried is the target (a dial of
an address), the words name it once. `a_refused_dial_of_an_address_names_it_once`
failed on the old code (`could not connect to 127.0.0.1:50891:
127.0.0.1:50891: Connection refused`) and passes; `cargo test -p pod-proxy
--all-features` and clippy pass. A dial of a name keeps the address of each
attempt after the name.


# T-075: `pod-proxy`: `HTTP_PROXY` for a target of `http://`

**Source:** the trial of the registry of podvm on `pod-proxy` (T-040, `44d1bd0`).
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The trial of the registry in podvm (`44d1bd0`): the client reads
`HTTPS_PROXY` and `ALL_PROXY`, not `HTTP_PROXY`/`http_proxy` for a request
of `http://` (ureq did; podvm's insecure registries fall back to plain
HTTP). A host with only `HTTP_PROXY` set loses its proxy there.

## Premise

- Read: `crates/pod-proxy/src/client.rs`, `src/env.rs`, `src/error.rs`.

## Approach

`Environment::proxy_for` takes the scheme of the request: `http://`
reads `http_proxy`/`HTTP_PROXY` (then `ALL_PROXY`), `https://` as now; the
rules of each consumer (`env::Rules`) say the order. Tests of each
variable for each scheme.

## Prove

```sh
cargo test -p pod-proxy --lib env
```

## Done

2026-11-06. The proxy decision takes the scheme of the connection
(`env::Scheme`: `Http`, `Https`). `Rules::http_proxy_vars` gives the
variables of a request of plain `http://`: `HTTP_PROXY_VARS` (the
default: `http_proxy`, `HTTP_PROXY`, `all_proxy`, `ALL_PROXY`),
`HTTP_PROXY_VARS_LOWER` (curl, no `HTTP_PROXY`) or
`HTTP_PROXY_VARS_UPPER_FIRST` (podnet). `Environment::http_proxy`,
`proxy_of(scheme)` and `proxy_for_scheme(scheme, host, port, &pins)`;
`Dialer::proxy_for_scheme` and `blocking::Dialer::proxy_for_scheme`;
`pod_core::Dial::dial_http` (the default is `dial`), which `Dialer`
implements and which `Client::send` and `send_streamed` call for an
`http://` URL. An HTTP proxy gets `CONNECT host:port` for each scheme.
`HTTPS_PROXY` alone no longer serves `http://`, as in curl. The loopback
rule and `NO_PROXY` hold for each scheme. Tests:
`the_scheme_chooses_the_variables_of_the_proxy` and
`the_user_and_a_file_hold_for_each_scheme` (`src/env_tests.rs`), 3 in
`tests/http_proxy.rs`, and `a_request_of_http_takes_the_proxy_of_http_proxy`
in `tests/blocking_streamed.rs`. On the old code,
`a_request_of_http_takes_the_proxy_of_http_proxy` failed (`GET
http://name.test:42861/v2/: could not reach the proxy 127.0.0.1:47215`:
the request took `HTTPS_PROXY`), and so did
`a_request_of_http_takes_https_proxy_when_it_is_the_only_one`.
`cargo test -p pod-proxy --all-features` (168 tests, 6 new),
`cargo test -p pod-dns --all-features` (59), clippy, fmt, `cargo todo check` and `check-size.py` pass.

The default list for `http://` ends with `https_proxy` and `HTTPS_PROXY`:
a host that names only the proxy of `https://` keeps it for `http://`, as
the dial and podvm's ureq did (no capability is lost); curl's list
`HTTP_PROXY_VARS_LOWER` does not read them.

# T-076: `pod-proxy`: redirects in the client

**Source:** the trial of the registry of podvm on `pod-proxy` (T-040, `44d1bd0`).
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

The client follows no redirect. podvm's glue follows 301/302/303/307/308
for GET and HEAD, five at most, with `Authorization` kept to its host and no
step from `https://` to `http://` (the registry trial, `44d1bd0`); each
consumer would write the same.

## Premise

- Read: `crates/pod-proxy/src/client.rs`, `src/env.rs`, `src/error.rs`.

## Approach

`Request::redirects(n)` (0 by default): the client follows them for GET
and HEAD with podvm's rules; podvm's glue then goes. Tests of each rule.

## Prove

```sh
cargo test -p pod-proxy --all-features --test streamed
```

## Done

2026-11-06. `Request::redirects(n)` (the field `redirects: u8`, 0 by
default: no change for a request that does not ask). `Client::send` and
`send_streamed` go through one loop (`src/client/redirect.rs`), and the
blocking face through them. With n > 0 a `GET` or a `HEAD` follows 301,
302, 303, 307 and 308, at most n; another method, and a 3xx with no
`Location`, give the 3xx as it is. The `Location` resolves against the URL
that gave it (absolute, `//host/path`, `/path`, a relative path with `.`
and `..`, `?query`; the fragment goes). Each hop takes the proxy of its own
URL and the trust of the request; `Request::timeout` covers the heads of
the chain, `body_idle` the last body. `Authorization`,
`Proxy-Authorization` and `Cookie` go only to the scheme, host and port of
the first URL. A step from `https://` to `http://` is a `Failure`
(`a redirect from https:// to http:// is refused: FROM to TO`), and so is
one more redirect than n (`more than N redirects; the last one to URL`);
the URLs in the words have no query. `Response::url`, `Streamed::url` and
`blocking::Streamed::url()` give the URL of the answer (after redirects,
the last one); an answer of `http::exchange` has an empty `url`. Tests:
`tests/redirect.rs` (8: a 302 on the same host, each status, the
credentials and another port, a redirect over HTTPS with the trust of the
request, https to http refused, n=2 with 3 redirects, a `POST` and the
default, a relative `Location`), `a_redirect_is_followed` in
`tests/blocking_streamed.rs`, and `a_location_joins_its_base`
(`src/client/redirect.rs`). With the loop off, 7 of the 8 failed, for
example `a_302_to_another_path_of_the_host_is_followed` (`left: (302, "")`,
`right: (200, "ok")`); `a_post_and_the_default_get_the_redirect_as_it_is`
keeps the old behaviour. `cargo test -p pod-proxy --all-features` (178),
`cargo test -p pod-dns --all-features` (59), clippy, fmt, rustdoc with
`-D warnings`, `cargo todo check` and `check-size.py` pass. podvm's
redirect glue goes with `Request::redirects(5)` (`crates/pod-proxy/README.md`,
adoption map).


# T-077: `pod-proxy`: keep-alive connections in the client

**Source:** the trial of the registry of podvm on `pod-proxy` (T-040, `44d1bd0`).
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

Each request of the client is a new TCP and TLS connection
(`Connection: close`); ureq kept a pool (the registry trial in podvm,
`44d1bd0`). A pull of many blobs from one registry pays a handshake each.

## Premise

- Read: `crates/pod-proxy/src/client.rs`, `src/env.rs`, `src/error.rs`.

## Approach

A small pool of idle connections for each host and port in `Client`,
with a limit of idle time; a streamed body returns its connection when it
was read to its end. A test counts the connections of three requests.

## Prove

```sh
cargo test -p pod-proxy --all-features
```

## Done

2026-11-06, the commit of this entry. `Client::with_keep_alive(on)` (and
`blocking::Client::with_keep_alive`), on by default: the registry pull
of podvm is the reason, and no test of a consumer shows a reason against
it. `Client::send`, `send_streamed` and each redirect take an idle
connection of the pool (`src/client/pool.rs`), shared by the clones of a
client. The key: the scheme, the host, the port, the way out (the same
`Arc<dyn Dial>`) and the trust (`Default`, the path of `File`, the `Arc`
of a caller's configuration). A connection goes back after an answer of
`HTTP/1.1` with no `Connection: close`, not 1xx, whose body was read to
its end by its length or its chunks (on the drop of `Streamed`); a body
dropped before its end closes it. Limits: `IDLE` 30 s, `PER_KEY` 4,
`TOTAL` 16. An idle connection that is closed, or has stray bytes, is
dropped before use; a `GET` or a `HEAD` that fails on an idle connection
before a byte of the answer goes again once on a new connection, another
method does not. With keep-alive on, the head has no `Connection` line;
off, `Connection: close`. `https_get` and `https_get_accepting` keep
`http::exchange` with `Connection: close` (a stream that the caller
opened), out of the pool. Tests: `tests/keep_alive.rs` (9) and
`tests/blocking_keep_alive.rs` (1), over a counting server
(`tests/kept/`). With keep-alive off by default (a plant), 7 of the 9 and
the blocking test failed, for example
`three_requests_to_one_host_use_one_connection` and
`the_blocking_client_keeps_its_connection` (`left: ("c1r1", "c2r1",
"c3r1")`); `with_keep_alive_off_each_request_opens_a_connection` does not
compile on the old code, and
`a_connection_that_the_server_closed_is_not_used` keeps the old
behaviour. With the retry off (a plant),
`a_get_on_a_stale_connection_goes_again_once` failed. Three tests that
waited for the close of a tunnel now drop the client
(`tests/blocking_streamed.rs`) or take the head of the proxy when it
comes (`tests/http_proxy.rs`); `tests/fetch.rs` checks that the head has
no `Connection` line. `cargo test -p pod-proxy --all-features` (188),
`cargo test -p pod-dns --all-features` (59), 30 loops of the tests of the
pool, clippy, fmt, rustdoc with `-D warnings`, `cargo todo check` and
`check-size.py` pass.

# T-078: `pod-services`: a second public Piping Server

**Source:** PodUtils/podssh#158 (COORD-004 of podssh, for PodUtils/podssh@T-497).
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podssh: "podssh's PodUtils/podssh@T-497 wants a second public Piping Server after
`ppng.io`, tried only once the first failed. A host of another party is
pod-services' data now, so podssh asks it of podcrates rather than
compiling it in."

## Premise

- Read: the README of Piping Server lists `ppng.io`, `piping.onrender.com`
  and `piping.glitch.me`; podssh measured `/version` 1.12.9 on the second
  and 410 on the third; the owner's archive `ff-relay` found that the second
  holds a streamed request body until its end.

## Approach

`services/onrender-piping.toml` (kind `piping`, rank 1) with the facts
`streams-request-body` (false) and `idle-stop-minutes` (15, Render's free
services), and `ppng-piping` gets rank 0.

## Prove

```sh
cargo test -p pod-services --all-features
```

## Done

2026-11-06. `the_piping_servers_come_in_their_order` passes, with each test
of `pod-services`. podssh reads `of_kind("piping")` from the release after
v0.6.0.

# T-079: `pod-services`: Tailscale's main DERP fleet after tailcat's

**Source:** PodUtils/podssh#159 (COORD-005 of podssh, for PodUtils/podssh@T-534).
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podssh: "Q74 asks podssh to try Tailscale's main DERP fleet after tailcat's
four servers, once a short test shows that it lets podssh in. ... 27 of 28
relayed; `derp24b.tailscale.com` (hnl) was reset during TLS on this host,
likely by its proxy." The ask: a service beside `tailcat-derp`, in the order
of the regions' ids and each region's first server first, with the map's
URL and its date as the source.

## Premise

- Read: `https://login.tailscale.com/derpmap/default` on 2026-11-06: 28
  regions, 91 servers, none marked `Avoid` or `NoMeasureNoHome`.

## Approach

`services/tailscale-derp.toml` (kind `derp`, rank 1): one endpoint for each
server (`CODE-N`), its role the code of its region, its IPv4 and IPv6
addresses, in the order of the regions' ids and of the servers in each;
`tailcat-derp` gets rank 0. The watch reads `/Regions` of the map.

## Prove

```sh
cargo test -p pod-services --all-features
```

## Done

2026-11-06. `the_derp_fleets_come_in_their_order` passes (tailcat first;
the first server `derp1f.tailscale.com` of `nyc`; 28 regions), with each test
of `pod-services`. podssh reads it from the release after v0.6.0.


# T-082: `pod-proxy`: the `Host` header of an IPv6 literal on port 443 (GitHub #20)

**Source:** GitHub #20
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Issue #20: `Client::https_get` and `https_get_accepting` write
`Host: ::1` for an IPv6 literal on port 443, and `Host: [::1]:8443` on
another port. RFC 9110, section 7.2, needs brackets in both cases. The
line `let host_header = if port == 443 { host.to_string() } else {
authority(host, port) };` is in `crates/pod-proxy/src/client.rs`. The
function `authority` in `crates/pod-proxy/src/host.rs` brackets a host
with a colon, but the port 443 branch skips it. `check_name` lets a
literal with colons pass.

## Premise

- Read: `crates/pod-proxy/src/client.rs`, `crates/pod-proxy/src/host.rs`.
  The siblings `crates/pod-proxy/src/client/request.rs` and
  `crates/pod-dns/src/get.rs` bracket the host correctly.
- Not measured: a TLS exchange with a server on `[::1]`.

## Approach

Add `host_header(host, port)` to `crates/pod-proxy/src/host.rs`. It
omits the port for 443 and brackets a host with a colon. Use it in
`crates/pod-proxy/src/client.rs`. The unit test of `host_header` is the
necessary check. The TLS test on `[::1]` is best effort: it skips when
the host has no IPv6 loopback.

## Prove

```sh
cargo test -p pod-proxy host_header
cargo test -p pod-proxy --test get
```

The unit test covers `[2001:db8::1]`, `[2001:db8::1]:8443`,
`example.com` and `example.com:8443`. A plant reverts the call in
`crates/pod-proxy/src/client.rs`; the test then fails.

## Done

2026-11-09, `394d1e6`, plant `4204dd8`. `host_header(host, port)` in `crates/pod-proxy/src/host.rs`, used by `Client::https_get_accepting`. `cargo test -p pod-proxy host_header`: `the_host_header_brackets_an_ipv6_literal_on_each_port` passes (`::1` and `2001:db8::1` on 443 and 8443, a bracketed literal, `example.com`). The plant `proxy-host-header-unbracketed` is caught. The TLS test on `[::1]` is not written (best effort in the entry).

# T-083: `pod-proxy`: the SOCKS5 datagram path (UDP ASSOCIATE and command 0x83) (GitHub #21)

**Source:** GitHub #21
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

Issue #21: `pod-proxy` dials TCP only. `crates/pod-proxy/src/socks5.rs`
has the command `CONNECT` (0x01) only, and the crate has no UDP code.
The move of `pod-warp` (T-028) needs the framing of podnet
(`PodUtils/podnet:crates/podnet-netns/src/upstream.rs`):
`socks5_udp_in_stream`, `udp_associate`, `wrap_udp`, `wrap_udp_name`,
`write_udp_frame`, `read_udp_frame`. A refused command 0x83 (reply 0x07)
must be a `DialError` variant. T-020 (`pod-session`, UDP in a stream) and
T-023 cover other mechanisms.

## Premise

- Read: `crates/pod-proxy/src/socks5.rs` (337 lines),
  `crates/pod-proxy/src/error.rs`, `docs/design.md` (the `pod-warp` row).
- Call C23 (`docs/decisions.md`) puts the framing in `pod-proxy`.
- Not measured: the size of the new module.

## Approach

A new module `socks5/udp.rs` under `crates/pod-proxy/src/` (the file
stays under 500 lines), exported from `crates/pod-proxy/src/lib.rs`. A
new variant `DialError::UdpRefused`. Update the `pod-warp` row of
`docs/design.md` and name the boundary with T-020: `pod-session` keeps
its own UDP-in-a-stream adapter.

## Prove

```sh
cargo test -p pod-proxy --test socks5_udp
sh scripts/plant-each.sh
```

Mock servers test the 0x83 round trip, the reply 0x07 followed by
ASSOCIATE, a close after 0x83 (a `DialError`), and the byte layout with
`Ok(None)` on end of stream. A plant flips the length prefix.

## Done

2026-11-09. Commits `176a8d9` (the module), `9995003` (the plant),
`8a22c7f` (the bound of the test). `crates/pod-proxy/src/socks5/udp.rs`
(204 lines) is `pod_proxy::udp`: `open_in_stream` (0x83), `associate` (UDP
ASSOCIATE), `write_frame`, `read_frame`, `wrap`, `wrap_name`, and the
constants of the commands. The reply 0x07 to 0x83 is
`DialError::UdpRefused` (stage Proxy, class Unsupported); a refusal of a
rule is a `ProxyRefused` (403), a close or a break of the protocol is a
`ProxyProtocol`. `wrap_name` refuses a name of more than 255 bytes
(`InvalidTarget`); podnet cut it. The caller dials the proxy and gives the
stream. `tests/socks5_udp.rs` (8 tests): the 0x83 round trip, 0x07 then
ASSOCIATE, a close after 0x83, the refusals, the bound address of each
type, the bytes of the frame and of the header, `Ok(None)` at the end of
the stream. Plant `proxy-udp-frame-length` (a length in little endian):
caught. `SOURCES` of `pod-proxy` has a block of `PodUtils/podnet` at
`29ba18a` for the file upstream.rs of `podnet-netns`: podnet freezes the
file. The `pod-warp` row of `docs/design.md` names the boundary with
T-020.

podnet took the datagram path in podnet `127c095` (PodUtils/podnet#417,
PodUtils/podnet@T-224): `PodUtils/podnet:crates/podnet-netns/src/upstream.rs` calls `pod_proxy::udp`. The
block `repo PodUtils/podnet 29ba18a` of `crates/pod-proxy/SOURCES` has
`taken 127c095`; `check-sources.py` reads 0 changes. The freeze of
GitHub #42 ends.

# T-089: `pod-services`: `tailscale-derp` watches the hosts and holds the four missing servers (GitHub #27)

**Source:** GitHub #27
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Issue #27: the only watch of
`crates/pod-services/services/tailscale-derp.toml` is
`pointer = "/Regions"`. It does not compare the hosts. The map of
2026-11-07 has 95 nodes and the file has 91 endpoints. The servers
`derp1j`, `derp1k`, `derp4k` and `derp4l` are missing. The sibling
`crates/pod-services/services/tailcat-derp.toml` uses
`member = "HostName"` and `role = "region"`.

## Premise

- Read: both files, and the `http-json` row of
  `crates/pod-services/README.md`. `grep -c '^\[\[endpoint\]\]'` gives 91.
- Not measured: the live map (no egress in a cloud session).
- The watch runs only when T-050 builds the runner; the data change does
  not wait for it. T-079 added the fleet.

## Approach

Add `member = "HostName"` and `role = "region"` to the watch. Add the
four servers, with the addresses and `read` dates of the live map, in the
order of the file. T-050 gets a line for the "count" watch kind.

## Prove

```sh
cargo test -p pod-services
```

The compare script of issue #27 (live map against the file) prints no
host in either difference list. It needs egress: run it in a workflow or
let the owner run it.

## Done

2026-11-09, `fa09d1e`. The live map (`https://controlplane.tailscale.com/derpmap/default`, read 2026-11-09) has 95 nodes; the file had 91 and no extra host. The servers `derp1j`, `derp1k` (`nyc-5`, `nyc-6`), `derp4k` and `derp4l` (`fra-6`, `fra-7`) have their addresses and a `read` date. The watch of `/Regions` has `member = "HostName"`; it has no `role`: the roles of the file are the codes of the regions, so no endpoint has the role `region`, and with no `role` the watch compares each endpoint (`crates/pod-services/README.md`). `cargo test -p pod-services`: `the_tailscale_fleet_holds_the_four_servers_and_a_watch_of_the_hosts` passes. The compare of the live map against the file printed no host in either list. The line for the watch kind "count" in T-050 is not added: the watch uses `http-json` with `member`.

# T-090: `pod-services`: `ajam-relay` watches its seven hosts and the pool (GitHub #28)

**Source:** GitHub #28
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Issue #28: `crates/pod-services/services/ajam-relay.toml` has 7
endpoints (`default` and 6 `seed-*`) and 3 watches (`/limits`,
`/version`, `rdap`). No watch names a host. The six seeds are the default
pool of podssh and podnet. The note "in the member pool" has no re-read.
The sibling `crates/pod-services/services/ajam-ts-relay.toml` uses
`tls-name`.

## Premise

- Read: the file, its sibling, and the watch forms of
  `crates/pod-services/README.md`.
- Not measured: the live pool.
- The run of the watches waits for T-050; the data change does not.

## Approach

Add three watches: `tls-name` (role `seed`, and the endpoint `default`),
`http-json` with `pointer = "/pool"`, `member = "host"`, `role = "seed"`,
and `http-status` with role `seed`, path `/health`, status 200.

## Prove

```sh
cargo test -p pod-services
grep -c 'kind = "tls-name"' crates/pod-services/services/ajam-relay.toml
```

The `grep` prints 2 or more.

## Done

2026-11-09, `09cfb80`. `ajam-relay.toml` has two `tls-name` watches (role `seed`, endpoint `default`), an `http-json` watch (`/pool`, `member = "host"`, `role = "seed"`) and an `http-status` watch (role `seed`, `/health`, 200). `grep -c 'kind = "tls-name"'` prints 2; `cargo test -p pod-services`: `the_relay_watches_its_hosts_and_its_pool` passes. The live `/relays.json` has a list `pool` of objects with a `host` member. The run of the watches waits for T-050.

# T-091: `pod-dns`: `Way::lookup` refuses a name that is not DNS-safe (GitHub #29)

**Source:** GitHub #29
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Issue #29: `Way::lookup` and `Way::lookup_with` reach `Doh::target` with
no `dns_safe` check. The JSON arm of `Doh::target` writes `?name={name}`
raw into the query string. A space, `&`, `#` or `%` in the name changes
the question. `dns_safe` is in `crates/pod-dns/src/lib.rs`. The files
`crates/pod-dns/src/question.rs`, `crates/pod-dns/src/wire.rs` and
`crates/pod-dns/src/resolver.rs` apply it; `crates/pod-dns/src/way.rs`,
`crates/pod-dns/src/ask.rs` and `crates/pod-dns/src/doh.rs` do not.
`crates/pod-proxy/src/http.rs` refuses only CR and LF.

## Premise

- Read: the files above. `grep dns_safe crates/pod-dns/src/lookup.rs`
  has no hit.
- Not measured: the answer of a public resolver to a changed question.

## Approach

Check `dns_safe` in `ask_kept` (`crates/pod-dns/src/ask.rs`) and give
`Missed::words` on failure. Percent-encode the name in the JSON arm of
`Doh::target`. Keep the check of the wire form.

## Prove

```sh
cargo test -p pod-dns refuses_a_name
sh scripts/plant-each.sh
```

The tests: `a b.example`, `a&b.example`, `x#y.example` and
`a%b.example` give `Err`; `relay.example` and `a-b_c.example` keep their
target. A plant removes the new check.

## Done

2026-11-09, `e81f5f2`, plant `1959e37`. `ask_kept` refuses a name that is not DNS-safe with `Missed::words`; the JSON arm of `Doh::target` percent-encodes the name. `cargo test -p pod-dns`: 45 unit tests and the tests of `tests/` pass, among them `refuses_a_name_that_changes_the_question` (`a b.example`, `a&b.example`, `x#y.example`, `a%b.example`; the same through `lookup_with`) and `the_json_name_is_percent_encoded` (`relay.example` and `a-b_c.example` keep their target). The plant `dns-name-not-checked` is caught.

# T-096: `pod-proxy` README: the six variables of `Rules::http_proxy_vars` (GitHub #34)

**Source:** GitHub #34
**Category:** docs
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

Issue #34: the row of `crates/pod-proxy/README.md` for
`Rules::http_proxy_vars` lists 4 variables for each list. Each list has 6
entries in `crates/pod-proxy/src/env.rs`. The test
`crates/pod-proxy/src/env_tests.rs` pins the fallback to `HTTPS_PROXY`
for the default list only. `HTTP_PROXY_VARS_UPPER_FIRST` appears there in
a helper only.

## Premise

- Read: the README, `crates/pod-proxy/src/env.rs`,
  `crates/pod-proxy/src/env_tests.rs`.
- Not measured: the behaviour of a consumer with the two missing names.

## Approach

List the 6 variables of each list. State the fallback for both lists in
the other places of the README that describe it. Add a test for
`HTTP_PROXY_VARS_UPPER_FIRST` in `crates/pod-proxy/src/env_tests.rs`.
Option: a check that the row lists as many names as the code.

## Prove

```sh
cargo test -p pod-proxy env_tests
cargo todo check
```

## Done

2026-11-09. Commit `4865802`. The row of `Rules::http_proxy_vars` and the
two places that describe the fallback list the six variables of each list.
The new test `the_upper_case_first_list_of_http_ends_with_the_proxy_of_https`
in `crates/pod-proxy/src/env_tests.rs` pins the order and the fallback to
`https_proxy` and `HTTPS_PROXY` of `HTTP_PROXY_VARS_UPPER_FIRST`.
`cargo test -p pod-proxy env_tests`: the test passes. `cargo todo check`:
the record agrees.

# T-112: `pod-services`: the party of each service as a filter, and the effective set as JSON (GitHub #35)

**Source:** GitHub #35 (A-S-04)
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** S
**Status:** open

## Problem

podssh needs one switch over the services of other parties (`PODSSH_PUBLIC=off`, `fallback` or `default`).
Each service file of `pod-services` names its party.

## Premise

- Read: `crates/pod-services/` and the ask of A-S-04.
- Not measured: a dial recorder.

## Approach

Add `of_party` and a filter by own or third party. Give the effective set as JSON. The switch stays in
podssh.

## Prove

```sh
cargo test -p pod-services
```

# T-125: `pod-dns`: one kept connection for each DoH server (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 5 (PodUtils/podnet@T-175, PodUtils/podnet#370)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Each DoH query opens a new TLS connection: the exchange of `pod-dns` sends `Connection: close`
through `pod-proxy`.

## Premise

- Read: the ask of podnet; the keep-alive of the client of `pod-proxy` (T-077).
- Not measured: the time of N queries with one connection.

## Approach

Keep one connection for each server and way out in `Resolver`, through the keep-alive of
`pod-proxy`; close it after about 5 s idle; one reconnect; the deadline covers one exchange. A
loopback test: N queries, one connection.

## Prove

```sh
cargo test -p pod-dns --all-features
```

## Done

2026-11-09, `5d3f791`, plant `d4e9244`. `Kept` (`crates/pod-dns/src/kept.rs`) holds at most one connection for each host and port, 5 s idle; `Resolver` owns one and a `Way` has the field `kept` (`None`: a connection for each question). HTTP/1.1 uses the new `pod_proxy::http::exchange_keep`; HTTP/2 keeps its `SendRequest`. A kept connection that failed gets one reconnect. `cargo test -p pod-dns --all-features`: `many_questions_use_one_connection` (four questions, one connection), `a_closed_connection_is_replaced_by_one_reconnect`, `no_kept_connection_opens_one_for_each_question`; `cargo test -p pod-proxy --test http`: `exchange_keep_gives_the_stream_back_only_when_it_can_carry_another_request`. The plant `dns-connection-not-kept` is caught. Not measured: the time of N queries with one connection.

# T-126: `pod-dns`: a pin table that the caller can replace (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 6 (PodUtils/podnet@T-172, PodUtils/podnet#367)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

A caller cannot pin a name that becomes resolvable while the process runs: the pins of the
resolver are fixed at its start.

## Premise

- Read: the ask of podnet; `pod_core::Resolve::pinned`.
- Not measured: a change.

## Approach

A pin table of the resolver that the caller can replace at run time (an `Arc` swap). A test that
replaces a pin between two lookups.

## Prove

```sh
cargo test -p pod-dns --all-features
```

## Done

2026-11-09, `3fb3d0c`, plant `6a803a5`. `Resolver::set_pins` swaps the table (an `Arc` under a lock), `Resolver::pins` reads it; `Settings::pins` is the first table. `cargo test -p pod-dns --all-features`: `a_pin_table_replaced_at_run_time_counts_from_the_next_lookup` (a pin set after the start, a replacement between two lookups, an empty table) and `the_pins_of_the_settings_are_the_first_table` pass. The plant `dns-pins-not-replaced` is caught.

# T-127: `pod-services`: the file host of PyPI (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 7 (PodUtils/podnet@T-182, PodUtils/podnet#378)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`podnet doctor --agent` checks the host of a package index only. podnet reads the file hosts of
docker and cargo from the answers of the index; PyPI names its file host in no answer.

## Premise

- Read: the ask of podnet.
- Not measured: the host from a session.

## Approach

A service file for `files.pythonhosted.org` (the file host of PyPI), in the form of the docker
registry file, with a source and a watch.

## Prove

```sh
cargo test -p pod-services
```

## Done

2026-11-09, `c1e17f2`. `crates/pod-services/services/pypi-files.toml`: the kind `package-files`, the endpoint `files` (`files.pythonhosted.org`, no address: a CDN serves it), the fact `probe-path` (the wheel of a release) and the watches `tls-name` and `http-status` (200 on the probe path, read 2026-11-09). `cargo test -p pod-services`: `the_file_host_of_pypi_is_a_service` passes.

# T-130: `pod-services`: the public Wisp servers, a kind `wisp-public` (PodUtils/podssh#175)

**Source:** PodUtils/podssh#175 (COORD-008 of podssh; PodUtils/podssh@T-535, the owner's ruling Q75 of podssh)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podssh's fallback tries the public Wisp servers after its own forwarders
(the kind `forwarder`, last `wisp-forwarder`). podssh holds no host of
another party, so the list must be data of `pod-services`. No kind
`wisp-public` exists; podssh reads an empty list today.

## Premise

- Read: the ask (PodUtils/podssh#175); `crates/pod-services/services/wisp-forwarder.toml` (the form, its watches).
- Not read: the public Wisp servers and their pages; `PodUtils/podssh:docs/carriers.md` (the sweep of 2026-11).

## Approach

A kind `wisp-public`: one service file for each server, with `[rank]`
after `wisp-forwarder`, one endpoint `forwarder` (scheme `wisp+wss`, host,
port, path), the facts `word` (unique among `forwarder` and
`wisp-public`), `template` (`wisp+wss://HOST/PATH/`) and `note` (the
server, its operator, its terms and limits, a filter of port 22 or of
names), each with a `source` (URL and the date read), and the watches of
`wisp-forwarder` (websocket-upgrade, forward-banner to `github-ssh`).
A server qualifies only when (Q75) its operator runs it for the use of
others and says so on a page of its own, with no account, on the
operator's own domain; and (Q79) the domain does not look taken over, the
server does not come from the list of a web proxy, and the sweep of
2026-11 did not flag it. A short live check: a forward to `github.com:22`
gives GitHub's version line, from GitHub Actions. If no server qualifies,
a note on PodUtils/podssh#175 is the answer.

## Prove

```sh
cargo test -p pod-services
```

The live check of each server runs in a dispatched workflow; its run id
goes in the Done section.

## Done

2026-11-10, commit `c4c9197`. No server qualifies; no file of the kind `wisp-public` exists, and the answer is a note on PodUtils/podssh#175 (written by the parent session). What was looked at: the search mirrors (queries for public Wisp servers, lists of them, "free to use" pages), the TitaniumNetwork and Mercury Workshop pages, the READMEs of the server projects (wisp-server-python, wisp-js, epoxy-tls, ghostproxyofficial/wisp-server, wasmer/wisp-server), the v86 networking page, and `PodUtils/podssh:docs/carriers.md` (the sweep of 2026-11-02). Result:

* `wisp.mercurywork.shop` (Mercury Workshop): already the kind `forwarder` (`wisp-forwarder`); not repeated.
* `anura.pro` (Mercury Workshop): the Wisp server of the demo of its own web proxy, not offered to others; flagged by the sweep.
* `relay.widgetry.org`: the wsproxy of v86's example, not a Wisp server for others (the sweep).
* The 76 servers of the lists of web proxies: refused by Q79 (a web proxy's list; 54 flagged as other parties' domains).
* The server projects (ghostproxyofficial and the others): software for the user's own deployment; no page of an operator that offers a running server to others (read 2026-11-10).

No page of an operator, on the operator's own domain, offers a Wisp server to others with no account (Q75), apart from Mercury Workshop's. The test `a_wisp_server_is_a_wss_forwarder_with_a_unique_word` in `crates/pod-services/tests/public_carriers.rs` holds the rules for a file that a later session adds: a `forwarder` endpoint on `wisp+wss`, a unique `word`, the two watches. `cargo test -p pod-services` passes. No live run is needed: no server.

# T-133: `pod-services`: the public proxy lists and nodes, the kinds `proxy-list` and `proxy-node` (PodUtils/podssh#177)

**Source:** PodUtils/podssh#177 (COORD-010 of podssh; PodUtils/podssh@T-574, the rulings Q78, Q79 and Q90 of podssh)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

podssh's last fallback of its forward road (for `podssh ssh` only) tries the
public proxy nodes of aggregator lists (VLESS and Trojan). podssh names no
host of another party, so the lists and a short list of nodes must be data of
`pod-services`. The kinds `proxy-list` and `proxy-node` do not exist; podssh
reads an empty catalog today and changes no behaviour.

## Premise

- Read: the ask (PodUtils/podssh#177), with the fields of each kind.
- Not read: the public lists, their maintainers and their nodes.

## Approach

`proxy-list`: one service for each public list of `vless://` and `trojan://`
share links at an https URL (`party` the maintainer, `[rank]`, one endpoint
`list` with `path`, a fact `note` with how often the list changes and the
share of VLESS or Trojan nodes on 443, a watch `http-status` 200).
`proxy-node`: the short list for a host that cannot fetch a list (Q90): one
service for each node (the endpoint as the link gives it, a fact `link` in
the form podssh's `Server::parse` reads, no `flow=`, no `security=reality`,
VLESS `type` `ws` or `xhttp`, Trojan `type` `tcp` or `ws`), and a watch that
carries GitHub's SSH version line through the node before each release.
Each entry keeps the rules of the ask: a source for each fact (Q79); no host,
SNI or `Host` under `.edu`, `.k12.`, `.ac.`, `.sch.`, `.gov`, `.mil` or a
church's name, and none that fronts another party's name; port 443 with TLS,
and a public address or one name for host, SNI and `Host`; no VMess
(PodUtils/podssh@T-573); at most one short probe a day to each list and node. The share links
of public nodes go in `.github/secrets-allow.txt` with the reason. If no list
or node qualifies, a note on PodUtils/podssh#177 is the answer.

## Prove

```sh
cargo test -p pod-services
```

The live check of each node runs in a dispatched workflow; its run id goes in
the Done section.

## Done

2026-11-10, commit `c4c9197`. Partial: the files and the test are done; the live check of the nodes remains (no workflow of this shape exists in podcrates: the runner of the watches is T-050, and the egress of a cloud session is an allowlist).

Lists (kind `proxy-list`, read 2026-11-10, each file counted on the day of the read; the maintainer's repository README names the file):

* `barry-far-list`: https://raw.githubusercontent.com/barry-far/V2ray-Config/main/All_Configs_Sub.txt (MIT; source https://github.com/barry-far/V2ray-Config): 8777 nodes, 7076 VLESS or Trojan, 4299 of those on 443, 314 that podssh takes.
* `mahdibland-list`: https://raw.githubusercontent.com/mahdibland/V2RayAggregator/master/sub/sub_merge.txt (source https://github.com/mahdibland/V2RayAggregator): 4585 nodes, 766 VLESS or Trojan, 406 on 443, 146 that podssh takes.
* `v2go-list`: https://raw.githubusercontent.com/Danialsamadi/v2go/main/AllConfigsSub.txt (source https://github.com/Danialsamadi/v2go): 589 nodes, 288 VLESS or Trojan, 188 on 443, 6 that podssh takes.

Nodes (kind `proxy-node`, from the lists above; each link is a line of the list, with no name after `#`):

* `node-bolab-trs02` (trs02.bolab.net, Trojan tcp), `node-bonds-sg1` (sg1-trojan.bonds.id, Trojan tcp), `node-udpgw-trojan1` (trojan1.udpgw.com, Trojan tcp), `node-tr202607-nl1` (nl-1.tr202607.com, Trojan tcp): the list of mahdibland.
* `node-xhttp-193-160-208-240` (193.160.208.240) and `node-xhttp-78-17-39-129` (78.17.39.129): VLESS xhttp with TLS, a public address for host and SNI; each is in the lists of barry-far and Danialsamadi.

Rules applied: port 443 with TLS; one name for host, SNI and Host, or a public address; no `flow=`, no `security=reality`; VLESS `type` ws or xhttp; Trojan `type` tcp or ws; no VMess; no host under `.edu`, `.k12.`, `.ac.`, `.sch.`, `.gov`, `.mil`. The domains of the named nodes have no category of adult or security at the domain mirror (read 2026-11-10); `udpgw.com` is in the category Anonymizer (a name made for a proxy).

Left out: the other lists (MatinGhanbari/v2ray-configs: 3 nodes qualify; Pawdroid/Free-servers: none; the lists of Epodonios, ebrasha and soroushmirzaei: no file at the path read); the aggregator sites with their own subscription pages (their terms were not read). Left out of the nodes: each host that looks taken over or that fronts another party's name (`onecloudtraining.com`, `lnmarketplace.net`, `findlocaljobnetworkandbuildcommunityconnections.com`, `pleadcourt.org`, `gadgetflow.my` and similar names; read 2026-11-10); each node whose SNI or Host differs from its host (a front); the nodes on the platforms `*.up.railway.app`, `*.onrender.com`, `*.workers.dev` and `*.hf.space` (a project of one user on another party's platform, removed within days); the Trojan node `au.tronsg.com` (its password holds an `@`, which a parser can read in two ways); all VMess, Reality and `flow=` lines.

Files: `crates/pod-services/services/{barry-far-list,mahdibland-list,v2go-list}.toml` and `node-*.toml` (6). The watch of a node is a new kind `proxy-banner` (`fact`, `target`), listed in the README; its runner is T-050. `crates/pod-services/tests/public_carriers.rs` (6 tests) holds the rules. `cargo test -p pod-services` passes.

The live check (T-050): `services.yml`, run 42803032733 (GitHub Actions, 2026-11-10): the three lists answer 200; `node-bolab-trs02` and `node-bonds-sg1` time out on the TCP connect, `node-udpgw-trojan1` refuses the TLS name (`UnrecognisedName`), `node-tr202607-nl1` answers the Trojan request with `HTTP/1.1 400 Bad Request` (a password that it no longer takes); the two XHTTP nodes are skipped (T-152). The nodes that fail leave or change by T-157.

The clause of before: the live check (Q75, Q90), before the tag that carries the nodes: for each node, a session through its share link to `github.com:22` gives GitHub's SSH version line (`SSH-2.0-...`), from GitHub Actions; at most one short probe a day to each list and node; each node that fails leaves by a change of its file. For each list: a GET gives 200 (`http-status`). `.github/secrets-allow.txt`: a line for each finding of the scanner on the links, when the run of `secrets.yml` on the push names one (the link has no `user:password` form).

# T-135: `pod-services`: Pinggy's SSH inside TLS, its host key and its facts (PodUtils/podssh#178)

**Source:** PodUtils/podssh#178 (COORD-011 of podssh; PodUtils/podssh@T-548)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podssh's node dials SSH inside TLS to Pinggy and pins the host key. The
service `pinggy` has the endpoint `ssh` (raw SSH) and the fact
`dashboard-host` only. podssh keeps the rest as constants.

## Premise

- Read: the ask (PodUtils/podssh#178); the pages `pinggy.io/docs/client_behind_proxy/`,
  `pinggy.io/docs/tls_tunnels/` and `pinggy.io/`, through read.web.pkgforge.dev on
  2026-11-09. Each value is on its page.
- Not read: a live SSH handshake with the edge.

## Approach

Add to `services/pinggy.toml`, with no change to what is there: the endpoints
`ssh-tls` (port 443) and `ssh-tls-alt` (port 7878), scheme `ssh+tls`, `sni`
`free.pinggy.io`; the facts `host-key-sha256`, `tunnel-zone` and
`free-tunnel-minutes` (unit `min`); a watch `ssh-host-key` on the endpoint
`ssh` that names the fact. The schema takes any scheme, unit and watch word,
so its code does not change. The README lists the scheme, the unit and the
watch kind. The runner of the kind is T-050.

## Prove

```sh
cargo test -p pod-services
```

## Done

2026-11-09, `48539d2`. `crates/pod-services/services/pinggy.toml` holds the two endpoints, the three facts and the watch `ssh-host-key`. `crates/pod-services/README.md` lists `ssh+tls`, `min` and the kind. `cargo test -p pod-services`: `pinggy_has_ssh_inside_tls_and_its_host_key` passes.

# T-151: `pod-services`: the watch scripts of podssh and podnet that the watches replace leave them

**Source:** T-050 (the scripts of the inventories that `services.yml` now does).
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

podssh and podnet still watch facts that `pod-services` holds and that
`services.yml` watches each day. Two checks of one fact drift. podssh:
`PodUtils/podssh:scripts/check-domain.py` (the expiry of `ajam.dev`; a step of
`relay-watch.yml`, and the issues of `relay_watch.py`),
`PodUtils/podssh:scripts/check-relay-limits.py` (the limits of `/relays.json` against
`PodUtils/podssh:crates/podssh-probe/facts/relay-limits.toml`; steps of `relay-watch.yml`
and `build.yml`), `PodUtils/podssh:scripts/check-upstream.py` (cloudflared against
`quick/cloudflared-pin.txt`; `upstream.yml`), and the parts `roots`,
`discover`, `matrix`, `api` and `through` of
`PodUtils/podssh:scripts/quick-tunnel-edge.py`, which measure the edge as
`scripts/services/edge-facts.sh` does. podnet: `tools/check-upstream.py`
(cloudflared against `src/cloudflare/cloudflared-pin.txt`;
`upstream.yml`). Each compares a file of the consumer that the consumer
reads today; the consumers did not take the rows of `relay-limits.toml`
and `cloudflared-pin.txt` of the adoption maps.

## Premise

- Read: podssh `961acb1` and podnet `5bf153d` (the files above, their
  workflows); the adoption maps of `crates/pod-services/README.md`.
- Not done: no trial of podssh; the take of the pin of cloudflared waits
  for `pod-cloudflare` (T-029) or the take of the facts.

## Approach

1. podssh: when its code reads the limits of `ajam-relay` (the row of
   `relay-limits.toml`), a trial `adoption/podssh/pod-services-watch.*`
   removes `check-domain.py`, `check-relay-limits.py`, their steps and
   the domain part of `relay_watch.py`, and keeps the relay spec (T-047).
   `services.yml` opens no issue: the red daily run is the signal, or an
   issue step joins `services.yml` first (a call of this entry).
2. podssh and podnet: `check-upstream.py` and `upstream.yml` leave with
   the take of the facts `cloudflared-*` (the rows `cloudflared-pin.txt`).
3. podssh: the generic parts of `quick-tunnel-edge.py` go to
   `scripts/services/` beside `edge-facts.sh`; its `run` and `relay` parts
   stay (they run podssh and cloudflared).

## Prove

```sh
sh scripts/adoption.sh podssh pod-services-watch
sh scripts/adoption.sh podnet pod-services-watch
```

# T-152: `pod-services-watch`: `proxy-banner` speaks no VLESS over XHTTP

**Source:** T-050 (the live run: two nodes skipped).
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

The watch `proxy-banner` speaks Trojan and VLESS over TCP, with or without
TLS. The nodes `node-xhttp-193-160-208-240` and `node-xhttp-78-17-39-129`
are VLESS over XHTTP (`mode=auto`, `mode=packet-up`, `alpn=h2,http/1.1`):
the watch reports them `skipped`, so no daily check proves that they carry
a session (T-133, the live check of the nodes).

## Premise

- Read: the links of the two files; the code of `src/proxy.rs`.
- Not read: the XHTTP transport of Xray (its modes, its paths and its
  padding) at its source.

## Approach

Read the XHTTP transport of Xray-core at a pinned commit. Add the
packet-up mode over HTTP/1.1 and HTTP/2 (`pod-proxy`'s client for the
uploads, a streamed GET for the downloads) to `proxy-banner`, with a
stand-in test of each mode and a plant. A node that no mode reaches leaves
the data (podssh decides, PodUtils/podssh#177).

## Decision

C52 (`docs/decisions.md`). The client follows XTLS/Xray-core at
`c43ee98ec7c5a9d34b32cedf891b4c8707e9c8b0` (`transport/internet/splithttp/`,
read 2026-11-11). `auto` is `packet-up` for TLS and for no security. The
HTTP version is the protocol that the server chose by ALPN; with no
choice, the version of Xray's client (`decideHTTPVersion`). The other forms
stay `skipped`, with the reason.

## Prove

```sh
cargo test -p pod-services-watch --test watches -- ssh:: xhttp::
sh scripts/plant-each.sh services-xhttp-no-sequence
```

## Done

2026-11-11, the commit of this entry.

Read: XTLS/Xray-core at `c43ee98ec7c5a9d34b32cedf891b4c8707e9c8b0`, the
default branch: `dialer.go`, `client.go`, `config.go`, `xpadding.go`,
`hub.go` and `upload_queue.go` of `transport/internet/splithttp/`, and
`transport/internet/tls/config.go` (the default ALPN: `h2`, `http/1.1`).

What the watch speaks now (`crates/pod-services-watch/src/xhttp.rs`):
VLESS over XHTTP in the mode `packet-up`, with TLS or with no security. A
streamed GET of `PATH/SESSION` carries the downloads. A POST of
`PATH/SESSION/0` carries the VLESS request of
`crates/pod-services-watch/src/proxy.rs` (the same bytes as over TCP); the
watch then reads the VLESS answer and the banner as over TCP. Each request
has the padding `x_padding` (100 to 1000 `X`) in the query of a `Referer`.
The session id is a random UUID of version 4. The path has a `/` at each
end and keeps the query of the path of the link. TLS offers the `alpn` of
the link, else `h2` and `http/1.1` (`Net::tls_alpn`). HTTP/1.1: the GET and
the POST each on a connection of their own, through `pod_proxy::http`
(`exchange`, `read_head`, `Body`). HTTP/2: two streams of one connection,
through `h2`. Skipped, with the reason: the modes `stream-up` and
`stream-one`, HTTP/3, the `extra` settings of a link, a VLESS `flow` or
`encryption`.

Dependencies: `h2`, `http` and `bytes` (pure Rust; in the workspace for
`pod-dns` and `pod-ws`). `pod_proxy::Client` speaks HTTP/1.1 alone, and it
takes the host of its URL as the dial target, the server name and the
`Host` together.

Fixed in the same work: `Link::parse` percent-decodes the user and each
value of the query (a path `%2F...`, a password with `%40`); the transport
names `raw` (the new name of `tcp`) and `splithttp` (the old name of
`xhttp`) are spoken; a VLESS answer with a version other than 0 is a
failure; the helper of the test certificates
(`crates/pod-tls/tests/common/cert_encoding.rs`) printed two debug lines
for each certificate.

Tests (`crates/pod-services-watch/tests/watches/xhttp.rs`, 3):
`proxy_banner_speaks_xhttp_packet_up_over_http1` (a stand-in node on
HTTP/1.1 over TLS; a path with a query, percent-encoded; a path that the
node refuses gives `answered HTTP 400`),
`proxy_banner_speaks_xhttp_packet_up_over_http2` (a stand-in node on
HTTP/2 over TLS, the server of `h2`, with the forms of the links of the
two nodes), `proxy_banner_skips_an_xhttp_form_that_it_does_not_speak`. Each
stand-in checks the host, the path and its query, the padding, the session
id, and the sequence numbers from 0 in order; then it sends the VLESS
answer and a banner. `cargo test -p pod-services-watch --all-features`: 34
passed. `cargo test -p pod-services --all-features`: passed.

Plant: `scripts/plants/services-xhttp-no-sequence.plant` (each upload with
no sequence number). Applied by hand: both tests of `packet-up` failed
(`the XHTTP upload answered HTTP 400`); then reversed. The patch of
`scripts/plants/services-trojan-no-password.patch` moves to the new line of
the Trojan request; applied by hand, its test failed.
`sh scripts/plant-each.sh services-xhttp-no-sequence services-trojan-no-password`:
2 caught, 0 missed or stale.

Not run: a live node. The sandbox reaches no proxy node. The next run of
`services.yml` checks the two nodes.

# T-153: `pod-services`: the services `serveo` and `ssh-j` (PodUtils/podssh#182)

**Source:** PodUtils/podssh#182 (COORD-015 of podssh; PodUtils/podssh@T-549, PodUtils/podssh@T-570, the ruling Q78 of podssh)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podssh's Serveo road and ssh-j road are last fallbacks. podssh names no
host of another party in its code, so each road reads its server from
`pod-services`, which had neither service.

## Premise

- Read: https://serveo.net/docs/ (2026-11-10): port 443 and port 22, the
  jump by alias, and the Ed25519 host-key fingerprint
  `SHA256:GnmVK+70U6GqbupoV+gg7LnHHUsW1IjrK0cLqvDJxIk` (the RSA fingerprint
  on the same page is an example of a user key). https://ssh-j.com/
  answers 200; its host key is not read yet.

## Approach

`services/serveo.toml` (endpoints `ssh-443`, `ssh`; the fact
`host-key-sha256`; the watch `ssh-host-key`) and `services/ssh-j.toml`
(endpoint `ssh`; the watch `ssh-banner`); a row of each in the README.

## Prove

```sh
cargo test -p pod-services
```

## Done

2026-11-10. Commit: the commit that names T-153. `cargo test -p
pod-services` passes. The host key of ssh-j comes as a fact when a live
run of podssh reads it.

# T-156: `pod-services`: the service `croc` (PodUtils/podssh#183)

**Source:** PodUtils/podssh#183 (COORD-016 of podssh; PodUtils/podssh@T-694, the ruling Q78 of podssh)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podssh's croc road meets a pair at croc's public relay through croc-web's
WebSocket bridge, and reads the bridge from `pod-services` alone.

## Premise

- Read: croc v11.6.4: `src/webrelay/webrelay.go` (`/ws?relay=INDEX&port=PORT`),
  `src/publicrelay/publicrelay.go` (`1.getcroc.com:9009` to
  `4.getcroc.com:9009`), `src/models/constants.go` (`DEFAULT_PASSPHRASE`
  is `pass123`, croc's public default).
- `Endpoint::url()` keeps the path: `wss://getcroc.com/ws`.

## Approach

`services/croc.toml`: the endpoint `bridge`, the facts `relays`,
`relay-port` and `password`, each with its source line; the watch
`websocket-upgrade`; a row in the README.

## Prove

```sh
cargo test -p pod-services
```

## Done

2026-11-10. Commit: the commit that names T-156. `cargo test -p
pod-services` passes.

# T-157: `pod-services`: four public Trojan nodes fail the live check

**Source:** T-050 (`services.yml`, run 42803032733); T-133.
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The live check of the proxy nodes (the watch `proxy-banner`, from GitHub
Actions on 2026-11-10) fails for each Trojan node of T-133:
`node-bolab-trs02` and `node-bonds-sg1` (the TCP connect to port 443
times out after 29 s), `node-udpgw-trojan1` (TLS: `received fatal alert:
UnrecognisedName`), `node-tr202607-nl1` (the answer to the Trojan request
is `HTTP/1.1 400 Bad Request`: the node no longer takes the password of
the link). podssh tries these nodes as its last fallback; a node that does
not carry a session costs each user a timeout.

## Premise

- Measured: run 42803032733 of `services.yml` (one try of each node).
- Not measured: a second day; the lists of 2026-11-11 and later.

## Approach

Run the watch again by hand (`services.yml`, input `kind` `proxy-banner`)
on a second day. Remove the file of each node that fails both days, or
replace it with a Trojan node of the same lists that passes the watch and
the rules of T-133. Write the change to podssh (PodUtils/podssh#177), which
reads the nodes.

## Prove

```sh
cargo test -p pod-services --test public_carriers
cargo run -q -p pod-services-watch -- --kind proxy-banner
```

## Done

2026-11-11. The second day: `services.yml` run 42933757949 (input `kind`
`proxy-banner`) failed each of the four nodes with the same reasons as
run 42803032733 (two connect timeouts after 29 s, `UnrecognisedName`,
`HTTP/1.1 400 Bad Request`). The files `node-bolab-trs02.toml`,
`node-bonds-sg1.toml`, `node-udpgw-trojan1.toml` and
`node-tr202607-nl1.toml` are removed; the README counts 2 nodes; the
refusal test takes a name of `example.net` in place of a removed host.
`cargo test -p pod-services --all-features` passes. The short list now
holds no Trojan node: T-174 looks for a replacement. podssh (PodUtils/podssh@T-574) hears
of it in its brief of 2026-11-11.

# T-158: `pod-services`: the watch of `croc` upgrades with no relay, and the bridge refuses it

**Source:** T-050 (`services.yml`, run 42806121501); T-156.
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The watch `websocket-upgrade` of `croc` upgrades `wss://getcroc.com/ws`
with no query. The bridge answers `HTTP 403: relay index is not allowed`:
it takes `/ws?relay=INDEX&port=PORT` (the note of the endpoint). The daily
run of `services.yml` fails on each day, and no watch proves that the
bridge carries a session to the pool.

## Premise

- Measured: run 42806121501 of `services.yml` (one upgrade, 403).
- Read: croc v11.6.4 `src/webrelay/webrelay.go`: `strconv.Atoi` of the
  query `relay`, `0 <= index < len(relayHosts)`: the index is 0-based.

## Approach

Read the handler of croc-web at v11.6.4. Give the watch the query of one
relay of the pool: a new optional argument `query` of
`websocket-upgrade` (`relay=INDEX&port=9009`, from the facts), with its
test and its plant in `pod-services-watch`, and its row in the README of
`pod-services`; or a watch kind that sends the handshake of croc's relay
through the bridge. Run the watch by hand (`services.yml`, input
`service` `croc`).

## Prove

```sh
cargo test -p pod-services-watch --test watches -- ws::
cargo run -q -p pod-services-watch -- --service croc
```

## Done

2026-11-10. Commit: the commit that names T-158. The simplest of the two
approaches, with no new argument: an endpoint `bridge-relay-0` of `croc`
with the path `/ws?relay=0&port=9009`, which the watch upgrades (the
endpoint `bridge` stays the address of podssh's client). `cargo test -p
pod-services -p pod-services-watch` passes; `cargo run -p
pod-services-watch -- --service croc` here: "holds croc websocket-upgrade:
wss://getcroc.com:443/ws?relay=0&port=9009 upgraded".

# T-164: `pod-services`: the Actions control-plane hosts and the pin of `actions/runner`, with watches (PodUtils/podvm#227)

**Source:** PodUtils/podvm#227 section 2.5 (COORD-021 of podvm, 2026-11-10)
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podrunner needs the hosts that the official runner reaches (the default `allow` policy and `hello.control_hosts`), and the age of the `actions/runner` pin: the runner must be updated within 30 days of a release (SPEC sections 6.4 and 11.3 of the design review). GitHub changed the broker flow in 2025 (plan risk 5). The ruling "Services crate" makes third-party hosts data with a watch.

## Premise

- Read: `crates/pod-services/services/` holds `ghcr-registry.toml` and `github-ssh.toml` as the only GitHub files. It has no Actions host. This agrees with the issue.
- Read: `crates/pod-services-watch/src/lib.rs` has the kinds `github-release`, `github-file`, `github-files`, `http-json` and others (T-050, call C42). A first form needs no new watch code (inferred).
- Not read: the list of the "Self-hosted runners reference" of GitHub, and the `domains.actions` list of GitHub's meta API (the issue says that it did not read it). Not run: any watch of an Actions host.

## Approach

Add a data file of `pod-services` with the hosts that the official runner reaches. Each host has its source and the date read. A multi-tenant host (for example the blob storage domain) carries a flag. Add a daily watch for drift: candidate source is the `domains.actions` list of GitHub's meta API through an `http-json` watch, else a `github-file` watch. Add a second file or entry for the release pin of `actions/runner`, with the `github-release` watch.

## Prove

```sh
cargo test -p pod-services --all-features
cargo test -p pod-services-watch --all-features
sh scripts/agent/check.sh
```

The tests that the issue names:

1. The file lists the hosts with their source and the date read, and the multi-tenant hosts carry a flag.
2. The daily watch fails on a host that the source adds or removes.
3. The `github-release` watch of `actions/runner` reports the age of the pin.

## Start condition

None. The owner named podrunner on 2026-11-11 (the ruling "podrunner online"; call C49: DR-1 and DR-2 are yes). Take the entry when podrunner's work needs it (`TODO/PROGRESS.md`, Work order).

Depends on T-050 (done).

## Done

2026-11-11. Read: GitHub's meta API (`/meta`, through the mirror
forge.web.pkgforge.dev), `domains.actions_inbound`: 218 full domains and 6
wildcards; `actions/runner` `v2.338.2` (published 2026-10-26).

- `services/github-actions.toml` (kind `ci-service`): 7 endpoints (role
  `control`: `github.com`, `api.github.com`, the broker, the runner auth,
  the results receiver; role `download`: `codeload.github.com`,
  `release-assets.githubusercontent.com`); the facts `full-domains` and
  `wildcard-domains` (each with an `http-json` watch of the meta API) and
  `multi-tenant` (`*.core.windows.net`, `*.githubusercontent.com`: the
  storage of each customer of Azure, the raw files and assets of each user
  of GitHub). The source of each value is the meta API, read 2026-11-11.
- `services/github-actions-runner.toml` (kind `runner-release`): the fact
  `runner-tag` and a `github-release` watch with `max-age-days = 30`: a pin
  of that age can be past GitHub's rule of 30 days after a new release, so
  the watch warns early, never late.
- Tests: `tests/actions.rs` of `pod-services` (3: the source and the date
  of each list, the endpoints in the list, the flags among the wildcards;
  the two list watches; the watch of the pin); the watch test
  `http_json_fails_on_a_host_that_a_list_adds_or_removes` (a host added,
  a host removed). `cargo test -p pod-services --all-features` and
  `cargo test -p pod-services-watch --all-features` pass.

# T-165: `pod-tls`: mutual TLS with pinned keys (PodUtils/podvm#227)

**Source:** PodUtils/podvm#227 section 2.6 (COORD-021 of podvm, 2026-11-10)
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

The remote mint and the unit host of podrunner use mutual TLS with pinned keys on both sides (SPEC section 5.3 and `contracts/source-driver.md` of the design review, transport 3). Each config of `pod-tls` calls `with_no_client_auth()`. This is needed only if the owner answers yes to DR-4 (trusted jobs at the `redirect` boundary on hosts with no root), which builds WP-05b of podrunner.

## Premise

- Read: `with_no_client_auth` is at `crates/pod-tls/src/context.rs:226`, `crates/pod-tls/src/server.rs:70`, `crates/pod-tls/src/kept.rs:104`, `crates/pod-tls/src/ech/config.rs:89` and `crates/pod-tls/src/fragment.rs:339`. This agrees with the issue.
- Not measured: a handshake with a client certificate. Not read: the contract of the design review.

## Approach

Add a client config that presents a certificate and key. Add a server config that requires a client certificate and accepts only pinned public keys. Both use the caller's provider (rule 4 of `docs/design.md`).

## Prove

```sh
cargo test -p pod-tls --all-features
sh scripts/agent/check.sh
```

The tests that the issue names:

1. A loopback test: a client with a pinned key connects. A client with another key, or with no certificate, is refused at the handshake.
2. The server reports the key that the peer proved, for the audit line.
3. A key rotation: two pinned keys at once, then one.
4. The static probe links the feature on each target.

## Start condition

The owner answers yes to DR-4 (PodUtils/podrunner#4): the remote mint of podrunner (PodUtils/podrunner#31) is its only consumer. DR-1 and DR-2 are yes (the ruling "podrunner online", 2026-11-11; call C49).

## Done

2026-11-11 (PodUtils/podrunner#31 needs it; DR-4 is yes). `pod_tls::mutual`:
`server_config` (a client verifier that takes only a pinned key, and a
client is mandatory), `client_config` (a server verifier that takes only a
pinned key, and the client's key presented at each request), `Pin`
(`sha256/BASE64` of the SubjectPublicKeyInfo; `validity::spki` reads it
with the DER reader of the crate), `Pins` (a shared set, changed while a
server runs), `peer_pin`; `server::certified_key(chain, key)`. Both take
the caller's provider; TLS 1.3 only. `sha2` is a dependency of each build
now (pure Rust).

- Defect found by the test of the rotation and fixed: with TLS 1.3
  resumption, a client whose key the rotation removed connected again on
  a resumed session (the server kept the client of the first handshake).
  Both configurations turn resumption off, so each connection proves its
  key against the pins of now.
- Tests (`tests/mutual.rs`, 5, each on the pure provider and on ring where
  it applies): a pinned client connects and the server reports its pin; a
  client of another key and a client with no certificate are refused; a
  client refuses a server of another key; a rotation (two pins, then one);
  the text form of a pin. The static probe links a server of mutual TLS.
  The plant `tls-mutual-pin-not-checked` fails 3 of the 5.

# T-171: `pod-services`: the services `localhost-run`, `localtunnel` and `bore` (PodUtils/podssh#186)

**Source:** PodUtils/podssh#186 (COORD-019 of podssh; PodUtils/podssh@T-565, PodUtils/podssh@T-568, PodUtils/podssh@T-569, the ruling Q78 of podssh)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Three more last fallbacks of podssh read their server from `pod-services`
alone, and are off until a tag names them: localhost.run, localtunnel and
bore.pub.

## Premise

- Read: PodUtils/podssh#186 (the facts, read by podssh on 2026-11-10 through
  web.pkgforge.dev). Not read here: each source. Read each again before the
  data, as T-153 and T-156 did.
- `localhost-run`: kind `ssh-tunnel`; endpoint `ssh` = `localhost.run:22`,
  user `nokey`; no host key is published (podssh's live run reads it).
- `localtunnel`: kind `localtunnel` (a new kind: check the schema and the
  watch kinds of `pod-services-watch`); endpoint `api` =
  `https://localtunnel.me:443`; fact `tunnel-zone` = `loca.lt`; watch
  `rdap` of `localtunnel.me` (`/` redirects, `/?new` makes a tunnel).
- `bore`: kind `tcp-tunnel` (new; check the schema); endpoint `control` =
  `tcp://bore.pub:7835` (`CONTROL_PORT` of ekzhang/bore `src/shared.rs` at
  `00a735a89917`); fact `tunnel-hours` = 24 (ekzhang/bore issue 150).

## Approach

One file under `crates/pod-services/services/` for each, each fact with its
source, each with a watch that `pod-services-watch` can run (or a new watch
kind with its test and plant); a row of each in the README. Run the watches
by hand once (`cargo run -p pod-services-watch -- --service ID`).

## Prove

```sh
cargo test -p pod-services -p pod-services-watch
```

## Done

2026-11-11. Three files under `crates/pod-services/services/`, each fact
read again at its source on 2026-11-11:

- `localhost-run` (`ssh-tunnel`): endpoint `ssh` `localhost.run:22` (the
  CLI reference), fact `user` `nokey` (the FAQ: "For free tunnels, skip the
  key check with the nokey user"). No host key is published. Watches:
  `ssh-banner` of `ssh`, `rdap` of `localhost.run`.
- `localtunnel` (`localtunnel`): endpoint `api` `https://localtunnel.me:443`
  (`lib/Tunnel.js` of localtunnel/localtunnel at `decafd5c0fce`), fact
  `tunnel-zone` `loca.lt` (measured: a name under `loca.lt` resolves to the
  address of `localtunnel.me`). Watches: `github-file` of `lib/Tunnel.js`
  (the host of `api`), `rdap` of `localtunnel.me`. No watch asks for a
  tunnel (`/?new` makes one).
- `bore` (`tcp-tunnel`): endpoint `control` `tcp://bore.pub:7835`, facts
  `control-port` 7835 (`src/shared.rs` of ekzhang/bore at `00a735a89917`)
  and `tunnel-hours` 24 (ekzhang/bore issue 150; the issue is open, and
  `bore.pub` resolves on 2026-11-11). Watches: `github-file` of
  `src/shared.rs` (the fact) and of `README.md` (the host), `rdap` of
  `bore.pub`.

No new watch kind: a service kind is free text, and each watch is of a
kind that `pod-services-watch` runs. The README has a row of each. Test:
`the_tunnels_of_localhost_run_localtunnel_and_bore` (`tests/api.rs`).
podssh took the three in `v0.18.0` (`50ef8922`), and its live run
42967245046 (`last-roads-live.yml`) passed for each road, with each planted
stranger refused (PodUtils/podssh@T-565, PodUtils/podssh@T-568,
PodUtils/podssh@T-569).

# T-172: `pod-session`: `Rewound::new`, a public constructor (PodUtils/podssh#186)

**Source:** PodUtils/podssh#186 (COORD-019 of podssh; PodUtils/podssh@T-568).
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podssh's localtunnel router reads the head of a request to choose who
serves it, then hands the link on with that head read again first. That is
`Rewound` of `pod-session` (from podssh's `session/client.rs`), which has no
public constructor. podssh keeps a private copy (`Replay`) until it can
call the crate.

## Premise

- Read: PodUtils/podssh#186. `Rewound` is in `crates/pod-session`.

## Approach

`pub fn new(first: Vec<u8>, link: L) -> Rewound<L>`, with a test that reads
the head again and then the rest of the link; the README's API. podssh
drops `Replay` when it takes the crate.

## Prove

```sh
cargo test -p pod-session
```

## Done

2026-11-11. `client::Rewound::new(first, link)`; `Client::rewound` uses it.
The README names it. Tests (`tests/rewound.rs`, 3):
`a_rewound_link_reads_the_head_again_then_the_rest`,
`a_small_buffer_reads_the_head_in_parts_and_a_write_reaches_the_link`,
`an_empty_head_is_the_link_alone`. podssh drops `Replay` when it takes the
tag.

# T-174: `pod-services`: a public Trojan node of the lists that passes the live check

**Source:** T-157 (the four Trojan nodes of the short list are removed).
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The short list of `proxy-node` services (podssh's last fallback of its
forward road, PodUtils/podssh#177, Q90) holds two VLESS nodes over XHTTP
and no Trojan node since T-157. A host whose path refuses XHTTP has no
node to try.

## Premise

- Read: the run 42933757949 of `services.yml`; the two files that stay.
- Not read: the lists of 2026-11-11 and later (`mahdibland-list`,
  `barry-far-list`, `v2go-list`).

## Approach

Read the three lists, pick Trojan nodes on 443 with TLS that pass the
rules of T-133, add each as `services/node-*.toml` with its `source`, run
the watch `proxy-banner` on them in `services.yml` on two days, and keep
the ones that pass both days. Tell podssh (PodUtils/podssh@T-574) the new ids.

## Prove

```sh
cargo test -p pod-services --test public_carriers
cargo run -q -p pod-services-watch -- --kind proxy-banner
```

## Done

2026-11-12. Seven Trojan nodes held the live check on both days; one
left.

- Read: the three lists on 2026-11-11 (`mahdibland-list` 759 Trojan
  links, `barry-far-list` 716, `v2go-list` 108). 541 links pass the static
  rules of T-133 (port 443, TLS, type `tcp` or `ws`, no flow, no insecure
  flag, one name or a public address, no refused suffix); 91 host:port are
  in two or more lists. The watch speaks Trojan over TCP only (not `ws`),
  and it checks the certificate, so a node with an address and no name of
  its own fails. Kept: 8 Trojan `tcp` nodes that are in all three lists,
  each an address with its own name in `sni` (no name of another party):
  `services/node-trojan-*.toml`, rank 7, the endpoint `sni` set, the
  README row (10 nodes).
- Defect found and fixed: `proxy-banner` took a link with no `security`
  as plain TCP. Trojan has no plain form, and share links often leave the
  parameter out: a Trojan link now defaults to TLS. Test
  `proxy_banner_speaks_tls_to_a_trojan_link_with_no_security`; plant
  `services-trojan-plain-by-default`.
- `cargo test -p pod-services --all-features` (the rules of
  `tests/public_carriers.rs` hold for the 8 nodes) and
  `cargo test -p pod-services-watch --test watches proxy_banner` pass.
- Day 1 (2026-11-11): `services.yml` 42999819735 to 42999852055, one
  run for each node: each of the 8 holds (for example
  `trojan 70.34.207.14:443 to github.com:22 gave "SSH-2.0-a07fe5ff"`).
- Day 2 (2026-11-12): `services.yml` 43145931146 to 43145957804, one run
  for each node (the scheduled run of the day had not started): seven
  hold; `node-trojan-70-34-207-14` timed out on the TCP connect (29 s) and
  left. The seven that stay: `node-trojan-149-28-130-108`,
  `node-trojan-167-88-61-175`, `node-trojan-169-197-140-218`,
  `node-trojan-216-238-104-154`, `node-trojan-45-63-30-142`,
  `node-trojan-64-176-7-205`, `node-trojan-66-163-112-121`. The README
  row counts 9 nodes. podssh (PodUtils/podssh@T-574) gets the ids with the
  next hand-off.


# T-181: `pod-services`: the host key of `ssh-j` as a fact (podssh's message, 2026-11-11)

**Source:** podssh's message of 2026-11-11 to podcrates (PodUtils/podssh@T-570, the run 42954754303 of `ssh-j-live.yml`)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The service `ssh-j` has no host key. podssh takes the key of `ssh-j.com`
unpinned, so a road through it trusts the first key that it sees.

## Premise

- Read: podssh's message. Its live run on a GitHub runner read the key
  `SHA256:EQqn3kjIF8XkXoN40tNJmRRE/eiz+AQmK2EWuEzOz6Y` on 2026-11-11, and
  the job `live` of the run passed.
- Read: `https://ssh-j.com/` publishes no fingerprint. The egress of this
  sandbox has no SSH: the key cannot be read here.

## Approach

Add the fact `host-key-sha256` to `services/ssh-j.toml` with the measured
source, and the watch `ssh-host-key` of the endpoint `ssh`, as `serveo`
has. `services.yml` runs the watch on GitHub each day: a wrong or changed
key is a finding there.

## Prove

```sh
cargo test -p pod-services
```

The next run of `services.yml` reads the key and agrees.

## Done

2026-11-11. `services/ssh-j.toml` has the fact `host-key-sha256`
(`SHA256:EQqn3kjIF8XkXoN40tNJmRRE/eiz+AQmK2EWuEzOz6Y`, measured by the run
42954754303 of podssh's `ssh-j-live.yml`) and the watch `ssh-host-key` of
the endpoint `ssh`; the README row names it. `cargo test -p pod-services`
passes. The first run of `services.yml` after the push reads the key
again.

# T-185: `pod-services`: the host key of `localhost-run` as a fact (podssh's message, 2026-11-11)

**Source:** podssh's message of 2026-11-11 to podcrates (PodUtils/podssh@T-565, the run 42967245046 of `last-roads-live.yml`)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The service `localhost-run` has no host key. podssh takes the key of
`localhost.run` unpinned, so a road through it trusts the first key that
it sees.

## Premise

- Read: podssh's message. Its live run on a GitHub runner read the key
  `SHA256:pG6qrBxubYfWa1Zadu/V0NUgjEDiBds/7e2xzte/QNM` on 2026-11-11; the
  greet and 1 MiB down passed, and a planted stranger key was refused.
- Read: localhost.run publishes no fingerprint (T-171). The egress of this
  sandbox has no SSH.

## Approach

Add the fact `host-key-sha256` to `services/localhost-run.toml` with the
measured source, and the watch `ssh-host-key` of the endpoint `ssh`, as
T-181 did for `ssh-j`. `services.yml` reads the key again each day.

## Prove

```sh
cargo test -p pod-services
```

## Done

2026-11-11. `services/localhost-run.toml` has the fact `host-key-sha256`
(`SHA256:pG6qrBxubYfWa1Zadu/V0NUgjEDiBds/7e2xzte/QNM`, measured by the run
42967245046 of podssh's `last-roads-live.yml`) and the watch
`ssh-host-key` of `ssh`; the README row names it; the test
`the_tunnels_of_localhost_run_localtunnel_and_bore` reads it.
`cargo test -p pod-services` passes.

# T-186: `pod-services`: the service `sandhole`, the demo of Sandhole (podssh's message, 2026-11-11)

**Source:** podssh's message of 2026-11-11 to podcrates (PodUtils/podssh@T-743; the runs 42970029119 and 42970697875 of podssh)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podssh adds one more last fallback, the demo server of Sandhole, of the
shape of `localhost-run`. It reads its server from `pod-services` alone,
so it is off until a tag names it.

## Premise

- Read: podssh's message; the README of EpicEric/sandhole at
  `6a57467237f4` (`ssh -R 80:localhost:4321 demo.sandhole.com.br`); the CLI
  of the Sandhole book (`--connect-ssh-on-https-port`: SSH on the HTTPS
  port as well); the DNS of `demo.sandhole.com.br` and of a name under it
  (`178.156.220.190`).
- Measured by podssh only: SSH on port 443, the host key, a request that
  reached the server of the runner. The site publishes no fingerprint, and
  this sandbox has no SSH.

## Approach

`services/sandhole.toml`: endpoints `ssh` (22) and `ssh-443` (443), facts
`host-key-sha256` and `tunnel-zone`, watches `ssh-banner`, `ssh-host-key`
on each port, `github-file` of the README, `rdap` of `sandhole.com.br`; a
row of the README; a test.

## Prove

```sh
cargo test -p pod-services
```

## Done

2026-11-11. `services/sandhole.toml` as the Approach says, with the
sources of the Premise; the README row; the test
`the_demo_of_sandhole_has_two_ssh_ports_and_one_key`.
`cargo test -p pod-services` passes. The first run of `services.yml` reads
the key on both ports again.

# T-187: `pod-services`: the kind `bare-node` and the node `openbare-xyz` (podssh's message, 2026-11-11)

**Source:** podssh's message of 2026-11-11 to podcrates (PodUtils/podssh@T-740)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podssh uses a TompHTTP Bare v3 server as its last WebSocket hop. It reads
the node from `pod-services` alone, so the hop is off until a tag names it.

## Premise

- Read: podssh's message; the README of nirholas/openbare at
  `da38032de3a0` (`servers: ['https://openbare.xyz/bare/']`, "Live
  Example"); tomphttp/specifications `BareServerV3.md` at `e27d6370b816`.
- Measured here through fetch.web.pkgforge.dev on 2026-11-11: `/` answers
  `"service":"openbare-edge"`, `"protocol":"bare-v3"`, `"websocket":true`;
  `/bare/` answers `"versions":["v1","v2","v3"]`.
- The repository's licence is proprietary; podssh speaks the open protocol
  and uses the public node, which the data names only.

## Approach

`services/openbare-xyz.toml`: kind `bare-node` (a kind of a service is
free text), endpoint `bare`, facts `service` and `protocol`, watches
`http-json` of each fact, `http-status` of `/bare/`, `rdap` of
`openbare.xyz`; a row of the README; a test.

## Prove

```sh
cargo test -p pod-services
```

## Done

2026-11-11. `services/openbare-xyz.toml` as the Approach says; the README
row; the test `the_bare_node_of_openbare_is_a_bare_v3_endpoint`.
`cargo test -p pod-services` passes.

# T-189: `pod-proxy`: a TCP congestion controller on request for each dialed socket, read back (PodUtils/podnet#437)

**Source:** PodUtils/podnet#437 (COORD-019 of podnet), the owner's donor
review: satmux 3. The donor: Th0rgal/satmux at
`2922ebb835d0eb939ddbdfa3b5e7e0f247a0dc0c` (MIT), `tune_socket` in
satmux.py.
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

A host sends a large upload (a backup, a log batch, `git push`) on a path
with a long round trip and some loss (a satellite link, a mobile uplink).
The default controller of the host is usually `cubic`, which halves its
window at each loss. An operator knows that `bbr` is available and wants it
for each connection, with no sysctl and no root. `pod-proxy` has no setting
for it. A call written by hand fails with no message on a host that has no
such module. A report must never say `bbr` when the socket runs `cubic`.

The donor sets `TCP_CONGESTION` to `bbr` (with `TCP_NODELAY`,
`SO_KEEPALIVE`, buffers of 2 MiB and `TCP_MAXSEG` 1280) on each carrier
socket, ignores each error, and reads nothing back. On the Linux 6.12 host
of the review, the kernel refused `bbr` (ENOENT), and the socket kept
`cubic` with no message.

## Premise

- Read (this tree): `crates/pod-proxy/src/lowat.rs:1-56` is the model. It
  tries `TCP_NOTSENT_LOWAT`, gives the words of the system on a refusal,
  and its test reads the value back with `get_on`. Two facts differ from
  the review: `lowat` works on Linux and macOS
  (`crates/pod-proxy/src/lowat.rs:12-15`), not on Linux only; `get_on` is
  a function of the tests only (`:46`). The new `get` must be public, for
  a doctor line.
- Read: `crates/pod-proxy/src/dial.rs:67-70` (the field `lowat`),
  `:79-92` (`Settings::new`) and `:264-273` (`Dialer::tuned` calls
  `lowat::set` and writes `TCP_NOTSENT_LOWAT is not set` at debug). They
  agree with the review. `Dialer::tuned` runs on each stream of
  `Dialer::open` (`:250`) and of the dial of addresses (`:348`).
- Read: `crates/pod-proxy/src/stream.rs:20-28`. `tuned` changes only a TCP
  stream: on the direct road, the socket to the target; through an HTTP or
  SOCKS5 proxy, the socket to the proxy; through a proxy on a Unix socket
  or at a command, no socket. `blocking::Dialer` wraps the async `Dialer`
  (`crates/pod-proxy/src/blocking.rs:55-58`), so it gets the setting too.
- Read: `crates/pod-proxy/README.md:187` (the parity row of `TCP_NODELAY`
  and `TCP_NOTSENT_LOWAT`) and `crates/pod-proxy/tests/mptcp.rs:153` (the
  ignored live test of MPTCP). They agree with the review.
- No crate of this tree sets `TCP_CONGESTION`. The controller of the
  userspace stack of `pod-netstack` is a different setting (T-188).
- Not read: the podnet callers that the review names (src/egress.rs,
  src/fetch.rs, src/proxy.rs, src/warp/nc.rs,
  crates/podnet-serve/src/ingress.rs of PodUtils/podnet at `649c8e3`).
- Not measured here: each observation of the review. Open questions of
  the review: an unprivileged process can select only a controller of
  `tcp_allowed_congestion_control` (Inferred); `bbr` gives more throughput
  on a path of PodUtils (Unverified); a buffer size set by hand stops the
  autotuning of the kernel (Inferred).

## Approach

Licence: satmux is MIT. The donor is Python. Write new Rust code from this
description. No donor text goes into the crate, so the crate needs no
notice.

- A new module congestion.rs of `pod-proxy` with four functions:
  `set(tcp, name)` gives `Result<(), String>`; `get(tcp)` gives
  `Option<String>`; `available()` and `allowed()` give the names
  (`Vec<String>`). The last two read
  `/proc/sys/net/ipv4/tcp_available_congestion_control` and
  `/proc/sys/net/ipv4/tcp_allowed_congestion_control`. Linux only:
  elsewhere `set` gives `Err("this system has no such option")`, as
  `lowat` does, and `get` gives `None`.
- `Settings::congestion: Option<String>`. `Settings::new` gives `None`:
  the default of the kernel stays, and no consumer changes.
- `Dialer::tuned` calls `set` after `lowat`, then `get`. When `set` fails,
  or the name read back is not the name of the request, it writes one
  debug line with both names and the words of the system. A refusal never
  fails the dial.
- The doc comment gives the limits: the setting changes what this host
  sends, not a download; the kernel refuses a name that it does not know;
  the controller of `pod-netstack` and of the QUIC roads are other
  settings; no `TCP_MAXSEG` and no buffer size.
- The parity row of the README adds `TCP_CONGESTION` on request, read
  back.
- Then measure: an upload of 64 MiB through a loopback shaper in userspace
  (1 percent loss, 300 ms delay), with `cubic` and with `bbr`, three runs
  each. Write the host, the kernel and the result in the Done section. The
  setting stays off by default whatever the result.
- The consumers: podnet (a flag or a row of `podnet doctor`) and podssh
  (`podssh doctor`) take it after the release. No consumer changes here.

## Prove

```sh
cargo test -p pod-proxy --all-features
cargo test -p pod-proxy --all-features -- --ignored congestion
```

The tests:

- On a Linux TCP socket, `set` of the default controller gives `Ok`, and
  `get` gives the same name.
- `set("no-such-controller")` gives an `Err` with the words of the system,
  and `get` still gives the default (the negative case).
- A live test, ignored: it selects `bbr` where `available()` lists it and
  asserts the read back. Where `bbr` is not listed, it prints "did not
  run" with the reason, as the live test of MPTCP does, and asserts
  nothing.
- A dial with `Settings::congestion = Some("bbr")` on a host with no `bbr`
  connects and writes the debug line.
- `Settings::new` gives `congestion: None`.
- Where the host has a controller that is available but not allowed, an
  unprivileged test asks for it. Its result answers the Inferred question
  of the review.

# T-198: `pod-services`: the WebSocket of `openbare-xyz` fails (podssh's messages, 2026-11-11)

**Source:** podssh's messages of 2026-11-11 to podcrates (its take of `v0.21.0`, PodUtils/podssh#189; its live run 42994885604)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`openbare-xyz` names the kind `bare-node`, the protocol `bare-v3` and an
edge that says `features.websocket: true`. podssh's live road found that
no WebSocket passes the edge: the connect form of BareServerV3.md got 400,
and with the header `X-Bare-URL` each upgrade ends in 500. A consumer that
reads the service tries a hop that cannot carry a byte.

## Premise

- Read: podssh's two messages; its live run 42994885604 ("WebSocket
  Constructor: The protocol header token is invalid"); nirholas/openbare
  `edge/src/websocket.js` at `65a4d8d`, as podssh names it (the edge calls
  `new WebSocket(url, {headers})` on Workers, where the second argument
  is the protocols). The licence of openbare is proprietary: read, copy
  nothing.
- The watch `websocket-upgrade` asserts 101 only; no watch kind asserts an
  upgrade that fails.

## Approach

A fact `websocket-upgrade` (`500`) of `openbare-xyz` with podssh's run as
its source; a test; the row of the README. A public Bare node whose
WebSocket works is T-200.

## Prove

```sh
cargo test -p pod-services
```

## Done

2026-11-11. `services/openbare-xyz.toml` has the fact `websocket-upgrade`
(`500`, the source podssh's run 42994885604 and the line of openbare's
edge), with a comment that no WebSocket passes the edge; the README row
names it; `the_bare_node_of_openbare_is_a_bare_v3_endpoint` reads it.
`cargo test -p pod-services --all-features` passes. The fact
`connect-form` of the first approach is not written: no form passes. podssh speaks both forms at `6de121b2` (`bare.rs`, the header
form; rustfmt at `d1ec1e0b`).

# T-200: `pod-services`: a public Bare node whose WebSocket passes (PodUtils/podssh@T-740)

**Source:** T-198; podssh's message of 2026-11-11 (PodUtils/podssh@T-740 is blocked until a `bare-node` passes its live job)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

podssh's last WebSocket hop needs a public TompHTTP Bare server whose
upgrade gives 101 and carries bytes. The one node of the data,
`openbare-xyz`, gives 500 on each upgrade (T-198).

## Premise

- Read: `services/openbare-xyz.toml`; T-187 and T-198.
- Not read: the lists of public Bare servers (tomphttp's own list, the
  lists of the proxy front ends that use Bare), and their licences.

## Approach

Read the public lists, pick nodes that need no account and pass the rules
of T-133 (443 with TLS, no refused suffix, no name of another party),
add each as a `bare-node` with a watch `websocket-upgrade` and a watch
that carries GitHub's SSH banner through it, run `services.yml` on two
days, keep the ones that pass both days, and tell podssh the ids. If none
passes, write that on PodUtils/podssh@T-740's ask and keep the entry
open.

## Prove

```sh
cargo test -p pod-services
```

# T-203: `pod-services`: the TCP relay of ZeroTier (PodUtils/podssh#190)

**Source:** PodUtils/podssh#190 (COORD-023 of podssh, for PodUtils/podssh@T-733)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podssh's ZeroTier road links the core of ZeroTierOne; the roots come from
its planet, and the one host that podssh needs is the TCP fallback relay.
podssh must not write the host in its code, so the live run of its road
waits for a service of `pod-services`.

## Premise

- Read: PodUtils/podssh#190; ZeroTierOne `service/OneService.cpp` at
  `899352e38405968516bb12a770f0ac02f6058fa8` (line 164:
  `#define ZT_TCP_FALLBACK_RELAY "204.80.128.1/443"`) and its
  `tcp-proxy/README.md` ("an anycast address"); the default branch holds
  the same line on 2026-11-11.
- No watch kind does a TCP connect alone.

## Approach

`services/zerotier-tcp-relay.toml`: kind `tcp-relay`, endpoint `tcp`
(`204.80.128.1:443`), fact `relay`, two `github-file` watches (the
define and the README); a row of the README; a test. A watch of a TCP
connect is a later kind if a consumer asks for one.

## Prove

```sh
cargo test -p pod-services
```

## Done

2026-11-11. `services/zerotier-tcp-relay.toml`: kind `tcp-relay`, endpoint
`tcp` (`204.80.128.1:443`), fact `relay` (`204.80.128.1/443`, the form of
`ZT_TCP_FALLBACK_RELAY`), the source ZeroTierOne `service/OneService.cpp` at
`899352e3`, and two `github-file` watches (the define and
`tcp-proxy/README.md` on the default branch). The README row names it; the
test `the_tcp_relay_of_zerotier_is_its_fallback_address` reads the
endpoint and the fact. `cargo test -p pod-services --all-features` passes.
podssh needs no watch of a TCP connect (its amended PodUtils/podssh#190).


# T-211: `pod-services`: gcr.io, mirror.gcr.io and the NIC modules of podvm's guest (PodUtils/podvm#263)

**Source:** PodUtils/podvm#263 (COORD-044 of podvm, for PodUtils/podvm@T-1679)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm reads its hosts and pins from `pod-services` and keeps no copy. A
test of podvm compares the hosts of its sources with the hosts of the
crate. Three gaps stay in a list of podvm that the test reports: the
registry `gcr.io` (the image of the profile `distroless-static`), the
mirror `mirror.gcr.io` (the measure workload pulls there where
public.ecr.aws answers 429), and the SHA-256 of the NIC modules that the
machine tier loads out of `boot/initramfs-virt` (`failover`,
`net_failover`, `virtio_net`).

## Premise

- Read: PodUtils/podvm#263; podvm at `1cb2fb1`:
  `PodUtils/podvm:crates/podvm/src/machine/guest/kernel.rs` (`MODULES`),
  `PodUtils/podvm:crates/podvm/src/profile/builtin.rs` (`gcr.io/distroless/static-debian12`,
  no digest), `PodUtils/podvm:crates/podvm-measure/src/workload.rs` (`ALPINE_MIRROR_REF`).
- Measured on 2026-11-11: the archive `alpine-netboot-3.22.6-x86_64.tar.gz`
  has the SHA-256 of the fact `archive-sha256`, and its `boot/vmlinuz-virt`
  the one of `vmlinuz-virt-sha256`. Its `boot/initramfs-virt` (a gzip newc
  cpio) holds `lib/modules/6.12.110-0-virt/kernel/net/core/failover.ko`,
  `kernel/drivers/net/net_failover.ko` and
  `kernel/drivers/net/virtio_net.ko`, plain `.ko` files whose SHA-256 are
  podvm's values. `GET https://gcr.io/v2/` and `https://mirror.gcr.io/v2/`
  answer 401 with a Bearer challenge (mirror.gcr.io names no service).
  The tag `latest` of `distroless/static-debian12` is the index
  `sha256:d75cdd72…`. mirror.gcr.io serves `library/alpine` at the digest
  of `guest-userland` of `ecr-public-registry`.

## Approach

A service `gcr-registry` (kind `oci-registry`, endpoint `registry`
`gcr.io`, facts `token-realm` and `images`). A service
`mirror-gcr-registry` (`mirror.gcr.io`, facts `mirror-of`, `token-realm`,
`images`), not an endpoint of `docker-hub-registry`: Google runs it, and
its challenge and its quota are not those of Docker Hub (call C59). The
facts `kernel-release`, `module-failover-sha256`,
`module-net-failover-sha256` and `module-virtio-net-sha256` of
`alpine-netboot`. A test of each value of podvm.

## Prove

```sh
cargo test -p pod-services --all-features
cargo run -q -p pod-services-watch -- --service gcr-registry --service mirror-gcr-registry
```

## Done

2026-11-11. The two services and the four facts; the README rows.
`the_gaps_of_podvm_s_inventory_come_from_the_files` checks each value of
podvm, and `each_registry_answers_at_v2_over_https` lists seven
registries. `cargo test -p pod-services --all-features`: each suite
passes. The live watches: 4 hold (the two `oci-ping`, the two
`oci-digest`).

# T-224: `pod-services`: the facts of an SSH forward relay, and a reader of its banner across chunks (PodUtils/podnet#455)

**Source:** PodUtils/podnet#455 (COORD-037 of podnet; PodUtils/podnet@T-165, PodUtils/podnet@T-166)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podnet has the command `podnet ssh-relay --server HOST:PORT --user USER
--url-match TEXT [--url-skip TEXT]` (PodUtils/podnet@T-165). The flags
are required until `pod-services` holds the facts of each relay. Then
they become overrides. The ask, for each SSH forward relay, as data (no
host in podnet's code):

- the SSH endpoints (`HOST:PORT`, a list that podnet tries in order);
- the SSH user that the relay expects (a fixed word or a token form);
- the rule of the URL in the banner: the texts that the URL holds
  (`match`) and the texts that it does not hold (`skip`);
- the transport: plain SSH on TCP, and whether the relay answers on 443;
- the limits: the session time limit, and the keepalive that the relay
  needs.

Also: a reader of the banner that holds a line across two chunks. podnet
reads the banner chunk by chunk, so it loses a URL that two chunks cut.

## Premise

- Read: PodUtils/podnet#455; podnet `main` at `9df5438`:
  `PodUtils/podnet:crates/podnet-serve/src/relay.rs` (`UrlRule`: a token
  that starts with `http://` or `https://`, with no text of `skip` and
  each text of `hold`; `RelayConfig::default()`: the user `podnet` and the
  endpoint `ssh` of `pinggy`), `PodUtils/podnet:src/cli/cmd/ssh_relay.rs`
  (`--url-match` fills `hold`, "the URL holds each text"; `--url-skip`
  fills `skip`, "the URL holds none of the texts"; a keepalive of 30 s),
  and the banner of `free.pinggy.io` that podnet read on 2026-10-09 (the
  test `picks_the_tunnel_url_and_not_the_dashboard` of
  `PodUtils/podnet:crates/podnet-serve/src/relay/tests.rs`).
- Read on 2026-11-11, through the proxy and read.web.pkgforge.dev: the
  pages of Pinggy (`pinggy.io/docs/`, `/docs/usages/`,
  `/docs/guides/long_running_tunnels/`, `pinggy.io/` and its script), of
  Serveo (`serveo.net/docs/`, its article on SSH servers, `serveo.net/`),
  of localhost.run (`localhost.run/`, `/docs/`, `/docs/faq`,
  `/docs/forever-free`) and of Sandhole (`sandhole.com.br/cli.html`, and
  `src/ssh/forwarding.rs` of EpicEric/sandhole at `6a57467237f4`).
- Read: podssh's facts of the same relays
  (`PodUtils/podssh:docs/carriers.md`; the line of Sandhole that its runs
  42970029119 and 42970697875 measured; its words and its reader `Lines`
  in `PodUtils/podssh:crates/podssh-relay/src/ssh_http/`).
- The fact `dashboard-host` of `pinggy` has two readers: podnet
  (`UrlRule::default()`) and podssh (`pinggy::dashboard()`,
  `PodUtils/podssh:crates/podssh-relay/src/pinggy/mod.rs`).
- Not measured: the banner of a relay. This sandbox has no SSH, and the
  watch `ssh-banner` reads the version line, not the text of a session.

## Approach

1. The facts go into the files of the kind `ssh-tunnel`, each with its
   source and its date, and only where a source states them. Keep the
   names that exist: `user` (of `localhost-run`) and
   `free-tunnel-minutes` (of `pinggy`). New names: `url-match` and
   `url-skip` (lists of texts), and `keepalive-seconds` (unit `s`). Move
   `dashboard-host` of `pinggy` into `url-skip`, so that one name holds
   the skip texts of each relay.
2. The order of the endpoints: the endpoints of the scheme `ssh` (plain
   SSH on TCP), in the order of the file, an endpoint on 443 first. Put
   `ssh-443` of `sandhole` before `ssh`.
3. In `pod-services` (code over its facts, with no relay in it): a service
   read as a forward relay (`forward::Relay`), podnet's rule of the URL
   (`forward::UrlRule`: each text of `url-match`, no text of `url-skip`),
   and the reader of a banner in chunks (`forward::Banner`: a URL is one
   word, so the reader holds the word that a chunk cut, with a bound).
4. Tests of the data and of the reader, the README, plants for the
   reader, the adoption map for podnet and podssh.

## Prove

```sh
cargo test -p pod-services --all-features
cargo clippy -p pod-services -p pod-services-watch --all-targets --all-features -- -D warnings
sh scripts/plant-each.sh services-forward-word-not-held services-forward-held-unbounded
```

## Done

2026-11-11, commit `719961a` (the work), and the commit of this section
(the plants). Each value has its source and its date in its file
(`crates/pod-services/README.md`, "SSH forward relays"; call C66):

- `pinggy`: plain SSH `ssh` (443); `user` `http` (`/docs/usages/`: the
  user carries the keywords, and `http` is the type of the tunnel);
  `url-match` `https://`, `.run.pinggy-free.link` (`/docs/`: the two
  sample URLs); `url-skip` `dashboard.pinggy.io` (the banner that podnet
  read on 2026-10-09; it was `dashboard-host`); `keepalive-seconds` 30
  (`pinggy.io/`: the setting Keep Alive adds `-o ServerAliveInterval=30`);
  `free-tunnel-minutes` 60 (kept).
- `serveo`: plain SSH `ssh-443`, `ssh`; `url-match` `https://`,
  `.serveo.net` (`/docs/`: "A subdomain of serveo.net will be assigned");
  `keepalive-seconds` 30 (its article on SSH servers).
- `localhost-run`: plain SSH `ssh` (22); `user` `nokey` (kept);
  `url-match` `https://`, `.lhr.life` (the sample of the landing page);
  `keepalive-seconds` 60 (`/docs/faq`).
- `sandhole`: plain SSH `ssh-443`, then `ssh`; `url-match` `https://`,
  `.demo.sandhole.com.br` (podssh's runs 42970029119 and 42970697875;
  `src/ssh/forwarding.rs` of Sandhole).
- `ssh-j` gives no URL: none of the facts.

Not read, so not in the files: the `user` of `serveo` and of `sandhole`
(each takes any user); `free-tunnel-minutes` of `serveo` (the 10 minutes
of its documents are of an anonymous public TCP port), of `localhost-run`
(its free names change after some hours; no session limit is stated) and
of `sandhole`; `keepalive-seconds` of `sandhole` (the server sends its
own, `--ssh-keepalive-interval`; the values of the demo are not known);
SSH on 443 of `localhost-run`; a `url-skip` of `serveo` (no source shows
its banner: `console.serveo.net` would hold `.serveo.net`). Not measured:
a banner with the user `http` of Pinggy. T-225 reads the banners.

The code: `crates/pod-services/src/forward.rs`. `Relay::of(service)`
(`endpoints`, `user`, `url_rule`, `session_limit`, `keepalive`);
`UrlRule { matches, skips }` (`accepts`: each text of `matches` and no
text of `skips`, as podnet's `--url-match` and `--url-skip`; `pick`);
`Banner` (`new`, `push`, `finish`, `url`, `held`, `MOST_HELD` = 2048). A
word ends at a space or a control byte; the URL of a word starts at
`http://` or `https://`. podnet's `UrlRule::pick` takes a token that
starts with `http`, so it misses the URL of Sandhole, whose escape of a
color comes first; `Banner` takes it (the test of the banner of Sandhole
in `crates/pod-services/tests/forward.rs`). The moved fact has two
readers that change at their next tag: podnet `UrlRule::default()` and
podssh `pinggy::dashboard()` (the adoption map of the README).

Checks:
- `cargo test -p pod-services --all-features`: 75 passed (`forward` 5,
  `banner` 10, `api` 19 with `url-skip`, and the rest as before).
- `cargo test -p pod-services-watch --all-features`: 35 passed.
- `cargo clippy -p pod-services -p pod-services-watch --all-targets
  --all-features -- -D warnings`, `cargo fmt --all --check`,
  `cargo todo check`, `python3 -I scripts/check-design-paths.py`: 0.
- The plants (`cargo test -p pod-services --test banner` on each patch):
  `services-forward-word-not-held` fails 4 tests, with
  `a_url_that_two_chunks_cut_is_read_whole`;
  `services-forward-held-unbounded` fails 2, with
  `a_banner_with_no_end_of_line_holds_at_most_the_bound`.
  `sh scripts/plant-each.sh --apply`: 0 stale.

# T-225: `pod-services-watch`: a watch that reads the banner of each SSH forward relay and holds its URL rule

**Source:** T-224 (the facts of the SSH forward relays come from documents, and no watch reads a banner)
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

The facts `url-match` and `url-skip` of `pinggy`, `serveo`,
`localhost-run` and `sandhole` (T-224) come from the documents of the
relays and from podssh's runs. No watch reads the banner of a relay: the
watch `ssh-banner` reads the version line, and `ssh-host-key` reads the
key of the key exchange. A relay that moves the zone of its tunnels
(podssh's `ssh_http::word_of_name` says that the zone of localhost.run
moves) or writes another URL before the
tunnel's breaks the default of `podnet ssh-relay` with no finding in
`services.yml`. No source shows the banner of Serveo (its documents name
the zone only), and the user `http` of Pinggy is not measured.

## Premise

- Read: `crates/pod-services-watch/src/ssh.rs` (a key exchange up to the
  reply of the server; no authentication, no channel).
- The documents that hold the zones today, in their raw HTML (read on
  2026-11-11): `https://pinggy.io/docs/` (`run.pinggy-free.link`),
  `https://localhost.run/` (`a1b2c3d4e5f6.lhr.life`) and
  `https://serveo.net/docs/` ("A subdomain of serveo.net"). The watch
  `http-text` takes an endpoint, not a URL, and these relays have no
  HTTPS endpoint.
- Not measured: a banner, from this sandbox (no SSH).
- PodUtils/podnet#460: a banner of Pinggy of 2026-10-09 held URLs in
  `*.free.pinggy.net`; the fact `url-match` of `pinggy` holds
  `.run.pinggy-free.link`. If the live banner has only the other zone,
  `podnet ssh-relay --service pinggy` finds no URL. podnet took
  `pod_services::forward` at `6aa41b3` and holds no relay host now.

## Approach

1. `http-text` takes `url` in place of `endpoint` (as `http-json` does),
   with its test and its plant. A watch of each document above, with the
   text of the zone: a document that moves the zone is a finding.
2. A watch kind `forward-url` (`endpoint`): an SSH client with a key made
   for the run and given up after it, the user of the fact `user` (else a
   fixed word), a remote forward of port 80 (`tcpip-forward`), a session
   channel with a shell, the banner read through `forward::Banner` with
   the rule of the service, a time limit of 45 s, then the close. It
   reports the host of the URL, and finds a banner with no URL that the
   rule accepts. The client: russh (as podnet's leg) or the OpenSSH of the
   runner; `pod-services-watch` runs in CI alone.
3. Its offline test against a stand-in SSH server on loopback that writes
   a banner in chunks; its plant; the watches of the four relays in their
   files. `services.yml` runs it at most once a day for each relay.

## Prove

```sh
cargo test -p pod-services-watch --test watches
cargo run -q -p pod-services-watch -- --service pinggy --service serveo --service localhost-run --service sandhole
```

## Done

2026-11-12. `52220f9` (the work, the entry T-267, the calls C88 and C89),
and the commit of this section (the plants and the record).

- `http-text` takes `url` in place of `endpoint`
  (`crates/pod-services-watch/src/http.rs`); `cloudflare-dns` gives the
  URL of its trace.
- The documents of the zones, read on 2026-11-12 through the proxy of the
  session with the user agent of the watch (each 200): `https://pinggy.io/docs/`
  holds `.run.pinggy-free.link` (its two sample URLs),
  `https://localhost.run/` holds `.lhr.life` (`a1b2c3d4e5f6.lhr.life`),
  `https://serveo.net/docs/` holds "A subdomain of serveo.net". Each is a
  watch `http-text` of its service.
- The kind `forward-url` (`crates/pod-services-watch/src/forward.rs`, call
  C88): russh 0.65 on ring and pure-Rust RSA over the dial of `Net`; an
  Ed25519 key made for the run, then no key; the fact `user`, else
  `pod-services-watch`; `tcpip-forward` of `localhost` and the port
  (`port`, 80 if none); a session channel with a shell and no terminal;
  `forward::Banner` with the rule of the service, within 45 s (`seconds`);
  the close. It reports the host of the URL. A banner with other URLs and
  none that the rule accepts is a `MOVED` on `url-match`; a banner with no
  URL is a failure. The watches: `pinggy` `ssh` with `port = 0` (its
  document writes `-R0:localhost:8000`), `serveo` `ssh-443`,
  `localhost-run` `ssh` (the user `nokey`), `sandhole` `ssh-443`.
- `services.yml` (call C89): the daily run opens one session with each
  relay; a run by hand that names no service and no kind gives the new
  `--skip-kind forward-url` to the runner.
- Tests: `tests/watches/forward.rs`, a forward relay of russh's server on
  the loopback that writes its banner in chunks, with six tests (a URL
  that chunks cut, `MOVED` on `url-match`, no URL, the bound, no key and
  the port of the watch, a refused forward and a service that is no
  relay); `http::http_text_wants_the_text` with a URL and a 404;
  `each_forward_relay_has_a_watch_of_its_banner_and_of_the_document_of_its_zone`
  in `crates/pod-services/tests/forward.rs`.
- Plants, each run in place with its patch and caught:
  `services-text-not-searched` (its patch made again for the URL) and
  `services-text-any-status` fail `http::http_text_wants_the_text`;
  `services-forward-url-skips-dropped` fails
  `forward::forward_url_finds_a_banner_with_no_url_that_the_rule_accepts`
  (the dashboard passes as the tunnel);
  `services-forward-url-user-dropped` fails
  `forward::forward_url_reads_a_url_that_two_chunks_cut`.

- A flaw that the Prove's run showed, fixed in place: `rdap` gave the
  failure of its last server alone (rdap.org's 429 for `localhost.run`)
  and dropped the failure of the registry's own server. The finding names
  each server now (`crates/pod-services-watch/src/rdap.rs`); the new case
  of `apis::rdap_wants_a_registration_that_does_not_end_soon` fails on the
  old code, which named the 429 alone.

Not measured: a live banner. The proxy of this session resets each SSH
connection. The Prove's run gives 5 holds (the three `http-text`, `rdap`
of `sandhole`, `github-file`) and 13 failures: each `forward-url`
(`free.pinggy.io:443 as http`, `localhost.run:22 as nokey`,
`serveo.net:443` and `demo.sandhole.com.br:443` as `pod-services-watch`),
`ssh-banner` and `ssh-host-key` with "Connection reset by peer", and
`rdap` of `localhost-run` (rdap.org answered 429; the same watch held in a
run some minutes earlier). The lead runs `services.yml` with the kind
`forward-url` on GitHub; its lines give the host of the URL of each
relay. T-267 proves the URL through the tunnel.

Checks: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets
--all-features -- -D warnings`; `cargo test -p pod-services -p
pod-services-watch --all-features` (pod-services 84 passed,
pod-services-watch 45 passed); `cargo todo check`; `python3 -I
scripts/check-design-paths.py`; `sh scripts/plant-each.sh --apply` (0
stale); `cargo deny --all-features --locked check advisories licenses bans
sources`; the runner links static on `x86_64-unknown-linux-musl` (`cargo
zigbuild`: no `PT_INTERP`, no `NEEDED`).
- The live run on GitHub (`services.yml`, kind `forward-url`, run
  43162302910 at `cb228fa`): 3 hold. `pinggy` (`free.pinggy.io:443`, the
  user `http`, a key of the run): `hxwaw-64-236-177-98.run.pinggy-free.link`
  (the zone of the fact holds; PodUtils/podnet#460). `localhost-run`
  (`nokey`, no key): `4b9581eb802ee3.lhr.life`. `sandhole` (a key of the
  run): `bc4urp.demo.sandhole.com.br`. `serveo` failed: it offers
  `publickey, keyboard-interactive`, and the watch tried a key and no key
  alone (T-272). The kind `http-text` (run 43162306650): each holds.


# T-267: `pod-services-watch`: `forward-url` proves the URL of the tunnel with a request through it

**Source:** T-225 (a URL that the rule of a relay accepts is not always the URL of the tunnel)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The watch `forward-url` (T-225) holds when the banner of a relay holds a
URL that the rule of the service accepts. The rule is a list of texts, so
a URL of the service that is not the tunnel's can pass it: the fact
`url-match` of `serveo` holds `.serveo.net`, and a URL of
`console.serveo.net` holds that text too (T-224; no source shows the
banner of Serveo). The watch then holds, and reports the host of a page
of the service. podnet's `ssh-relay` would give that URL to its user.

## Premise

- Read: `crates/pod-services-watch/src/forward.rs` (the remote forward
  goes to no local server; the client takes no channel `forwarded-tcpip`).
- Not measured: a banner of a relay, and the answer of a relay to a first
  request of a client that is no browser (a relay can answer with a page
  of its own). This sandbox has no SSH.

## Approach

1. The client takes each channel `forwarded-tcpip` of the session, and
   answers one HTTP request on it with a fixed answer that holds a word
   made for the run.
2. After the URL, a GET of the URL through the dial of `Net`, within the
   bound of the session. An answer with the word holds; another answer is
   a finding on `url-match` with the host.
3. The offline test: the stand-in relay opens a channel `forwarded-tcpip`
   for a request to its own HTTPS front. A plant that drops the compare
   of the word.

## Prove

```sh
cargo test -p pod-services-watch --test watches -- forward::
cargo run -q -p pod-services-watch -- --kind forward-url
sh scripts/plant-each.sh --group services
```

## Done

2026-11-12. The commit that names T-267.

- `crates/pod-services-watch/src/forward.rs`: the client takes each
  channel `forwarded-tcpip` and answers the head of its request with
  `200` and a word of the run (`pod-services-watch-` and 32 hex digits
  of the SHA-256 fingerprint of the key made for the run). After the URL,
  a GET of it through the dial (three redirects at most) within 20 s
  (`PROOF_LIMIT`): an answer of 2xx with the word holds; another answer,
  or none, is a finding on `url-match` (else `url-skip`) with the URL.
  The argument `prove` (true if none) turns the proof off.
- Tests (`crates/pod-services-watch/tests/watches/forward.rs`): the
  stand-in relay has an HTTPS front for `stand.test` that gives each
  request to the client through a channel `forwarded-tcpip`
  (`forward_url_proves_the_url_with_a_get_through_the_forward`) or answers
  with a page of its own (`forward_url_finds_a_url_that_is_a_page_of_the_relay`).
  The three tests of a banner whose host is no stand-in take `prove =
  false`. `cargo test -p pod-services-watch --all-features --test watches
  forward`: 12 passed.
- The plant `services-forward-url-word-not-compared` (any answer of 2xx
  holds) is caught by the test of a page of the relay.
- The READMEs of both crates name `prove` and the front of the stand-in.
- Not measured: the four live relays. The next run of `services.yml`
  (`--kind forward-url`) proves each URL from GitHub; a relay that
  answers a first request with a page of its own is a finding there.

# T-233: `pod-services`: the registry mcr.microsoft.com of podvm's profile playwright-chromium (PodUtils/podvm#269)

**Source:** PodUtils/podvm#269 (COORD-050 of podvm, for PodUtils/podvm@T-1694)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm's profile `playwright-chromium` names
`mcr.microsoft.com/playwright:v1.65.0-noble@sha256:9beb5460…`. The host
test of podvm finds `mcr.microsoft.com`, which `pod-services` does not
list, and podvm keeps the host in its list of reported hosts until the
crate lists it.

## Premise

- Read: PodUtils/podvm#269. The profile is not on podvm's `main` at
  `65e0c59` yet (PodUtils/podvm@T-1694 is open there).
- Measured on 2026-11-12: `GET https://mcr.microsoft.com/v2/` answers 200
  with no challenge. A HEAD of `/v2/playwright/manifests/v1.65.0-noble`
  with no token answers 200 with the digest of podvm's pin (an OCI image
  index).

## Approach

A service `mcr-registry` (kind `oci-registry`, endpoint `registry`
`mcr.microsoft.com`, fact `images`, no `token-realm`), the watches
`oci-ping` and `oci-digest`, as `opensuse-registry`. A test of podvm's
pin; the README row.

## Prove

```sh
cargo test -p pod-services --all-features --test podvm
cargo run -q -p pod-services-watch -- --service mcr-registry
```

## Done

2026-11-12. The service `mcr-registry` and its README row.
`the_registry_of_playwright_holds_the_pin_of_podvm` checks podvm's pin
and the absence of a token realm; `each_registry_answers_at_v2_over_https`
lists eight registries. `cargo test -p pod-services --all-features`: each
suite passes. The live watches: 2 hold (`oci-ping`, `oci-digest`).


# T-244: `pod-services`: the public TCP relays of Tox on port 443, with the key of each node and a watch of the list (PodUtils/podssh#196)

**Source:** PodUtils/podssh#196 (COORD-028 of podssh, for PodUtils/podssh@T-734)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

PodUtils/podssh@T-734 adds Tox's public TCP relays as a last road of a
pair. podssh writes no host of another party, so it needs the relays from
`pod-services`. The ask:

- A service `tox-tcp-relay`: each node of `https://nodes.tox.chat/json`
  whose `tcp_ports` holds 443 and whose `status_tcp` is true, as an IP
  literal and port 443.
- For each endpoint, the node's `public_key` (64 hex digits): the TCP
  relay's handshake boxes to that key.
- A watch: the list moves, so a watch of that JSON keeps the file current.
- podssh picks a relay by the SHA-256 of the pair's name among the
  endpoints in the file's order. A stable order helps both ends of a pair
  agree.
- The third party: the Tox project's public nodes, run by volunteers, no
  account. Each relay sees the keys of the two ends, the times and the
  sizes. podssh's end-to-end channel keeps the bytes from it.

## Premise

- Read: PodUtils/podssh#196; `crates/pod-services/services/zerotier-tcp-relay.toml`
  (the kind `tcp-relay`); `crates/pod-services-watch/src/http.rs` (the
  watch `http-json` and its form `member`, which finds the hosts of the
  endpoints anywhere in a document, but no port, no key and no condition
  of one item).
- Measured on 2026-11-12: `GET https://nodes.tox.chat/json` (its
  `last_scan` is 02:29 UTC) gives 28 nodes. Five list 443 in `tcp_ports`
  with `status_tcp` true: 119.59.101.63, 205.185.115.131, 188.245.84.166,
  172.104.215.182, 139.162.110.188. Each has `status_udp` true. The list
  writes `-` for a node with no IPv6.
- Not measured: a TCP connect or a handshake to a relay from here. The
  egress of the sandbox is an HTTPS proxy. The watch trusts the list's
  own `status_tcp`.

## Approach

1. `services/tox-tcp-relay.toml`: kind `tcp-relay`, one endpoint for each
   node (scheme `tcp`, the IPv4 literal, port 443, the name `node-` and
   the first eight digits of its key in lower case), and a fact
   `public-key-NAME` for each endpoint (the key as the list gives it).
   The endpoints come in the order of their keys (C83).
2. A general form of `http-json` (`key`): the list at the pointer holds
   one item for each endpoint, found by its member `key`, which equals
   the fact `fact-prefix` + NAME. The item holds the endpoint's host in a
   member of `member`, its port in `port-member` (a number or a list),
   and `true` in each member of `true-members`. An item with the port and
   the true members that the file does not have is counted, not found.
   A test against a stand-in document; a plant.
3. A test of the service in `crates/pod-services/tests/`; the README rows.

## Prove

```sh
cargo test -p pod-services -p pod-services-watch --all-features
cargo run -q -p pod-services-watch -- --service tox-tcp-relay
sh scripts/plant-each.sh --apply
```

## Done

2026-11-12. `f49e0c0` (the entry) and `09eb44c` (the work).

- `crates/pod-services/services/tox-tcp-relay.toml`: kind `tcp-relay`,
  five endpoints in the order of their keys (C83): `node-197f7466`
  (119.59.101.63), `node-3091c6be` (205.185.115.131), `node-96b66d30`
  (188.245.84.166), `node-da2bd927` (172.104.215.182), `node-f76a1128`
  (139.162.110.188), each scheme `tcp`, port 443; five facts
  `public-key-node-KEY8` (64 hex digits). Each source is the list, read on
  2026-11-12.
- `http-json` takes the form `key` (`key`, `fact-prefix`, `member`,
  `port-member`, `true-members`) in
  `crates/pod-services-watch/src/http.rs`; a host compares as an IP
  address in its one text form; `args::Args::opt_texts` is new. The
  READMEs of both crates name the form, the service and
  `crates/pod-services/tests/tox.rs`.
- `cargo test -p pod-services -p pod-services-watch --all-features`:
  each suite passes (`tests/tox.rs` 3, `tests/watches` 37 with the two
  new tests of the form). `cargo clippy -p pod-services -p
  pod-services-watch --all-targets --all-features -- -D warnings` and
  `cargo fmt --all --check` pass.
- The live watch, `cargo run -q -p pod-services-watch -- --service
  tox-tcp-relay`: `holds tox-tcp-relay http-json: 5 endpoints at /nodes of
  https://nodes.tox.chat/json; 5 of 28 items with tcp_ports 443 and
  status_tcp true, 0 of them not in the file`. With `--file` of a copy
  whose first key and second host are changed, it gives two `MOVED`
  lines (no item with the key; the host is `ipv4 "205.185.115.131"`) and
  exits 1.
- The plants `scripts/plants/services-json-items-key-not-compared.plant`
  and `scripts/plants/services-json-items-flag-dropped.plant` are caught
  (exit 101: their tests fail), each run as `scripts/plant-each.sh` runs
  it, with the target directory of the session.
  `sh scripts/plant-each.sh --apply`: 0 stale.
- Not measured: a TCP connect or a handshake to a relay. The watch trusts
  the list's `status_tcp`.

# T-252: podnet's change of `src/doctor.rs` for the lanes of A-C-10 (PodUtils/podnet#462)

**Source:** PodUtils/podnet#462 (COORD-042 of podnet; PodUtils/podnet@T-260, commit `9814f2d`)
**Category:** chore
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

PodUtils/podnet@T-260 gives a probe to the lanes `masque_udp443` and
`ws_upgrade` of `doctor --json`. It changed `src/doctor.rs`, a file of the
podnet block of `crates/pod-services/SOURCES`: a field `probes` of
`Report`, the calls of `lane_probes::start` and `finish`, the module
`lane_probes`. podnet asks whether `pod-services` takes the same lines.

## Premise

- Read: PodUtils/podnet#462; `crates/pod-services/SOURCES` (the podnet
  block: only the values of other parties moved; the code stays in
  podnet; the block has `taken bf0c7ca`, so the check compares its files
  with the parent of that commit).
- The changed lines hold no value of another party: no host, no port, no
  key.

## Approach

No port: the probes are podnet's code. Answer podnet in the next
hand-off: a new value of another party that a probe needs (a host, a
port) comes to `pod-services` by a COORD issue.

## Prove

```sh
python3 -I scripts/check-sources.py pod-services
```

## Done

2026-11-12. No change of `pod-services`: the lines are code, and the
block's take at `bf0c7ca` ended the freeze of its code. The answer goes
in the hand-off of the next tag.

# T-253: `pod-services`: a watch of the relay contract that `pod-relay` implements (PodUtils/podnet#463)

**Source:** PodUtils/podnet#463 (COORD-043 of podnet; the owner's ruling in podnet, "Relay contract watch"; PodUtils/podnet@T-134)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The owner ruled in podnet that the watch of the relay contract belongs to
`pod-relay` in podcrates: a scheduled job that reads
`https://tcp.ssh.relay.ajam.dev/llms-full.txt` and fails when the
contract that `pod-relay` implements changes (the frame format, the
paths, the limits of a session). No workflow of podcrates reads it.

## Premise

- Measured on 2026-11-12: `GET https://tcp.ssh.relay.ajam.dev/llms-full.txt`
  answers 200, `text/plain`, 18,359 bytes, SHA-256 `88eb1b0b…`, the same
  on two reads.
- `services.yml` runs the watches of `pod-services` each day; the watch
  kind `http-sha256` compares the SHA-256 and the length of a file at an
  endpoint with facts.

## Approach

An endpoint `contract` (role `contract`, path `/llms-full.txt`) of the
service `ajam-relay`, the facts `contract-sha256` and `contract-bytes`,
and an `http-sha256` watch of them. Each edit of the document is a
finding; the session reads the change against `pod-relay`, moves the
facts, and changes the crate where the contract changed.

## Prove

```sh
cargo test -p pod-services --all-features
cargo run -q -p pod-services-watch -- --service ajam-relay
```

## Done

2026-11-12. The endpoint, the two facts and the watch in
`crates/pod-services/services/ajam-relay.toml`. `cargo test -p
pod-services -p pod-relay --all-features` passes. The live watch:
`holds ajam-relay http-sha256: the SHA-256 of
https://tcp.ssh.relay.ajam.dev/llms-full.txt is the fact's; ... is 18359
bytes` (8 hold for the service). The watch kind has its own tests and
plants (T-050).

# T-254: `pod-services`: the npm archive of workerd for podvm's profile workerd-test (PodUtils/podvm#274)

**Source:** PodUtils/podvm#274 (COORD-053 of podvm, for PodUtils/podvm@T-1661)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm's profile `workerd-test` fetches the npm package
`@cloudflare/workerd-linux-64` (Cloudflare gives no OCI image of
workerd). podvm keeps the pins in its code and the host
`registry.npmjs.org` in its list of reported hosts until the crate lists
them.

## Premise

- Read: PodUtils/podvm#274 (the facts that podvm measured; podvm
  `392a1b92` adds the profile).
- Measured on 2026-11-12: the registry's document of version
  `1.20261029.1` (licence Apache-2.0) names the tarball; the archive is
  43,615,013 bytes, SHA-256 `771936b7…`, and its SHA-512 is
  `dist.integrity`; its member `package/bin/workerd` is 148,775,704
  bytes, SHA-256 `685acc33…`. Each value is podvm's.

## Approach

A service `cloudflare-workerd-npm` (kind `tool`, endpoint `archive`, the
facts of the table of podvm), an `http-sha256` watch of the archive, as
`alpine-netboot`; a test of podvm's pins; the README row.

## Prove

```sh
cargo test -p pod-services --all-features --test podvm
cargo run -q -p pod-services-watch -- --service cloudflare-workerd-npm
```

## Done

2026-11-12. `crates/pod-services/services/cloudflare-workerd-npm.toml`
and the README row. `the_archive_of_workerd_holds_the_pins_of_podvm`;
`cargo test -p pod-services --all-features --test podvm`: 9 pass. The
live watch: `holds cloudflare-workerd-npm http-sha256: the SHA-256 of
... is the fact's; ... is 43615013 bytes`.

# T-255: `pod-services`: the release archive of wasmtime for podvm's profile wasmtime-wasi (PodUtils/podvm#275)

**Source:** PodUtils/podvm#275 (COORD-054 of podvm, for PodUtils/podvm@T-1661)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm's profile `wasmtime-wasi` fetches the release archive of wasmtime.
podvm keeps its pins (`WASMTIME`) in its code until the crate holds them.
The Bytecode Alliance publishes no checksum file for the archives.

## Premise

- Read: PodUtils/podvm#275 (the facts that podvm measured; podvm
  `9817d687` adds the profile).
- Measured on 2026-11-12: the archive of the release `v50.0.1`
  (published 2026-10-29) is 11,577,424 bytes, SHA-256 `68546645…`, the
  digest that the release API gives for the asset; its member
  `wasmtime-v50.0.1-x86_64-linux/wasmtime` is 67,964,264 bytes, SHA-256
  `983e5757…`. Each value is podvm's.

## Approach

A service `wasmtime-release` (kind `tool`, endpoint `archive` on
`github.com`, the facts of podvm's table), an `http-sha256` watch of the
archive (it follows the redirect of GitHub), as T-254; a test of podvm's
pins; the README row.

## Prove

```sh
cargo test -p pod-services --all-features --test podvm
cargo run -q -p pod-services-watch -- --service wasmtime-release
```

## Done

2026-11-12. `crates/pod-services/services/wasmtime-release.toml` and the
README row. `the_archive_of_wasmtime_holds_the_pins_of_podvm`; `cargo
test -p pod-services --all-features --test podvm`: 10 pass. The live
watch: `holds wasmtime-release http-sha256: the SHA-256 of ... is the
fact's; ... is 11577424 bytes`.

# T-256: `pod-services`: the public wss:// peers of Yggdrasil on port 443 (PodUtils/podssh#203)

**Source:** PodUtils/podssh#203 (COORD-029 of podssh, for PodUtils/podssh@T-735)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

PodUtils/podssh@T-735, the last fallback of a pair after the road of Tox,
joins Yggdrasil's network through one WebSocket (`ygg-ws`), also
through an HTTPS proxy. It needs public peers that listen on `wss://`
and port 443, from `pod-services`: podssh reads the fact `urls` of the
service `yggdrasil-wss-peer` and picks one peer by a hash of the pair's
name.

## Premise

- Read: PodUtils/podssh#203; the list of the network
  (`yggdrasil-network/public-peers` at `0dc7fe1c`): seven `wss://` peers,
  six on port 443; the status page `https://publicpeers.neilalexander.dev/`
  on 2026-11-12: two online (`ygg-evn-1.wgos.org`, 100%;
  `donotclickthis.link/api/v2/socket`, 95%), four offline for 5 weeks.
- Measured on 2026-11-12: each of the two answers the WebSocket upgrade
  (the watch `websocket-upgrade`).
- Not measured: a join of the network through them from this sandbox.

## Approach

A service `yggdrasil-wss-peer` (kind `mesh-peer`), an endpoint for each
online peer, the fact `urls` sorted (one order for both ends of a pair),
a `websocket-upgrade` watch and a `github-file` watch of the list for
each peer; a test of the form; the README rows.

## Prove

```sh
cargo test -p pod-services --all-features --test yggdrasil
cargo run -q -p pod-services-watch -- --service yggdrasil-wss-peer
```

## Done

2026-11-12. `crates/pod-services/services/yggdrasil-wss-peer.toml`,
`tests/yggdrasil.rs` (2 tests), the README rows. The live watches: 4
hold (each peer upgraded; each host is in its file of the list). Only
two peers fit; the offline four stay out.

# T-260: `pod-services`: the public bootstrap nodes of HyperDHT (PodUtils/podssh#204)

**Source:** PodUtils/podssh#204 (COORD-030 of podssh, for PodUtils/podssh@T-736)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

PodUtils/podssh@T-736 is a road of HyperDHT where UDP passes: a node of
a pair announces a topic of a key, and the client connects by that key
through a hole that both ends punch. podssh adds no host of another
party, so the bootstrap nodes of HyperDHT's public network come from
`pod-services`: podssh reads the fact `nodes` of the service
`hyperdht-bootstrap` (`PodUtils/podssh:crates/podssh-cli/src/hyperdht.rs`,
`listed()`).
Until the list names a node, the road is off unless the user names a
DHT.

## Premise

- Read: PodUtils/podssh#204; `lib/constants.js` of `hyperdht` 6.36.0
  (the npm archive, `gitHead` `4fa85cdf`; the newest release on
  2026-11-12): `BOOTSTRAP_NODES` names three nodes in the form
  `IP@NAME:PORT`, each on port 49737. podssh read the same three in
  `peeroxide-dht` 1.7.4 (`DEFAULT_BOOTSTRAP`).
- Measured on 2026-11-12: the A record of each name holds the address of
  its text.
- Not measured: a DHT query to a node over UDP (no UDP from this
  sandbox).

## Approach

A service `hyperdht-bootstrap` (kind `dht-bootstrap`), an endpoint for
each node (role `node`, scheme `udp`, its address), the fact `nodes` in
the order of the source, the fact `addresses`; a `dns-addresses` watch
of the addresses and a `github-file` watch of `lib/constants.js`; a test
of the form; the README rows.

## Prove

```sh
cargo test -p pod-services --all-features --test hyperdht
cargo run -q -p pod-services-watch -- --service hyperdht-bootstrap
```

## Done

2026-11-12. `crates/pod-services/services/hyperdht-bootstrap.toml`,
`tests/hyperdht.rs` (2 tests), the README rows. The live watches: 2 hold
(each address is in the A records; `holepunchto/hyperdht`
`lib/constants.js` holds each text of `nodes`).

# T-271: `pod-services`: the VLESS node `node-xhttp-78-17-39-129` leaves (PodUtils/podssh#207)

**Source:** PodUtils/podssh#207 (COORD-033 of podssh, the live check of Q90 in PodUtils/podssh@T-574)
**Category:** chore
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podssh's live check of the public proxy nodes (its workflow
`proxy-nodes-live.yml`, run 43157171177 at podssh `0872c2c6`, on
`v0.27.0`): each node alone carries a session to `github.com:22`. Two of
the ten failed: the VLESS node `78.17.39.129:443` (path
`/assets/finvip02/`: connection refused) and the Trojan node
`70.34.207.14:443` (a timeout after 7 s). Q90 of podssh says a node that
fails leaves the catalog.

## Premise

- Read: PodUtils/podssh#207.
- `node-trojan-70-34-207-14` left with T-174 (`v0.28.0`).
- Not measured here: the nodes (no egress to them from this sandbox).

## Approach

Remove the file of the service `node-xhttp-78-17-39-129` from
`crates/pod-services/services/`; the README count of the kind
`proxy-node`.

## Prove

```sh
cargo test -p pod-services -p pod-services-watch --all-features
```

## Done

2026-11-12. The file is gone; 8 nodes of the kind `proxy-node` stay (one
VLESS, seven Trojan), the ones that carried the session in podssh's run.
A comment of `crates/pod-services-watch/tests/watches/xhttp.rs` no longer
names the node.

# T-272: `pod-services-watch`: `forward-url` tries keyboard-interactive, which Serveo's anonymous tunnel takes

**Source:** T-225 (its live run)
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The live run of `forward-url` (`services.yml` run 43162302910) failed on
`serveo`: "the relay takes neither a key of the run nor no key; it
offers: publickey, keyboard-interactive". `https://serveo.net/docs/`
says that an anonymous tunnel needs no account and no key. OpenSSH tries
keyboard-interactive after a key and no key; the watch did not, so it
found a failure that a user does not see.

## Premise

- Read: `https://serveo.net/docs/` ("You can try an anonymous tunnel
  without creating an account or key"); russh 0.65
  `authenticate_keyboard_interactive_start` and `_respond`.
- Not measured: the prompts of Serveo (no SSH from this sandbox); the
  next live run reads them.

## Approach

After a key of the run and no key, where the relay offers
`keyboard-interactive`: an empty answer to each prompt, for four rounds
at most. A test with a stand-in that takes only that login; a plant.

## Prove

```sh
cargo test -p pod-services-watch --all-features --test watches -- forward
```

## Done

2026-11-12. `crates/pod-services-watch/src/forward.rs`
(`Session::keyboard_interactive`): the method of a session that it lets
in reads "keyboard-interactive, no answer". Test:
`forward_url_takes_an_empty_keyboard_interactive_login` (a stand-in that
refuses a key and no key and asks one prompt); the 10 tests of
`forward` pass. The plant `services-forward-url-no-keyboard-interactive`
is caught. The live run follows the push.

# T-276: `pod-services`: the public Syncthing relays on port 443, with their device IDs (PodUtils/podssh#209)

**Source:** PodUtils/podssh#209 (COORD-035 of podssh, for PodUtils/podssh@T-571)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

PodUtils/podssh@T-571 adds Syncthing's relay road (podssh's feature
`strelay`) as a last fallback of the reverse road. It reads the service
`syncthing-relay` of `pod-services` and has no relays until it exists.
podssh pins each relay to its device ID in TLS.

## Premise

- Read: PodUtils/podssh#209: 8 of the 100 relays of
  `https://relays.syncthing.net/endpoint` listen on 443 (2026-11-12), with
  their device IDs in the issue.
- Not measured here: the pool, the relays.

## Approach

A service `syncthing-relay` (kind `relay`, or a kind that the README
names for it): an endpoint for each relay on 443 (its IP literal), a fact
`device-id-NAME` for each (as `public-key-node-KEY8` of `tox-tcp-relay`,
call C83), each value read from the pool endpoint with a date; an
`http-json` watch of the pool (form `key`, as T-244) that holds each
endpoint to the pool; a test of the form; the README rows.

## Prove

```sh
cargo test -p pod-services --all-features
cargo run -q -p pod-services-watch -- --service syncthing-relay
```

## Done

2026-11-12. `c71f9a5` (the work and call C104).

- `crates/pod-services/services/syncthing-relay.toml`: kind
  `syncthing-relay`, 25 endpoints `relay-ID7` (scheme `tls`, port 443,
  each an IPv4 literal), each with the fact `device-id-relay-ID7` (the
  device ID of the URL of the relay); the order of the device IDs. The
  rule (call C104): each relay of `/endpoint/full` on 443 with an IP
  literal and 7 days or more of uptime at the read. On 2026-11-12 the pool
  had 1036 relays, 68 of them on 443. `/endpoint` gives 100 relays at
  random: the 8 relays of PodUtils/podssh#209 were one sample; 2 of them
  are in the file, 5 were up for 1 to 2 days (one operator has 32 relays
  on 443, 30 of them up for less than 3 days), and 1 for 6.6 days.
- `http-json` takes `url-member` (`crates/pod-services-watch/src/http.rs`,
  `url_members`): the URL of an item gives the members `host` (an IPv6
  address with no brackets), `port` and each parameter of its query,
  decoded. The READMEs of both crates name the form, the service and
  `crates/pod-services/tests/syncthing.rs`.
- Tests: `crates/pod-services/tests/syncthing.rs` (3: each relay an IP
  literal on 443 with a device ID of eight groups of seven whose head is
  in its name; the order; the watch).
  `http_json_items_read_the_host_the_port_and_the_query_of_a_url` in
  `crates/pod-services-watch/tests/watches/http.rs` failed with the code
  before (no item has the key of its fact) and passes. `cargo test -p
  pod-services -p pod-services-watch --all-features`: each suite passes.
- The plant `services-json-url-query-dropped` (the query gives no member)
  is caught by that test.
- The live watch, `cargo run -q -p pod-services-watch -- --service
  syncthing-relay`: `holds syncthing-relay http-json: 25 endpoints at
  /relays of https://relays.syncthing.net/endpoint/full; 68 of 1036 items
  with port 443, 43 of them not in the file`.
- Not measured: a session through a relay (podssh's
  `strelay-interop.yml` runs it after its take).

# T-278: `pod-identity`: a display form of a fingerprint: base32 in five groups with a checksum of 4 bytes (PodUtils/podnet#479)

**Source:** PodUtils/podnet#479 (COORD-048 of podnet, ask A1, for PodUtils/podnet@T-261)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podnet's link workstream shows the identity of a peer to a person. It asks for a display form of a fingerprint beside the OpenSSH form `SHA256:`: base32 in five groups, with a checksum of 4 bytes, so that a person can read it aloud and a typing error fails.

## Premise

- Read: PodUtils/podnet#479 and its ruling "The link workstream" (podnet's
  `docs/decisions.md`, 2026-11-12); the workstream is P3 in podnet.
- Not measured: podnet's use of it; podnet writes no code for it now.
- `pod_identity` gives the OpenSSH form only. Find the alphabet, the group size and the checksum (a CRC-32 or the head of a SHA-256) in a published form first; record the choice as a call.

## Approach

`pod_identity::fingerprint::display(&key)` and its parse, which refuses a wrong checksum; tests of a known key, of each one-character change and of a swap of two characters.

## Prove

```sh
cargo test -p pod-identity --all-features
```

## Start condition

podnet starts its PodUtils/podnet@T-261 (it writes a COORD issue).

## Done

2026-11-12. The commit that names T-278 (call C118).

- `crates/pod-identity/src/display.rs`: `DisplayFingerprint::{of, parse,
  matches}`, its `Display` (`XXXXXXXX-XXXXXXXX-XXXXXXXX-XXXXXXXX-XXXXXXXX`),
  `DisplayError` (`Length`, `Character`, `Checksum`), and
  `PublicKey::display_fingerprint`. `PublicKey::fingerprint` and the form
  share one digest (`PublicKey::digest`).
- Tests (5): the form and its parse (either case, `-` or spaces or none);
  the form holds the head of the digest of the OpenSSH fingerprint; each
  change of one character (40 times 31) and each swap of two neighbours
  that differ fails the checksum; another length or alphabet is refused; a
  known vector, the key of the zero seed, computed apart with Python's
  `cryptography`: `SHA256:tAXFyTXI8xtDaujAEcwJslAYc9/6FKcUkd2Lw0xDhPo` and
  `WQC4LSJV-ZDZRWQ3K-5DABDTAJ-WJIBQ467-7KYVQCOL`. `cargo test -p
  pod-identity --all-features`: each suite passes; clippy is clean.
- Not done: the form in an allowlist file (`KeyName` keeps its forms, C118).

# T-304: `pod-tls`: a verifier that pins an Ed25519 identity of the peer (PodUtils/podnet#479)

**Source:** PodUtils/podnet#479 (COORD-048 of podnet, ask A7, for PodUtils/podnet@T-268 and PodUtils/podnet@T-273)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

podnet's link authenticates a peer by its Ed25519 identity in TLS. The `mutual` module of `pod-tls` pins P-256 keys only.

## Premise

- Read: PodUtils/podnet#479 and its ruling "The link workstream" (podnet's
  `docs/decisions.md`, 2026-11-12); the workstream is P3 in podnet.
- Not measured: podnet's use of it; podnet writes no code for it now.
- Read `crates/pod-tls/src/mutual*` before the change: the pin of a P-256 key and the certificate it takes; rustls takes Ed25519 certificates with ring.

## Approach

A verifier of each side that takes a certificate whose key is the pinned Ed25519 key (a raw public key or a self-signed certificate), and refuses another key; tests with a minted certificate of each kind.

## Prove

```sh
cargo test -p pod-tls --all-features
```

## Start condition

podnet starts its PodUtils/podnet@T-268.

# T-305: `pod-dns`: a reader of TXT and CNAME answers with EDNS0 and the TC bit, for DNS carriers (PodUtils/podnet#479)

**Source:** PodUtils/podnet#479 (COORD-048 of podnet, ask A10, for PodUtils/podnet@T-278 and PodUtils/podnet@T-281)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

podnet's DNS carriers move bytes in TXT and CNAME answers. They need a reader of these answers that sends EDNS0 with a size and sees the TC bit (a cut answer).

## Premise

- Read: PodUtils/podnet#479 and its ruling "The link workstream" (podnet's
  `docs/decisions.md`, 2026-11-12); the workstream is P3 in podnet.
- Not measured: podnet's use of it; podnet writes no code for it now.
- Read `crates/pod-dns/src` before the change: which record types the parser reads, and whether a query sends an OPT record.

## Approach

The parse of TXT (the strings of each record) and CNAME; a query with EDNS0 and a size; the TC bit in the answer; tests against a stand-in server and the fuzz target of the parser.

## Prove

```sh
cargo test -p pod-dns --all-features
```

## Start condition

podnet starts its PodUtils/podnet@T-278.

# T-329: `pod-services`: the services `smee-io` and `hostc` (PodUtils/podssh#213)

**Source:** PodUtils/podssh#213 (COORD-039), PodUtils/podssh@T-741 and PodUtils/podssh@T-742.
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Two last fallbacks of podssh (smee.io, a relay of webhooks; hostc, a
tunnel service) read their servers from `pod-services` alone. Until a
tag has them, each road stays off.

## Premise

- Read: `lib/server.js` of probot/smee.io at `922d925` (2026-11-12):
  `fastify.get('/:channel')` answers `text/event-stream`;
  `fastify.post('/:channel')` takes the payload.
- Read: akazwz/hostc at `c19ff67` (2026-11-12): `apps/cli/tsdown.config.ts`
  (`HOSTC_DEFAULT_SERVER ?? "https://hostc.dev"`),
  `packages/protocol/src/constants.ts` (`PROTOCOL_VERSION = 5`), the README
  (tunnels under `hostc.app`).
- Measured: IANA's RDAP bootstrap has no row for `io` and a row for `dev`
  (2026-11-12).

## Approach

`services/smee-io.toml` (kind `smee`, endpoint `server`) and
`services/hostc.toml` (kind `hostc`, endpoint `api`, facts `tunnel-zone`
and `protocol-version`), each with watches.

## Prove

```sh
cargo test -p pod-services -p pod-services-watch --all-features
cargo run -q -p pod-services-watch -- --service smee-io --service hostc
```

## Done

2026-11-12. `smee-io`: the fact `routes` held by a `github-file` watch of
`lib/server.js`, and `tls-name` of `server` (no `rdap`: `.io` has no RDAP
server). `hostc`: a `github-file` watch that the CLI's default server is
`hostc.dev`, an `http-text` watch of the whole statement
`export const PROTOCOL_VERSION = 5;` (a `github-file` watch of the value 5
would match any digit 5 of the file), and `rdap` of `hostc.dev`. Test
`the_servers_of_smee_io_and_hostc` (`tests/api.rs`); rows in the README.
The run of the watches: 5 hold.

# T-370: `pod-proxy`: MESSAGE-INTEGRITY, FINGERPRINT and the long-term key in `stun`, for TURN (PodUtils/podssh#214)

**Source:** PodUtils/podssh#214 (COORD-040), PodUtils/podssh@T-621 (TURN with RFC 6062).
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`pod_proxy::stun` reads and writes the attributes of a message, but it
does not sign or check them. podssh's TURN road needs MESSAGE-INTEGRITY
(HMAC-SHA1 over the message, with the header's length set to cover the
attribute), its check (a later FINGERPRINT outside the length), the
FINGERPRINT (CRC-32 XOR `0x5354554e`), and the long-term key
(MD5 of `user:realm:password`). podssh holds copies in
`PodUtils/podssh:crates/podssh-ws/src/turn/msg.rs`.

## Premise

- Read: `crates/pod-proxy/src/stun.rs` (the codec, no I/O);
  `crates/pod-proxy/src/digest.rs` has an MD5.
- Read: podssh's ask: its functions pass RFC 5769's vectors 2.1, 2.2 and
  2.4.

## Approach

`stun::signed`, `stun::integrity`, `stun::fingerprint` and
`stun::fingerprint_ok`, `stun::long_term_key`, with TURN's methods and
attributes (RFC 8656, RFC 6062) as constants. HMAC-SHA1 through `hmac`
and `sha1` (pure Rust); the CRC-32 of the IEEE in the module. The tests
of RFC 5769, 2.1, 2.2 and 2.4, and a plant.

## Prove

```sh
cargo test -p pod-proxy --all-features stun
```

## Done

2026-11-12. `crates/pod-proxy/src/stun_auth.rs` (in `stun`):
`signed`, `integrity` (with `IntegrityError`; the compare of `hmac`
takes the same time for each value), `fingerprinted`, `fingerprint_ok`,
`long_term_key` (no SASLprep: the caller gives the prepared texts),
`message_type` with `Class`, and the methods and attributes of TURN.
HMAC-SHA1 through `hmac` 0.13 and `sha1` 0.11; the CRC-32 in the module.
Tests (`stun_auth_tests.rs`): the vectors of RFC 5769, 2.1, 2.2 and 2.4
check, and signing writes each again byte for byte; a changed byte and a
wrong key fail; the types of TURN. Plant
`proxy-stun-integrity-length-kept`.
