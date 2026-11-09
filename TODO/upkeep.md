This file holds the entries of milestone M5: the work that keeps the crates
in step with their consumers.

# T-040: The trial of podvm

**Source:** `docs/design.md`, section 8, step 7; `docs/decisions.md`,
call C16.
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** partial

## Problem

podvm uses the blocking facade of `pod-relay` and `pod-term`. Its trial
needs its repository.

## Premise

- PodUtils/podvm is public: this session reads it through the mirror and
  a clone, which a trial needs; it writes nothing there. podvm takes
  `pod-term`, `pod-tls`, `pod-proxy`, `pod-relay` (feature `blocking`),
  `pod-services`, `pod-egress`, `pod-netstack` and `pod-todo`, in this
  order (PodUtils/podvm#194).

## Approach

adoption/podvm/: the commit, the patch; `adoption.yml` runs it. The first
trial is `pod-term` (podvm's line discipline), at podvm `0249527` (C21).

## Prove

```sh
gh workflow run adoption.yml -f consumer=podvm -f crate=pod-relay
```

## Start condition

podvm gave the commit of its rename (PodUtils/podvm#194: `0249527`). The
edition of podvm is not a condition: a crate of edition 2021 takes a
dependency of edition 2024; the patch raises podvm's `rust-version`.

## Done

2026-11-06, partial. The patch of the trial of `pod-term` (`784469a`), at podvm `0249527`:
`PodUtils/podvm:crates/podvm-ssh/src/session.rs` takes `pod_term::echo` and keeps its
supervisor; podvm's own 29 tests of the discipline pass on pod-term, with
two of them changed to pod-term's Home and End (the screen's cursor moves
with the line's); clippy of podvm's workspace, rustfmt, podvm's record
gate, the toolchains and the static binary `shell` pass
(`sh scripts/adoption.sh podvm pod-term`). podvm took `pod-term` at
`v0.3.0` in podvm `75ed306` (PodUtils/podvm#197): each line of the `.run`
passed there, `session_interactive` too (12 passed against sshd); the trial
leaves the directory of podvm.

2026-11-06, partial. The trial of `pod-tls` at podvm `b5473a2`
(the patch of the trial of `pod-tls`, `87a1338`, 8 files, 119 lines in, 148 out, with no
default feature: no tokio). `podvm-image` keeps its choice of the host
bundle, its words and its verifier for a registry named insecure; pod-tls
reads the bundle alone as a `CaFile`, else the compiled-in roots, and makes
the configuration over ring. `podvm-ssh` takes a `Context` with its host
bundles and `Context::tls_over_blocking`; the handshake comes before the
WebSocket upgrade, so `mux.rs` sets the write bound before TLS. podvm drops
`rustls-pemfile` and its own `webpki-roots`; the `pod-term` tag moves to
`v0.4.0`. One test changed: `garbage_server_name_is_refused_before_the_handshake`
expects pod-tls's words (`is not a valid server name`). Each step of
`sh scripts/adoption.sh podvm pod-tls` passed: podvm-ssh 101 tests,
podvm-image 23 (`tls`, `registry`, `transport`), podvm-complete 15 (`pkg`,
`reach`), clippy, rustfmt, the record gate, the toolchains, the static
`node` and `podvm`; `mux_two_client` runs only where sshd is. A loopback
check outside the patch gave the three results of the handshake: an
unknown issuer with the hint, TLS 1.3 to a private CA, a wrong name refused.
Release sizes, x86_64 musl: `podvm` 3281976 to 3314744 bytes (+32768),
`node` 1622872 to 1676120 (+53248), `operator` 1594264 to 1635224
(+40960). Changes of behaviour: `podvm-ssh` trusts the first usable host
bundle and the compiled-in roots together (before: each host bundle, the
compiled-in roots only when none loaded); a bundle with a block that is not
base64 is passed over (pod-tls's PEM reader refuses the whole file). Gap of
pod-tls: `Words` has no form for a consumer whose only place of a CA adds
to the roots (`podvm-ssh`: the host bundle); its hint says "replace". Left:
the trial of `pod-proxy` (the synchronous face, T-063), then the other
crates of the order of PodUtils/podvm#194.

podvm took `pod-tls` 0.4.0 with no tokio in podvm `b917f97`
(PodUtils/podvm#203): each line of the `.run` passed there, `mux_two_client`
too (15 against sshd); podvm accepts the three changes of behaviour (its
call C9). The trial leaves the directory of podvm. The next hand-offs start
at podvm `b917f97` or later: podvm splits its files to 500 lines
(PodUtils/podvm@T-1674), and the files of M6 are split already.

2026-11-06, partial. The trial of `pod-proxy` (feature `blocking`) at
podvm `f9eae80` (the patch of the trial of `pod-proxy` (`27f8688`), 6 files, 277 lines in,
3 out). `podvm-ssh` takes `pod-proxy` at `v0.4.0`. `transport::Dialer`
gets `from_environment` and `with_environment`: when a proxy variable is
set, `transport/egress.rs` makes a `blocking::Dialer` with
`Settings::new("podvm", ProxyChoice::FromEnvironment(..))` and
`resolve::System`; `proxy_at` decides, and `stream` carries a dial through
the proxy. A direct target (the loopback, `NO_PROXY`) keeps podvm's direct
dial, with its bound on the resolution and each address in turn. With no
proxy variable, no runtime starts. The relay legs (`dial_ws`: the node and
the operator) use it; the `proxy` binary (an SSH `ProxyCommand`) stays
direct. An unusable proxy variable now fails the relay dial. Six new tests
(`transport/egress/tests.rs`): a stand-in CONNECT proxy on the loopback for
`HTTPS_PROXY` and `ALL_PROXY`, the loopback and `NO_PROXY` around it, an
unusable setting refused, no runtime with no proxy. Each step of
`sh scripts/adoption.sh podvm pod-proxy` passed: podvm-ssh 107 tests,
clippy, rustfmt, the record gate, the toolchains, the static `node`,
`operator` and `podvm`; `mux_two_client` runs only where sshd is. tokio
comes to `podvm-ssh` only (`pod-core`, `pod-tls`, `pod-proxy`); `podvm`
has none. Release sizes, x86_64 musl: `podvm` 3314744 bytes, no change;
`node` 1672024 to 1913816 (+241792); `operator` 1635224 to 1864728
(+229504). `podvm-image` and `podvm-complete` keep ureq 2 and podvm's own
proxy rules: `blocking::Client` reads each body into memory, and a blob of
a registry is streamed to disk. Gaps of pod-proxy: no streamed body in
`blocking::Client`; the drop of a `blocking::Dialer` waits for a lookup of
the system resolver that its timeout left behind (`Runtime` drop waits for
`spawn_blocking`); a `blocking::Stream::Exec` has no read limit. Left: the
take of this patch by podvm, the registry client, then the other crates of
the order of PodUtils/podvm#194.

podvm took `pod-proxy` (feature `blocking`) for the relay legs in podvm
`b1138c2` (PodUtils/podvm#205): each line of the `.run` passed there,
`mux_two_client` too; podvm accepts the size, the refusal of an unusable
variable and of a proxy at a command (its call C10). podvm's registry and
image pulls can move to `blocking::Client::send_streamed` (T-069).

podvm moved to v0.5.0 in podvm `cec4bcb` (PodUtils/podvm#207): its podcrates
dependencies name `v0.5.0`, and podvm-ssh's `Words` set `replace_roots`
empty and `add_roots` to the host bundle (T-068). podvm asks podcrates for
the next trial: podvm-image's registry transport and the guest image fetch
on `blocking::Client::send_streamed`, at podvm `cec4bcb` or later, with a
pull through a local CONNECT stand-in and its `NO_PROXY` bypass.

2026-11-06, partial. The trial of `pod-proxy`'s streamed client in podvm
(PodUtils/podvm@T-1677, PodUtils/podvm#207) at podvm `f3eccd8`
(the patch of the trial of the registry (`8b30023`), 16 files, 896 lines in, 353
out). `podvm-image` takes `pod-proxy` (feature `blocking`) and `pod-core` at
`v0.5.0`. The new module `net.rs` carries each HTTP request of podvm through
`blocking::Client::send_streamed`: the registry (manifests, tags, blobs, the
token realm), the Windows guest image fetch of `podvm` and the mirror probe
of `podvm-complete` (`reach.rs`). Each blob streams through
`blocking::Streamed` into podvm's verifier and the staged file. The proxy is
the environment's by the rules of `pod-proxy`; podvm joins `NO_PROXY` and
`no_proxy` and pins `ip6-localhost` to `::1`, so its own rules hold, and
`Policy::use_proxy` goes. Each request takes podvm's TLS configuration by
`Trust::caller` (verified, or accept-any for a registry named insecure).
`net.rs` follows a redirect of a GET or a HEAD (five at most), keeps
`Authorization` on its host and refuses `https://` to `http://`: the crate
follows none. podvm names the host of a transport failure once
(PodUtils/podvm@T-1420): it takes the repeats of the host out of the words
of the crate (`could not connect to H: H: Connection refused`). ureq leaves
podvm. New tests: `net::` (the loopback and `NO_PROXY` decisions, a command
refused, redirects and the credential, a status) and `tests/proxy_pull.rs`
(a pull of the fixture of PodUtils/podvm@T-1531 through a stand-in CONNECT
proxy that `HTTPS_PROXY` names, by a name that only the proxy resolves, and
its `NO_PROXY` bypass, each in a child process). Changed tests: `registry::tests` (the origin, the refused port and
the lowercase causes through `net`), the loopback test moves from
`transport.rs` to `net::tests`, and `windows::setup`'s hung origin; only
ureq's types differ. Each step of `sh scripts/adoption.sh podvm
pod-proxy-registry` passed: podvm-image 173 tests, the Windows fetch,
store_gates, pkg:: and reach::, podvm-ssh 107 tests, clippy, rustfmt, the
runtime-state rows, the record gate, the toolchains, no ureq in the graph,
the static `node` and `podvm`; `mux_two_client` runs only where sshd is.
Release size of `podvm`, x86_64 musl: 3314744 to
3282072 bytes (-32672): tokio comes in (`pod-core`, `pod-tls`, `pod-proxy`,
`tokio-rustls` under `podvm-image`), and ureq, `url`, the ICU crates and
base64 0.22 go. Gaps of pod-proxy: no redirect; no keep-alive, so each
request is a new TCP and TLS connection; one limit for the dial and the head;
`HTTP_PROXY` is not read; the words of a dial failure repeat the target.
podvm's `docs/decisions.md` C10 ("never into the `podvm` binary") needs a
new call of podvm. Left: the take of this patch by podvm.

podvm took the registry patch in podvm `06f2427` (PodUtils/podvm#208):
each HTTP request of podvm goes through `blocking::Client::send_streamed`,
and ureq leaves; each line of the `.run` passed there, `mux_two_client`
too. podvm's call C10 now lets the private runtime of `pod-proxy` into its
binaries. podvm waits for T-075 and T-077 and the trial of `pod-hostprobe`.

2026-11-06, partial. The trial of `pod-hostprobe` in podvm at `b32b19c`
(`adoption/podvm/pod-hostprobe.patch`, T-055): each step of `sh
scripts/adoption.sh podvm pod-hostprobe` passed; `adoption.yml` runs podvm
again. podvm took the patch in podvm `8899c2e` (PodUtils/podvm#210).

# T-097: Ports and snapshot moves of the drift of `pod-core` and `pod-services`

**Source:** the red job `sources` of the `upstream` workflow (triage of GitHub #18 to #34)
**Category:** chore
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

`scripts/check-sources.py` reports changes of `pod-core` (2 blocks) and
`pod-services` (3 blocks). Four of the changes touch no value of the
data: a split, a call instead of a constant, a test that reads the
default relay from `pod-services`. One value may need a port: podnet
(T-068) uses `1.1.1.1:443` as the default of the TCP probe, and
`crates/pod-services/services/cloudflare-dns.toml` has no `tcp` endpoint
for it (it has `udp-1`, `udp-2` and `trace`).

## Premise

- Read: the report of the check with the clones of podssh, podnet and
  podvm at their `main`, and the data files.
- Not measured: a run of the check after the move.

## Approach

Decide the value: add an endpoint `tcp-1` with a source, or record that
the probe target is code of podnet. Then move the snapshot of the other
blocks (new blob ids and commit) in `crates/pod-core/SOURCES` and
`crates/pod-services/SOURCES`.

## Prove

```sh
python3 -I scripts/check-sources.py
cargo test -p pod-services
```

The first command runs with the clones and exits 0.

# T-098: The asks of cfb (GitHub #35) and githome (GitHub #36) go to their owners

**Source:** GitHub #35 and #36
**Category:** chore
**Milestone:** M5
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

GitHub #35 holds 84 asks of cfb, and GitHub #36 holds 12 asks of githome,
a forge CI runner host. Each ask goes to a crate of podcrates, to podssh,
podnet or podvm, or to a new project. Some asks need a ruling of the owner
(new projects, platforms other than Linux, signed releases).

## Premise

- Read: the bodies of #35 and #36, `docs/design.md` (section 4) and the
  records of the consumers.
- Not measured: the asks against the code of each repository.

## Approach

Triage each ask. Write an entry of podcrates for each ask that a crate
takes. Send each ask of a consumer to its session in a COORD issue. Write
each question for the owner in `TODO/PROGRESS.md`. Add each new crate name
to GitHub #1.

## Prove

```sh
cargo todo check
```

Each ask of #35 and #36 names its entry, its COORD issue or its question
in `TODO/issues.md`.

## Done

2026-11-09. Each of the 96 asks of #35 and #36 has its route in
`docs/research/fleet/asks.md`: an entry of podcrates (T-099 to T-120,
`62729c9`), a COORD issue to its consumer (#38 to podssh, #39 to podvm,
#40 to podnet), a new project of the owner's ruling, or a deferral of the
owner. The owner ruled each question on 2026-11-09 (`docs/decisions.md`,
`2ebbfef`). GitHub #1 has the new crate names. `cargo todo check` agrees.
