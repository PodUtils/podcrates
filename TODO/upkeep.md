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
(the patch pod-hostprobe.patch of adoption/podvm, in git at `5535a27`, T-055): each step of `sh
scripts/adoption.sh podvm pod-hostprobe` passed; `adoption.yml` runs podvm
again. podvm took the patch in podvm `8899c2e` (PodUtils/podvm#210).

# T-097: Ports and snapshot moves of the drift of `pod-core` and `pod-services`

**Source:** the red job `sources` of the `upstream` workflow (triage of GitHub #18 to #34)
**Category:** chore
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** done

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

## Done

2026-11-09. Each change was code only, and no value of `pod-services` or
`pod-core` changed: a split (podvm `f9eae80`), a call in place of a
constant (podnet `bf0c7ca`, `380d9a4`, `0293f91`), the `socks5h` scheme of
the tailscale proxy (podssh `ccfef9ee`), a test that reads the default relay
from `pod-services` (podssh `72fb94d`). The five blocks moved to the commit
that the check reads (main, or the parent of the take) with the new blob
ids. podnet's TCP probe of T-068 (`1.1.1.1:443`) is the endpoint `trace`
of `cloudflare-dns.toml`; its comment names the use. podnet keeps the
default in its own code (`src/cli/cmd/warp/args.rs`) until `pod-warp`
takes it (T-028). `python3 -I scripts/check-sources.py` with the clones of
podssh, podnet and podvm exits 0, and `cargo test -p pod-services` passes.

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

# T-168: Tests that bind a Unix socket under the temporary directory fail when its path is long

**Source:** T-020 (`sh scripts/agent/check.sh` with a `TMPDIR` of 75 bytes).
**Category:** defect
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

A Unix socket path has at most 107 bytes (`SUN_LEN`). With a `TMPDIR` of
75 bytes, the three tests of `crates/pod-gsr/tests/blocking.rs` fail at the
bind ("path must be shorter than SUN_LEN"). Other tests make socket files
under `std::env::temp_dir()` with long names too
(`crates/pod-gsr/tests/unix_loopback.rs`, `crates/pod-broker/tests/`,
`crates/pod-proxy/tests/egress.rs`, `crates/pod-guest/src/`).

## Premise

- Measured: the three tests of `blocking.rs` fail with that `TMPDIR` and
  pass with `/tmp`.
- Not measured: each other test that binds a Unix socket.

## Approach

Add to `pod-testkit` a socket path under a short directory: the temporary
directory when the path fits, else a new private directory under `/tmp`.
Each test that binds a Unix socket takes it. A test with a `TMPDIR` of 100
bytes proves it.

## Prove

```sh
TMPDIR=$(mktemp -d /tmp/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.XXXX) cargo test --workspace --all-features
```

## Done

2026-11-10, the commit of this entry. `pod_testkit::host::socket_dir` and
`socket_path` give a socket path under the temporary directory, or under
`/tmp` when that path is too long. The tests of `pod-gsr` (`blocking`,
`unix_loopback`), `pod-proxy` (`egress`), `pod-guest` (`dos`), `pod-term`
(`calibration`, the tmux socket) and `pod-broker` (`control`, `tunnel`;
its own copy is gone) take them. `pod-supervise` addresses its socket by
`/proc/self/fd` and needs no change. With a `TMPDIR` of 74 bytes under
`/tmp`, `cargo test --workspace --all-features` passes (exit 0).

# T-199: `pod-e2e`: snow on the line of `chacha20poly1305` 0.11

**Source:** T-021 (`cargo deny check bans`).
**Category:** chore
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

snow 0.10.0, the newest release on crates.io on 2026-11-11, takes
`chacha20poly1305` 0.10 and `chacha20` 0.9. The workspace takes 0.11 of
each (`pod-tls`, `pod-session`). A binary with `pod-e2e` thus carries two
ChaCha20-Poly1305 stacks, and `cargo deny check bans` gives two warnings
of a duplicate more. podssh had the same tree before its take.

## Premise

- Measured: `cargo tree -i chacha20poly1305@0.10.1` names snow alone;
  `cargo search snow` gives 0.10.0 as the newest release.
- Not measured: the size of the second stack in a release binary.

## Approach

When snow releases a version on the 0.11 line of RustCrypto, take it with
the same features, then run the tests and the plants of the crate and the
static probe. Until then, nothing changes: a duplicate is a warning of
`deny.toml` (`multiple-versions = "warn"`).

## Prove

```sh
cargo tree -i chacha20poly1305@0.10.1
cargo test -p pod-e2e --all-features
sh scripts/plant-each.sh --group e2e
```

The first command finds no such package.

## Start condition

A release of snow that takes `chacha20poly1305` 0.11.

# T-210: A plant whose file changed goes stale, and only `plants.yml` finds it

**Source:** the `plants` run 43020335301 (on `e71f813`): `probe-lean-tokio` STALE
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`f105d34` (T-024) added lines to `examples/static-probe-lean/Cargo.toml`.
The patch of the plant `probe-lean-tokio` held the old context, so it did
not apply. No check of a push applies a patch: `plants.yml` found it 20
minutes after the push, and `check.sh` passed.

## Premise

- Read: the log of the run (`STALE probe-lean-tokio: the patch does not
  apply: error: patch failed: examples/static-probe-lean/Cargo.toml:14`);
  `scripts/plant-each.sh` (a stale patch fails only in a full run).

## Approach

`plant-each.sh --apply`: `git apply --check` of each patch against the
tree, in less than one second. `check.sh` (step `paths`) and the lint job
of `ci.yml` run it. Regenerate the patch of `probe-lean-tokio`. The plant
`plants-stale` breaks the context of one patch.

## Prove

```sh
sh scripts/plant-each.sh --apply
sh scripts/plant-each.sh plants-stale probe-lean-tokio
```

## Done

2026-11-11. `plant-each.sh --apply`; `check.sh` and `ci.yml` run it. The
patch of `probe-lean-tokio` is made again against the manifest with
`pod-wg` and `pod-confine`. The old patch gives `STALE probe-lean-tokio`
and exit 1; the new one gives 0 stale. `plant-each.sh plants-stale
probe-lean-tokio`: 2 caught.

# T-243: Dependabot of 2026-11-12: trufflehog 3.99.8, toml 1.2.1, and foreign-types 0.5.0 declined (GitHub #79, #80, #81)

**Source:** GitHub #79, #80, #81 (pull requests of Dependabot)
**Category:** chore
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

Dependabot opened three pull requests against `14719eb`: the image of
TruffleHog (`images/trufflehog/Dockerfile`, 3.99.7 to 3.99.8), `toml`
(1.2.0 to 1.2.1, with `toml_datetime` and `serde_spanned`), and
`foreign-types` (0.3.2 to 0.5.0). The ruling "Dependabot" applies each
judged update as an own commit on `main` and closes the pull request.

## Premise

- Read: the diff of each pull request. Measured on 2026-11-12: the tag
  `3.99.8` of `trufflesecurity/trufflehog` at Docker Hub is the index
  `sha256:674b2f18…`, the digest of the pull request.
- `foreign-types` 0.3.2 comes only from `openssl` 0.10.82 (`pod-gsr`),
  which takes `foreign-types` 0.3: a 0.5.0 cannot replace it in the lock.

## Approach

Apply #79 and #80 as own commits; decline #81 with its reason; close the
three pull requests with a link to the commit or the reason.

## Prove

```sh
cargo test --workspace --all-features
```

## Done

2026-11-12. `e738466` (trufflehog 3.99.8 by its digest) and `60903e7`
(`toml` 1.2.1, `toml_datetime` 1.1.3, `serde_spanned` 1.1.3, in both
locks). #81 declined: `openssl` 0.10 takes `foreign-types` 0.3.

# T-306: `pod-services`: six podvm files of its block changed after `2be31b2` (`upstream.yml` red since 2026-11-10)

**Source:** `upstream.yml`, job `sources`, runs 42820129877 and 43017701535 (red on schedule since 2026-11-10; no entry knew it).
**Category:** chore
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`scripts/check-sources.py` finds changes of podvm files of the block of
`2be31b2` in `crates/pod-services/SOURCES`:
`PodUtils/podvm:crates/podvm-measure/src/workload.rs`,
`PodUtils/podvm:crates/podvm/src/images/list.rs`,
`PodUtils/podvm:crates/podvm/src/images/login_extract.rs`,
`PodUtils/podvm:experiments/390-machine-ssh.sh`,
`PodUtils/podvm:experiments/lib/kvm-guest-base.sh`, and the removal of
`PodUtils/podvm:experiments/220-extract-path-safety.sh`. Each change is a
port of a value of `pod-services` or a re-record of the block.

## Premise

- Read: the diff of each file from `2be31b2` to podvm's `7af63a0`
  (`main`), with a local clone.

## Approach

For each file: a value of `pod-services` that moved is a port; a file
whose values podvm now reads from `pod-services` is a take (`taken`); a
change of no value moves the file to a block at a newer commit.

## Prove

```sh
python3 -I scripts/check-sources.py --clone PodUtils/podvm=CLONE --clone PodUtils/podnet=CLONE --clone PodUtils/podssh=CLONE pod-services
```

## Done

2026-11-12. No value of `pod-services` changed; no port.

- `workload.rs`, `list.rs`, `login_extract.rs`: podvm's `b0a3e05` (its
  PodUtils/podvm@T-1679) reads the pinned image, the kernel's archive and SHA-256, and
  the default server of login from `pod-services`: a take. `list.rs` and
  `login_extract.rs` keep their blobs at the parent of the take and get
  `taken b0a3e05` in the block of `2be31b2`. `workload.rs` changed before
  the take (`60d11c5`: the pin from `mirror.gcr.io`, which
  `mirror-gcr-registry` holds; `36b15ec`: the guest's init and console):
  a block at `fff8805` (the parent of the take) with `taken b0a3e05`.
- `390-machine-ssh.sh` and `kvm-guest-base.sh`: podvm's runs of the guests
  on a hosted runner and the check of the digest of the vendor image after
  the setup; no value moved: a block at `7af63a0`.
- `220-extract-path-safety.sh`: podvm retired it at `323a4a8` (its
  PodUtils/podvm@T-1734; its checks are tests of `pod-extract`). It leaves the block; the
  fact `experiment-images` of `docker-hub-registry` stays for the
  experiments 70, 80 and 90.
- The Prove command with the three clones: each block of `pod-services`
  has 0 changes.

# T-307: The nightly at `0edd703` is red: a test of `pod-guest` with no Linux gate, the TCP-list tests on targets with `socketcall(2)`, a bound of 1 s under riscv64

**Source:** `nightly.yml` run 43179371651 at `0edd703` (dispatched to prove the gate of `crates/pod-oci/tests/live.rs`).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** open

## Problem

Three causes make the nightly red:

1. macOS and Windows: `crates/pod-guest/tests/convert.rs` has no
   `#![cfg(target_os = "linux")]`; the crate is empty on another target,
   so the test does not build (`unresolved imports pod_guest::convert_argv`).
2. i686 and powerpc64le: since T-262 a spec with a TCP list is refused on
   a target with `socketcall(2)`. Nine tests compiled or ran such a spec
   and unwrapped the result: four rule tests of
   `crates/pod-confine/src/compile/tests.rs`, two of
   `crates/pod-confine/tests/apply.rs`, one of
   `crates/pod-confine/tests/sockets.rs`, two of
   `crates/pod-enter/tests/confine/linux.rs`.
3. riscv64 (emulated): `forward_url_ends_at_its_bound` of
   `crates/pod-services-watch/tests/watches/forward.rs` had a bound of
   1 s, and the authentication alone took longer.

## Premise

- Read: the logs of the jobs of run 43179371651.
- Not measured here: the macOS, Windows, powerpc64le and riscv64 jobs (the
  next nightly).

## Approach

1. The Linux gate of the test.
2. `Spec::compile_on(abi, socketcall)` (for the tests): the rule tests
   compile as for a target with no `socketcall(2)`, so they check the
   rules on each target; the refusal keeps its own test. The kernel tests
   check the refusal on a target with `socketcall(2)` and end there.
3. A bound of 5 s.

## Prove

```sh
cargo test -p pod-confine -p pod-enter -p pod-guest --all-features
```

and a dispatch of `nightly.yml`.

# T-308: `pod-core` and `pod-relay`: three podssh files of their blocks changed (`upstream.yml` at `c553207`)

**Source:** `upstream.yml` run 43185447717, job `sources`, at `c553207` (after T-306).
**Category:** chore
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`scripts/check-sources.py` finds three changed podssh files:
`PodUtils/podssh:crates/podssh-relay/src/open.rs` and
`PodUtils/podssh:crates/podssh-relay/tests/cache.rs` (the block of
`600d59e` of `crates/pod-core/SOURCES`), and
`PodUtils/podssh:crates/podssh-relay/src/lib.rs` (the block of `75a5031`
of `crates/pod-relay/SOURCES`).

## Premise

- Read: the diffs and the logs of the three files in a clone of podssh.

## Approach

A port where podssh changed code of the crate; else a `taken` line or a
block at a newer commit.

## Prove

```sh
python3 -I scripts/check-sources.py --clone Azathothas/podssh=CLONE --clone PodUtils/podssh=CLONE pod-core pod-relay
```

## Done

2026-11-12. No port.

- `open.rs` and `tests/cache.rs`: podssh's take of `pod-relay`
  (`0872c2c6`) took the code that the block of `pod-core` reads; both
  blobs at the parent of the take are the recorded ones. The block gets
  `taken 0872c2c6` for the two paths.
- `lib.rs`: before its take podssh added the modules of its own roads
  (`tox`, `ygg`, `hyperdht`, `packet_stream`; PodUtils/podssh@T-734 to
  PodUtils/podssh@T-736), which are no code of `pod-relay`. The file moves
  to a block at `a4b21a5` (the parent of the take) with `taken 0872c2c6`.
- `check-sources.py` with clones of podssh, podnet, podvm and podrunner,
  over each crate: exit 0, no change.

# T-323: `services.yml` at `613058c` is red: Serveo's banner names its console, a text watch of shiftr names an endpoint, no RDAP for `.me`, and openbare.xyz in 66 days

**Source:** `services.yml` run 43195020023 at `613058c`.
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

Four findings of the daily watch:

- `MOVED serveo forward-url`: the banner of `serveo.net:443` names
  `https://console.serveo.net/settings` first. The rule (`https://`,
  `.serveo.net`) accepts it, and the GET of T-267 shows no forward there.
- `FAILED shiftr-mqtt http-text`: the two text watches of `shiftr-mqtt`
  name an `endpoint`, and `http-text` reads only `url`.
- `FAILED localtunnel rdap`: `.me` has no RDAP server in IANA's
  bootstrap, and `rdap.org` answers 404 for `localtunnel.me`.
- `FAILED openbare-xyz rdap`: the registration ends on 2027-01-17, in 66
  days; the bound is 90 days.

## Premise

- Read: https://serveo.net/docs/ (2026-11-12) links
  `console.serveo.net/domains` and `console.serveo.net/ssh/keys`: the
  console of the account.
- Measured: `https://data.iana.org/rdap/dns.json` has no row for `me`;
  `https://rdap.org/domain/localtunnel.me` answers 404 (2026-11-12).

## Approach

- `serveo`: the fact `url-skip` = `console.serveo.net`.
- `http-text` takes `url` or `endpoint` (the URL of the endpoint).
- `localtunnel`: a watch `tls-name` of `api` in place of `rdap`.
- `rdap`: the argument `ahead-days`, 30 where the watch gives none;
  `ajam.dev` gives 90 (call C125).

## Prove

```sh
cargo test -p pod-services -p pod-services-watch --all-features
cargo run -q -p pod-services-watch -- --service serveo --service shiftr-mqtt --service localtunnel --service openbare-xyz
```

## Done

2026-11-12. The four findings have their fix; the next `services.yml`
run proves them.

- `serveo.toml`: `url-skip` with the measured banner as its source;
  `tests/forward.rs` picks the tunnel after the console URL.
- `http.rs`: `text` reads `url`, else the URL of `endpoint`; a watch with
  neither fails by name. Test `http_text_wants_the_text`.
- `localtunnel.toml`: `tls-name` of `api`; `tests/api.rs` holds it.
- `rdap.rs`: `ahead-days` (`AHEAD_DAYS` 30), a note under `NOTE_DAYS`
  (90) with no finding; test `rdap_wants_a_registration_that_does_not_end_soon`.
- The shiftr defect passed each test because no test reads the
  arguments of each watch of the data: T-324.

# T-324: `pod-services-watch`: no offline test checks the arguments of each watch of the data

**Source:** T-323 (the text watches of `shiftr-mqtt`).
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

A watch of the data with a missing or a misnamed argument passes `cargo
test`, and fails only in the daily `services.yml` run against the live
service. `shiftr-mqtt` named `endpoint` for `http-text`, which reads
`url`; the defect showed one day after the data landed.

## Premise

- Read: `crates/pod-services-watch/tests/watches/data.rs` checks the kind
  of each watch, not its arguments; `src/args.rs` finds a missing argument
  only when the watch runs.

## Approach

Each kind declares its arguments: the ones it needs, the optional ones,
and the groups of which one is needed (`url` or `endpoint`). A test reads
each watch of `pod_services::all()` against the declaration of its kind:
a missing argument, an unknown argument and a name of an endpoint or a
fact that the service does not have fail with the service and the watch.
The declaration is the one source of the table "The schema" of
`crates/pod-services/README.md`, or a test compares the two.

## Prove

```sh
cargo test -p pod-services-watch --test watches data
```

## Done

2026-11-12. `src/schema.rs`: `arguments(kind)` (the needed arguments, the
groups of which one is needed, the optional ones) and `faults(service,
watch, is_service)` (a missing or an unknown argument; a name of no
endpoint, no role, no fact, no service). `run` checks the arguments
before the network. Tests `data::each_watch_of_the_data_has_the_arguments_of_its_kind`,
`data::each_kind_has_a_table_of_arguments`, and the cases `t5` and `t6`
of `http::http_text_wants_the_text`. Plant
`services-unknown-argument-passes`. Each watch of the data passes.

# T-325: The nightly at `613058c` is red: the stage of a socket does not fit on macOS, and four bounds under riscv64 are not scaled

**Source:** `nightly.yml` run 43195242737 at `613058c` (jobs 131104397543, macOS arm64; 131104398223, the cross test of riscv64 under qemu-user).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

- macOS: `a_socket_has_its_mode_under_each_umask` (`pod-broker`) saw the
  socket with mode 777 under the umask 000.
- riscv64: `the_channel_rides_the_layer_through_cut_links_with_one_handshake`
  (`pod-relay`), `a_large_download_crosses_a_bounded_link` (`pod-netstack`),
  three tests of `pod-quic/tests/datagrams.rs` and
  `a_session_carries_bytes_that_the_relay_never_sees_in_plain`
  (`pod-e2e`) went past their bounds.

## Premise

- Read: `crates/pod-broker/src/control/bind.rs`. The stage path is the
  parent, `/.pod-broker-` and 16 hex digits, and `/s`: 31 bytes more than
  the parent. macOS gives `sun_path` 104 bytes, and its temporary
  directory with the name of the test is about 75. A stage that does not
  fit is skipped: a bind at the path, then a `chmod`, the window of T-196.
- Read: the four bounds of the riscv64 failures do not use
  `pod_testkit::host::scaled`; the download ran at 578 kB/s for 29 s
  under qemu-user.

## Approach

- The name of the stage is `.pb-` and 16 hex digits (20 bytes): each
  socket directory of a test (80 bytes at most, `pod_testkit`) takes the
  stage. A unit test holds the bound.
- `scaled` on the bounds of the four tests; the floor of the rate of the
  download is a fifth under an emulator.

## Prove

```sh
cargo test -p pod-broker --all-features
cargo test -p pod-netstack -p pod-quic -p pod-e2e -p pod-relay --all-features
```

## Done

2026-11-12. The next nightly proves both platforms.

- `bind.rs`: `random_name` gives `.pb-` and 16 hex digits; `stage_fits`;
  test `a_parent_of_80_bytes_takes_the_stage`. The plant
  `broker-bind-at-the-path` is regenerated.
- `scaled` in `pod-relay/tests/e2e_layered.rs`,
  `pod-quic/tests/datagrams.rs`, `pod-e2e/tests/e2e_harness/mod.rs` and
  `pod-netstack/src/stack/tests/link.rs` (with `pod-testkit` as a
  dev-dependency of `pod-netstack`).

# T-351: `services.yml`: a host that gives no answer to one runner fails the daily run

**Source:** `docs/live-calls.md` (row L21); `services.yml` runs
42803032733, 42806121501, 42933757949, 43145957804.
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** M
**Status:** partial

## Problem

A watch fails the daily run when its host gives no answer to the runner:
a connect that times out, no route, or a reset before the first byte.
The runners of GitHub reach the network from AWS or from Azure, and some
hosts do not answer the addresses of one cloud. Thus a red run can say
nothing about the service. Four of the seven red runs of `services.yml`
that were read hold such a failure: Trojan nodes "timed out after 29s".
A red run that is not ours hides a finding that is ours.

## Premise

- Read: https://docs.github.com/en/actions/concepts/runners/github-hosted-runners
  (2026-11-12): Linux runners of 2 and 4 cores run on AWS or Azure.
- Measured (the logs of the runs, read 2026-11-12): `node-bolab-trs02`
  and `node-bonds-sg1` timed out in runs 42803032733, 42806121501 and
  42933757949; `node-trojan-70-34-207-14` in run 43145957804. The Eclipse
  MQTT sandbox gave no answer to `live.yml` run 43172235777 and each day
  after, and it left the catalog (call C94): a service that left gives no
  answer on each run.
- Not measured: whether the nodes answer from another network on the same
  day.

## Approach

Call C153. The runner gives each failure a class: `no-answer` (a connect
timeout, no route, a reset before the first byte) or the others. A
`no-answer` failure is a finding of a scheduled run only when the last
scheduled run had a `no-answer` failure of the same watch. The first one
prints `FAILED (first no answer)` in the summary and does not fail the
run. A run by hand fails on each finding.

The state is one file, the list of the `no-answer` watches of the run,
kept in the cache of the job (actions/cache 6.1.0 at
`55cc8345863c7cc4c66a329aec7e433d2d1c52a9`): the key
`services-no-answer-RUN_ID`, the restore prefix `services-no-answer-`.
With no state (the first run, a cache that expired), each `no-answer`
failure is a first one.

## Prove

```sh
cargo test -p pod-services-watch --all-features
sh scripts/plant-each.sh services-no-answer-twice
gh workflow run services.yml
```

A plant: a watch of a closed port on the loopback with a state that names
it must fail; with no state it must pass with the line `first no answer`.

## Done

2026-11-12, commit `ad10864` (call C155 gives the class):

- `Finding::NoAnswer`, given where the failure is made: a dial whose
  `pod_proxy::DialError` is a connect timeout, no route, or a refused or
  reset connect (`Net::tcp`, `Net::send`, `Net::streamed`, the WebSocket
  dial), and a TLS handshake that the network ended before the first byte
  of the server (`src/heard.rs` notes the first byte). `answer::State` is
  the rule of call C153; the runner takes `--no-answer-was` and
  `--no-answer-now`. `services.yml` restores the list of the last
  scheduled run from the cache of the job (actions/cache 6.1.0 at
  `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`, key
  `services-no-answer-RUN_ID`, restore prefix `services-no-answer-`), and
  saves the list of the run.
- `cargo test -p pod-services-watch --all-features`: 4 unit, 55 in
  `tests/watches` (5 new in `answer.rs`), each passed.
- `sh scripts/plant-each.sh services-no-answer-twice
  services-no-answer-class-dropped services-no-answer-heard-ignored`:
  3 caught. `services-forward-url-word-not-compared` and
  `services-tls-names-host-alone` made again: caught.
- Not run: `gh workflow run services.yml` (the session has no write to
  GitHub). The first scheduled run after the merge must show each
  Trojan node that times out as `FAILED (first no answer)`, a green run
  for that reason alone, and a cache `services-no-answer-RUN_ID`; the
  second must fail on a node that times out again.

# T-352: `live.yml` runs each week, and no step of `ci.yml` reaches a host of another party

**Source:** `docs/live-calls.md` (rows L2, L3, L5); call C150.
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`live.yml` runs each day (call C24). Its steps reach the relay's edge
(TLS and MPTCP), ECR Public, the public DoH resolvers and Alpine's CDN.
Only the watches of `services.yml` must run each day; the code of each
client is proven on each push against a stand-in. A daily live run adds
a chance of a red run for a reason of the network, and proves no new
fact on most days:

- `pod-proxy`, MPTCP to the edge: the test asserts only the connection.
  The edge answered plain TCP (run 43172235777). The step measures one
  fact that does not change, each day.
- `pod-tls`, the handshake with the relay: the servers of
  `crates/pod-tls/tests/common` prove the provider on each push.
- `pod-guest`, the modloop of the newest Alpine 3.22: the watch
  `http-sha256` of `alpine-netboot` holds the pinned file each day.

No check stops a new step of `ci.yml` that reaches a host of another
party.

## Premise

- Read: `.github/workflows/live.yml` (the cron "53 1 * * *"; the
  mailboxes run on Thursdays), `crates/pod-proxy/tests/mptcp.rs`,
  `scripts/check-workflows.py`.
- Measured: the log of `live.yml` run 43172235777 (2026-11-12) says
  "Multipath TCP asked for; the server answered with plain TCP".
- Not measured: how often the steps of the relay's edge fail; each run
  read passed them.

## Approach

1. `live.yml`: the cron runs on Thursday only ("53 1 * * 4"). The step of
   the public mailboxes loses its test of the day. The step of MPTCP runs
   only by hand (an input `measure: mptcp`).
2. `scripts/check-workflows.py`: a step of `ci.yml`, `plants.yml` or
   `adoption.yml` that runs a test with `--ignored`, or that names a host
   that is not in an allowlist of the downloads (github.com, crates.io,
   static.rust-lang.org, the registries of `scripts/pull-image.sh`, the
   zig mirrors, pypi.org and files.pythonhosted.org), fails the check. A
   plant proves it.
3. Call C150 replaces the daily cadence of C24 for `live.yml`.

## Prove

```sh
python3 -I scripts/check-workflows.py
sh scripts/plant-each.sh ci-step-reaches-a-host
gh workflow run live.yml
```

## Done

2026-11-12. `live.yml`: the cron "53 1 * * 4" (Thursday); the step of
MPTCP runs by hand alone (the input `measure: mptcp`); its comments name
call C150. `scripts/check-workflows.py`: a workflow of each push
(`ci.yml`, `plants.yml`, `adoption.yml`) refuses a test with `--ignored`,
and a host in its text, or in a script of `scripts/` that it runs, that is
not a host of the downloads (`DOWNLOADS` and the mirrors of
`scripts/zig-mirrors.txt`). Today's workflows: 0 faults. Plant
`ci-step-reaches-a-host` (group `workflows`): caught.

# T-355: The downloads of each job come from a cache, and the tools from their release binaries

**Source:** `docs/live-calls.md` (rows L30 to L32); call C152.
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

Each job downloads the toolchain, the index and the crates of crates.io,
and on the zig jobs zig and the source of cargo-zigbuild, which it then
builds (`cargo install`): 8 jobs of `static`, 7 of `cross` and 3 of
`adoption` for each run. `fuzz.yml` builds cargo-fuzz too. Each download
is a chance of a red run for a fault of the network (`scripts/retry.sh`
exists for them), and each build of a tool costs minutes.

## Premise

- Read: `.github/workflows/ci.yml`, `.github/workflows/nightly.yml`,
  `.github/workflows/adoption.yml`, `scripts/fuzz.sh`
  (`CARGO_FUZZ_VERSION=0.13.2`), `scripts/fetch-zig.sh`.
- Read (2026-11-12): https://github.com/actions/cache/releases (v6.1.0,
  the tag at `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`, MIT);
  https://github.com/rust-cross/cargo-zigbuild/releases/tag/v0.23.4 (the
  asset `cargo-zigbuild-x86_64-unknown-linux-musl.tar.xz`, SHA-256
  `9e3cf73485edbd45905c8aadbc0fdf869c7ddc3848f0c898229f2680db52e44b`);
  https://github.com/rust-fuzz/cargo-fuzz/releases (0.13.2 has
  `cargo-fuzz-0.13.2-x86_64-unknown-linux-musl.tar.gz` and no checksum
  file).
- Not measured: the time of a job with the cache; the size of the cache
  of the registry (the limit of a repository is 10 GB).

## Approach

Call C152.

1. Each job that runs cargo restores `~/.cargo/registry/index`,
   `~/.cargo/registry/cache` and `~/.cargo/git/db` with actions/cache at
   the pinned commit, keyed by the OS and the hash of `Cargo.lock`, with
   the prefix of the OS as the restore key. `target/` is not cached.
2. zig: the cache keyed by `ZIG_SHA256`; `scripts/fetch-zig.sh` runs on a
   miss only.
3. cargo-zigbuild: the release binary of `ZIGBUILD_VERSION`, checked by
   its SHA-256 (`ZIGBUILD_SHA256` beside the version, as `deny.yml` does
   for cargo-deny). cargo-fuzz: the same, with the SHA-256 measured at the
   first download and written in `scripts/fuzz.sh`.
4. `scripts/check-workflows.py`: each new `uses:` is pinned by a commit.

## Prove

```sh
python3 -I scripts/check-workflows.py
gh workflow run ci.yml
gh workflow run nightly.yml
```

The second run of `ci.yml` on the same `Cargo.lock` logs a hit of each
cache and no `cargo install`.

## Done

2026-11-12, commit `cd8a4a2`; calls C152 and C185. `scripts/ci/fetch-tool.sh`
takes one program of a release archive by its SHA-256;
`scripts/ci/zig-tools.sh` gives zig (from the cache, else
`scripts/fetch-zig.sh`) and cargo-zigbuild 0.23.4 (its release binary,
`ZIGBUILD_SHA256`); `scripts/fuzz.sh` takes cargo-fuzz 0.13.2 the same way
on Linux x86_64; `scripts/cloud-setup.sh` too. actions/cache 6.1.0 at
`55cc834` keeps the registry of each job (`ci.yml`, `nightly.yml`,
`adoption.yml`, `fuzz.yml`; keyed by the OS, the job and the lock files),
the archives of zig and cargo-zigbuild (keyed by both SHA-256 values) and
the cache of pip (keyed by `scripts/ci/wormhole-servers.txt`). The toolchain
stays a download (C185). The plant `ci-step-reaches-a-host` is made again.
Proof: `ci` run 43289686281 of `bb330bb` passed; its log reads a hit on
the primary key of the zig cache and of the registry cache, and the step
"zig and cargo-zigbuild" takes 3 to 4 s in each job of zig (a build of
cargo-zigbuild took minutes); `adoption` run 43287309397 passed with the
same step.

# T-371: `pod-services-watch`: the DNS watches, `stun-binding` and the TLS of the HTTP client give no class "no answer"

**Source:** the work of T-351 (call C155).
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

Call C153 holds the first "no answer" of a watch on a scheduled run. The
way out gives the class `Finding::NoAnswer` for a dial (`Net::tcp`,
`Net::send`, `Net::streamed`, the WebSocket dial) and for the TLS
handshakes of `Net::tls` and `Net::tls_alpn`. Three paths give no class,
and a host that does not answer a runner there still fails the run:

- the DNS watches (`dns-srv`, `dns-addresses`, `doh-answer`): `pod-dns`
  gives a `pod_proxy::Missed` or words, and `src/dns.rs` makes a
  `Finding::failed` of the words;
- `stun-binding`: three tries over UDP with no answer is a failure;
- the TLS handshake inside `pod_proxy::Client` (the HTTP watches): a reset
  before the first byte of the server is a failure of another kind.

## Premise

- Read (2026-11-12): `crates/pod-services-watch/src/dns.rs` (each error
  of `Way` becomes `Finding::failed`); `crates/pod-dns/src/get.rs` (a
  dial's `Missed` keeps its cut, and so does a TLS handshake's, so the
  cut alone does not tell the dial from the handshake, nor a handshake
  cut before the first byte from one cut after it);
  `crates/pod-services-watch/src/stun.rs`.
- Measured: no red run of `services.yml` read holds such a failure of
  these kinds (`docs/live-calls.md`, rows L19 and L20).
- Not measured: whether Quad9's DoH answers a runner of the other cloud.

## Approach

1. `pod-dns`: `Missed` tells where the connection ended (the dial, or the
   handshake before or after the first byte of the server), as a field
   that `get.rs` sets where it makes the failure.
2. `src/dns.rs`: a `Missed` of the dial, or of a handshake before the
   first byte, is `Finding::NoAnswer`; a lookup of more than one resolver
   is "no answer" only when each one gave none.
3. `stun-binding`: three tries with no datagram back is
   `Finding::NoAnswer`; an answer of another kind stays a failure.
4. `pod-proxy`: the client keeps the first byte of a handshake in the
   failure (a stream that notes it, as `src/heard.rs` of
   `pod-services-watch` does), so that `Net::send` classes it.

## Prove

```sh
cargo test -p pod-dns --all-features
cargo test -p pod-services-watch --all-features
sh scripts/plant-each.sh --group services
```

A test of each path: a resolver, a STUN server and an HTTPS server on the
loopback that close before a byte give `NoAnswer`; one that sends a byte
first gives `Failed`.

# T-488: The nightly at `613058c` fails to compile on Windows: `reverse_plain.rs` of `pod-relay` binds a Unix socket with no `cfg(unix)`

**Source:** `nightly.yml` run 43195242737 at `613058c` (jobs 131104397812, `test (windows-2025)`; 131104397800, `test (windows-11-arm)`).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

Both Windows jobs of the nightly stop at the build of the tests:

```text
error[E0433]: cannot find `UnixListener` in `net`
   --> crates\pod-relay\tests\reverse_plain.rs:224:32
error: could not compile `pod-relay` (test "reverse_plain") due to 1 previous error
```

tokio has `UnixListener` only under `cfg(unix)`. The red build hides
each other result of the Windows jobs. T-325 (the same run) does not
name these jobs.

## Premise

- Read (2026-11-13): the logs of jobs 131104397812 and 131104397800
  through the read-only mirror; the same error in each.
- Read: `crates/pod-relay/tests/reverse_plain.rs`. The test
  `a_session_to_a_closed_port_is_refused_with_the_reason` binds
  `tokio::net::UnixListener` for the `unix:` target and then checks a
  closed TCP port. The file has no `cfg(unix)` gate; it came with T-026.
- Not measured: the other tests of the Windows jobs after this build.

## Approach

Gate the `unix:` half of the test with `#[cfg(unix)]`, and keep the half
of the closed TCP port on each platform (two tests, or one test with a
gated block). Then dispatch `nightly.yml` and read the Windows jobs; a
new failure there is a new entry.

## Prove

```sh
cargo test -p pod-relay --all-features --test reverse_plain
```

and a dispatch of `nightly.yml` whose Windows jobs build the tests.

## Correction

2026-11-13: the nightly at `d23a368` (run 43372791771) builds
`reverse_plain.rs` on Windows; the build then stops at the next fault:
`crates/pod-link/tests/common/mod.rs` names `pod_link::unix` with no
`cfg(unix)`, and seven test targets of `pod-link` include it. A red build
hides each fault after the first, so the fix checks the whole workspace
at once: `scripts/agent/check-windows.sh` (`cargo check --tests` for
x86_64-pc-windows-gnu, zig as the C compiler). It found that fault and
nine warnings of items that only Unix tests use (in `pod-link`,
`pod-broker`, `pod-proxy`, `pod-relay` and `pod-tls`); each is gated now,
and the script gives 0 faults. The next nightly proves the Windows jobs.

## Done

2026-11-13, commits `d23a368`, `6924dd1` and `a627ab0`. The Unix half
of `reverse_plain.rs`, the unix helper of the tests of `pod-link` and
nine items that only Unix tests use are gated with `cfg(unix)`;
`scripts/agent/check-windows.sh` checks the tests of the workspace for
Windows here and gives 0 faults. The nightly at `a627ab0` (run
43382143089) builds the tests on both Windows jobs; the first failures
of the tests are T-507.

# T-507: The tests of `pod-services-watch` fail on Windows: a reset loses the bytes of a stand-in, and a runner with no `SystemRoot` has no Winsock

**Source:** `nightly.yml` run 43382143089 at `a627ab0` (job 131742137667, `test (windows-2025)`).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

Three tests of `crates/pod-services-watch/tests/watches/answer.rs` fail
on Windows. (1) and (2): `a_handshake_cut_after_a_byte_of_the_server_is_an_answer`
and `a_handshake_with_another_protocol_is_an_answer` get `os error
10053` (`WSAECONNABORTED`): the stand-in writes its bytes and closes
with the client's hello unread, so Windows sends a reset, and the client
loses the bytes before it reads them. (3)
`a_second_scheduled_run_with_no_answer_fails` gets `os error 10106`
(`WSAEPROVIDERFAILEDINIT`): the runner starts with `env_clear()`, and
with no `SystemRoot` Winsock does not start. The step `Test` of the
nightly stops at the first red test binary, so the results of the
binaries after it are not known.

## Premise

- Read (2026-11-13): the log of job 131742137667 through the GitHub
  tools; `answer.rs`, `crates/pod-testkit/src/stand.rs` (`raw_over`).
- Not measured: the tests after `pod-services-watch` on Windows.

## Approach

The two stand-ins read the client's first TLS record whole before they
answer; the runner keeps `SystemRoot` on Windows. The step `Test` of the
nightly takes `--no-fail-fast`. Then dispatch the nightly: each new
failure of a test on Windows is a new entry.

## Prove

A dispatch of `nightly.yml` whose Windows jobs pass the tests of
`pod-services-watch`.

## Done

2026-11-13, commits `17737c3` and the commit of this section. The nightly
at `923309a` (run 43406798368, with `--no-fail-fast`) passes the tests of
`pod-services-watch` on Windows. It shows one more failure there, on both
jobs: `pod-wg`'s `crypto::tests::stamps_only_go_forward` built a stamp of
999_999_999 ns from a `SystemTime`, and the clock of Windows counts in
steps of 100 ns, so the carry case did not happen. The test makes that
stamp from its bytes now. Each other test of the Windows jobs passes. The
next nightly proves the jobs green.

