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
**Status:** partial

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
**Status:** partial

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
**Status:** partial

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

# T-050: `pod-services`: the watches, their workflows and the measurement scripts

**Source:** the owner, 2026-11-05 (`docs/decisions.md`, Services crate, Services: the design).
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** L
**Status:** open

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

## Prove

```sh
cargo test -p pod-services-watch
sh scripts/plant-each.sh --group services
```

## Start condition

T-049 is done.

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
**Status:** open

## Problem

podvm compiles in the URL of the relay, the endpoints of registries and
the sources of guest images (PodUtils/podvm#194). The services crate holds
the hosts of other parties of each consumer.

## Premise

- Not read yet: podvm's inventory, asked in the answer to
  PodUtils/podvm#194.

## Approach

podvm's inventory in a page podvm.md of `docs/research/services/`, as T-048 did for
podssh and podnet; its data files join T-049.

## Prove

```sh
cargo todo check
```

# T-063: The synchronous face for podvm: `pod-tls` with no tokio, a blocking `pod-proxy`

**Source:** PodUtils/podvm#194, question 1; `docs/decisions.md`, call C20.
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** open

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
cargo check -p pod-tls --no-default-features
cargo test -p pod-proxy --features blocking
```


# T-065: A vectored write through `pod_proxy::Stream` sends only its first buffer

**Source:** the podssh session, 2026-11-06: the step `resume.cut.up.sftp` of
podssh's release gate after podssh took `pod-proxy` (podssh `a791553`).
**Category:** defect
**Milestone:** M1
**Priority:** P0
**Effort:** S
**Status:** open

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
