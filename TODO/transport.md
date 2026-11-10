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
**Status:** partial

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
  trial of podnet ends. podssh's take remains.

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
**Status:** open

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

## Prove

```sh
cargo test -p pod-services-watch --test watches -- ssh::
```

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
**Status:** open

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

# T-158: `pod-services`: the watch of `croc` upgrades with no relay, and the bridge refuses it

**Source:** T-050 (`services.yml`, run 42806121501); T-156.
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

The watch `websocket-upgrade` of `croc` upgrades `wss://getcroc.com/ws`
with no query. The bridge answers `HTTP 403: relay index is not allowed`:
it takes `/ws?relay=INDEX&port=PORT` (the note of the endpoint). The daily
run of `services.yml` fails on each day, and no watch proves that the
bridge carries a session to the pool.

## Premise

- Measured: run 42806121501 of `services.yml` (one upgrade, 403).
- Not read: the first index of `relay` in croc's `src/webrelay/webrelay.go`
  at v11.6.4 (0 or 1).

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

