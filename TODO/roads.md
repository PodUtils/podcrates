This file holds the entries of milestone M4: the other roads of podssh.
Each entry starts with a study (the API, the C on each target, the fork),
then follows the procedure of `docs/design.md`, section 8.

# T-031: `pod-derp`: the DERP client and the DERP carrier of a node

**Source:** `docs/design.md`, section 4 (M4).
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

podssh links two keys through the public DERP servers of Tailscale.

## Premise

- Read: `PodUtils/podssh:crates/podssh-relay/src/derp/` (3,543); tests
  `derp_link`, `derp_live` (ignored), `derp_stand_in/`.

## Approach

The study, then the procedure of `docs/design.md`, section 8.

## Prove

```sh
cargo test -p pod-derp
sh scripts/agent/check.sh
```

## Start condition

T-020 is done.

# T-032: `pod-piping`: the Piping Server carrier

**Source:** `docs/design.md`, section 4 (M4).
**Category:** feature
**Milestone:** M4
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

podssh carries a node's sessions through a Piping Server.

## Premise

- Read: `PodUtils/podssh:crates/podssh-relay/src/piping/` (2,439); tests
  `piping`, `piping_live` (ignored), `piping_harness/`.

## Approach

The study, then the procedure of `docs/design.md`, section 8.

## Prove

```sh
cargo test -p pod-piping
sh scripts/agent/check.sh
```

## Start condition

T-026 is done.

# T-033: `pod-opentunnel`: the OpenTunnel carrier

**Source:** `docs/design.md`, section 4 (M4).
**Category:** feature
**Milestone:** M4
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

podssh carries a node's sessions through OpenTunnel.

## Premise

- Read: `PodUtils/podssh:crates/podssh-relay/src/opentunnel/` (2,563); it
  uses the generic cache files of `cache.rs` (`pod-core`).

## Approach

The study, then the procedure of `docs/design.md`, section 8.

## Prove

```sh
cargo test -p pod-opentunnel
sh scripts/agent/check.sh
```

## Start condition

T-020 is done.

# T-034: `pod-acme`: the ACME client

**Source:** `docs/design.md`, section 4 (M4).
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

A public listener of podssh or podnet needs a certificate.

## Premise

- Read: `PodUtils/podssh:crates/podssh-cli/src/acme/` (1,372: TLS-ALPN-01,
  the keys on aws-lc-rs); its interop test runs against Pebble.

## Approach

The study, then the procedure of `docs/design.md`, section 8; the keys
through the provider of the caller.

## Prove

```sh
cargo test -p pod-acme
sh scripts/agent/check.sh
```

## Start condition

T-014 is done.

# T-035: `pod-egress`: the other roads out (via, VLESS, wstunnel, Wisp, Shadowsocks and more)

**Source:** `docs/design.md`, section 4 (M4).
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

podssh has roads out that podnet does not have: an HTTPS proxy, VLESS,
wstunnel, Wisp, Shadowsocks, forwarders, meek, WebTunnel, EICE, IAP and
the Access client of Cloudflare.

## Premise

- Read: podssh-ws `via/`, `vless/`, `wstunnel/`, `wisp/`, `shadowsocks/`,
  `forwarder/`, `meek.rs`, `meek/`, `webtunnel.rs`, `pt.rs`, `eice/`,
  `iap/`, `cloudflare.rs` (about 10,000 lines), and podssh's interop
  workflows of `via`, `vless` and `wstunnel`.

## Approach

The study, then the procedure of `docs/design.md`, section 8; each road
is a `Dial`.

## Prove

```sh
cargo test -p pod-egress --all-features
sh scripts/agent/check.sh
```

## Start condition

T-016 is done.

# T-036: `pod-relay-serve`: the relay role and its mailbox

**Source:** `docs/design.md`, section 4 (M4).
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

podssh serves the relay contract itself (the relay role), with a mailbox
server.

## Premise

- Read: `PodUtils/podssh:crates/podssh-relay/src/serve/` (13,192 with
  `serve/mailbox/`); `serve/` is the most changed module of podssh (100
  file changes in 50 commits).

## Approach

The study, then the procedure of `docs/design.md`, section 8.

## Prove

```sh
cargo test -p pod-relay-serve
sh scripts/agent/check.sh
```

## Start condition

T-026 and T-030 are done.

# T-037: `pod-iroh`: the iroh road

**Source:** `docs/design.md`, section 4 (M4).
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

podssh carries sessions over QUIC between keys with iroh, through a relay
when UDP is refused.

## Premise

- Read: `PodUtils/podssh:crates/podssh-iroh` (3,640); iroh 1.4 with
  `tls-aws-lc-rs`; the patched iroh-relay of `PodUtils/podssh:vendor/iroh-relay`.
- aws-lc-sys is not proven on i686, arm, loongarch64 and powerpc64le musl
  (`docs/design.md`, section 7).

## Approach

The study (the static link on each target, ring where iroh allows it),
then the procedure of `docs/design.md`, section 8, with the fork in
vendor/.

## Prove

```sh
cargo test -p pod-iroh
sh scripts/agent/check.sh
```

## Start condition

T-031 is done.

# T-038: `pod-tor`: the Tor road through arti

**Source:** `docs/design.md`, section 4 (M4).
**Category:** feature
**Milestone:** M4
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

podssh carries a stream through Tor with arti.

## Premise

- Read: `PodUtils/podssh:crates/podssh-tor` (938); arti with a bundled
  SQLite (C).

## Approach

The study (SQLite static on each target), then the procedure of
`docs/design.md`, section 8.

## Prove

```sh
cargo test -p pod-tor
sh scripts/agent/check.sh
```

## Start condition

T-035 is done.

# T-039: `pod-ts`: a Tailscale node in the process

**Source:** `docs/design.md`, section 4 (M4); `docs/decisions.md`, call
C12.
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

podssh runs a tailnet node in its process on a fork of tailscale-rs with
29 patches (DERP over WebSocket, proxies, reconnect).

## Premise

- Read: `PodUtils/podssh:crates/podssh-ts` (895);
  `PodUtils/podssh:vendor/tailscale-rs` (70,135 lines, 40 manifests) and
  its patches; `tailscale` 0.6.1 on crates.io; the headscale harness of
  podssh.

## Approach

The study (the fork against upstream, the static link, aws-lc on each
target), then the procedure of `docs/design.md`, section 8, with the fork
in vendor/.

## Prove

```sh
cargo test -p pod-ts
sh scripts/agent/check.sh
```

## Start condition

T-014 is done.

# T-086: Import `pod-gsr` and `hackerschoice-gsr.toml` (GitHub #24)

**Source:** GitHub #24
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** L
**Status:** done

## Problem

Issue #24: no crate on `main` speaks the GSRN relay of gsocket. A
measured prototype exists outside the tree. The issue asks for the crate
`pod-gsr` (modules `identity`, `settings`, `wire`, `session`, `srp`,
`stream`) and the data file hackerschoice-gsr.toml of `pod-services` (12
facts with sources). The upstream protocol source is
`hackerschoice/gsocket` (C). Changes first: a shard count above 26 is
`Class::Config`; a fuzz target `gsr_message` for `wire::Reader`; the TLS
1.2 version cap on the OpenSSL that SRP needs. A static OpenSSL for SRP
is allowed by call C11 when the static probe proves it (the issue
reports 5,830,416 bytes and no `NEEDED`); no new ruling is needed.
The name `pod-gsr` is not in `docs/design.md`, section 4. It needs a
row there and a reservation on crates.io (T-043). Issues #23 and #24 name
two commits of `hackerschoice/gsocket`; the import takes the commit that
holds the four files.

## Premise

- Read: no clone of podssh, podnet, podvm or podcrates holds the
  prototype, and code search of PodUtils gives 0 hits. The prototype is
  in a place that no session here can read.
- Not measured: the prototype, its size, its tests.

## Approach

Import the crate and the data file by the procedure of `docs/design.md`.
Add the `pod-gsr` row to section 4 (M4, beside `pod-piping`) and reserve
the name. Add the fuzz target. Add the crate to the static probe. T-087
and T-088 follow.

## Prove

```sh
cargo test -p pod-gsr --all-features
cargo test -p pod-services
python3 -I scripts/check-crates-io-names.py
sh scripts/check-static.sh
cargo todo check
```

## Start condition

The owner deferred this entry on 2026-11-09 (`docs/decisions.md`). It
starts when the owner gives the source of the prototype (a branch, a gist
or a patch on issue #24) and the commit of `hackerschoice/gsocket`
(`ad546b2e` in #23, `5950647f` in #24). T-085 must be done before the
import.

The owner attached the prototype to #24 on 2026-11-09 (`pod-gsr.zip`). A
session cannot read an attachment of an issue (GitHub answers 403 with no
signed-in session): the import needs a copy that a session can fetch (a
gist, a release asset or a commit).

The condition is met: on 2026-11-09 the owner gave `pod-gsr.zip` to the
session of the import, and the ruling "Priority" of 2026-11-09 puts
`pod-gsr` in the work order now.

## Correction

2026-11-09: the prototype is `pod-gsr` 0.3.0 (`pod-gsr.zip`: 8 source
files, 3 test files, its data file and a `build.rs`; 2,310 lines with the
documents). Its data file names `hackerschoice/gsocket` at `5950647f`
(the head of `master` on 2026-10-19). `include/gsocket/gsocket.h`,
`lib/gsocket-engine.c`, `lib/gsocket-ssl.c` and `lib/gsocket-util.c` have
the same blobs at `5950647f` and at `ad546b2e` (the tag `v1.4.43`), read
in a clone. The import takes `5950647f`.

## Done

2026-11-09, `ea884e3`. `crates/pod-gsr` from the prototype (`identity`,
`settings`, `wire`, `session`, `srp`, `stream`; each test with its name),
and `crates/pod-services/services/hackerschoice-gsr.toml` (14 facts and the
endpoint `default`, each with its source; watches of four files of gsocket
and of `thc.org`). The three changes of #24: a shard count outside 1 to 26
is `Class::Config` (and the letter holds to `a` to `z`); the fuzz target
`gsr_message` with seeds and the plant `gsr-reader-forgets-bytes`; both
sides cap TLS 1.2 on OpenSSL 3.6.3, and a cap on the server alone and no
cap pair too (call C34). `SOURCES` names `hackerschoice/gsocket` at
`5950647f`; the README names the prototype as the source.

- `cargo test -p pod-gsr --all-features`: 25 unit, 4 loopback, 7 over
  `AF_UNIX`, 1 live ignored.
- `cargo test -p pod-services --all-features`: 42 passed.
- `cargo test --workspace --all-features`: 1012 passed, 13 ignored.
- `cargo check -p pod-gsr --no-default-features`: no tokio in the tree.
- `cargo test --manifest-path fuzz/Cargo.toml --lib`: 2 passed;
  `sh scripts/plant-each.sh gsr-reader-forgets-bytes`: caught.
- `python3 -I scripts/check-sources.py pod-gsr`: 4 files, 0 changes.
- `python3 -I scripts/check-crates-io-names.py`: `pod-gsr` is on
  crates.io (the one fault is `pod-unit`, T-102).
- `cargo zigbuild --release -p static-probe` and `sh scripts/check-static.sh`
  for `x86_64`, `aarch64`, `armv7`, `arm`, `riscv64gc`, `i686`,
  `loongarch64` and `powerpc64le` musl: static (x86_64: 7,177,568 bytes,
  `pod-gsr: o.gs.thc.org, OpenSSL 3.6.3`). i686 needed
  `__atomic_is_lock_free` (`crates/pod-gsr/src/atomic.rs`). macOS and
  Windows: CI (`scripts/openssl-perl.sh` gives Windows a perl for the
  Configure of OpenSSL).

# T-087: `pod_gsr::Stream` records how it ended (GitHub #25)

**Source:** GitHub #25
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Issue #25: `Stream` maps `UnexpectedEof`, reason 294 and `ZERO_RETURN` to
a clean end (the file stream.rs of the prototype). A cut in a
record looks like a completed transfer. The issue measured no data-loss
case.

## Premise

- Read: the issue text only. The crate is not in the tree (T-086).
- Not measured: any behaviour of `Stream`.

## Approach

An enum `End { Clean, Cut }`, an accessor `Stream::end()`, and `is_end`
split into `is_clean` and `is_cut`. `AsyncRead` keeps its results.

## Prove

```sh
cargo test -p pod-gsr --test loopback the_end_of_the_peer_ends_the_stream
cargo test -p pod-gsr --test loopback
```

Two new tests: `End::Cut` after a drop, `End::Clean` after
`poll_shutdown`.

## Start condition

T-086 is done.

## Done

2026-11-09, `8be5ec8`. `pod_gsr::End { Clean, Cut }` and `Stream::end()`;
OpenSSL's shutdown state (`SSL_RECEIVED_SHUTDOWN`) decides the record, and
`is_end` is split into `is_clean` and `is_cut`. `AsyncRead` gives the same
results. `poll_shutdown` sends the close_notify at once and holds the
transport open until the peer's end was read (call C35): with the shutdown
of the prototype, two peers of this crate could only end with a cut.

- `cargo test -p pod-gsr --test loopback`: 6 passed (the new
  `a_drop_of_the_peer_ends_the_stream_as_a_cut` and
  `a_shutdown_of_the_peer_ends_the_stream_as_clean`;
  `the_end_of_the_peer_ends_the_stream` passes unchanged).
- `cargo test -p pod-gsr --test unix_loopback`: 9 passed, with the same
  two tests.
- 30 runs of both test files: no failure.
- The clean test on the shutdown of the prototype: fails at its time
  limit of 10 s (the listener gets no end).

# T-088: `pod-gsr`: the synchronous face (GitHub #26)

**Source:** GitHub #26
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

Issue #26: `open` is async only (`tokio::time::Instant`). A consumer with
no tokio runtime cannot use the crate. The pattern exists in
`crates/pod-proxy/src/blocking.rs`. The `End` accessor is shared with
T-087.

## Premise

- Read: the issue text and `crates/pod-proxy/src/blocking.rs`. The crate
  is not in the tree (T-086).
- Not measured: the size of the core with no tokio.

## Approach

A feature `blocking` over a private current-thread runtime, as in
`pod-proxy`. The core builds with no tokio. A call inside a runtime fails
with `Class::Usage`. The static probe calls it.

## Prove

```sh
cargo test -p pod-gsr --features blocking
cargo check -p pod-gsr --no-default-features
cargo check -p pod-gsr --no-default-features --features blocking
```

## Start condition

T-086 is done.

# T-117: `pod-egress`: the Access TCP client as a local listener (GitHub #35)

**Source:** GitHub #35 (A-N-07, A-C-04, A-X-12)
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

A-N-07 asks an Access TCP client. The Access client of podssh-ws `cloudflare.rs` goes to `pod-egress`.
podssh serves the operator command on Windows and macOS (ruling of 2026-11-09).

## Premise

- Read: the asks and `docs/design.md`, section 4.
- Not measured: the listener.

## Approach

A local listener API with a service token from a file. Exit classes 77 and 78. The command line stays in
podssh.

## Prove

```sh
cargo test -p pod-egress --all-features
```

## Start condition

T-035 is done.
