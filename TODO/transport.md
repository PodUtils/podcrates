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
**Status:** open

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

## Prove

```sh
cargo test -p pod-proxy
sh scripts/agent/check.sh
```

## Start condition

T-005 and T-012 are done.

# T-014: `pod-proxy`: podnet's proxy chain, Digest and fetch

**Source:** `docs/design.md`, section 8, step 5.
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** M
**Status:** open

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

## Prove

```sh
cargo test -p pod-proxy --all-features
sh scripts/agent/check.sh
```

## Start condition

T-013 is done.

# T-015: `pod-dns`: the resolver: system, DoH in both forms, pins, HTTPS records

**Source:** `docs/design.md`, section 4 (M1).
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** M
**Status:** open

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

## Prove

```sh
cargo test -p pod-dns --all-features
sh scripts/agent/check.sh
```

## Start condition

T-014 is done.

# T-016: `pod-ws`: the WebSocket client and server, HTTP/2 and cleartext roads

**Source:** `docs/design.md`, section 4 (M1).
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

podssh has its own WebSocket client and server; podnet uses
tokio-tungstenite and a bridge of its own. One crate serves both.

## Premise

- Read: podssh-ws `client.rs`, `frame.rs`, `handshake.rs`, `session.rs`,
  `url.rs`, `close.rs`, `error.rs`, `plain.rs`, `server.rs`, `h2_road/`,
  `cleartext/`, `wake.rs`; podnet `PodUtils/podnet:src/ws.rs` (596).
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

# T-017: The fuzz workspace and its weekly workflow

**Source:** `docs/design.md`, section 8, step 4.
**Category:** chore
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** open

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
**Status:** open

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

# T-045: The adoption trials check the static binaries of the consumer and build each feature

**Source:** `docs/design.md`, section 8, step 7; the trial of `pod-tls`.
**Category:** chore
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

Step 7 of the procedure asks each trial for "the static check of the
consumer binary on its targets" and "a build with the consumer's
features". The trials of `pod-tls` build the default features and the
features of podssh's gate, but not `tor` and `ts`, and no `.run` checks a
static binary.

## Premise

- Read: `adoption/podssh/pod-tls.run`, `adoption/podnet/pod-tls.run`.
- Read: podssh has `scripts/check-static.sh` (PodUtils/podssh@T-685); this repository has
  `scripts/check-static.sh` for the probe.

## Approach

1. Each `.run` builds the consumer's binary for `x86_64-unknown-linux-musl`
   with zig and checks it with the static check of the consumer (else of
   podcrates).
2. The podssh trial builds the features `tor` and `ts` too.

## Prove

```sh
sh scripts/adoption.sh podssh pod-tls
sh scripts/adoption.sh podnet pod-tls
```

Each trial checks a static binary of the consumer.

## Start condition

T-012 is done.
