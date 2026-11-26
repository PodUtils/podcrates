This file holds the entries of milestone M4: the other roads of podssh.
Each entry starts with a study (the API, the C on each target, the fork),
then follows the procedure of `docs/design.md`, section 8.

# T-031: `pod-derp`: the DERP client and the DERP carrier of a node

**Source:** `docs/design.md`, section 4 (M4).
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podssh links two keys through the public DERP servers of Tailscale.

## Premise

- Read: `PodUtils/podssh:crates/podssh-relay/src/derp/` (3,543); tests
  `derp_link`, `derp_live` (ignored), `derp_stand_in/`.

## Approach

The study, then the procedure of `docs/design.md`, section 8.

## Decision

The calls C606 to C609 (`docs/decisions.md`): a connection fails with a
`pod_core::Failure`, a parse of the user's text with its words; the
stand-in DERP server is the feature `standin`; the consumer's names are
`Names`, its lines `pod_core::Events`, and the label of a node's key and
the magic of a link stay; podssh keeps glue in `src/derp.rs`.

## Prove

```sh
cargo test -p pod-derp --all-features
sh scripts/plant-each.sh --group derp
sh scripts/agent/check.sh
```

## Start condition

T-020 is done.

## Done

2026-11-14. The move of podssh's DERP road (PodUtils/podssh@T-684, Q101;
PodUtils/podssh#248), from podssh `b9c04657`:

- `75d6251`: the import of `PodUtils/podssh:crates/podssh-relay/src/derp/` and its tests
  with no change, with `crates/pod-derp/SOURCES` (19 files). The source is
  in edition 2024: no edition commit; the review of section 8, step 2, is
  in the next commit.
- `2757b4f`: the adaptation: the caller's `pod_ws::Way` and
  `pod_proxy::Client` in place of podssh's `ProxyChoice`; no variable
  read (`servers::chosen`, `fleet_with`, `for_client(home, main)`);
  `Names`; events in place of `Say` (`conn::HEALTH`, `far::{ONLINE,
  ENDED, UNREACHABLE, RETRY, MOVED}`); the keeper's `Tell` given to
  `far::serve`; typed failures; `pod_core::backoff`; conn.rs split; the
  stand-in as `pod_derp::standin`. Two new tests:
  `a_node_s_lines_are_events_of_their_kinds`,
  `a_failure_of_the_road_carries_its_stage_and_class`.
- `48317ed`: 9 plants (group `derp`), each proven by hand: caught
  (`derp-t449-registered-late` of podssh, `derp-no-resend`,
  `derp-for-client-alone`, `derp-home-stops-at-refusal`,
  `derp-user-agent-fixed`, `derp-app-name-raw`,
  `derp-chosen-main-ignored`, `derp-health-unsaid`,
  `derp-failure-untyped`); `plant-each.sh --apply`: 0 stale. `live.yml`
  runs `derp_live` and `derp_main_live` by hand (the measures
  `derp-tailcat`, `derp-main`).
- `7f8e93b`: the README, the static probe, the trial patch of podssh and
  the calls.

`cargo test -p pod-derp --all-features`: 32 unit tests, 8 of `derp_link`
(3 ignored: the interop against derper), 1 of `derp_main_live` (1 live,
ignored), `derp_live` (1 live, ignored); clippy with `-D warnings`,
rustdoc with `-D warnings`, rustfmt: clean.

The trial (adoption/podssh/pod-derp.patch, `.commit`, `.run`) was
checked on a scratch copy of podssh at `b9c04657`: cargo check of the
changed crates, clippy, rustfmt, `podssh-todo check` and `check-repo.py`
pass; `adoption.yml` runs its tests. What podssh must do: apply the patch
at its take of the tag that holds pod-derp (`v0.49.0`): delete
`PodUtils/podssh:crates/podssh-relay/src/derp/`, its tests and stand-in,
`derp-main-live.yml` and the plant `t449-derp-registered-late`; keep the
glue `src/derp.rs`; take the stand-in through the feature `standin` in
podssh-cli and podssh-iroh; run `PodUtils/podssh:scripts/interop-derp.sh` with
`PODCRATES` set to a checkout of podcrates; and then leave the lines of
`crates/pod-derp/SOURCES` to its take (a `taken` line).

# T-032: `pod-piping`: the Piping Server carrier

**Source:** `docs/design.md`, section 4 (M4).
**Category:** feature
**Milestone:** M4
**Priority:** P3
**Effort:** S
**Status:** done

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
sh scripts/ci/piping-tests.sh
sh scripts/agent/check.sh
```

## Start condition

T-026 is done.

## Done

2026-11-14, commits `585197f` (the import of podssh `b9c04657`, its
SOURCES), `b2f7293` (the adaptation), `641e834` (the tests, the plants,
the fuzz targets, CI), `d74f7b3` (README, the static probe, the tables),
`4192a6c` (the adoption trial, call C602). The source is in edition 2024
already: no edition commit. The dial and the probe take the caller's
`pod_ws::Way` (`dial::tls(way, trust)`, `probe::upgrade(relay, name, way,
trust, limit)`); the user agent is the caller's
(`Http::new(server, dial, user_agent)`); the lines stay a `Say` callback
(call C60, T-212); the wire keeps its labels. Tests: `cargo test -p
pod-piping --all-features`: 23 unit tests and 8 of the HTTP/1.1 answers
pass; `tests/piping.rs`, 8 passed in 64 s against piping-server 1.12.9
and piping-server-rust 0.18.0 on the loopback (`scripts/ci/piping-tests.sh`
installs both in `ci.yml`, and fails on "did not run");
`tests/piping_live.rs` (2, ignored) runs in `live.yml`. The plants, each
proved by hand: `piping-t437-paths-swapped` (the test of the ends' paths
fails, and a session through piping-server-rust fails) and
`piping-t575-third-before-second` (the test of the order of the public
servers fails). The fuzz targets `piping_hello` and `piping_answer` passed
their seeds and 20,000 mutations of each seed in a scratch crate.

The trial, in a scratch copy of podssh at `b9c04657` with the crates of
this tree: cargo check of `podssh-relay` (features `piping`, `smee`,
`filehost`, with its tests) and of `podssh-cli` (with its tests) passes;
the lib tests of smee and filehost (53) and `tests/http_road.rs` (10) pass;
podssh-todo check agrees after the remap of `Cargo.lock`; check-repo.py
passes. `adoption.yml` runs adoption/podssh/pod-piping.run in CI.

What podssh must do (PodUtils/podssh@T-684): apply
adoption/podssh/pod-piping.patch at the release that holds the crate
(the patch names the tag `v0.49.0`): `podssh_relay::piping` becomes
`pub use pod_piping as piping`; `piping_road::http` gives
`dial::tls(Arc::new(podssh_ws::client::way(&ProxyChoice::FromEnvironment)), trust)`
and `podssh_ws::dial::user_agent()` to `Http::new`; `node_piping::unasked`
gives the same way to `probe::upgrade`; the road's tests, harness, plants
and `piping-rate.yml` leave podssh; then the lines of src/piping/ leave
`crates/pod-piping/SOURCES` here.

# T-033: `pod-opentunnel`: the OpenTunnel carrier

**Source:** `docs/design.md`, section 4 (M4).
**Category:** feature
**Milestone:** M4
**Priority:** P3
**Effort:** S
**Status:** done

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

## Done

2026-11-14, commits `90d6b8a` (the import of podssh `b9c04657`, its
SOURCES), `51d849b` (the adaptation and two fixes), `b7478b0` (the tests,
the plants, the fuzz targets, the interop and the live run in CI),
`d58b815` (README, the static probe, the tables, call C603), `97b451a` (the
adoption trial). The source is in edition 2024 already: no edition commit.
The dials take the caller's `pod_ws::Way`, the node's TLS the caller's
provider, the API the caller's user agent, the record the caller's places,
and `setting` the name of the caller's variable (call C603). The new fuzz
target `opentunnel_answer` found two defects, fixed in place: an IPv6
service on port 443 lost its brackets, and a service's host was not
checked. Tests: `cargo test -p pod-opentunnel --all-features`: 25 unit
tests and 1 of the fuzzing module pass; `tests/opentunnel_link.rs`, 7
passed against the stand-in; `tests/opentunnel_interop.rs` runs in
`opentunnel-interop.yml` against OpenTunnel's own server (it skips with no
server here); `tests/opentunnel_live.rs` (ignored) runs in `live.yml`. The
plants, each proved by hand: `opentunnel-t530-tunnel-not-kept` (the test
of a second start fails), `opentunnel-ipv6-service-unbracketed` and
`opentunnel-service-host-unchecked` (the tests of the two fixes fail). The
fuzz targets `opentunnel_message` and `opentunnel_answer` passed their
seeds and 20,000 mutations of each seed in a scratch crate.

The trial, in a scratch copy of podssh at `b9c04657` after the patch of
`pod-piping`, with the crates of this tree: cargo check of `podssh-relay`
(all features, with its tests), `podssh-cli` and `podssh-iroh` (with their
tests) passes; the lib tests of pinggy, localtunnel, hostc, bore and
opentunnel (59) pass; podssh-cli `tests/opentunnel_road.rs` (4) and
`tests/funnel_opentunnel.rs` (4) pass; podssh-todo check agrees after the
remap of `Cargo.lock`. `adoption.yml` runs
adoption/podssh/pod-opentunnel.run in CI.

What podssh must do (PodUtils/podssh@T-684): apply
adoption/podssh/pod-opentunnel.patch after the patch of `pod-piping`, at
the release that holds the crate (the tag `v0.49.0`):
`podssh_relay::opentunnel` becomes `pub use pod_opentunnel::*` with
podssh's `ENV`; the calls of the CLI take podssh's way, user agent,
provider and places (the adoption map of `crates/pod-opentunnel/README.md`);
the road's tests, the plant t530 and the two workflows leave podssh; then
the lines of src/opentunnel/ leave `crates/pod-opentunnel/SOURCES` here.

# T-034: `pod-acme`: the ACME client

**Source:** `docs/design.md`, section 4 (M4).
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** S
**Status:** done

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

## Done

2026-11-14. Commits `ac2b861` (the import of
PodUtils/podssh:crates/podssh-cli/src/acme at `b9c04657`, frozen there for
the move: PodUtils/podssh#248, PodUtils/podssh@T-684), `f966b57` (the
edition), `be3c395` (the adaptation), `e6679fd` (the plants), `399833c`
(the README, the crate table, the static probe) and `8e92954` (the
adoption trial); call C601.

- The study: the source signs with aws-lc-rs (C); the pure provider of
  `pod-tls`, which podssh's binary takes, loads no private key, so "keys
  through the given provider" cannot sign there. The keys are P-256 keys of
  `p256` (pure Rust), as the server of `pod-tls` signs (C601). rcgen with
  no crypto feature and x509-parser are pure Rust; `cargo deny check
  licenses bans sources` passed.
- `crates/pod-acme`: `Acme` (`new`, `held`, `obtain`), `keep`,
  `renew_at`, `next_retry`, `about`, `lets_encrypt()` of `pod-services`;
  `key::Key`, `jws::Account`, `certs`, `client::{Http, Client,
  Directory}` over the caller's `pod_proxy::Client`, `order`, `store`
  under the caller's cache directories, `url::Url`; `Failed` with a stage
  and a class. No variable of the environment, no output, no process
  state; the lines are events `acme.*`.
- Tests: `cargo test -p pod-acme`: 26 passed (the 21 of the source with
  their names, and the key in both forms of its signature, a key that ring
  wrote, a key of another curve, the stage and the class of each
  failure). `cargo clippy -p pod-acme --all-targets --all-features -- -D
  warnings`, `cargo doc` with `-D warnings` and `cargo clippy -p
  static-probe`: clean.
- Plants (group `acme`): `acme-csr-subject`, `acme-challenge-not-critical`,
  `acme-jws-kid-and-jwk`, `acme-nonce-any-shape`, `acme-renew-too-early`,
  `acme-jws-signature-swapped`, `acme-store-not-private`; each applied by
  hand, failed its test with its EXPECT text, and was removed;
  `sh scripts/plant-each.sh --apply`: 0 stale.
- Fixed in place: the static probe's `src/main.rs` had 502 lines after the
  line of `pod-transit`; its `wormhole()` went to its own file.
- The adoption trial: adoption/podssh/pod-acme.patch, `.run`, `.commit`
  (`b9c04657`) and `.needs` (`pod-transit`). On a scratch copy of podssh
  with both patches and each podcrates dependency on this tree,
  `cargo check -p podssh-cli --lib --tests` passed with no warning;
  rustfmt and podssh's record check passed; `adoption.yml` runs the whole
  `.run` in CI, with podssh's Pebble interop of the binary. podssh must:
  take the tag of the release (`v0.49.0` in the patches); make
  `acme/mod.rs` the glue (re-exports, `new`, `keep`, `Say`); call
  `acme::new` in `relay_serve/certs.rs` and `node_publish_end.rs`; remove
  the files and tests of the module; move `rcgen` to the
  dev-dependencies of podssh-cli; keep PodUtils/podssh:scripts/interop-acme.sh and
  `acme-interop.yml`; fix the citations of its record.
- Not here: a test of a whole order against Pebble in podcrates. podssh's
  interop runs it through the binary (the `.run` of the trial); a loopback
  test of the crate alone is a finding for the parent.

# T-035: `pod-egress`: the other roads out (via, VLESS, wstunnel, Wisp, Shadowsocks and more)

**Source:** `docs/design.md`, section 4 (M4).
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** L
**Status:** done

## Problem

podssh has roads out that podnet does not have: an HTTPS proxy, VLESS,
wstunnel, Wisp, Shadowsocks, forwarders, meek, WebTunnel, EICE, IAP and
the Access client of Cloudflare.

## Premise

- Read: podssh-ws `via/`, `vless/`, `wstunnel/`, `wisp/`, `shadowsocks/`,
  `forwarder/`, `meek.rs`, `meek/`, `webtunnel.rs`, `pt.rs`, `eice/`,
  `iap/`, `cloudflare.rs` (about 10,000 lines), and podssh's interop
  workflows of `via`, `vless` and `wstunnel`.
- podssh's take of `v0.29.0` (PodUtils/podssh#211): podssh-ws has
  `vmess/` too (PodUtils/podssh@T-739, VMess's AEAD form, `e475a63`;
  `vmess-interop.yml` run 43178390705 passed against Xray-core
  v26.10.21). The import takes `vmess/` with `vless/` and `trojan/`.

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

## Done

2026-11-15, partial: part 1, the crate and seven roads. Commits `3e69fb4`
(the import of PodUtils/podssh:crates/podssh-ws/src at `2ffb7cd7`: via/,
forwarder/, wstunnel/, wisp/, meek.rs, meek/, webtunnel.rs, pt.rs,
cloudflare.rs, with their tests), `e6cdb2e` (the edition), `dd2b2c0` (the
adaptation), `640b76e` (the tests, the plants, the fuzz target), `3092df8`
(the README, the static probe, `forwarder-live.yml`, the tables), `4f9f6bd`
(the adoption trial); calls C616 to C620.

- `crates/pod-egress`: a feature for each road (`via`, `forwarder`,
  `wisp`, `wstunnel`, `meek`, `webtunnel`, `cloudflare`); the shared parts
  with no feature (`carry`, `end`, `Note`, `Duplex`); each road dials
  through the caller's `pod_ws::Way`; `via::Client`,
  `forwarder::Forwarder`, `wisp::Road` and `wstunnel::Tunnel` are
  `pod_core::Dial`, and their errors a `pod_core::Failure` with a stage and
  a class (C616); a peer's words go to `pod_core::Events`; the README says
  how a new road joins the crate.
- `cargo test -p pod-egress --all-features`: 59 unit tests; via 6,
  forwarder 7, wisp 10, wstunnel 6, meek 6, webtunnel 6, cloudflare_access
  6; live_forwarder 4 ignored (`forwarder-live.yml`).
- The plants (group `egress`), each caught by hand: `egress-wisp-credit`,
  `egress-via-no-credential`, `egress-wstunnel-wrong-secret` (podssh's
  t438, t439, t441), `egress-via-dial-untyped`,
  `egress-forwarder-close-untyped`, `egress-wstunnel-refusal-untyped`,
  `egress-wisp-blocked-untyped`. The fuzz target `wisp_packet`.
- The trial adoption/podssh/pod-egress.patch (`.run`, `.commit`), on a
  scratch copy of podssh at `2ffb7cd7` with this tree: `podssh-todo check`
  (but the lines of `Cargo.lock`, which the run remaps first),
  `check-repo.py`, clippy and the tests of podssh-ws (all features: 166
  unit tests and each test target), clippy of podssh-cli and podssh-tor,
  the CLI's unit tests of the roads (36), `forwarder_cli` (4),
  `forwarder_public` (16), `cloudflare_cli` (1), rustfmt, the two consumer
  checks. The tests of podssh-tor run in `adoption.yml`.

What podssh must do: take adoption/podssh/pod-egress.patch with the tag
that carries the crate (the patch names `v0.51.0`), and drop its own copies
of the moved tests, plants and `forwarder-live.yml`.

2026-11-15, partial: part 2, four roads more. Commits `70cfa91` (the
import of PodUtils/podssh:crates/podssh-ws/src at `2ffb7cd7`: vless/,
vmess/, trojan/, shadowsocks/, with their tests), `196eba4` (the edition),
`1dcd1fe` (the adaptation), `f407c2a` (the plants, `vmess-interop.yml`),
`8d71a4c` (the README, the tables, the calls), and the trial; calls C621
to C625; finding T-652.

- `crates/pod-egress`: the features `vless`, `vmess`, `trojan`,
  `shadowsocks`; each `Dial` holds the caller's way and trust;
  `vless::Road`, `vmess::Road`, `trojan::Road`, `shadowsocks::Road` are
  `pod_core::Dial`, and each error a `pod_core::Failure` (C621, C622); the
  user agent and the proxy of a silence are the way's (C623); md5 and
  blake3 (pure) join the workspace (C624).
- `cargo test -p pod-egress --all-features`: 112 unit tests; vless 9,
  trojan 5, shadowsocks 10, shadowsocks_2022 5, vmess 3 (against Xray-core
  v26.10.21 here, by `POD_EGRESS_XRAY_TEST_BIN`; `vmess-interop.yml` runs
  it each day); the tests of part 1 as before.
- The plants (group `egress`), each caught by hand:
  `egress-vless-refusal-unsaid` (podssh's t440), `egress-vless-dial-untyped`,
  `egress-trojan-dial-untyped`, `egress-shadowsocks-dial-untyped`.
- The trial adoption/podssh/pod-egress-part2.patch (`.run`, `.commit`,
  `.needs`: after `pod-egress.patch`), on a scratch copy of podssh at
  `2ffb7cd7` with this tree: `podssh-todo check` (the moved Prove targets
  named in a `## Correction` of PodUtils/podssh@T-440, PodUtils/podssh@T-456,
  PodUtils/podssh@T-491, PodUtils/podssh@T-492 and PodUtils/podssh@T-739; each
  citation of a moved file names PodUtils/podcrates), `check-repo.py`,
  clippy and the tests of podssh-ws (all features: 114 unit tests and each
  test target), clippy of podssh-cli, the CLI's unit tests of the roads
  (127), `one_road` (3), `shadowsocks_cli` (2), `proxy_lists` (9), rustfmt,
  the two consumer checks.

What podssh must do: take adoption/podssh/pod-egress-part2.patch after
`pod-egress.patch` with the tag that carries part 2 (the patch names
`v0.52.0`), and drop its own copies of the moved tests, the plant t440 and
`vmess-interop.yml`.

Left (part 3): `eice/` and `iap/`, by the procedure of `docs/design.md`,
section 8, onto the shared parts; their trial in podssh; then the entry is

2026-11-15, partial: part 3, EICE and IAP. Commits `f434e83` (the import
of PodUtils/podssh:crates/podssh-ws/src/eice and iap at `2ffb7cd7`, with
their tests), `61d4665` (the edition), `b9b9e2e` (the adaptation),
`880a0de` (the stand-in of IAP), `873db38` (the plants, the fuzz target),
`90e0548` (the README, the static probe, the tables), `8786cc4` (the
adoption trial); calls C631 to C634.

- `crates/pod-egress`: the features `eice` and `iap`; each road dials
  through the caller's `pod_ws::Way` and sends its user agent; neither is
  a `pod_core::Dial` (C632). The IAP carrier says its steps as events of
  the caller's `Note` (`iap.lost`, `iap.resumed`, `iap.step`; C631), and
  its `NotOpen` and `Ended` give a `pod_core::Failure` with the stage and
  the class of each Close (`close::stage_and_class`).
- `cargo test -p pod-egress --all-features`: 93 unit tests (34 of part 3);
  `eice` 3 and `iap` 2 against stand-ins on the loopback, through a way
  whose dialer takes port 443 to the stand-in (C633).
- The plants (group `egress`), each caught by hand:
  `egress-eice-refusal-shows-credential`,
  `egress-eice-credential-unchecked`, `egress-iap-refusal-untyped`,
  `egress-iap-refusal-shows-token`. The fuzz target `iap_message`.
- The trial adoption/podssh/pod-egress-part3.patch (`.run`, `.commit`,
  `.needs`: after `pod-egress.patch`), on a scratch copy of podssh at
  `2ffb7cd7` with this tree, each step passed: `podssh-todo check` (the
  citations of the moved files name PodUtils/podcrates), `check-repo.py`,
  clippy and the tests of podssh-ws (all features: 133 unit tests and each
  test target), clippy of podssh-cli, the CLI's unit tests `ssh::cloud`
  and `cp::link` (29), `iap_cli` (6) and `eice_cli` (5) through the
  stand-in proxy, rustfmt, the two consumer checks.

What podssh must do for part 3: take adoption/podssh/pod-egress-part3.patch
with the tag that carries it (the patch names `v0.52.0`).

Left (part 2): `vless/`, `vmess/`, `trojan/` and `shadowsocks/` (with
podssh's `vless` interop), by the procedure of `docs/design.md`, section
8, onto the shared parts of part 1; its trial in podssh; then the entry is
done.

2026-11-15, done: podssh took the crate. Part 1 at `3f6a9fe` with `v0.51.0` (PodUtils/podssh#260), parts 2 and 3 at `94ee6ac` with `v0.52.0` (PodUtils/podssh#263): each step of the three `.run` files passed there, and `cargo test --workspace` of podssh passed (3837 tests). `crates/pod-egress/SOURCES` has the `taken` lines; the trials left `adoption/podssh/`. The tag `v0.52.0` (`7998b72`, ci 43711409358, adoption 43711411992) carries the whole crate.

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

- PodUtils/podnet#604 (COORD-098, ask 1): the relay of the pool held the
  socket of a node for minutes after a reset, and answered `409` to each
  new connect of the node; the role must drop the socket of a node at once
  after a reset or a failed write, and say why a node does not open (a
  second run of podnet failed at the relay with 1013, a node open timeout,
  at 18:15 UTC). The import takes the fix, or makes it (T-805 is the node's
  side).
- A fix of the relay role to take at the import (PodUtils/podssh#296,
  podssh T-801, podssh `d8e54f0`): a failed write to a node lets its name
  go at once, and a node socket silent for three pings of 10 s is let go
  in 30 to 40 s, under `pod-relay`'s `lost_hold` of 60 s; the line
  `reverse node lost: WHY`; the test
  `PodUtils/podssh:crates/podssh-relay/tests/relay_serve_lost.rs` (a real RST on the
  loopback). The import reads podssh at `d8e54f0` or later.
- Two ideas of the dig study that podssh took into the role
  (PodUtils/podssh#299, answer to COORD-127): an upgrade with a missing or
  wrong token counts against its address in the brake (podssh T-806,
  `2c4df79`), and `podssh relay serve --proxy-protocol RANGES` reads the
  PROXY header (v1, v2) of the owner's balancer for the brake, the slots and
  the log, tested on haproxy 2.8.30's own bytes (podssh T-807, `2c6323e`).
  The import reads podssh at `2c6323e` or later. The two speeds of the role
  are the multi-thread runtime's placement of tasks, not the role
  (`PodUtils/podssh:crates/podssh-relay/tests/relay_serve_rate.rs`): podssh keeps
  the runtime.

## Approach

The study, then the procedure of `docs/design.md`, section 8. podssh's tests
that run the role and that `pod-relay` left (T-025) come with it:
`tests/session_relay_resume.rs`, `relay_serve_*.rs`, `reverse_carrier.rs`,
`reverse_restart.rs`, `http_road.rs`, `pair_code.rs` with its
`mailbox_harness/` (the pairing by code at the role's mailbox; `pod-relay`
runs the same tests at `pod-wormhole`'s stand-in), and the plants
`t173-resume-to-the-shared-name`, `t255-backoff-back` and
`t326-cleartext-first`, which the trial of `pod-relay` removes from podssh,
as their patches change code of `pod-relay`. The role gives `pod-relay`'s
`reverse::wire::Door` for its carrier (podssh's trial implements it for
its `serve::reverse::carrier::Door`). The live run of the drops through the
role on a runner (podssh's `reverse-drops.yml` with `relay: role`, which
the trial removes) comes with it.

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
**Status:** done

## Problem

podssh carries sessions over QUIC between keys with iroh, through a relay
when UDP is refused.

## Premise

- Read: `PodUtils/podssh:crates/podssh-iroh` (3,640); iroh 1.4 with
  `tls-aws-lc-rs`; the patched iroh-relay of `PodUtils/podssh:vendor/iroh-relay`.
- aws-lc-sys is not proven on i686, arm, loongarch64 and powerpc64le musl
  (`docs/design.md`, section 7).
- Read (2026-11-16, podssh at `0370b0b`): the crate podssh-iroh is 5,854
  lines with its tests. It takes from `podssh-relay` the session of the
  layer (`session::{client, keep::Keeper, Settings, resume, race, record,
  budget}`: `pod-session` here), `derp` (`pod-derp`), `identity`
  (`pod-identity`) and parts of `pinggy` (`Pin::of_spki`, which is
  `pod_tls::mutual::Pin` here; `Address::parse`, `urls::is_name`, which
  podcrates does not have); from `podssh-ws` the dial, the TLS, the names
  and the resolver (`pod-proxy`, `pod-tls`, `pod-ws`, `pod-dns`).
- Read: podssh builds iroh 1.4.1 with its own copy of `iroh-relay` 1.4.1
  (its directory vendor/iroh-relay, `[patch.crates-io]` of its root `Cargo.toml`): a
  patch that dials a relay through a `socks5://` or `socks5h://` proxy, which
  iroh-relay does not do; `tests/proxy_env.rs` of the crate proves it. Here
  the fork goes to vendor/iroh-relay with its own licence, and the
  `[patch.crates-io]` of the root applies to the whole workspace.
- Read (`cargo info iroh@1.4.1`): iroh has the feature `tls-ring` (`noq/ring`,
  `iroh-relay/tls-ring`, `iroh-dns/tls-ring`; in its default features) beside
  `tls-aws-lc-rs`: the crate takes `tls-ring`, so no aws-lc is in its tree
  and the question of aws-lc-sys on the four targets above does not come up.
- Read: the pinggy client stays in podssh (its `podssh-relay`, 1,223 lines;
  no entry of podcrates takes it). The crate uses it only in the pkarr
  record of a node (`pkarr_record.rs`: a road at Pinggy as `NAME#PIN`, and
  `urls::is_name`): the adaptation either keeps the road of the record as
  its text, which podssh's glue reads with its own pinggy, or moves that
  small parse with the crate; a call decides it.

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

## Done

2026-11-16, the commits from the import (`0c47ad6`) to `94c7975`. The
import: the crate podssh-iroh of podssh at `0370b0b` whole, and podssh's
copy of iroh-relay 1.4.1 with its patch (`vendor/iroh-relay`,
`vendor/patches/iroh-relay`; `SOURCES`); its edition was 2024 already. The
adaptation (`9b81c39`, call C767): iroh builds with `tls-ring` (`cargo tree
-i aws-lc-sys` finds none in the workspace); the fork comes in by
`[patch.crates-io]` of the root, and `exclude` keeps it out of the
workspace. The endpoint takes the caller's `pod_tls::Context` (its
provider, with `presets::Empty`), its resolver (`resolve::Chain` over a
`pod_core::Resolve`) and its `pod_proxy::ProxyChoice` for the relay's dial;
pkarr, `/ping` and the question of the version go through the caller's
`pod_ws::Way`. The crate reads no variable: the `from_environment`
functions and `pkarr::choose` are gone, the names of the variables stay as
constants, and the words of what a user does after a refused version are a
parameter. podssh's parts are `pod-session`, `pod-identity`, `pod-derp` and
`pod-opentunnel`; the Pinggy address of a record is checked for its form by
`pod_iroh::pinggy`, in podssh's wire form. `_podssh` and the ALPNs
`podssh/1`, `podssh/2` stay. podssh's ids in the comments are
`PodUtils/podssh@T-NNN`. Tests: 69, each with its name of the source (36
unit tests and 33 in `tests/`), pass with `cargo test -p pod-iroh
--all-features`; `tests/proxy_env.rs` reads an environment of its own, and
sets no variable of the process. The static probe (`be1af67`,
`examples/static-probe/src/iroh.rs`) parses iroh's ticket and signs and
reads a record; its musl binary built with zig passes `check-static.sh`,
and makes an endpoint with the pure-Rust provider of `pod-tls`.
`cargo deny` (advisories, licenses, bans, sources; licenses and sources
with all features) passes with no change of `deny.toml`. The README
(`f6cd810`) has the API, the settings, the parity list, the adoption map
and the fork. `sh scripts/agent/check.sh notest`: fmt, clippy, lint,
record, size, paths and static pass; doc passes after `125c5c0`, which
repairs a link of `pod-tor` to its private module. The adoption trial of
podssh is the main session's.

2026-11-16: `ci.yml` run 43925109993 at `c5fa726` failed the step "The
probe is static" on `aarch64-apple-darwin` and `x86_64-apple-darwin`: the
probe names `Foundation.framework`, which iroh links on macOS and which
`check-static.sh` did not list. Foundation is a framework of the system;
the check takes it (call C768).

# T-038: `pod-tor`: the Tor road through arti

**Source:** `docs/design.md`, section 4 (M4).
**Category:** feature
**Milestone:** M4
**Priority:** P3
**Effort:** S
**Status:** done

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

## Done

2026-11-16, the commits from the import (`63204a7`) to the trial. The
study: podssh builds arti 0.48 with `static-sqlite`, so SQLite's C is
built in; the static probe (`examples/static-probe/src/tor.rs`) links
arti and its SQLite for x86_64 musl with zig, and its binary passes
`check-static.sh` (34.8 MB); CI builds it on each target. The import:
the crate podssh-tor of podssh at `0370b0b` whole, and `scripts/tor-net.sh`
(`SOURCES`); its edition was 2024 already. The adaptation: the way out is
a `pod_ws::Way` (the relays through its `pod_core::Dial`, the transports
through `pod_egress::pt::Reach`, meek_lite and WebTunnel of `pod-egress`),
the trust is `pod_tls::Trust`; a stream of a dial is wrapped in
`sync_wrapper::SyncWrapper`, as Arti asks for a `Sync` stream; podssh's
ids in the comments are `PodUtils/podssh@T-NNN`; the variables of the
tests are `POD_TEST_TOR_NETWORK`, `POD_TEST_TOR_BRIDGES` and
`POD_TEST_TOR_ONION_LIVE`. `deny.toml` takes the crates of arti that it
needs, each named, and ignores two advisories that only `pod-tor` brings
(call C766). Tests: 11 unit tests, `tests/proxied_tcp.rs` (3),
`tests/transports.rs` (4: Arti's own client of a transport against the
sentinels), and `tests/private_network.rs` and `tests/onion_service.rs`,
which say that no network is there and pass here; `tor.yml` (new) starts
chutney's private network on the runner and runs them, and runs the live
onion service by hand (rows L55, L56 of `docs/live-calls.md`). The
README has the API, the settings, the parity list and the adoption map.
The trial adoption/podssh/pod-tor.patch (removed at the take of podssh, PodUtils/podssh#289): the crate podssh-tor leaves
podssh, podssh-cli takes `pod-tor` with its feature `tor` and gives it
`podssh_ws::client::way`; its build, clippy, library tests and
`tor_private` pass here. Release notes: `pod-tor` (new): the road through
Tor with Arti, each first hop and each transport of a bridge through the
caller's way out, and an onion service.

2026-11-16: `ci.yml` run 43914083793 at `3c2bcd5` failed in the step "Docs"
of the job `lint`: the module document of `pod-tor` linked the private
module `socks` (`rustdoc::private-intra-doc-links`); the checks of this
entry ran `cargo doc` before the crate existed. The document names the
file now (`125c5c0`), and `cargo doc` with `-D warnings` runs in the checks
before each push.

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

2026-11-09, `79116e3`. `crates/pod-gsr` from the prototype (`identity`,
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

2026-11-09, `509e59d`. `pod_gsr::End { Clean, Cut }` and `Stream::end()`;
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
**Status:** done

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

## Done

2026-11-09, `cd8d049`. `pod_gsr::blocking::{open, Stream}` (feature
`blocking`) on a private current-thread runtime with the background
shutdown of `pod_proxy::blocking`; the deadline is a `std::time::Instant`;
`Stream` is `Read` and `Write` with `end()`, `peer_finished()`,
`shutdown()` and `set_timeout()`. A call inside a tokio task is
`Class::Usage`; a read or a write there is `io::ErrorKind::Unsupported`.
With `blocking` and no `async`, `open` and `Stream` are not public. The
static probe calls `blocking::open`.

- `cargo test -p pod-gsr --features blocking`: 25 unit, 6 loopback, 9
  over `AF_UNIX`, 5 blocking, 1 live ignored; 30 runs of
  `tests/blocking.rs` with no failure.
- `cargo check -p pod-gsr --no-default-features`: no warning, no tokio in
  `cargo tree -e normal`.
- `cargo check -p pod-gsr --no-default-features --features blocking`: no
  warning; clippy of the same: no warning.
- `sh scripts/agent/check.sh`: each step 0; the workspace 1045 passed, 13
  ignored; the static probe of x86_64 musl with the blocking face:
  7,354,736 bytes, static.

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

# T-129: `pod-gsr`: the tests over `AF_UNIX` do not build on Windows

**Source:** the nightly run 42676423140 at `9414191` (jobs `test (windows-2025)` and `test (windows-11-arm)`).
**Category:** defect
**Milestone:** M4
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`cargo test --workspace --all-features` failed on Windows with exit code
101. The test targets `unix_loopback` and `blocking` of `pod-gsr` use
`tokio::net::UnixListener` and `UnixStream`, which tokio does not have on
Windows (`error[E0432]: unresolved imports`). The LNK4099 messages of the
vendored OpenSSL are warnings and do not fail the build.

## Premise

- Read: the log of job 129358892561. The only error is the E0432 of
  `unix_loopback`.

## Approach

Both test files get `#![cfg(unix)]`. `tests/loopback.rs` (TCP) and the
unit tests still run on Windows.

## Prove

```sh
cargo test -p pod-gsr --all-features
```

## Done

2026-11-09. The two files carry `#![cfg(unix)]`. The nightly dispatched
after the landing is the proof for the Windows jobs.

2026-11-09, second pass. The Windows jobs then failed in the unit test
`a_shard_count_that_no_letter_names_is_a_configuration_failure`: Windows
checks out the data file with CRLF, and the test searches it for LF. The
test now replaces CRLF with LF, and `.gitattributes` sets `eol=lf` for
each text file. The nightly dispatched after the landing is the proof.

# T-131: `pod-gsr`: the handshake tests crash on Windows ARM64

**Source:** the nightly run 42685319279 at `ceb5a74` (job `test (windows-11-arm)`).
**Category:** defect
**Milestone:** M4
**Priority:** P1
**Effort:** M
**Status:** partial

## Problem

`cargo test -p pod-gsr --test loopback` ends with `0xc0000005,
STATUS_ACCESS_VIOLATION` on `aarch64-pc-windows-msvc`, after the test
`a_connector_with_no_listener_is_unavailable` passed. The 25 unit tests, and
each test on `x86_64-pc-windows-msvc`, pass. The tests that crash are those
that run a TLS handshake with SRP in the vendored OpenSSL. The runner has no
NASM, so `openssl-src` builds OpenSSL with `no-asm` there.

## Premise

- Read: the log of job 129390452858. The harness hides the output of the
  crashed test; the name of the test is not known.
- Not measured: which test and which call crash.

## Approach

`nightly.yml` runs `pod-gsr` alone on Windows ARM64 with `--nocapture
--test-threads=1` and `continue-on-error`, and leaves it out of the main
test step. Name the call from that output, and then fix the code or the
OpenSSL build options. Remove the exclusion with the fix.

## Prove

```sh
cargo test -p pod-gsr --all-features   # on windows-11-arm
```

## Done

2026-11-20, partial (a measurement, no fix): the step "Test pod-gsr (Windows ARM64, open defect)" of the nightly run 44696810225 at `68fbc96` (job 136155413649, runner image win11-vs2026-arm64 20261115.219) passes each test of `pod-gsr` with `--test-threads=1`: 25 unit tests and 6 of `tests/loopback.rs`, with the SRP handshake (`one_megabyte_crosses_the_relay`, `different_secrets_fail_the_handshake`). The same step in the run 43467939399 at `1335e9a` (job 132031980992, image 20261101.203, the same `openssl-src` 300.6.1+3.6.3 and Rust 1.99) ended with `STATUS_ACCESS_VIOLATION` after `a_connector_with_no_listener_is_unavailable`. The runs between them skipped the step. No change of `pod-gsr` between the two commits changes its code. One green run does not prove a fix: the crash can depend on the image of the runner or be intermittent. Next: keep the step; when it passes in three runs more, run `pod-gsr` in the main step on Windows ARM64 (with its default threads) and read the result before the exclusion goes.

# T-217: Research: the base of `pod-git`, a git server in Rust with no installed git (PodUtils/podnet#453)

**Source:** PodUtils/podnet#453 (COORD-035 of podnet; the owner's ruling of 2026-11-11 in podnet: no native git server in podnet, a crate `pod-git` of our own)
**Category:** research
**Milestone:** M4
**Priority:** P2
**Effort:** M
**Status:** blocked

## Problem

`podnet git serve` (smart HTTP through a quick tunnel, SSH, the relay
road) runs the host's `git upload-pack` and `git receive-pack`, and a
host of podnet can have no `git`. podnet asks for `pod-git`, a server over
byte streams with the contract and the acceptance of PodUtils/podnet#453.
No crate holds a server today: gix has none upstream (its maintainer
closed the server pull requests); grit-lib 0.5.33 (MIT) has an
`UploadPack` and a `ReceivePack` with delta compression, but it is weeks
old, has about 150 crates, and breaks its API before 0.6.

## Premise

- Read: PodUtils/podnet#453 (the contract: `upload_pack`,
  `receive_pack`, `advertise`, `rpc`, `init_bare`, `refs`, `head`,
  `is_repository`; a hook trait before the ref update; a policy value;
  no process, no shell, no environment read; protocol v0, v1 and v2 for
  upload-pack, v0 and v1 for receive-pack with their capabilities; the
  on-disk format of git; SHA-1; pure Rust, no ring or aws-lc; at most 3 MB
  in podnet's binary and 60 new packages; 64 MiB RSS for the reference
  clone and a blob of 100 MiB).
- Read (crates.io, 2026-11-11): `pod-git` is free; grit-lib 0.5.33
  (2026-11-09), gix-protocol 0.68.0 and gix-pack 0.77.0 (2026-10-22).
- Not read: the code of grit-lib's server; the server parts that gix has
  (the pack writer, the delta search); the size and the package count of
  each base in a musl binary.

## Approach

Measure each base against the contract: own code over the gix data layer
(`gix-pack`, `gix-odb`, `gix-ref`, `gix-protocol`'s pkt-line), a fork of
grit-lib's server parts, or grit-lib as a dependency. For each: the
packages and the size in a musl binary of a small server, the RSS of a
clone of a repository of about 100,000 objects, the time against `git
upload-pack`, the licence of each package, and the clone and push smoke
test with git 2.39 and the newest git. Record the choice as a call, and
amend T-218 to T-220 and the row of `docs/design.md`.

## Prove

```sh
cargo todo check
```

## Blocker

The owner, 2026-11-11 and 2026-11-12 (`docs/decisions.md`, the rulings
"pod-git" and "Renames"): the work goes to `PodUtils/podforge`, the home
of git hosts and alternatives such as Radicle behind one interface, and
it is deferred. The owner unblocks it; this entry then moves to the
record of `PodUtils/podforge`.

# T-218: `pod-git`: upload-pack (v0, v1, v2) and the pack writer (PodUtils/podnet#453)

**Source:** PodUtils/podnet#453
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** L
**Status:** blocked

## Problem

The fetch half of the contract of PodUtils/podnet#453: `upload_pack`
over a reader and a writer, protocol v0, v1 and v2 (`ls-refs`, `fetch`),
`side-band-64k`, `ofs-delta`, `multi_ack_detailed`, `no-done`, `shallow`
and `deepen` (depth, since, not), `filter` (`blob:none` at least),
`include-tag`, `thin-pack`, `allow-tip-sha1-in-want` off by default; a
pack with delta compression, as a stream.

## Premise

- Read: PodUtils/podnet#453.
- The base comes from T-217.

## Approach

On the base of T-217. Tests with the `git` client (2.39 and the newest):
clone over v0, v1 and v2, `--depth 1`, `--filter=blob:none`, an
incremental fetch; the pack of the reference repository at most 1.5 times
the size of `git upload-pack`'s; 64 MiB RSS. Fuzz targets of the
pkt-line reader and of the request parser.

## Prove

```sh
cargo test -p pod-git --all-features
```

## Blocker

The owner, 2026-11-11 and 2026-11-12 (`docs/decisions.md`, the rulings
"pod-git" and "Renames"): the work goes to `PodUtils/podforge`, the home
of git hosts and alternatives such as Radicle behind one interface, and
it is deferred. The owner unblocks it; this entry then moves to the
record of `PodUtils/podforge`.

## Start condition

T-217 is done.

# T-219: `pod-git`: receive-pack, the policy and the hook (PodUtils/podnet#453)

**Source:** PodUtils/podnet#453
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** L
**Status:** blocked

## Problem

The push half of the contract of PodUtils/podnet#453: `receive_pack`
over a reader and a writer, v0 and v1, `report-status` and
`report-status-v2`, `delete-refs`, `atomic`, `push-options`,
`side-band-64k`, `quiet`, `ofs-delta`, thin packs completed from the
repository; the index of the pack, the connectivity check, the check of
each new object (as `receive.fsckObjects`), the ref update under its
lock; a policy value (deny a non-fast-forward, a delete, the update of
the checked-out branch, an atomic push, the largest pack); a hook trait
`check(&[RefUpdate]) -> Result<(), Refusal>` whose text reaches the
client.

## Premise

- Read: PodUtils/podnet#453.
- The base comes from T-217.

## Approach

Tests with the `git` client: a new branch, a fast-forward, a
non-fast-forward refused and allowed, a delete refused and allowed, an
atomic push of two refs where one fails, push options to the hook, a thin
pack, the text of a refusal printed by the client, `git fsck --strict`
after each push, pushes at the same time with no corrupt ref, a blob of
100 MiB under 64 MiB RSS. A fuzz target of the pack and index reader.

## Prove

```sh
cargo test -p pod-git --all-features
```

## Blocker

The owner, 2026-11-11 and 2026-11-12 (`docs/decisions.md`, the rulings
"pod-git" and "Renames"): the work goes to `PodUtils/podforge`, the home
of git hosts and alternatives such as Radicle behind one interface, and
it is deferred. The owner unblocks it; this entry then moves to the
record of `PodUtils/podforge`.

## Start condition

T-218 is done.

# T-220: `pod-git`: stateless smart HTTP, the repository calls, the budgets and podnet's trial (PodUtils/podnet#453)

**Source:** PodUtils/podnet#453
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** M
**Status:** blocked

## Problem

The rest of the contract of PodUtils/podnet#453: `advertise(service,
version)` and `rpc(service, version, body, writer)` for stateless smart
HTTP (the caller gives the gzip and the chunks), `init_bare`, `refs`,
`head`, `is_repository`; the acceptance: a repository that `git` wrote
serves through `pod-git` and the reverse; the reference repository
(about 100,000 objects, pinned) in at most 2 times the time of `git
upload-pack`; at most 3 MB and 60 packages in a musl binary; the trial of
podnet (a patch `pod-git.patch` of `adoption/podnet/`: a backend trait in front of
`PodUtils/podnet:crates/podnet-serve/src/git*`, `git serve --git-backend git|native|auto`).

## Premise

- Read: PodUtils/podnet#453.

## Approach

The HTTP calls over the halves of T-218 and T-219; a test of each with
the `git` client over a loopback HTTP server of the test; the budgets as
a check of CI on a musl build of a small binary; the patch of podnet and
its live steps ("Git over HTTPS through a quick tunnel", "Git over SSH
through the reverse relay") with `--git-backend native`.

## Prove

```sh
cargo test -p pod-git --all-features
```

## Blocker

The owner, 2026-11-11 and 2026-11-12 (`docs/decisions.md`, the rulings
"pod-git" and "Renames"): the work goes to `PodUtils/podforge`, the home
of git hosts and alternatives such as Radicle behind one interface, and
it is deferred. The owner unblocks it; this entry then moves to the
record of `PodUtils/podforge`.

## Start condition

T-219 is done.

# T-302: `pod-egress`: the accept side of the SIP004 (Shadowsocks AEAD) and Trojan codecs (PodUtils/podnet#479)

**Source:** PodUtils/podnet#479 (COORD-048 of podnet, ask A4, for PodUtils/podnet@T-294 and PodUtils/podnet@T-295)
**Category:** feature
**Milestone:** M4
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

podnet's link serves Shadowsocks AEAD (SIP004) and Trojan at a peer, so it needs the accept side of both codecs, tested with the published test vectors of SIP004.

## Premise

- Read: PodUtils/podnet#479 and its ruling "The link workstream" (podnet's
  `docs/decisions.md`, 2026-11-12); the workstream is P3 in podnet.
- Not measured: podnet's use of it; podnet writes no code for it now.
- `pod-egress` (T-035) is not written: the client side comes first.

## Approach

With T-035: the server side of each codec (the salt and the key of SIP004 with HKDF-SHA1, the chunks; the Trojan header with the SHA-224 of the password); the test vectors of SIP004; a test of a client of the crate against its server.

## Prove

```sh
cargo test -p pod-egress --all-features
```

## Start condition

T-035 is done, and podnet starts its PodUtils/podnet@T-294.

# T-357: `pod-gsr`: gs-netcat as the other end, through the stand-in of the relay

**Source:** `docs/live-calls.md` (row L10).
**Category:** feature
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

Both ends of each offline test of `pod-gsr` are `pod-gsr`
(`crates/pod-gsr/tests/loopback.rs`, `crates/pod-gsr/tests/blocking.rs`).
A defect that both ends share passes. Only the live test
(`crates/pod-gsr/tests/live.rs`, by hand, in no workflow) meets the relay
of THC, and there the other end is `pod-gsr` too. No test meets the
client of gsocket itself (the SRP and TLS of the ends, the address of a
secret).

## Premise

- Read (2026-11-12): https://github.com/hackerschoice/gsocket (BSD-2-Clause;
  release v1.4.43 with the static `gs-netcat_linux-x86_64`;
  `lib/gsocket-engine.c` reads `GSOCKET_IP`, `GSOCKET_PORT` and
  `GSOCKET_HOST`, so the client takes a relay on the loopback).
- Measured: the SHA-256 of `gs-netcat_linux-x86_64` of v1.4.43,
  `55d141bb2f62c527c3fea3413b8429fed7d52633d0b123b5e611bfa42165229f`
  (2,846,128 bytes, static-pie), downloaded through fetch.web.pkgforge.dev
  on 2026-11-12. The release publishes no checksum.
- Read (2026-11-12): https://github.com/hackerschoice/gsocket-relay: its
  LICENSE gives no permission to use the code. The relay of THC cannot
  run in a job; the stand-in of `loopback.rs` stays the relay.
- Not measured: gs-netcat against the stand-in (the stand-in may lack a
  message that the real client sends).

## Approach

scripts/ci/gsocket-tests.sh: gs-netcat of v1.4.43 from its release,
checked by the SHA-256 above. A test of `pod-gsr` starts the stand-in
relay, then gs-netcat with `GSOCKET_IP=127.0.0.1` and the port of the
stand-in, in each role (`-l` and the connect), and `pod-gsr` in the other
role with the same secret; 1 MiB crosses in each direction. A message
that the stand-in does not speak is added to the stand-in, with the
source of its form. With no gs-netcat the test skips, unless
`POD_GSR_TEST_GSNETCAT=required`.

## Prove

```sh
sh scripts/ci/gsocket-tests.sh
cargo test -p pod-gsr --all-features
```

# T-361: `pod-relay`: its end-to-end tests against the self-hosted relay of `pod-relay-serve`, then against workerd

**Source:** `docs/live-calls.md` (row L9).
**Category:** feature
**Milestone:** M4
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

The client of `pod-relay` is proven offline against the stand-ins of
`crates/pod-relay/tests/stand_in` and `crates/pod-relay/tests/e2e_harness`,
which replay the frames of the live relay. The live relay is reached only
by hand (the job `relay` of `live.yml`), and a run on a runner meets the
network of the runner (T-327). The relay role of T-036 is a real server
of the contract: on the loopback it proves the client on each push. The
relay Worker (`podrelay-worker`, the ruling of Q-A7) can run in workerd
on the loopback.

## Premise

- Read: T-036 (the relay role and its mailbox, open); the ruling "The
  relay Worker (Q-A7)" of `docs/decisions.md`.
- Read (2026-11-12): https://github.com/cloudflare/workerd/releases
  (v1.20261112.1, Apache-2.0; `workerd-linux-64.gz`).
- Not measured: the start of the role and of workerd in a job.

## Approach

1. After T-036: the end-to-end tests of `pod-relay` (mint, pair, reverse,
   idle, drops, grants) run on each push against the role on 127.0.0.1,
   with the same cases as the `*_live.rs` tests.
2. After the first release of `podrelay-worker`: the same cases against
   its Worker in workerd on 127.0.0.1, workerd from its release checked by
   its SHA-256 (call C151).
3. The live tests stay by hand: their purpose is the owner's relay.

## Prove

```sh
cargo test -p pod-relay --all-features --test e2e_layered
```

## Start condition

T-036 is done.

# T-636: `pod-derp`: the tests of the DERP link against Tailscale's derper on the loopback in CI

**Source:** T-031 (a finding of its work).
**Category:** chore
**Milestone:** M4
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

Three tests of `crates/pod-derp/tests/derp_link.rs` run against a real derper and are ignored with no server. podssh runs them from a script by hand with Podman; no workflow of podcrates runs them, so a line of the parity list of the crate stays open.

## Premise

- Read: the Done section of T-031 and calls C606 to C609; `crates/pod-derp/tests/derp_link.rs` (`PODSSH_DERP_TEST_SERVER`, `PODSSH_DERP_TEST_CA`); podssh's images/derper at b9c04657 (derper v1.104.4 built from its tag in a pinned Go image).

## Approach

A new script scripts/ci/derper-tests.sh: build derper v1.104.4 from its tag with a pinned Go toolchain (checked by its SHA-256), serve it on the loopback with its own certificate, and run the three tests with the server and the CA set; a skip with the server required fails. A step of `ci.yml`, or of a weekly workflow when the build is long. A plant for each test, and a row of `docs/live-calls.md`.

## Prove

```sh
cargo test -p pod-derp --all-features --test derp_link -- --ignored
```

# T-637: `pod-acme`: a whole order against Pebble on the loopback in CI

**Source:** T-034 (a finding of its work).
**Category:** chore
**Milestone:** M4
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

The tests of `pod-acme` check the parts of an order (the JWS, the nonce, the CSR, the challenge, the renewal, the store), but no test of podcrates runs a whole order: `Client::post` with a bad nonce, `authorize`, `poll` and the finalize. Only podssh's interop with Pebble, through its binary, covers it.

## Premise

- Read: the Done section of T-034 and call C601; `crates/pod-acme`; the `.run` file of adoption/podssh/pod-acme.run (podssh's Pebble interop); the other scripts of `scripts/ci/` that fetch a pinned release (`fetch-tool.sh`).

## Approach

A new script scripts/ci/acme-tests.sh: fetch pinned releases of Pebble and pebble-challtestsrv (checked by their SHA-256), start them on the loopback with Pebble's test root, and run an ignored test of `pod-acme` that drives `Acme::obtain` and serves TLS-ALPN-01 through `pod_tls::server::Served`. A step of `ci.yml`, a plant that breaks the order, and a row of `docs/live-calls.md`.

## Prove

```sh
cargo test -p pod-acme --all-features -- --ignored
```

# T-638: `pod-opentunnel`: the stand-in of OpenTunnel behind the feature `standin`

**Source:** T-033 (a finding of its work).
**Category:** feature
**Milestone:** M4
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

The tests of podssh's command line (`funnel_opentunnel`, `opentunnel_road`, `pinggy_stand_in`, `ssh_tunnel_http`) include the stand-in of OpenTunnel of podssh-relay by its path, so the adoption patch of `pod-opentunnel` keeps a copy of the stand-in in podssh. The stand-in of `pod-opentunnel` is private to its tests.

## Premise

- Read: the Done section of T-033 and calls C602, C603; `crates/pod-opentunnel/tests`; adoption/podssh/pod-opentunnel.patch; the feature `standin` of `crates/pod-wormhole/Cargo.toml` and `crates/pod-derp/Cargo.toml`.

## Approach

A module `standin` of `pod-opentunnel` behind the feature `standin` (its dependencies, such as rcgen and p256, optional), with the API that podssh's tests use; the tests of the crate take it from the crate. The README names the feature and says that no binary turns it on; `scripts/check-probe.py` lists it. The adoption patch of podssh then drops its copy.

## Prove

```sh
cargo test -p pod-opentunnel --all-features
```

# T-642: `pod-egress`: the Python packages of `forwarder-live.yml` by a hashed lock

**Source:** T-035 (a finding of its work, part 1).
**Category:** chore
**Milestone:** M4
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

`forwarder-live.yml` installs wisp-server-python 0.9.0 and the packages it needs (asyncudp, python-socks, uvloop) by version alone, as podssh did. The other Python installs of CI (`scripts/ci/wormhole-servers.txt`) check each package by its hash.

## Premise

- Read: `.github/workflows/forwarder-live.yml`; `scripts/ci/wormhole-servers.txt` and `scripts/ci/mailbox-tests.sh` (the form of a hashed install).

## Approach

A requirements file with the hash of each package (`uv pip compile --generate-hashes`, or `pip-compile --generate-hashes`), installed with `pip install --require-hashes`; `scripts/check-workflows.py` refuses a `pip install` with no `--require-hashes` in a workflow.

## Prove

```sh
python3 scripts/check-workflows.py
```

# T-652: `pod-egress`: the stage of a failed TLS or upgrade on the roads of VLESS, VMess and Trojan

**Source:** T-035 (a finding of its work, part 2; call C622).
**Category:** feature
**Milestone:** M4
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

`VlessError::Carry`, `VmessError::Carry` and `TrojanError::Carry` hold one text for a failed TLS handshake, a failed WebSocket upgrade and a failed stream of HTTP/2, as podssh had them. Their class is `WebSocket`/`Unavailable` for each: a certificate that is not trusted (`Tls`/`Refused`), a clock that is wrong and a cut of the network have the class of a server that went away. `ws::failed` reads the `pod_ws::ConnectError` and drops its stage and class.

## Premise

- Read: `crates/pod-egress/src/vless/ws.rs` (`failed`), `crates/pod-egress/src/trojan/ws.rs`, `crates/pod-egress/src/vmess/ws.rs`, `crates/pod-egress/src/vless/xhttp.rs` (`tls`); `pod_ws::ConnectError::stage_and_class`; call C622.

## Approach

A variant `Connect(pod_ws::ConnectError)` in each of the three errors for a failure of TCP, TLS or the upgrade, with the stage and the class of the `ConnectError`, and the same words in `line`; XHTTP's own TLS gives a `ConnectError::Tls` with the kind of the cut. `Carry` stays for the failures of HTTP/2 and of the pump. A test of each road: a server whose certificate is not trusted fails with `Tls`.

## Prove

```sh
cargo test -p pod-egress --all-features --test vless --test trojan
```

# T-673: `pod-egress`: the features `vless`, `vmess`, `trojan` and `shadowsocks` are in no static probe

**Source:** T-670 (a finding of its work: `python3 -I scripts/check-probe.py` on the branch `integ` at `f6c4fbde`).
**Category:** defect
**Milestone:** M1
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`scripts/check-probe.py` reports four faults after the merge of T-035, part 2: `pod-egress: feature vless is in no static probe, and not in ALLOWED`, and the same for `vmess`, `trojan` and `shadowsocks`. `sh scripts/agent/check.sh` runs that check, so its step `paths` is red, and the static link of the four roads is not proved (`docs/design.md`, section 7).

## Premise

- Measured: `python3 -I scripts/check-probe.py` gives `48 crates, 4 faults` with and without the change of T-670.
- Read: `scripts/check-probe.py` (`ALLOWED`), `examples/static-probe*/Cargo.toml`.

## Approach

Turn the four features of `pod-egress` on in `examples/static-probe` (the probe that takes tokio), with a call of each road in the probe so that the linker keeps it; then `sh scripts/check-static.sh` on x86_64 musl. None goes into `ALLOWED`: each one is a feature of a consumer binary (podssh).

## Prove

```sh
python3 -I scripts/check-probe.py
```

## Done

2026-11-15, the commit of the change. `examples/static-probe` turns on `vless`, `vmess`, `trojan` and `shadowsocks` of `pod-egress`, and `egress::part2` makes a dialer of each with a made-up server, as for the roads of part 1. `python3 scripts/check-probe.py`: 48 crates, 0 faults. The probe built with `cargo zigbuild --release --target x86_64-unknown-linux-musl -p static-probe`; `sh scripts/check-static.sh` says static (24,065,704 bytes); its line names `VLESS vless://vless.example:443/ timeout, VMess vmess://vmess.example:443/ timeout, Trojan trojan://trojan.example:443/ timeout, Shadowsocks ss://192.0.2.1:8888 timeout`.

# T-808: `pod-iroh`: the fork of iroh-relay sends the name of the relay to a `socks5://` proxy (T-037)

**Source:** T-037 (a finding of the adaptation).
**Category:** defect
**Milestone:** M4
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The patch of the fork (`vendor/iroh-relay/src/client/socks5.rs`,
`vendor/patches/iroh-relay/0001-socks5-proxy.patch`) sends the relay's name
with the address type 3 for both `socks5://` and `socks5h://`. The rule of
podcrates (`pod-proxy`, as curl does) is: `socks5://` resolves the name
locally and sends the address; `socks5h://` sends the name
(`docs/design.md`, section 8, the known differences).

## Premise

- Read: the patch and its unit tests (`client::socks5`); the tests of
  `crates/pod-iroh/tests/proxy_env.rs`.
- Not measured: a relay dial through a `socks5://` proxy that cannot
  resolve names.

## Approach

For `socks5://`, resolve the relay's name with the resolver of the
endpoint and send its address (address type 1 or 4); `socks5h://` keeps
the name. Make the patch again from the changed fork, and add a case of
`socks5://` to `tests/proxy_env.rs` (the stand-in proxy sees an address,
not a name).

## Prove

```sh
cargo test -p pod-iroh --all-features --test proxy_env
```

## Done

2026-11-16, the commit of the fix. `socks5::resolves_here` of the fork
(`vendor/iroh-relay/src/client/socks5.rs`): a `socks5://` proxy gets an
address of the relay from the dial's resolver (the caller's, through
`pod-iroh`), with the address type 1 or 4; `socks5h://` keeps the name,
with its fall back to an address on reply 8 (`dial_socks5` in
`client/tls.rs`). The change is
`vendor/patches/iroh-relay/0002-socks5-resolves-here.patch`, which applies
after `0001-socks5-proxy.patch` (it applies in reverse to the tree). Test:
`a_socks5_proxy_without_h_gets_the_relay_s_address` of
`crates/pod-iroh/tests/proxy_env.rs`: the relay's name pinned to
`192.0.2.7` (not the loopback, which the dialer sends through no proxy),
the stand-in proxy forwards that address to the relay, and each tunnel is
of type 1. With the defect planted it fails: the relay dial comes by name
(type 3). Plant `iroh-socks5-sends-the-name`. Release notes: `pod-iroh`: a
`socks5://` proxy gets the relay's address, as `pod-proxy` takes the
scheme.

# T-809: `pod-iroh`: the relay dial of iroh takes a proxy URL, not the caller's dialer (T-037)

**Source:** T-037 (a finding of the adaptation).
**Category:** feature
**Milestone:** M4
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

iroh dials its relay itself, with a proxy URL alone (`Options::proxy`, a
`pod_proxy::ProxyChoice`), not through a `pod_core::Dial` of the caller.
So a way out that is not a proxy URL (WARP, a road of `pod-egress`, a
proxy over a Unix socket or a command) cannot carry the relay, and the
caller must keep `Options::proxy` the same as its `Way` (design rule: the
caller gives the way out).

## Premise

- Read: `crates/pod-iroh/src/endpoint.rs`; iroh 1.4's feature
  `unstable-custom-transports`.
- Not measured: whether a custom transport of iroh 1.4 can carry the
  relay's connection.

## Approach

Either a custom transport of iroh (`unstable-custom-transports`) over the
caller's `Dial`, or a SOCKS5 bridge in the process: a listener on the
loopback whose each connection goes through `Way::dial`, given to iroh as
its proxy. Take the first where iroh's API carries a relay; a test with a
`Dial` that is not a proxy.

## Prove

```sh
cargo test -p pod-iroh --all-features
```

# T-810: `pod-iroh`: the adoption trial of podssh (T-037)

**Source:** T-037 (`docs/design.md`, section 8, step 7).
**Category:** chore
**Milestone:** M4
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

T-037 made `pod-iroh`; no trial of podssh shows that podssh's feature
`iroh` builds and passes its tests on it. `crates/pod-iroh/README.md` has
the adoption map.

## Premise

- Read: the adoption map of `crates/pod-iroh/README.md`;
  adoption/podssh/pod-tor.patch and `.run` (the form of a trial of the
  move).
- Read: a `[patch.crates-io]` of podcrates does not reach a consumer:
  podssh keeps its own `[patch.crates-io] iroh-relay`, pointed at the
  fork of podcrates' tag.

## Approach

The patch, the commit and the run of the trial of podssh (`adoption/podssh/`), as the trial of
`pod-tor`: crates/podssh-iroh and podssh's copy of iroh-relay leave
podssh; podssh-cli takes `pod-iroh` with its feature `iroh` and gives it
its TLS provider, its resolver, its proxy and its way out; podssh's root
`[patch.crates-io] iroh-relay` names the fork of podcrates' tag. The run:
the build, clippy and the tests of the CLI with the feature
(`iroh-test`, the road on a relay of the loopback).

## Prove

```sh
sh scripts/adoption.sh podssh pod-iroh
```

## Done

2026-11-16, `2c9cc64`. adoption/podssh/pod-iroh.patch (removed at the take of podssh)
(podssh at `0370b0b`, `pod-iroh.commit`): crates/podssh-iroh leaves
podssh, with its tests, its four plants and podssh's copy of iroh-relay
(`vendor/iroh-relay`, `vendor/patches/iroh-relay`, its section of
`PodUtils/podssh:vendor/README.md`, its watch in `check-vendored.py`). podssh-cli takes
`pod-iroh` with its feature `iroh` (`iroh-test` takes `test-relay`),
`pod-ws` for the type of the way, and `iroh` with no TLS feature. The new
`src/iroh_way.rs` of podssh-cli gives the crate podssh's TLS context
(`podssh_ws::tls::context`), its resolver (`podssh_ws::resolve::resolver`),
the proxy of the environment (`podssh_ws::dial::settings`) and the way out
(`podssh_ws::client::way`), and reads `PODSSH_IROH_RELAY`, `_PKARR`, `_UDP`
and `_QUIC` by name; a unit test compares the names with pod-iroh's
constants, and the test of the manual's facts reads that file only with the
feature. The words of a refused version stay podssh's (`REMEDY`). The
Pinggy address of a record goes to podssh's own parser. Each podcrates tag
moves to `v0.65.0`. The root `[patch.crates-io]` names
`iroh-relay = { git = "https://github.com/PodUtils/podcrates", tag =
"v0.65.0" }`; `scripts/repo_scans.py` of podssh takes the lock's git
source of that patch (with a plant and a control in `check-repo.py`). The
gate, the listeners' allowance, AGENTS.md and the docs that named the crate
name `pod-iroh`.

Measured: cargo takes a package from a git repository whose workspace
excludes it: a project with `[patch.crates-io] iroh-relay = { git =
"https://github.com/PodUtils/podcrates", rev = "6a965ef" }` locked
`iroh-relay 1.4.1` from `git+https://github.com/PodUtils/podcrates?rev=6a965ef`
(the fork in `vendor/iroh-relay`), with no warning. `scripts/adoption.sh`
now points each other `NAME = { git = podcrates, tag }` of a manifest to
the directory of NAME under `vendor/`, as it points each crate to its directory under `crates/`.

The run, each step passed (`sh scripts/adoption.sh podssh pod-iroh`):
`check-repo.py`; no `PodUtils/podssh:crates/podssh-iroh` and no copy of iroh-relay; no
manifest names podssh-iroh; the patch of iroh-relay is the path of this
tree; the build with `iroh`; pod-iroh and the fork of this tree in its
tree; clippy with `iroh` and with `iroh-test`; the library's tests with
`iroh` (1,173 passed, 1 ignored); with `iroh-test`, `iroh_road` (4, 1
ignored: live), `iroh_cli` (6), `chat_iroh` (1), `pipe_iroh` (2),
`pipe_iroh_record` (2), `pinggy_road` (8), `opentunnel_road` (5),
`man_page` (11); the format of podssh-cli; the toolchains and the test
targets of podssh's gate; the musl binary with `iroh` by zig, which
`check-static.sh` passes. The four plants of podssh's crate are pod-iroh's
now (`scripts/plants/iroh-record-no-derp.plant`, and `iroh-record-name-not-read`,
`iroh-quic-unset`, `iroh-share-while-made` beside it), each caught by its run
here. The README's
adoption map names each change above.

# T-871: `pod-ws`: a shared HTTP/2 connection dies with the runtime that opened it (PodUtils/podssh#310)

**Source:** PodUtils/podssh#310 (COORD-113 of podssh, PodUtils/podssh@T-811).
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`pod_ws::h2_road::pool::drive` drives each pooled HTTP/2 connection with `tokio::spawn`, on the runtime that opened it. The pool is shared by the process (`pod_relay::Context`'s `Shared::h2`). A consumer that runs sessions on more than one runtime shares a connection whose task ends when the opener's runtime drops: each other session's stream on it breaks. podssh's `each` against its relay role lost the links of two hosts of three, also inside the handshake of the layer.

## Premise

- Read: PodUtils/podssh#310; `crates/pod-ws/src/h2_road/pool.rs` (`drive`, `Conn`, `Pool::place`).

## Approach

Each connection keeps the id of the runtime that drives it (`tokio::runtime::Handle::id`); a session takes a kept connection only of its own runtime, and a connection whose task ended (also with its runtime) counts as retired. What the pool knows of a host (it takes HTTP/1.1) stays shared. A test: a session on runtime A, one on runtime B gets a connection of its own, A drops, and B's session carries bytes.

## Prove

```sh
cargo test -p pod-ws --all-features --test h2_road
```

## Done

2026-11-17, 4473c58f0, 0f7c155f8: each `Conn` of the pool keeps the id of the runtime that drives it (`tokio::runtime::Handle::id`, stable); `Pool::place` gives a session a kept connection only of the caller's runtime, and a connection whose task ended (also with its runtime) counts as retired, so it leaves the pool. What the pool knows of a host (it takes HTTP/1.1) stays shared by each runtime. Test `a_session_takes_only_a_connection_of_its_own_runtime` (`crates/pod-ws/tests/h2_road/runtimes.rs`): a stand-in on a third runtime; a session of runtime A and one of runtime B each open a TLS connection; A drops; B's session carries 64 KiB each way, and a second session of B shares B's connection (two TLS connections in all). The tests of `h2_road` pass. Plant `ws-h2-pool-across-runtimes` (a session takes a connection of any runtime), caught.

# T-901: `pod-ws`: a request of the upgrade with no token header, for a probe (PodUtils/podnet#652)

**Source:** PodUtils/podnet#652 (COORD-105 of podnet; ask 3 of #221).
**Category:** feature
**Milestone:** M3
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podnet's doctor (PodUtils/podnet:src/doctor/lane_probes.rs) builds its own WebSocket upgrade: `build_request*` of `pod-ws` always writes the token header.

## Premise

- Read: PodUtils/podnet#652.

## Approach

A form of `build_request` with no token (an `Option` of the token), for a probe; a test that the request has no `X-Relay-Token`. podnet drops its builder.

## Prove

```sh
cargo test -p pod-ws --all-features handshake
```

## Done

2026-11-17, 424690190, f4150e03b: `pod_ws::handshake::build_probe_request(host, path, key, extra)`: the upgrade with no `X-Relay-Token` line, and each other line as `build_request_with` writes it (one writer of the request, with the token as an option). Test `a_probe_request_holds_no_token` (no token header; the request of a session less its token line is the probe's). Plant `ws-probe-sends-a-token`, caught.

# T-903: `pod-dns`: one line of 1000 characters, where each resolver of DNS over HTTPS failed alike (PodUtils/podssh#313)

**Source:** PodUtils/podssh#313 (COORD-116 of podssh, item 1).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

A name that does not resolve, on a host whose way intercepts DNS over HTTPS by address, gives one line that names each resolver with the same TLS failure (six times, 1032 characters).

## Premise

- Read: PodUtils/podssh#313; `crates/pod-dns/src/lookup.rs`, `src/ask.rs`, `src/order.rs`.

## Approach

Fold the failures of the resolvers: a failure that each had in the same words (with the address of each taken out) is said once with the count; a resolver is named alone only where its failure differs. A test holds the length for six resolvers that fail alike.

## Prove

```sh
cargo test -p pod-dns --all-features --lib lookup
```

## Done

2026-11-17, b4d5cbb3b, 3eb3182d3: `lookup::fold` gives `each of N resolvers failed: CAUSE` where each resolver failed in the same words, with the address of each resolver in its own words taken out ("the address of the resolver"); else one part for each failure, `N resolvers (A, B): CAUSE` for a shared one and `A: CAUSE` for one alone, and a reason of no resolver (`out of time`) as it is. Test `the_failures_that_read_alike_are_said_once` (six alike: under 200 characters and no address; five alike and two others). The tests of `pod-dns` pass. Plant `dns-each-resolver-said-alone`, caught.

# T-911: `pod-proxy`: the same address twice in a race and in its error line (PodUtils/podssh#315, ask 1)

**Source:** PodUtils/podssh#315 (COORD-118 of podssh, PodUtils/podssh@T-828), ask 1.
**Category:** defect
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

A resolver answer can hold an address twice (a pin and the system resolver). The race dials both, and the error line counts both: `2 addresses (127.0.0.1:9443, 127.0.0.1:9443): Connection refused`.

## Premise

- Read: PodUtils/podssh#315; `crates/pod-proxy/src/race.rs` (`dial`, `race`, `group_errors`).

## Approach

`race` takes each address once, in its first place; `group_errors` counts each address once. A test of each.

## Prove

```sh
cargo test -p pod-proxy --all-features
```

## Done

2026-11-18, bb9c73d71. `race::distinct`: `race` takes each address once, in its first place (so `dial` and `dial_addrs` too); `group_errors` counts each address once. Tests `race::tests::an_address_twice_is_dialed_once_and_said_once` and `a_dial_of_one_refused_address_twice_names_it_once` (with `pod_testkit::refused`). Plant `proxy-race-address-twice`, caught.

# T-987: `pod-ws`: the dial error of a dialer of the caller names its target two times (PodUtils/podnet#658)

**Source:** PodUtils/podnet#658 (COORD-111 of podnet, PodUtils/podnet@T-497), part 3.
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

```text
$ podnet connect -L tcp://127.0.0.1:0 wss://127.0.0.1:1/__podnet/ws
podnet: the remote wss://127.0.0.1:1/__podnet/ws does not answer (--no-check starts without this check): TCP to 127.0.0.1: could not connect to 127.0.0.1:1: connecting to 127.0.0.1:1: Connection refused (os error 111)
$ podnet connect -L tcp://127.0.0.1:0 wss://nowhere.invalid/__podnet/ws
podnet: ...: TCP to nowhere.invalid: could not resolve nowhere.invalid: connecting to nowhere.invalid:443: could not resolve nowhere.invalid: system resolver: ...
```

`pod_ws::way::dial_error` (`crates/pod-ws/src/way.rs`) puts the text of the dialer's `Failure` into `DialError::Connect { target, detail }` or `DialError::Resolve { host, detail }`, and the `Display` of `DialError` (`crates/pod-proxy/src/error.rs`) puts `could not connect to TARGET: ` or `could not resolve HOST: ` in front. podnet's dialer says `connecting to HOST:PORT: ...` and `could not resolve HOST: ...` already.

## Premise

- Read: PodUtils/podnet#658 (part 3); `crates/pod-ws/src/way.rs` (`dial_error`); `crates/pod-proxy/src/error.rs`; podnet's dialer (PodUtils/podnet:src/egress.rs, `failed`).
- Not measured: podnet's line after the fix.

## Approach

`dial_error` removes a leading `connecting to TARGET: ` or `could not connect to TARGET: ` from the detail, and `could not resolve HOST: ` from the detail of a failure of the name. The helper of T-985 (`pod_core::failure::lead`) puts the words of `DialError` in front once. A test gives `dial_error` the texts of podnet's dialer.

## Prove

```sh
cargo test -p pod-ws --all-features --lib way::tests
cargo test -p pod-proxy --all-features
```

## Done

2026-11-20, 993bf43. `dial_error` removes a leading `connecting to TARGET: ` and `could not connect to TARGET: ` from the detail (`pod_core::failure::strip_lead`), and the `Display` of `DialError::Connect` and `DialError::Resolve` says its words once (`pod_core::failure::lead`). The lines now: `could not connect to 127.0.0.1:1: Connection refused (os error 111)` and `could not resolve nowhere.invalid: system resolver: ...`. Tests: `crates/pod-ws/src/way.rs::a_dial_error_names_its_target_once` (failed before the fix: `could not connect to 127.0.0.1:1: connecting to 127.0.0.1:1: connection refused`) and `error::tests::the_words_of_a_dial_error_are_said_once` of `pod-proxy`. `cargo test -p pod-ws --all-features` and `-p pod-proxy`: green (the run of T-985).

# T-1102: `pod-iroh`: the fork of iroh-relay at 1.4.2, so that iroh 1.4.2 builds it (PodUtils/podssh#341)

**Source:** PodUtils/podssh#341 (COORD-134 of podssh, author COLLABORATOR).
**Category:** chore
**Milestone:** M4
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podssh names the fork of podcrates (`vendor/iroh-relay`, iroh-relay 1.4.1
with the SOCKS5 patch) in its `[patch.crates-io]` at tag `v0.82.0`.
Dependabot moves podssh to `iroh` 1.4.2, `iroh-base` 1.5.0, `iroh-dns`
1.4.2 and `noq` 1.5.0. iroh 1.4.2 asks for `iroh-relay ^1.4.2`, so cargo
does not use the 1.4.1 fork (`[[patch.unused]]`): the relay dial loses
SOCKS5. The ask: carry `vendor/iroh-relay` to 1.4.2 with the patch, in a
tag, and move `pod-iroh` to iroh 1.4.2.

## Premise

- Read: the crates.io index of `iroh`, `iroh-relay`, `iroh-dns` and
  `noq`; the crate `iroh-relay` 1.4.2 and its diff against 1.4.1 (the
  relay protocol streams, the server, the client connection); the
  procedure of `crates/pod-iroh/README.md`, "The fork of iroh-relay".
- Measured: the tree of `vendor/iroh-relay` is the crate 1.4.1 with the two
  patches, byte for byte.
- Not measured: a relay dial through a real SOCKS5 proxy on the internet.

## Approach

Download the crate 1.4.2, compare its SHA-256 with the `cksum` of the
index, unpack it into `vendor/iroh-relay` and apply the patches of
`vendor/patches/iroh-relay/`. Make a patch that applies with an offset
again from the crate. Move the ranges of `iroh` and `iroh-dns` of
`pod-iroh` to `>=1.4.2, <1.5`, update `Cargo.lock`, and set
`version::IROH` to the version that the lock builds. The next tag of
podcrates carries it.

## Prove

```sh
cargo tree -p pod-iroh -i iroh-relay -e normal
cargo test -p pod-iroh --all-features
cargo clippy -p pod-iroh --all-targets --all-features -- -D warnings
sh scripts/plant-each.sh iroh-socks5-sends-the-name
```

## Done

2026-11-23, the commit of this change. `vendor/iroh-relay` is the crate
`iroh-relay` 1.4.2 (SHA-256 `ffa03fd9...2c79`, the `cksum` of the
crates.io index; n0-computer/iroh `2131f266`) with the two patches of
`vendor/patches/iroh-relay/`. `0001-socks5-proxy.patch` applied with line
offsets alone (1 to 3 lines; no fuzz, no changed hunk) and is made again
from the crate; `0002-socks5-resolves-here.patch` applies with no offset
and stays as it was. The tree is the crate and the patches, byte for byte.
`pod-iroh` takes `iroh` and `iroh-dns` `>=1.4.2, <1.5`; the lock builds
iroh 1.4.2, iroh-base 1.5.0, iroh-dns 1.4.2, iroh-relay 1.4.2 (the path
crate), noq, noq-proto and noq-udp 1.5.0, netwatch 0.20.2 and netdev
0.47.0. `cargo tree -p pod-iroh -i iroh-relay -e normal` shows
`vendor/iroh-relay`; `Cargo.lock` has no `[[patch.unused]]`.
`version::IROH` is `1.4.2`. The relay names and the pkarr URL of iroh
1.4.2, the source of iroh-dns 1.4.2 and the QUIC defaults of noq-proto
1.5.0 are the same as the readings of 1.4.1 and 1.4.0. `cargo test -p
pod-iroh --all-features`: 72 passed (the relay, the SOCKS5 and CONNECT
proxies and pkarr on the loopback). `cargo clippy -p pod-iroh` is clean;
the plant `iroh-socks5-sends-the-name` still applies and is caught.
`cargo deny` is not installed in the session; `deny.yml` runs it in CI.
Release notes: `pod-iroh`: iroh 1.4.2, with the fork of iroh-relay 1.4.2
(PodUtils/podssh#341).

# T-1103: `pod-iroh`: iroh 1.5 and iroh-relay 1.5, past the range of the crate (T-1102)

**Source:** T-1102 (a finding of the carry of the fork).
**Category:** chore
**Milestone:** M4
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

crates.io has `iroh` 1.5.0, `iroh-relay` 1.5.0, `iroh-dns` 1.5.0,
`iroh-base` 1.5.0 and `noq` 1.5.0. `pod-iroh` keeps `iroh` and `iroh-dns`
in `>=1.4.2, <1.5`, because iroh keeps the API of `unstable-net-report`
out of its semver (call C905). iroh 1.5.0 asks for `iroh-relay ^1.4.2`, so
a consumer that moves to iroh 1.5.0 can resolve `iroh-relay` 1.5.0 from
crates.io past the fork, or build the fork 1.4.2 under iroh 1.5.0, a pair
that no test of podcrates runs.

## Premise

- Read: the dependencies of `iroh` 1.5.0 and `iroh-relay` 1.5.0 in the
  crates.io index.
- Not measured: the changes of the API of `unstable-net-report` and of the
  relay dial between 1.4.2 and 1.5.0; the patch against iroh-relay 1.5.0.

## Approach

Read the changes of iroh 1.5.0 (the net report, the endpoint builder, the
relay client). Carry `vendor/iroh-relay` to 1.5.0 by the procedure of
`crates/pod-iroh/README.md`, "The fork of iroh-relay"; move the ranges of
`iroh` and `iroh-dns` to `>=1.5.0, <1.6`; read again the values of
`src/tuning.rs` in the noq-proto that iroh 1.5.0 builds. Tell the
consumers in the COORD issue of the next tag.

## Prove

```sh
cargo tree -p pod-iroh -i iroh-relay -e normal
cargo test -p pod-iroh --all-features
sh scripts/plant-each.sh iroh-socks5-sends-the-name
```

# T-1129: `pod-ws`: a refusal of the proxy from a dialer of the caller loses the stage Proxy, and its line names the target two times (PodUtils/podnet#707, ask 5)

**Source:** PodUtils/podnet#707 (COORD-115 of podnet, PodUtils/podnet@T-521, PodUtils/podnet@T-522), ask 5.
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The ask of podnet: "`crates/pod-ws/src/way.rs` (`dial_error`): keep the stage Proxy of a dialer failure whose cause is not a `DialError`, for a refusal of the proxy (403, 407, 502). Also strip a lead `TARGET through the proxy P`, as `connecting to` and `could not connect to` are stripped. Test: `Failure::caused(Stage::Proxy, Class::Refused, ...)` gives a `ConnectError` with the stage Proxy; the text "x:443 through the proxy http://p:1: refused" names x:443 once."

podnet's dialer gives a refusal of the proxy as a `Failure` at the stage Proxy, with the text `HOST:PORT through the proxy URL: ...` and its own error as the cause. `dial_error` makes each failure that is not at the stage Dns a `DialError::Connect`: the stage becomes Connect, and the line reads `could not connect to x:443: x:443 through the proxy http://p:1: refused`.

## Premise

- Read: PodUtils/podnet#707; `crates/pod-ws/src/way.rs` (`dial_error`, `connect_error`); `crates/pod-proxy/src/error.rs` (`DialError`, `stage_and_class`); podnet's dialer at `cabe7b8` (PodUtils/podnet:src/egress.rs, `failed` and `dial_failure`).
- Not measured: podnet's line after the fix (its test `tests/error_words.rs`).

## Approach

`dial_error` reads a lead `TARGET through the proxy P: ` of the text: the name of the proxy and the detail. A failure at the stage Proxy with no `DialError` cause becomes the `DialError` of its class: `ProxyRefused` with the status 0 (no status is known) for Refused, `ProxyProtocol` for Protocol, else `ProxyUnreachable`. The `Display` of `ProxyRefused` with the status 0 has no status, and a proxy with no name is "the proxy". The stage and the class of each stay Proxy and the class of the failure. A test gives `connect_error` the failure of the ask.

## Prove

```sh
cargo test -p pod-ws --all-features --lib way::tests
cargo test -p pod-proxy --all-features
```

## Done

2026-11-24, 67980e7. `dial_error` reads a lead `TARGET through the proxy P: `. A failure at the stage Proxy with no `DialError` cause becomes `ProxyRefused` with the status 0 (Refused), `ProxyProtocol` (Protocol) or `ProxyUnreachable`. The `Display` of `ProxyRefused` with the status 0 has no status, and `pod-relay` says "the proxy refused" for it. The line now: `the proxy http://p:1 refused CONNECT x:443: refused`, at (Proxy, Refused). Test: `crates/pod-ws/src/way.rs::a_refusal_of_the_proxy_keeps_its_stage` (failed before the fix: `(Connect, Unavailable)`). `sh scripts/agent/check.sh`: each step 0.

# T-1130: `pod-proxy`: the line of a proxy that does not answer names the proxy two times (PodUtils/podnet#707, ask 6)

**Source:** PodUtils/podnet#707 (COORD-115 of podnet, PodUtils/podnet@T-522), ask 6.
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The ask of podnet: "`crates/pod-proxy/src/stream.rs` `ProxyUnreachable` keeps "could not connect to P" as its detail, so the line reads "could not reach the proxy P: could not connect to P: ...". Strip that lead. Test: the text of a refused proxy names it once."

## Premise

- Read: PodUtils/podnet#707; `crates/pod-proxy/src/stream.rs` (`open_proxy`); `crates/pod-proxy/src/error.rs`; podnet's second path (`proxy_named_once` of PodUtils/podnet:src/egress.rs).
- Not measured: podnet's line after the fix.

## Approach

`open_proxy` keeps the detail of a `DialError::Connect` to the proxy without its lead `could not connect to TARGET` (`pod_core::failure::strip_lead`). The `Display` of `ProxyUnreachable` also strips a lead `could not connect to P` from a detail that another dialer made. A test dials through a port where nothing listens and counts the proxy in the line.

## Prove

```sh
cargo test -p pod-proxy --all-features --test dial an_unreachable_proxy_is_named
cargo test -p pod-proxy --all-features --lib error::tests
```

## Done

2026-11-24, 67980e7. `open_proxy` strips the lead `could not connect to TARGET` from the detail of a `DialError::Connect`, and the `Display` of `ProxyUnreachable` strips `could not connect to P` from the detail of another dialer. The line now: `could not reach the proxy 127.0.0.1:P: Connection refused (os error 111)`. Tests: `crates/pod-proxy/tests/dial.rs::an_unreachable_proxy_is_named` (failed before the fix: 2 names of the proxy) and `error::tests::the_words_of_a_dial_error_are_said_once`. The expected line of `api::tests::each_base_failed_at_one_hop_names_it_once` of `pod-warp` names the proxy once. `sh scripts/agent/check.sh`: each step 0.

# T-1131: `pod-dns`: the line of a name that does not resolve holds the list of each resolver of DNS over HTTPS, 567 bytes and more (PodUtils/podnet#707, ask 7)

**Source:** PodUtils/podnet#707 (COORD-115 of podnet, PodUtils/podnet@T-522, PodUtils/podnet#698), ask 7.
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The ask of podnet: "`crates/pod-dns/src/lookup.rs` (`fold`): one line of at most 200 bytes for a name that does not resolve: one phrase for each source (the system resolver, DoH), and the full list at debug. Test: the failures of #698 give a line under 200 bytes (the `bench` line is 631 bytes now)."

The line of PodUtils/podnet#698 in a lab with no network out:

```text
could not resolve nowhere.invalid: system resolver: failed to lookup address information: Try again; DNS over HTTPS: 4 resolvers (2606:4700:4700::1111, 2606:4700:4700::1001, 2001:4860:4860::8888, 2001:4860:4860::8844): could not connect to [the address of the resolver]:443: Address family not supported by protocol (os error 97); 4 resolvers (1.1.1.1, 1.0.0.1, 8.8.8.8, 8.8.4.4): could not connect to t...
```

## Premise

- Read: PodUtils/podnet#707 and #698; `crates/pod-dns/src/lookup.rs` (`over`, `fold`); `crates/pod-dns/src/order.rs` (`resolve_in`); `crates/pod-dns/src/resolver.rs`.
- Not measured: the line of podnet in its lab after the fix.

## Approach

`fold` gives one phrase: `each of N resolvers failed: CAUSE` when each failed alike, else `N resolvers failed, M with: CAUSE` with the cause of the most. A cause loses the address of its resolver (`could not connect to [A]:443: X` is `could not connect: X`) and the `(os error N)` of the system. `over` logs the full list at debug. `resolve_in` joins the phrase of each source in at most 160 bytes: the longest phrase is cut first (`pod_core::text::clip`), and the full line goes to debug. A test gives the failures of #698, of a lab and of a host whose proxy re-signs TLS, and measures the line of `DialError::Resolve`.

## Prove

```sh
cargo test -p pod-dns --all-features --lib
```

## Done

2026-11-24, 67980e7. `fold` gives one phrase (`each of N resolvers failed: CAUSE`, or `N resolvers failed, M with: CAUSE`), with no address and no `(os error N)`; `each` gives the full list at debug. `resolve_in` joins the phrases in at most 160 bytes (`fit`, `pod_core::text::clip`). The line of #698 now (195 bytes): `could not resolve nowhere.invalid: system resolver: failed to lookup address information: Try again; DNS over HTTPS: 8 resolvers failed, 4 with: could not connect: Address family not supported...`. Test: `crates/pod-dns/src/lookup.rs::a_name_that_does_not_resolve_gives_one_short_line` (failed before the fix: 472 bytes). The plant `dns-each-resolver-said-alone` patches the new `fold`. `sh scripts/agent/check.sh`: each step 0.

# T-1516: `pod-derp`: the derper tests read `PODSSH_DERP_TEST_*`, and the adoption map says that podnet has no DERP road (PodUtils/podnet#783)

**Source:** PodUtils/podnet#783 (COORD-138 of podnet, PodUtils/podnet@T-128; author MEMBER).
**Category:** chore
**Milestone:** M4
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The ignored tests `through_derper_*` of `crates/pod-derp/tests/derp_link.rs`
read `PODSSH_DERP_TEST_SERVER` and `PODSSH_DERP_TEST_CA`: the names of one
consumer, so another consumer cannot run them with names of its own. The
README of `pod-derp` (section "podnet" of the adoption map) says that
podnet has no DERP road; podnet takes the crate at `v0.86.0` (its T-128:
`podnet derp node`, `derp connect`, `send --derp`).

## Premise

- Read: the body of PodUtils/podnet#783; `crates/pod-derp/tests/derp_link.rs`;
  `crates/pod-derp/README.md`. No workflow of podcrates sets the two names
  (`grep -rn PODSSH_DERP .github scripts`).

## Approach

Rename the two variables to `POD_DERP_TEST_SERVER` and `POD_DERP_TEST_CA`
in the test and the README; write podnet's road in the adoption map, and
the fact of podnet about the leaf certificate of a derper
(`basicConstraints=critical,CA:FALSE`).

## Prove

```sh
cargo test -p pod-derp --all-features
```

## Done

2026-11-26. `derp_link.rs` reads `POD_DERP_TEST_SERVER` and
`POD_DERP_TEST_CA`; the README names them, the fact of the leaf
certificate, and podnet's DERP road. The tests that run with no derper
pass. Commit: see `git log` for T-1516.

# T-1432: `pod-egress`: the stand-ins of the second proxy over TLS, of a wstunnel server and of VLESS, Trojan, Shadowsocks and Wisp servers, behind the feature `standin` (PodUtils/podnet#779, ask 2; PodUtils/podnet#782, ask 2)

**Source:** PodUtils/podnet#779 (COORD-134 of podnet), ask 2; PodUtils/podnet@T-126; author MEMBER. PodUtils/podnet#782 (COORD-137 of podnet), ask 2; PodUtils/podnet@T-127; author MEMBER
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The test servers of `pod-egress` (a forward proxy over TLS, a wstunnel
server) are in its `tests/` folder, so a consumer cannot use them. podnet
wrote small stand-ins of its own for its tests
(`PodUtils/podnet:src/networks/road_stand.rs`). The ask: a feature
`standin` (as `pod-devtunnel` has) that exports a `via` stand-in (HTTP/1.1
and HTTP/2 CONNECT over TLS, with Basic) and a wstunnel stand-in (with the
path prefix), so that podnet drops its copies.

PodUtils/podnet#782, ask 2: "The tests of `pod-egress` have servers of
VLESS, Trojan, Shadowsocks and Wisp (`tests/*_support`). A consumer cannot
use them. Ask: a feature `standin` (or `pod-testkit`) with these servers,
for `[dev-dependencies]` only. podnet then drops
`src/networks/servers_stand.rs` and `src/networks/road_stand.rs`."

## Premise

- Read: `crates/pod-egress/tests/vless_support/mod.rs` (a VLESS server over
  a WebSocket and over XHTTP on HTTP/2 and HTTP/1.1, with a fixed made-up
  ID), `crates/pod-egress/tests/trojan_support/mod.rs` (Trojan in TLS and in
  a WebSocket, with its fallback), `crates/pod-egress/tests/wisp_support/mod.rs`
  (a Wisp server of version 1 and 2 that counts the credit), and the
  servers of `crates/pod-egress/tests/shadowsocks.rs` and
  `crates/pod-egress/tests/shadowsocks_2022.rs` (the specification's own
  ciphers: AES-GCM, ChaCha20-Poly1305, MD5, HKDF-SHA1, BLAKE3).
- Read: `crates/pod-egress/tests/via.rs` (the proxy over TLS: ALPN, a
  mode, Basic, HTTP/1.1's and h2's CONNECT, the bridge of a stream of
  HTTP/2) and `crates/pod-egress/tests/wstunnel.rs` (the reading of
  wstunnel 11's `handle_tunnel_request`, the answers of 9 and of another
  server, the masks, the Close).
- Read: `crates/pod-devtunnel/Cargo.toml` and
  `crates/pod-devtunnel/src/standin.rs`: the feature `standin`, a module of
  the library, and `required-features` on each test that uses it.
- The certificates of the tests come from
  `crates/pod-tls/tests/common/cert_for_test.rs`, a file of tests. The
  stand-in takes the caller's `rustls::ServerConfig`, so the library mints
  no certificate and adds no dependency.

## Approach

1. `pod_egress::standin` (feature `standin`): `standin::target` (a version
   line, then an echo); `standin::via` (`start` with a `Plan`: the ALPN,
   the `Mode`, the Basic credentials; `Seen`); `standin::wstunnel`
   (`start` with a `Wst`, over TLS or not; `Seen`); `standin::vless`,
   `standin::trojan`, `standin::wisp`, and `standin::shadowsocks` (with the
   feature `shadowsocks` too). A server of TLS takes the caller's
   `rustls::ServerConfig`. The ciphers of the Shadowsocks servers become
   optional dependencies of the feature.
2. The tests use the exported stand-ins; `tests/*_support` keep the
   certificates of the tests, the way out and the targets. The test names
   stay.
3. README of the crate; `crates/pod-egress/SOURCES` names the new place of
   the files that came from podssh.

## Prove

```sh
cargo test -p pod-egress --all-features
cargo clippy -p pod-egress --all-targets --all-features -- -D warnings
```

## Done

2026-11-26. Commit: see `git log` for T-1432.

- The feature `standin` of `pod-egress` exports `pod_egress::standin`
  (`crates/pod-egress/src/standin.rs`): `echo` and `BANNER`; `Stand<S>`
  (`port`, `seen`, `detach`); `standin::via` (a forward proxy over TLS:
  HTTP/1.1's CONNECT and h2's, `Plan { alpn, mode, credentials }`, `Mode`,
  Basic and `407`); `standin::wstunnel` (`Wst`, `Answer`: wstunnel 11, 10.0
  with the signature, 9, another server; the path prefix, the masks, the
  Close); `standin::vless` (a WebSocket at `/ws`, XHTTP on HTTP/2 and on
  HTTP/1.1, the made-up `ID`); `standin::trojan` (TLS alone and a
  WebSocket at `/tr`, the fallback, the made-up `PASSWORD`);
  `standin::wisp` (version 1 and 2, the credit counted, `grant`); and with
  the feature `shadowsocks` too, `standin::shadowsocks` (`aead`, `sip022`,
  `Spec`, `evp`, `Answer`). A server of TLS takes the caller's
  `rustls::ServerConfig`; the crate mints no certificate. AES-GCM and
  ChaCha20-Poly1305 are new optional dependencies of the feature.
- The tests of `via`, `wstunnel`, `vless`, `vmess`, `trojan`, `wisp`,
  `shadowsocks` and `shadowsocks_2022` use the exported stand-ins, with the
  same test names; `tests/*_support` keep the certificates of the tests,
  the way out and the targets. `tests/vless_support/xhttp.rs` and
  `xhttp1.rs` moved to `crates/pod-egress/src/standin/vless/`, and
  `crates/pod-egress/SOURCES` names them there.
- Commands: `cargo test -p pod-proxy -p pod-egress --all-features`: 491
  passed, 0 failed. `cargo clippy -p pod-proxy -p pod-egress --all-targets
  --all-features -- -D warnings` and `cargo fmt --all --check`: no finding.
  The plants of the moved tests (`egress-via-*`, `egress-wstunnel-*`,
  `egress-vless-*`, `egress-trojan-dial-untyped`, `egress-wisp-*`,
  `egress-shadowsocks-dial-untyped`): 10 caught, 0 missed;
  `scripts/plants/egress-via-dial-untyped.patch` now names
  `pod_core::Stage`, as `via/mod.rs` no longer imports `Stage` (the TLS
  hop moved to `pod-proxy`, T-1431). `sh scripts/plant-each.sh --apply`: 0
  stale.
- For podnet: a `[dev-dependencies]` entry
  `pod-egress = { ..., features = ["standin", "shadowsocks"] }` replaces
  `PodUtils/podnet:src/networks/servers_stand.rs` and
  `PodUtils/podnet:src/networks/road_stand.rs`. Not measured: a run of the
  tests of podnet on the exported stand-ins.

# T-1433: `pod-iroh`: an `https` proxy goes to iroh's relay dial by its address, so TLS checks the proxy's certificate for the address and not the name

**Source:** T-1431 (a finding of the work on PodUtils/podnet#779)
**Category:** defect
**Milestone:** M4
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

After T-1431, `Proxy::parse` takes `https://HOST`, and an environment
gives such a proxy to `pod-iroh`. `relay_proxy` in
`crates/pod-iroh/src/endpoint.rs` resolves the proxy host and gives iroh
`proxy.url_at(ADDRESS)`. The fork of iroh-relay
(`vendor/iroh-relay/src/client/tls.rs`, `dial_url_proxy`) makes TLS to a
proxy whose scheme is not `http` with the server name of the URL, which is
the address, and with the relay's TLS connector. A proxy certificate for
its name fails, with words that do not name the cause.

## Premise

- Read: `crates/pod-iroh/src/endpoint.rs` (`relay_proxy`, `sendable`) and
  `vendor/iroh-relay/src/client/tls.rs` (`dial_url_proxy`).
- Not measured: a run of the iroh road through an HTTPS proxy.

## Approach

`relay_proxy` refuses a proxy of `ProxyKind::Https` with words (the iroh
road takes an `http`, `socks5` or `socks5h` proxy), as it refuses a proxy
that is not on TCP; or it gives iroh `proxy.url()` with the name and a
resolver pin. A test of `relay_proxy` with `https://proxy.example`.

## Prove

```sh
cargo test -p pod-iroh --all-features
```

# T-1505: `pod-acme`: the contacts of the account, sent in newAccount, a refused contact named

**Source:** PodUtils/podnet#780 (COORD-135 of podnet), ask 1; PodUtils/podnet@T-129; author MEMBER.
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The text of the ask: "`Client::account` sends `termsOfServiceAgreed` only, and `Acme::new` takes no contact. A CA (Let's Encrypt) sends the warnings of expiry and of revocation to the contact of the account. podnet cannot give `--acme-email`. Ask: an optional list of contacts (`mailto:` URIs, RFC 8555 section 7.3) on `Acme::new` or on the account step, sent in `newAccount`. A contact that the CA refuses (`invalidContact`, `unsupportedContact`) gives a failure that names the contact." podnet adds `--acme-email` at the tag that has this ask.

## Premise

- Read: `crates/pod-acme/src/client.rs` (`Client::account` sends `{"termsOfServiceAgreed": true}` alone), `crates/pod-acme/src/lib.rs` (`Acme::new`, `Acme::obtain`), `crates/pod-acme/src/failed.rs`; RFC 8555, sections 6.7 (`invalidContact`, `unsupportedContact`), 7.3 (`contact`) and 7.3.2 (the update of an account).
- Read: `PodUtils/podnet:crates/podnet-serve/src/acme.rs` at its default branch: it calls `Acme::new` and matches each variant of `Failed` in `failure`. A new variant of `Failed` breaks its build.
- An account that exists keeps its contacts at newAccount (RFC 8555, section 7.3.1): a contact that a user adds later needs an update of the account.

## Approach

1. `contact::Contact` (crate root `Contact`): `parse` of a `mailto:` URI and `mailto` of an address; each checks one address, no `,`, no `?` header, no `%`, the characters of RFC 5322 atext, a domain of DNS labels, at most 254 characters; a refusal names the contact.
2. `Acme::with_contacts(Vec<Contact>)` and the field `Acme::contacts` (a builder: `Acme::new` does not change); `Client::contacts`. `Client::account` sends `contact`; on 200 (the account exists) with other contacts, it posts the update to the account URL.
3. A refusal of type `invalidContact` or `unsupportedContact` names the contact (the one that the problem names, else each), as `Failed::Ca`.
4. Unit tests of the check and of the refusal; the whole order against the stand-in of T-1506.

## Decision

No new variant of `Failed`: podnet's `failure` matches each variant, and a new one breaks its build. The CA's refusal of a contact is `Failed::Ca` (`Origin`/`Unavailable`, exit 69 in podnet), with the contact in its words. A bad contact is refused before the request by `Contact::parse` and `Contact::mailto`, whose `String` error the consumer maps to a usage error.

## Prove

```sh
cargo test -p pod-acme --all-features
sh scripts/plant-each.sh net780-t1505-contact-not-sent net780-t1505-contact-two-addresses
```

## Done

2026-11-26. Commit: see `git log` for T-1505.

- `contact::Contact` (crate root `Contact`): `parse("mailto:ADDR")` (the scheme in any case, kept in lower case), `mailto("ADDR")`, `as_str`, `address`, `Display`; `MAX_ADDRESS` (254). A refusal reads `the contact "TEXT": WHY`, for example `give one address alone: a contact has no ','`, `a contact has no header: remove '?' and the text after it`, `the character '%' is not allowed before '@'`.
- `Acme::with_contacts(Vec<Contact>)` and the field `Acme::contacts` (none by default); `Client::contacts`. `Client::account` sends `contact` with `termsOfServiceAgreed`; on 200 (the account exists) whose account object holds other contacts, it posts `{"contact": [...]}` to the account URL. With no contact, nothing is sent and the account keeps its contacts.
- A refusal of type `invalidContact` or `unsupportedContact` is `Failed::Ca` with the words "the account: the CA refuses the contact mailto:ADDR: invalidContact: DETAIL" (the contact that the detail names, else the one contact, else "one of the contacts A, B"). The line `acme.account` of a new account ends with ", contact mailto:ADDR".
- Tests: `contact::tests::a_mailto_uri_of_one_address_is_a_contact`, `contact::tests::a_contact_that_a_ca_refuses_is_refused_by_name_before_the_request`, `client::tests::a_contact_that_the_ca_refuses_is_named_in_the_failure`, `client::tests::an_account_holds_its_contacts_in_any_order`; against the stand-in of T-1506: `one_whole_order_gives_a_chain_of_the_ca_and_the_account_has_its_contact`, `a_contact_that_the_ca_refuses_ends_the_order_with_a_failure_that_names_it`, `an_account_that_exists_takes_the_contacts_that_the_user_adds_later` (`crates/pod-acme/tests/standin.rs`).
- Plants: `net780-t1505-contact-not-sent` (the account step sends no contact) and `net780-t1505-contact-two-addresses` (a `,` passes the check); each caught.
- podnet: no change to build at the tag (no new variant of `Failed`, `Acme::new` as before). For `--acme-email`: `Contact::mailto(email)` (its error is a usage error, exit 64), then `Acme::new(...)?.with_contacts(vec![contact])`.

# T-1506: `pod-acme`: a stand-in of an ACME CA behind the feature `standin`

**Source:** PodUtils/podnet#780 (COORD-135 of podnet), ask 2; PodUtils/podnet@T-129; author MEMBER.
**Category:** feature
**Milestone:** M4
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The text of the ask: "The crate has no stand-in of a CA. podnet tests the kept certificate, the modes, the challenge on the listener and a failed first order, but not one full order in `cargo test`. podnet runs Pebble in `live.yml` for the full order. Ask: a stand-in of a directory, an account, an order, a TLS-ALPN-01 challenge (the stand-in connects to the listener of the caller with ALPN `acme-tls/1` and checks the certificate of RFC 8737), a finalization and a certificate, behind a feature (`standin`), as `pod-derp` and `pod-egress` do. A consumer turns it on in `[dev-dependencies]` only." T-129 of podnet stays partial until its live step with Pebble passes.

## Premise

- Read: the stand-ins of `crates/pod-devtunnel/src/standin.rs` and `crates/pod-cloudflare/src/standin.rs` (a test CA of rcgen, TLS on the provider of the caller), the server of `crates/pod-tls/src/server.rs` and its `Served`, the request head of `pod_proxy::http::request`; RFC 8555, sections 6, 7.1 to 7.4 and 7.4.2; RFC 8737, sections 3 and 4.
- No test of podcrates runs a whole order (T-637: Pebble in CI, open).
- `rcgen` has no crypto feature here: each signature of the test CA is the P-256 `Key` of the crate. x509-parser has no `verify` feature: the stand-in checks the signature of a CSR with `p256`.

## Approach

1. The feature `standin` (tokio `net`, `io-util`, `rt`, `sync`). `src/standin.rs` with `Plan`, `Seen`, `Standin` (`start(provider, plan)`, `directory`, `ca_pem`, `trust`, `http`, `seen`), and the modules `ca`, `serve`, `signed`, `flow`, `validate`, each under 500 lines.
2. The checks of a CA: the JWS (ES256, its signature, the nonce, the `url`, `jwk` for newAccount alone, the `kid` of a known account), each object of its own account, the contacts (`unsupportedContact`, `invalidContact` for a contact that the plan names), the order state (`orderNotReady`), the CSR (its signature, its names, a key that is not the account's), TLS-ALPN-01 of RFC 8737 (ALPN `acme-tls/1` alone and chosen, SNI, one dNSName, a critical `acmeIdentifier` with the SHA-256 of the key authorization), and a chain signed by its test CA.
3. Tests (`tests/standin.rs`, `tests/standin_checks.rs`, feature `standin`): one full order through `Acme::obtain`, the refused contact, the update of the contacts, and a refusal of each check. Plants: the stand-in skips the `acmeIdentifier` check, the client omits it, the stand-in skips the JWS signature.

## Prove

```sh
cargo test -p pod-acme --all-features --test standin --test standin_checks
sh scripts/plant-each.sh net780-t1506-standin-skips-identifier net780-t1506-challenge-no-identifier net780-t1506-standin-skips-signature
```

## Done

2026-11-26. Commit: see `git log` for T-1506.

- The feature `standin` (tokio `net`, `io-util`, `rt`; no new dependency). `standin::Standin` (`start(provider, plan)` on the caller's runtime and provider, `directory`, `ca_pem`, `trust()`, `http()`, `seen()`; the task ends at its drop), `Plan { validate_at, refused_contacts, bad_nonces, days }` (`Plan::new(validate_at)`), `Seen { accounts, refusals, validations, issued, bad_nonces }`; the modules `ca` (the test CA: rcgen with the crate's P-256 `Key`; a CSR checked by x509-parser and `p256`), `serve` (HTTPS, HTTP/1.1 with keep-alive), `signed` (the JWS checks, `Problem`), `flow` and `flow/orders` (the resources), `validate` (TLS-ALPN-01: `validate`, `check_certificate`, `ACME_IDENTIFIER`). The largest file has 340 lines.
- The checks: ES256 and its signature, a nonce that the stand-in gave and no request used, the `url`, `jwk` at newAccount alone, the `kid` of a known account, the type `application/jose+json`; the terms; each contact (`unsupportedContact`, `invalidContact`, and `invalidContact` for each contact of the plan); the owner of each object; DNS names alone and no wildcard (`rejectedIdentifier`); `orderNotReady`; the CSR (its signature, its names against the order, a P-256 key that is not the account's: `badCSR`); TLS-ALPN-01 in a task after the client's POST (ALPN `acme-tls/1` alone and chosen, the name as SNI, one dNSName, one critical `acmeIdentifier` with the SHA-256 of the key authorization; the handshake signature checked with the key of the certificate, as webpki does not read a certificate with a critical acmeIdentifier). The authorization lists `http-01` too, and refuses it.
- Tests (`crates/pod-acme/tests/standin.rs`, `crates/pod-acme/tests/standin_checks.rs`, each with `need_loopback!`): `one_whole_order_gives_a_chain_of_the_ca_and_the_account_has_its_contact` (two good nonces refused and sent again, the chain verified for the name against the test CA alone, a second order on the same account), `the_stand_in_refuses_a_certificate_of_tls_alpn_01_that_rfc_8737_does_not_allow`, `the_stand_in_checks_each_jws_as_a_ca_does`, `the_stand_in_issues_for_a_csr_of_the_names_of_the_order_and_a_key_of_its_own`.
- `cargo test -p pod-acme --all-features`: 30 unit tests, 3 in `standin`, 3 in `standin_checks`, all passed. `cargo clippy -p pod-acme --all-targets` with `--all-features` and with no feature, `-D warnings`: clean. `cargo fmt --all --check`: clean.
- Plants: `net780-t1506-standin-skips-identifier` (the stand-in skips the digest of the acmeIdentifier), `net780-t1506-challenge-no-identifier` (the client's certificate has no acmeIdentifier: the whole order fails), `net780-t1506-standin-skips-signature` (the stand-in takes a JWS whose signature does not verify); each caught.
- podnet: `pod-acme = { git = "https://github.com/PodUtils/podcrates", tag = "vX.Y.Z", features = ["standin"] }` in the `[dev-dependencies]` of `podnet-serve`; a test binds its listener (`acceptor` with `HTTP1` and `ACME_TLS_1`, `serve_tls`), starts `Standin::start(provider, Plan::new(listener_addr))`, and gives `Standin::directory` and `Standin::http()` to `acme::open`.
- Not done: a whole order against Pebble in podcrates stays T-637.
