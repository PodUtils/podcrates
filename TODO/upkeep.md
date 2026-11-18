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
**Status:** done

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

## Done

2026-11-13. The fixes are `3d9f1f5`. `nightly.yml` run 43417105533 at
`d8c8ae6` (a descendant of `3d9f1f5`) proves each cause: the jobs
`test (macos-15)`, `test (macos-15-intel)`, `test (windows-2025)`,
`test (windows-11-arm)`, `cross (i686-unknown-linux-musl)` and
`cross (powerpc64le-unknown-linux-musl, ppc64le)` pass; in the job of
riscv64, `forward::forward_url_ends_at_its_bound` passes, and only two
bounds of `pod-warp` fail (T-532, done).

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
stamp from its bytes now. The nightly at `d8c8ae6` (run 43417105533)
passes both Windows jobs.

# T-532: The nightly at `d8c8ae6` is red on riscv64: the bounds of the tests of `pod-warp` are not scaled

**Source:** `nightly.yml` run 43417105533 at `d8c8ae6` (job 131857666801, the cross test of riscv64 under qemu-user).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

Under qemu-user for riscv64, `a_session_with_each_controller_carries_a_large_echo` (`pod-warp/tests/stack.rs`, "the echo in the limit") and `a_flow_through_an_l4_door_on_the_loopback` (`pod-warp/tests/l4.rs`, "the end of the flow") went past their bounds of 20 s and 10 s. Each other target passed.

## Premise

- Read: the job log. Both failures are `Elapsed`; no assert of a value failed.
- Read: the tests of `pod-warp` use a fixed `LIMIT`, not `pod_testkit::host::scaled`, the rule of T-325 for a bound that guards a hang.

## Approach

`limit()` gives `scaled` of the old bound in `tests/l4.rs`, `net.rs`, `proxy.rs`, `stack.rs` and `wg.rs`: five times the bound under an emulator, the same bound on a kernel. `live_session.rs` runs live only, not under an emulator.

## Prove

```sh
cargo test -p pod-warp --all-features
```

The next nightly proves riscv64.

## Done

2026-11-13. `limit()` with `scaled` in the five test files of `pod-warp`. The next nightly proves riscv64.

# T-533: `scripts/agent/check-macos.sh`: a local check of the build for macOS, as `check-windows.sh` does for Windows

**Source:** `ci.yml` run 43454211290 at `b98b432` (the static probes of macOS: `libc::SOCK_CLOEXEC` in the icmp carrier of `pod-link`).
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The local checks build Linux and Windows only. A Unix path that uses an item of Linux alone (`libc::SOCK_CLOEXEC`, a field of a struct) builds on Linux and fails on macOS. CI finds it after the push: the release commit of `v0.40.0` failed the static probes of macOS so.

## Premise

- Measured: `cargo check -p pod-link --all-features --lib --tests --target x86_64-apple-darwin` with zig as the C compiler (`zig cc -target x86_64-macos`) runs here; ring builds with it.
- Measured: `openssl-sys` (`pod-gsr`) does not build so; the probe of macOS needs it.

## Approach

Write `scripts/agent/check-macos.sh` from `check-windows.sh`: the target `x86_64-apple-darwin`, a zig wrapper that drops `--target`, `-arch` and `-mmacosx-version-min`, and `cargo check --tests` of the workspace without the crates whose C does not build so (each one named in the script with the reason). Remove `target/x86_64-apple-darwin` after. Name it in `docs/maintenance.md` beside `check-windows.sh`, and in the merge procedure.

## Prove

```sh
sh scripts/agent/check-macos.sh
```

and a planted `libc::SOCK_CLOEXEC` in a Unix path of `pod-link` fails it.

## Done

2026-11-13. `scripts/agent/check-macos.sh`: `cargo check --tests` of the workspace for `x86_64-apple-darwin`, with `zig cc -target x86_64-macos` and without `pod-gsr` and `static-probe`; 0 faults at `d531d20`. A planted `libc::SOCK_CLOEXEC` in the Unix path of the icmp carrier fails it with E0425. `docs/maintenance.md` and the row `scripts/agent/` of `docs/design.md` name it. No plant in `scripts/plants/`: the runners of `plants.yml` have no zig and no target of macOS.

# T-535: The nightly at `72632b2` is red on riscv64 and armv7: the bounds of the tests of `pod-link` are not scaled under an emulator

**Source:** `nightly.yml` run 43451625236 at `72632b2` (the cross tests of riscv64 and armv7 under qemu-user).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

Under qemu-user, tests of `pod-link` go past their bounds:

- riscv64: `eight_mib_cross_at_five_percent_loss_and_one_percent_reorder_with_nodelay` (`tests/udp_loss.rs`, "the transfer in time"), `the_bytes_before_a_drop_reach_the_peer` (`tests/quic.rs`, "the bytes and the end in time"), `a_link_over_a_chunked_stream_carries_streams_across_a_lost_answer` and `a_link_over_sse_carries_streams_across_a_lost_answer` (`tests/poll.rs`, "the echo in time").
- armv7: `a_query_of_a_stranger_changes_nothing` (`tests/dns.rs`): the dial fails with "the listener at l.test:53 closed before it took this end". The listener of the test has a `handshake_timeout` of 500 ms.

The macOS jobs of the same run fail to build `pod-link` (`libc::SOCK_CLOEXEC`); `3d6bbf0` fixes that (T-497, T-533).

## Premise

- Read: the logs of the jobs 131974971067 (riscv64) and 131974970870 (armv7). Each failure on riscv64 is `Elapsed`; no assert of a value failed.
- Read: `crates/pod-link/tests/common/mod.rs`: `LIMIT` is a fixed 30 s, not `pod_testkit::host::scaled`, the rule of T-325 for a bound that guards a hang. T-532 fixed the same class in `pod-warp`.
- Not measured: the time of the TLS handshake of the dns carrier under qemu-user for armv7. The 500 ms bound is the likely cause, not a proven one.

## Approach

1. `limit()` in `tests/common/mod.rs` gives `scaled(LIMIT)`; each test of `pod-link` takes it.
2. A `handshake_timeout` of a test that does not test that bound takes `scaled` too (`tests/dns.rs`).
3. The gates of E2-1 in `tests/udp_loss.rs` measure a release build: under an emulator, scale their bounds, or skip them by name with `pod_testkit::host::emulated`.

## Prove

```sh
cargo test -p pod-link --all-features
```

The next nightly proves riscv64 and armv7.

## Done

2026-11-13. `limit()` gives `pod_testkit::host::scaled` of the bound in each test file of `pod-link` that has one (`tests/common/mod.rs` and the files with their own `LIMIT`). The listener of `a_query_of_a_stranger_changes_nothing` takes `scaled(500 ms)`, and the transfer bound of `tests/udp_loss.rs` takes `scaled(120 s)`. The gates of E2-1 run in a release build alone (ignored in a debug build), so the cross jobs do not run them. The next nightly proves riscv64 and armv7.

# T-536: `pod-oci`: the test of the distribution registry with a bearer token fails at random: the registry refuses the connection

**Source:** `ci.yml` run 43459943301 at `3d6bbf0` (job `test`, step "pod-oci against the distribution registry").
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`an_index_of_two_platforms_resolves_through_a_bearer_token` (`crates/pod-oci/tests/distribution.rs`) panicked at `tests/common/distribution/push.rs`: "the registry accepts: Connection refused". The four other tests of the file passed in the same run, and the same step passed at the commits before.

## Premise

- Read: the log of job 132003860678; `tests/common/distribution/server.rs` (`Registry::start`, `wait`) and `mod.rs` (`free_port`).
- `free_port` binds port 0, reads the port and closes the socket; the registry binds the port later. The five tests run in parallel, and the token server of a test binds port 0 too. `wait` takes any listener on the port as the registry.
- Not measured: the output of the registry of the failed test (the step does not print it).

## Approach

1. `wait` checks that the child is alive (`try_wait`) and reads the line of the registry that names its address, not only a connect to the port.
2. When the registry fails to bind, start it again on a new port, at most three times, and print `said()` in the message.
3. Print `said()` when a request of `Pusher` fails to connect.

## Prove

```sh
cargo test -p pod-oci --test distribution
```

with the registry of `distribution` on the path, ten times with no failure.

## Done

2026-11-13. T-534 holds items 1 and 2 (`listens`: the line of the registry and a child that runs; a new port, at most three times). The Prove ran ten times with distribution 3.1.2: 10 passed. Item 3 is not needed: after `listens`, a refused connect means the registry ended, and the message of the registry is in `Registry::said`.

# T-537: `services.yml` at `4c2d17c` is red: the tox node `node-3091c6be` gives no TCP relay

**Source:** `services.yml` run 43402100735 at `4c2d17c` (the scheduled run of 2026-11-13).
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The daily watch found one fact moved, and the run is red:

```text
MOVED tox-tcp-relay http-json: services/tox-tcp-relay.toml endpoint node-3091c6be is port 443; the service gives tcp_ports []
MOVED tox-tcp-relay http-json: services/tox-tcp-relay.toml endpoint node-3091c6be is status_tcp true; the service gives status_tcp false
```

The other 175 watches hold, and no watch had a first "no answer" (T-351).

## Premise

- Read: the log of job 131807203413 of that run.
- Read: `crates/pod-services/services/tox-tcp-relay.toml`: the endpoint `node-3091c6be` (205.185.115.131, port 443) and its fact `public-key-node-3091c6be`, read on 2026-11-12.
- Not measured: whether the node offers TCP again on a later day (a node of the list can stop for a time).

## Approach

Read `https://nodes.tox.chat/json` again. When the node still gives no TCP port, remove the endpoint and its fact, or put in its place another node with 443 in `tcp_ports` and `status_tcp` true (the rule of T-244 for the choice of the nodes). Write the source with the date of the read.

## Prove

```sh
cargo test -p pod-services
cargo run -q -p pod-services-watch -- --service tox-tcp-relay
```

## Done

2026-11-13. The list of 2026-11-13 gives the node at 205.185.115.131 no TCP port and no UDP: it is down. The four other endpoints are the four items with 443 in `tcp_ports` and `status_tcp` true. `tox-tcp-relay.toml` drops `node-3091c6be` and its key; the sources of the other four have the read of 2026-11-13. `pod-services-watch --service tox-tcp-relay`: holds, 4 of 28 items, 0 of them not in the file.

# T-538: The rule of attribution of a comment on GitHub: the documents of podcrates and the practice of its sessions differ

**Source:** a finding of the housekeeping of 2026-11-13 (the comments on the issues of podcrates).
**Category:** docs
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** blocked

## Problem

`AGENTS.md` (section 3), `docs/decisions.md` (the ruling "Git" of
2026-11-05) and `docs/maintenance.md` ("Tools") say that no comment has an
agent attribution line. Each comment of a session of podcrates on GitHub
ends with the line "Generated by Claude Code" of the harness. The
`AGENTS.md` of PodUtils/podrunner records a later word of the owner
(2026-11-11): a comment on GitHub can end with that line; a commit
cannot. The record of podcrates does not have that word.

## Premise

- Read: `AGENTS.md`, `docs/decisions.md`, `docs/maintenance.md` of
  podcrates; the `AGENTS.md` of PodUtils/podrunner at its commit
  `1696466` ("a GitHub comment can end with the Generated-by line, a
  commit cannot (the owner)").
- Read: the comments of podcrates on #83, #88, #106 and #133: each ends
  with the line.
- Not read: the words of the owner themselves (they are in the session of
  podrunner, not on GitHub).

## Approach

The owner rules Q16. Then write the ruling in `docs/decisions.md`, and
make `AGENTS.md` and `docs/maintenance.md` agree with it.

## Prove

```sh
cargo todo check
```

## Blocker

The owner answers Q16 (`TODO/PROGRESS.md`): does the word of 2026-11-11
in PodUtils/podrunner (a comment on GitHub can end with the "Generated
by" line of the harness) apply to podcrates too?

# T-534: `ci.yml` at `3d6bbf0` is red: the registry of `pod-oci`'s test took a port that a parallel test held

**Source:** `ci.yml` run 43459943301 at `3d6bbf0` (job 132003860678, step "pod-oci against the distribution registry").
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`an_index_of_two_platforms_resolves_through_a_bearer_token` (`pod-oci/tests/distribution.rs`) failed: "the registry accepts: Connection refused". The start of the registry took a free port, and its wait ended at the first connect. A parallel test can take that port before the registry binds it. The registry then ends, the wait connects to the other server, and the push later finds no server.

## Premise

- Read: `tests/common/distribution/server.rs`: `free_port` binds port 0 and lets it go; `wait` connects in a loop.
- Measured: distribution 3.1.2 writes "listening on 127.0.0.1:PORT" when it binds.

## Approach

The wait reads the log of the registry for "listening on 127.0.0.1:PORT" while the child runs. When the child ends first, the start takes a new port, at most three times.

## Prove

```sh
sh scripts/ci/registry-tests.sh
```

## Done

2026-11-13. `listens` in `tests/common/distribution/server.rs`; `start` takes a new port when the registry ends. `scripts/ci/registry-tests.sh` with the pinned registry: 5 passed.

# T-555: `adoption.yml` at `7122cc7` is red for podssh: its test pins five tox relays, and T-537 left four

**Source:** `adoption.yml` run 43476670647 at `7122cc7` (job `adoption (podssh)`); the same at `3ace9ed` (run 43480013909).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The trial of podssh runs `cargo test -p podssh-cli --features warp --lib`. Its test `tox::tests::the_listed_relays_read_with_their_keys` asks for at least five relays in the service `tox-tcp-relay`. T-537 removed the node at 205.185.115.131, which is down, so the catalog lists four. podssh's main has the same assertion, so podssh's own tests fail when it takes the tag that holds T-537.

## Premise

- Read: the log of job 132062164377; podssh's crates/podssh-cli/src/tox.rs at the commit of the trial (`286e98d3`) and at its main.
- The catalog is right: the node list of 2026-11-13 gives four nodes with 443 and TCP up.

## Approach

the patch file of the pod-warp trial of podssh (removed after its take) takes a hunk that asks for at least four relays. The notes and the hand-off of `v0.41.0` tell podssh to change its test in the same way.

## Prove

The run of `adoption.yml` at the next head passes for podssh.

## Done

2026-11-14. The hunk is in the patch file of the pod-warp trial of podssh (removed after its take); `docs/releases/v0.41.0.md` names the change under "Breaking". The adoption run at the next head proves it.

# T-563: `adoption.yml` at `b0533d0` is red for podssh: `Route::One` of `pod-relay` breaks a match of its role

**Source:** `adoption.yml` run 43548582479 at `b0533d0` (job `adoption (podssh)`).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

T-559 adds the variant `One` to `pod_relay::http_road::Route`, and `http_road::route` gives it for `/v1/http/one/`. podssh's role matches `route()` with no wildcard (crates/podssh-relay/src/serve/http_road/mod.rs), so its build fails: E0004, `Some(Route::One)` not covered. podssh's main has the same match; its own one form serves the path before `route()`.

## Premise

- Read: the log of job 132305529047; podssh's crates/podssh-relay/src/serve/http_road/mod.rs at the commit of the trial (`286e98d3`) and at its main.

## Approach

the patch file of the pod-warp trial of podssh (removed after its take) takes a hunk: `None | Some(Route::One)` answer 404, as the trial's commit has no one form. The hunk has podssh's format (rustfmt with its `rustfmt.toml`). The notes and the hand-off of `v0.42.0` name the change under "Breaking": podssh adds an arm for `Route::One`.

## Prove

The run of `adoption.yml` at the next head passes for podssh.

## Done

2026-11-14. The hunk is in the patch file of the pod-warp trial of podssh (removed after its take); `docs/releases/v0.42.0.md` names it. The two lines that it adds move a citation of podssh's record (its TODO/relay.md cites that file at line 142 at `286e98d3`; the arm moves it to 144): a second hunk of the patch moves it, as `cargo todo remap` does (`adoption.yml` run 43550476602). The adoption run at the next head proves it.

# T-571: `adoption.yml` at `e830136` is red for podnet: the field `preshared` of `pod_wg::Keys`

**Source:** `adoption.yml` run 43555358813 at `e830136` (job `adoption (podnet)`).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

T-561 adds the field `preshared` to `pod_wg::Keys`. podnet builds `Keys` with a struct literal in src/networks/wg.rs, in src/warp/wg.rs (the file of its trial) and in the test src/wg_quick_tests.rs, so its build fails: E0063, missing field `preshared`.

## Premise

- Read: the log of the job; podnet's src/networks/wg.rs and src/warp/wg.rs at the commit of the trial (`9c1fc4b`), with the trial's patch applied.

## Approach

the patch file of the pod-warp trial of podnet (removed after its take) sets `preshared: None` in each literal, which keeps podnet's behaviour (no preshared key). The run file remaps the changed files of podnet, so each citation of podnet's record follows the added line. The notes of `v0.43.0` name the field under "Breaking".

## Prove

The run of `adoption.yml` at the next head passes for podnet.

## Done

2026-11-14. The hunks of the three literals are in the patch file of the pod-warp trial of podnet (removed after its take), and the run file of the pod-warp trial of podnet (removed after its take) remaps src/networks/wg.rs and src/wg_quick_tests.rs. The run at `5db4d56` (43558133970) built podnet and failed on the test literal; a search of each Rust file of podnet at `9c1fc4b` finds no other. The adoption run at the next head proves it.

# T-607: the nightly run at `f6479c7` is red on `windows-11-arm`: the ARQ of a test of `pod-link` through a resolver gives up in 7 s

**Source:** the nightly run 43578379242 at `f6479c7` (job `test (windows-11-arm)`).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`a_query_with_no_edns_gets_an_answer_of_512_bytes_or_less` of `crates/pod-link/tests/dns_resolver.rs` panicked at line 146 at `f6479c7` with `Timeout { seq: 109, retries: 8, waited: 6.87s }`. The ARQ of the test has `rto_ceiling` 1 s and the default 8 retries: a lost frame gets about 7 s, and a slow runner took longer to answer it. The test checks the size of the answers and the transfer, not the timer of the ARQ.

## Premise

- Read: the log of the job; `crates/pod-link/tests/dns_resolver.rs` (`link`); each `rto_ceiling` and `retries` of `crates/pod-link/tests` (`common/dns.rs`, `common/icmp.rs`: `fast_arq`, used by the tests of a silent peer, which check the timer).

## Approach

The ARQ of `link` takes `MAX_RETRIES` (16) and a ceiling of 2 s: a lost frame gets about 27 s, within the 60 s bound of a transfer. `fast_arq` stays: its tests check that a silent peer ends the link at the ARQ timeout.

## Prove

```sh
cargo test -p pod-link --all-features --test dns_resolver
```

## Done

2026-11-14, commit 7873f38. The ARQ of `link` of
`crates/pod-link/tests/dns_resolver.rs` takes `MAX_RETRIES` and a ceiling
of 2 s (call C459). The other ARQ settings of the tests of `pod-link` are
`fast_arq` (`tests/common/dns.rs`, `tests/common/icmp.rs`), whose tests
check the ARQ timeout of a silent peer: they stay. `cargo test -p pod-link
--all-features` passed; `--test dns_resolver` passed 10 runs in a loop.
No plant: the change gives a test more time and changes no behaviour of
the crate. The nightly run at the next head proves it on
`windows-11-arm`.

# T-608: `upstream.yml` is red: the sources of `pod-warp`, `pod-netstack` and `pod-gsr` changed

**Source:** `upstream.yml` run 43591429134 (job `sources`).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`scripts/check-sources.py` reports changes: `pod-warp` 30 of podnet at `e046560` and 5 of podssh at `562751d`; `pod-netstack` 1 of podnet at `e046560`; `pod-gsr` 1 of hackerschoice/gsocket at `5950647`.

## Premise

- Read: `scripts/check-sources.py` (the line `taken`); the commits of podnet, podssh and gsocket after each pin (local clones); podnet's take of v0.37.0 (`f6abdc3`) and its T-328 (`1649d8f`); podssh's take of v0.37.0 (`812c61c`); gsocket's `dd62a5a`.

## Approach

A take of a consumer is a `taken` line of its block, with the paths that the take changed when the block holds other files. A change of a source that is a fix is ported in a commit of its own; a change that is not moves the pin, with the reason.

## Prove

```sh
python3 -I scripts/check-sources.py pod-warp pod-netstack pod-gsr
```

## Done

2026-11-14, commit eaf2762 (calls C460, C461). No change is a fix, so no
port:

- `pod-warp`, podnet: the 30 changes are podnet's take of v0.37.0 at
  `f6abdc3` (the files of src/warp/ removed, or glue over pod-warp) and
  its T-328 at `1649d8f` (crates/podnet-netns/src/rules.rs reads
  `pod_warp::rules::matcher`). The block takes them with two `taken`
  lines by path; crates/podnet-core/src/services.rs stays compared with
  the default branch. No file of the block changed between the pin and
  the parent of a take. podssh: the 5 files are removed by its take of
  v0.37.0 at `812c61c`; the block takes it.
- `pod-netstack`: the change of src/warp/ip.rs is the same take of
  podnet (`f6abdc3`); the block takes it.
- `pod-gsr`: gsocket `dd62a5a` (lib/gsocket-util.c) makes `bin2hex` of a
  NULL source write no byte; the crate's hex of a byte slice has no NULL.
  The pin moves to `dd62a5a` with the blob of the file.

`python3 -I scripts/check-sources.py --clone ...` with local clones of
podnet, podssh and gsocket: `pod-warp` (31 files and 5 files),
`pod-netstack` (8, 1 and 1 files) and `pod-gsr` (4 files) report 0
changes. Through the mirror, `pod-gsr` reports 0 changes; the mirror
answered 403 to the many reads of the other two.
`pod-actions-store` and `pod-actions-results` do not report 0: podrunner
changed their sources after the pin `872b43f` (`de6850e`, `afa619d` and
`9dbefd1`); that is a port of its own.

# T-614: CI: a job that runs as root the tests that start a worker of uid 1000

**Source:** T-605 (a finding of its work).
**Category:** chore
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The proofs of T-581 and T-605 run only when the tests run as root, which can start a worker of uid 1000 (the set-id bits of a file, the `S_ISGID` that the kernel drops from a directory). The GitHub runner is not root, so CI skips these assertions.

## Premise

- Read: the reports of T-581 and T-605; `.github/workflows/ci.yml`; the tests that print `SKIP:` when they are not root.

## Approach

A job of `ci.yml` (or `nightly.yml`) runs those tests of `pod-extract` and `pod-rootfs` with `sudo -E` (the runner has passwordless sudo), so that the worker of uid 1000 runs. The job fails on a `SKIP:` line of those tests.

## Prove

```sh
python3 scripts/check-workflows.py
```

## Done

2026-11-14, commit db08402. `scripts/ci/root-uid1000-tests.sh`, a step of
the job `test` of `ci.yml`, builds the library test binaries of
`pod-extract` and `pod-rootfs` as the user and runs as root (`sudo -n`)
`drive::setid::set_id_bits_survive_the_write`, the two tests of
`drive::dirmodes` with a worker and
`closed::a_closed_etc_is_completed_and_keeps_its_mode`. A skip line
(`skipping`, `SKIP:`) fails it, and each test must report one pass
(C513). The script passed here as root. Plant
`extract-t614-root-test-skips` (the proof skips as root too), caught by
hand: "set_id_bits_survive_the_write skipped as root".
`check-workflows.py`: 0 faults. The next CI run proves the step on a
runner.

# T-621: `pod-warp`: the test `a_close_by_the_caller_fails_the_flows_of_the_session` fails at times with a refused connection to the edge stand-in

**Source:** the full check of the merge at `f3fd493` (2026-11-14), on a host where two agents built at the same time.
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`cargo test -p pod-warp --test net` failed once at `crates/pod-warp/tests/net.rs:283`: `connect()` gave `TCP to the MASQUE front door 192.0.2.1:443 through the moved dial of the test: Connection refused (os error 111)`. The test passed 8 runs of 8 alone after it. The moved dial carries the door to the stand-in edge of `world()`, so the edge was not yet listening, or another test on the host took its port, when the dial came.

## Premise

- Read: `crates/pod-warp/tests/net.rs` (`world`, `dead`, and the moved dial: `crates/pod-warp/tests/net.rs:412`), `crates/pod-warp/tests/common/edge.rs` (the listener of the stand-in edge, its port, and its loop of accepts).

## Approach

Find why the stand-in refused: a listener that is bound after `world()` returns, a port that the edge picks before it binds, or an accept loop that ends after one session. Make the stand-in bind before `world()` returns and keep its listener for the life of the test. A loop of the test under load (for example 50 runs beside a build) proves it.

## Prove

```sh
cargo test -p pod-warp --all-features --test net
```

## Correction

2026-11-15: `nightly.yml` run 43699451813 at `98017c3`: on macOS a connection to the port of a socket that is bound with no listen is not refused; it waits for the limit of the dial. `a_failure_of_both_legs_names_both_causes` of `tests/leg.rs` takes 15.7 s on both macOS jobs, and `tests/net.rs` takes 31 to 40 s there (2.1 s here). T-680 has the fix for macOS.

## Done

2026-11-14, commit d463022. The cause was not a late bind: `dead()` freed
the port of the IPv4 door at once. Another test's edge took that port, so
the first dial reached an edge whose key is not the pinned key; the client
read the record again and dialed the IPv4 door again, and that edge had
ended by then: `Connection refused`, with no word of the IPv6 door. A
test of the binary with that dial gave the same `Failure` word for word
(not kept). `common::dead()` now binds a socket with no listen and holds
it until the test binary ends (`net.rs`, `leg.rs`, `register.rs`). Each
accept loop of the stand-ins goes on after a failed accept
(`common::accept`: the edge, its echo server, and the TLS stand-ins of
`proxy.rs` and `live_session.rs`), so that a listener stays for the life
of the test. Call C520.

The binary of `--test net` passed 50 runs of 50 beside `cargo clippy` of
`pod-guest` and `pod-hostprobe`, and 50 of 50 again with each run beside
`cargo clippy -p pod-warp`. `cargo test -p pod-warp --all-features`
passed. No plant: the change is a fix of the tests and not of a
behaviour.

# T-645: `secrets.yml` is red on each push: three made-up URLs of `pod-broker` read as unknown

**Source:** the run of `secrets.yml` 43673806380 at `cca002b` (a red run that the record did not know).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The step "TruffleHog over the history" of `secrets.yml` fails on each push: "secrets-report: 56 findings, 3 verified or unknown". The three are URI findings with the state unknown in `crates/pod-broker/src/words.rs` at commit `7cbcfffa` (T-609): the doc comment of `without_userinfo` and two of its tests. Each URL has a made-up user and password at a reserved name (`proxy.example`, `dns.example`). `.github/secrets-allow.txt` has no line for them.

## Premise

- Read: the log of the job of the run; `scripts/secrets-report.py` (a finding fails the run when its state is verified or unknown and no line of `.github/secrets-allow.txt` allows it); `crates/pod-broker/src/words.rs` at `7cbcfffa`.
- Not measured: the first red run of this cause. Each run of `secrets.yml` that the mirror lists is red.

## Approach

A line in `.github/secrets-allow.txt` for each of the three findings, with the reason. The next push runs `secrets.yml`.

## Prove

```sh
python3 scripts/secrets-report.py --allow .github/secrets-allow.txt found.jsonl
```

The run of `secrets.yml` on the push of the fix is green.

## Done

2026-11-15, commit `a34fa71`. Three lines in `.github/secrets-allow.txt` allow the URI findings of `crates/pod-broker/src/words.rs` at `7cbcfffa` (lines 47, 114, 118). `secrets-report.py` with the allowlist gave "3 findings, 0 verified or unknown" on the three findings. The run of `secrets.yml` 43683783577 at `a634d3a` (by a dispatch, #168) passed.


# T-646: `adoption.yml`: the trial of `pod-egress` in podssh fails, as podssh's record cites the files that leave podssh

**Source:** the run of `adoption.yml` 43673650741 at `141849e` (a red run that the record did not know); T-035, part 1.
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The job "adoption (podssh)" fails at the first step of adoption/podssh/pod-egress.run: `podssh-todo remap Cargo.lock` reports 6 problems. Five citations of podssh's record name files that the patch removes from podssh: `.github/workflows/forwarder-live.yml` (four) and `PodUtils/podssh:scripts/plants/t439-via-no-credential.plant` (one). One citation of `PodUtils/podssh:crates/podssh-cli/src/doctor/forwarder.rs` moves by one line, as the patch changes that file. The trial must pass before `v0.51.0`, which carries the patch to podssh.

## Premise

- Read: the log of the job; adoption/podssh/pod-egress.patch and `.run`; podssh at `2ffb7cd7` with the patch applied.
- Measured: the same 6 problems in a scratch copy of podssh at `2ffb7cd7` with the patch.

## Approach

The patch changes the five citations to the bare name of the file and its new place in PodUtils/podcrates (the checker of podssh reads a bare file name as no path), and moves the citation of `forwarder.rs` with `podssh-todo remap`. The patch is made again with `git diff --cached HEAD --abbrev=8`, as before.

## Prove

```sh
sh scripts/adoption.sh podssh pod-egress
```

## Done

2026-11-15, the commit of the fix. adoption/podssh/pod-egress.patch changes the five citations to the bare name of the file and its new place in PodUtils/podcrates (`forwarder-live.yml`, `egress-via-no-credential.plant`), and moves the citation of `forwarder.rs` to 28-45 with `podssh-todo remap`. The patch is made again with `git diff --cached HEAD --abbrev=8`; the old patch made the same way gave the same bytes. `sh scripts/adoption.sh podssh pod-egress` passed each step of the `.run` here: the record (743 entries), `check-repo.py`, clippy and the tests of `podssh-ws`, `podssh-cli` and `podssh-tor`, rustfmt, the toolchains and the 15 test targets. The run of `adoption.yml` on the push proves it in CI.

# T-668: `pod-oci`: the token of the manifest for the parallel blob fetches of a pull (PodUtils/podvm#395)

**Source:** PodUtils/podvm#395 (COORD-129 of podvm; PodUtils/podvm@T-1850).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm timed the phases of a pull through the HTTPS proxy of its cloud host: the blob phase of `busybox:1.37.0-musl` (2 blobs) took 1190 ms, as long as the manifest phase (1223 ms). podvm's estimate (not measured alone): `Client::blob` takes `&mut self`, so each worker of `fetch_parallel` builds its own `pod_oci::registry::Client`, whose token cache is empty, and each blob starts with a 401 and a token request. The ask: a way for the workers of one pull to use the token of the manifest request, for example `Client::tokens(&self)` and `Client::with_tokens(self, map)` with the key `<endpoint>|<scope>`.

## Premise

- Read: PodUtils/podvm#395; `crates/pod-oci/src/registry.rs` (`Client`, its `tokens`), `crates/pod-oci/src/registry/http.rs` (the answer to a `Bearer` challenge), `crates/pod-oci/tests/distribution.rs`.
- Not measured: the time that podvm saves; podvm measures it again (its experiment 443).

## Approach

The first form of the ask: `Client::tokens` gives a copy of the cache, and `Client::with_tokens` adds a map to it. A token that the registry refuses is asked for again, as one that expired, so a stale map costs one request. A unit test of the round trip, and a step of the bearer test against the CNCF registry: a second client with the tokens of the first fetches a blob with no new token request.

## Prove

```sh
cargo test -p pod-oci --all-features
```

## Done

2026-11-15, the commit of the change. `Client::tokens` and `Client::with_tokens`; the test `the_tokens_of_a_client_go_to_another` and the step of `an_index_of_two_platforms_resolves_through_a_bearer_token` (the token server counts one request after the fetch of a worker). `cargo test -p pod-oci --all-features` passed (95 unit tests); the test against the registry runs in `ci.yml`, where the registry binary is.

# T-677: `plants.yml` passes its limit of 60 minutes: the plants run in shards

**Source:** the run of `plants.yml` 43685586038 at `482ba53` (#169), cancelled at its limit (a red run that the record did not know).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The one job of `plants.yml` runs each plant in turn. With 702 plants it passes its `timeout-minutes: 60`: the run 43685586038 was cancelled at 60 minutes in the step "Each plant is caught", and so was 43673650746 at `141849e`. Each other run of the list was cancelled by a newer one. No run of `plants.yml` passed for the tags `v0.49.0` and `v0.50.0`. The release block of #169 stops at it.

## Premise

- Read: the runs and the jobs of `plants.yml` (GitHub's list of runs); `.github/workflows/plants.yml`; `scripts/plant-each.sh` (it takes a list of ids).
- Not measured: the time of one shard; the run on the push proves it.

## Approach

A matrix of six shards. `scripts/plant-shard.py SHARDS SHARD` gives the ids of a shard: each group whole in one shard (its plants build the same crates), from the largest group down to the shard with the fewest plants. Each shard has its own limit of 60 minutes.

## Prove

```sh
for k in 0 1 2 3 4 5; do python3 -I scripts/plant-shard.py 6 $k; done | wc -w
python3 scripts/check-workflows.py
```

The run of `plants.yml` on the commit passes each shard.

## Done

2026-11-15, commit `98017c3`, and the fixes that its first run found in the next commit. `scripts/plant-shard.py` and the matrix of six shards in `plants.yml`. The run 43690607111 at `98017c3` ended each shard in 17 to 30 minutes (the limit is 60). Its findings: the plants `services-t552-forward-before-session` and `services-t552-shell-not-command` named a test module that moved (`forward_order`), and ran no test; the plants `extract-t581-kept-mode-not-recorded` and `extract-t605-kept-dir-mode-not-recorded` ran a test that skips as a user, and now run `scripts/ci/root-uid1000-tests.sh`; the four are caught here as root. Three other plants miss: T-678. The table of `scripts/` in `docs/design.md` names `plant-shard.py` (`check-design-paths.py`, the red step of `ci` 43690600539).

# T-678: three plants of `pod-extract` no longer catch: a second guard holds their check

**Source:** the run of `plants.yml` 43690607111 at `98017c3` (the first run of each shard, T-677).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The first run of each plant since the full set stopped (T-677) missed three plants of `pod-extract`, as root too:

- `extract-t597-pass-only-widens` (T-597): the patch makes the permission pass between layers set only the directories that it widens; `drive::umask::the_modes_of_the_image_stay_under_a_umask_of_027` still passes. The pass after the last layer (`modes::DirModes::set`, T-598) sets the mode of each directory again.
- `extract-t598-existing-directory-not-opened` (T-598): the patch removes the `fchmod` of a directory that exists in `apply.rs`; `drive::dirmodes::a_second_extraction_over_a_closed_tree_passes` (as root, through a worker of uid 1000) still passes.
- `extract-t598-whiteout-parent-not-opened` (T-598): the patch removes the `open_for_write` of the parent of a whiteout in `remove.rs`; the same test still passes.

A plant that defeats one guard of two proves nothing of the check.

## Premise

- Read: the log of shard 4 of the run; the three patches; `crates/pod-extract/src/apply.rs` (`repermission`, the `fchmod` of an existing directory), `crates/pod-extract/src/drive/umask.rs`, `crates/pod-extract/src/drive/dirmodes.rs`.
- Measured: each of the three plants missed here as root, with `sh scripts/ci/root-uid1000-tests.sh`.
- Not measured: which second guard holds each check.

## Approach

For each plant: find the second guard (apply the plant, then remove each candidate in turn until the test fails), then either make the plant defeat both (one patch, one defect: the behaviour that the entry fixed) or give the test a case that only the first guard holds. Run each with `plant-each.sh` as root.

## Prove

```sh
sh scripts/plant-each.sh extract-t597-pass-only-widens extract-t598-existing-directory-not-opened extract-t598-whiteout-parent-not-opened
```

## Done

2026-11-15, commit `f453b82e`. The second guard of each plant: for `extract-t597-pass-only-widens`, the pass after the last layer (`modes::DirModes::set`) sets the mode of each directory again; for the two T-598 plants, the walk before the first layer (`modes::open_tree`, T-606) adds the owner bits to each directory of the earlier tree. Each was found with the plant applied and the candidate removed, as root through the worker of uid 1000. The umask test now also checks the tree between two layers (`run_open`), which only the permission pass of the layer sets: the T-597 plant is caught there, as it is. The `fchmod` of an existing directory and the `open_for_write` of the parent of a whiteout are redundant in `crate::extract` (call C646); they stay as the guard of a caller of the public steps, and the two T-598 plants also remove the walk. `plant-each.sh` catches the three plants as root, and the eight other plants of `drive::umask` and `drive::dirmodes`; `plant-each.sh --apply` shows 0 stale.

# T-679: The nightly at `98017c3` is red on Windows: a test of `pod-actions-store` reads a removed variable by the wrong case

**Source:** `nightly.yml` run 43699451813 at `98017c3` (jobs `test (windows-11-arm)` 132767154797 and `test (windows-2025)` 132767155063).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`store::tests::the_git_command_takes_the_proxy_of_the_grant_and_https_only` (T-613) panicked at `crates\pod-actions-store\src\store\tests.rs:320:19` on both Windows jobs: "no entry found for key". The line reads `env["HTTPS_PROXY"]` from the map of `Command::get_envs`.

## Premise

- Read: the logs of both jobs; `crates/pod-actions-store/src/git.rs` (`PROXY_VARS` removes `http_proxy`, `https_proxy`, `HTTP_PROXY`, `HTTPS_PROXY` and the others in that order); the environment of `std::process::Command`: on Windows one key holds each name in any case, and a second change of the same name keeps the case of the first.
- Measured: the test is new in this run (commit `0457efc` is after `f6479c7`, the run before).
- Not measured: the test on Windows; this host builds it for `x86_64-pc-windows-gnu` and does not run it.

## Approach

The code is correct: Windows removes each spelling. The test reads each key in upper case on Windows.

## Prove

```sh
cargo test -p pod-actions-store --all-features
sh scripts/agent/check-windows.sh
```

The next nightly proves the two Windows jobs.

## Done

2026-11-15, commit `15e770ac`. The test reads each key of `Command::get_envs` in upper case on Windows; the code of `Git::command` stays. `cargo test -p pod-actions-store --all-features`: 25 passed. `sh scripts/agent/check-windows.sh`: 0 faults. No test fails here without the fix: Linux keeps each spelling. The next nightly proves the two Windows jobs.

# T-680: The nightly at `98017c3` is red on macOS: a port that a socket holds with no listen does not refuse there

**Source:** `nightly.yml` run 43699451813 at `98017c3` (jobs `test (macos-15)` 132767155119 and `test (macos-15-intel)` 132767154999).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`a_failure_of_both_legs_names_both_causes` of `crates/pod-warp/tests/leg.rs` (T-336) failed on both macOS jobs: "the relay waited for its delay: 15.708s" (macos-15) and "15.642s" (macos-15-intel), against a bound of 5 s. The direct dial goes to `common::dead()`. T-621 (call C520) made `dead()` a socket that is bound with no listen and held, in place of a port that is freed.

## Premise

- Read: the logs of both jobs; `crates/pod-warp/tests/common/mod.rs` (`dead`); the record of T-621 and the macOS note of an earlier entry (`TODO/transport.md`, "On macOS the refused port is free").
- Measured from the logs: `tests/net.rs` (which uses `dead()` too) passes on macOS in 39.5 s (macos-15) and 31.3 s (macos-15-intel), with gaps of about 7 s between its tests; here it passes in 2.1 s. The same leg test passes on Windows (4 s), riscv64 and ppc64le. Thus macOS gives no refusal to a connection to such a port, and the dial waits for its limit.
- Measured: the failure is new in this run (`d463022`, T-621, is after `f6479c7`).
- Not measured: the test on macOS; this host checks the build alone.

## Approach

On macOS, `dead()` gives the port of a listener that is bound and dropped: macOS picks each free port at random, so another test takes it seldom. The other hosts keep the held socket of C520.

## Prove

```sh
cargo test -p pod-warp --all-features
sh scripts/agent/check-macos.sh
```

The next nightly proves the two macOS jobs.

## Done

2026-11-15, commit `15e770ac`; call C656. `common::dead()` gives a freed port on macOS and the held socket elsewhere. `cargo test -p pod-warp --all-features`: 209 passed (`tests/net.rs` in 2.1 s). `sh scripts/agent/check-macos.sh`: 0 faults. No test fails here without the fix: Linux refuses the held port. The next nightly proves the two macOS jobs.

# T-681: The nightly at `98017c3` is red on windows-2025: a test of `pod-link` reads the answer of a POST before the rest of its body in the link

**Source:** `nightly.yml` run 43699451813 at `98017c3` (job `test (windows-2025)` 132767155063).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`a_body_up_goes_into_the_link_by_parts_and_a_cut_keeps_its_part` of `crates/pod-link/tests/poll_lanes.rs` (T-525) panicked on windows-2025 at the read of the answer: "the answer in time: Elapsed(())", after 30 s. The test sends the POST again with the whole body (128 KiB), reads the answer, and only then reads the rest (64 KiB) at the far end of the link.

## Premise

- Read: the log of the job; `crates/pod-link/src/poll/answer.rs` (`up`: each part goes into the pipe of the session, and the answer 202 comes after the last part).
- The listener answers after the whole body is in the link. When the pipe, the pump and the far end hold less than the rest, the write of the last part waits for a read of the far end, and the test reads it only after the answer: a deadlock of the test. The room on Linux holds the rest; the room on Windows does not.
- Measured: the test is new in this run (`c3b3403` is after `f6479c7`).
- Not measured: the room of each part of the link on Windows.

## Approach

The test reads the answer and the rest at the far end at the same time (`tokio::join!`). The assertions stay.

## Prove

```sh
cargo test -p pod-link --all-features --test poll_lanes
```

The next nightly proves windows-2025.

## Correction

2026-11-16: `nightly.yml` run 43817142905 at `1739b5f`: the test fails again on windows-2025 at the read of the answer. The cause is a write of TLS with no flush: the last records of the POST stay in the client. T-763 has the measurement and the fix; the `tokio::join!` stays.

## Done

2026-11-15, commit `15e770ac`. The test reads the answer and the rest at the far end with `tokio::join!`; the assertions stay. `cargo test -p pod-link --all-features`: 219 passed, `--test poll_lanes` 9. No test fails here without the fix: the link on Linux holds the rest. The next nightly proves windows-2025.

# T-682: The nightly at `98017c3` is red on riscv64 and ppc64le: the upper bound of the split form of `pod-relay` over HTTP/2 is not scaled under an emulator

**Source:** `nightly.yml` run 43699451813 at `98017c3` (jobs `cross (riscv64gc-unknown-linux-musl, riscv64)` 132767155083 and `cross (powerpc64le-unknown-linux-musl, ppc64le)` 132767155158).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`with_no_mark_on_the_stream_the_client_takes_the_split_form_within_the_bound` of `crates/pod-relay/tests/http_road_one_h2.rs` (T-560) failed under qemu-user: "took the split form after 4.821s" (riscv64) and "after 4.06s" (ppc64le), against a bound of 4 s.

## Premise

- Read: the logs of both jobs; the test; T-532 and T-535 (the bounds of tests under an emulator: `pod_testkit::host::scaled`, five times under qemu-user, never for a timer that the test checks).
- The bound has two parts: the wait for the mark (`MARK_WAIT`, 2 s, a timer of the client that the test checks) and the TLS and HTTP/2 work of a debug build (2 s).
- Measured: the test is new in this run (`bfeacab` is after `f6479c7`).

## Approach

The lower bound stays 2 s. The upper bound is 2 s and `scaled(2 s)`: 4 s on a kernel, 12 s under an emulator. On a kernel the bound proves as before that the client takes the split form at the end of the wait for the mark; under an emulator it proves that the client does not wait for the end of the body or a hang.

## Prove

```sh
cargo test -p pod-relay --all-features --test http_road_one_h2
```

The next nightly proves riscv64 and ppc64le.

## Done

2026-11-15, commit `15e770ac`. The upper bound is 2 s and `pod_testkit::host::scaled(2 s)`; the lower bound stays 2 s. `cargo test -p pod-relay --all-features`: 430 passed. The next nightly proves riscv64 and ppc64le.

# T-683: The nightly at `98017c3` is red on ppc64le: the kernel test of `pod-term` gives the numbers of `asm-generic` to a powerpc kernel

**Source:** `nightly.yml` run 43699451813 at `98017c3` (job `cross (powerpc64le-unknown-linux-musl, ppc64le)` 132767155158).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`tty_gives_the_bytes_of_a_kernel_pty_for_each_case` of `crates/pod-term/tests/termios_kernel.rs` (T-540) failed on ppc64le: "67 of 53 cases differ". The reads of the kernel's slave give many reads of 0, and the echo of `a` in the case "no ICANON, an erase" is `A`.

## Premise

- Read: the log of the job; the test (`Pty::set` copies `c_iflag`, `c_oflag`, `c_cflag`, `c_lflag` and `c_cc` of `Termios` into `libc::termios`); `crates/pod-term/src/termios/flags.rs` (the numbers of `include/uapi/asm-generic/termbits.h`); the termios constants of `libc` 0.2.191 for powerpc.
- powerpc gives other numbers to the flags and to the indexes of `c_cc` (for example `ICANON` is 0x100, `OLCUC` is 0o4, `VMIN` is 5). The generic `ONLCR` (0o4) is `OLCUC` on powerpc: thus the echo `A`. `Termios` keeps the generic numbers by design: a child of an SSH channel sends them.
- Measured: the test is new in this run (`f9f7339` is after `f6479c7`). The tests of pod-term build for `powerpc64le-unknown-linux-musl` here (`cargo check`). No qemu here: the run of the fix waits for the nightly.

## Approach

The test gives the kernel the numbers of its target: a table of each field (the generic mask and value, and the value of `libc`) and of each index of `c_cc`. A bit with no row fails the test. A test of the table: each flag of `Termios` has a value of the target, and no two indexes of `c_cc` are one.

## Prove

```sh
cargo test -p pod-term --all-features --test termios_kernel
cargo check -p pod-term --tests --all-features --target powerpc64le-unknown-linux-musl
```

The next nightly proves ppc64le.

## Done

2026-11-15, commit `15e770ac`; call C657. `Pty::set` gives the kernel each field and each index of `c_cc` in the numbers of `libc` for the target; a bit with no row fails. New test `each_flag_of_termios_has_a_value_of_this_target`. `cargo test -p pod-term --all-features`: 233 passed (`--test termios_kernel` 2, in 2.3 s). `cargo check -p pod-term --tests --all-features --target powerpc64le-unknown-linux-musl`: clean. No qemu here: the next nightly proves ppc64le.

# T-684: `services.yml` at `98017c3`: the Trojan node `node-trojan-149-28-130-108` ends the TLS handshake

**Source:** `services.yml` run 43699230156 at `98017c3` (job `watch` 132766442485).
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The scheduled run failed: "FAILED node-trojan-149-28-130-108 proxy-banner: TLS for searchcreativejobsandshowcaseyourartistictalents.com at 149.28.130.108:443: tls handshake eof". The node held on 2026-11-11 and 2026-11-12 (T-174) and in the scheduled run of 2026-11-14 (43578297319, green).

## Premise

- Read: the log of the job; crates/pod-services/services/node-trojan-149-28-130-108.toml; T-174 (the node `node-trojan-70-34-207-14` left after one failed day); `docs/maintenance.md`, "A red run of `services.yml`".
- The TCP connection opened; the node closed it in the TLS handshake. This is not a failure with no answer (call C153).
- Not measured: the node from here. The egress of this session is a proxy that resolves the name itself, so a TLS handshake to the address cannot be made. A dispatch of `services.yml` with the input `service` failed: "Actions has been disabled for this user".

## Approach

Dispatch `services.yml` with `service = node-trojan-149-28-130-108` (or ask for it by the issue `[gh action] requested`), and read the next scheduled run. When the failure stays, remove the node as T-174 removed `node-trojan-70-34-207-14`: its file, its row count in `crates/pod-services/README.md`, and the list of ids that podssh gets (PodUtils/podssh@T-574).

## Prove

```sh
cargo run -q -p pod-services-watch -- --service node-trojan-149-28-130-108
cargo test -p pod-services --all-features
```

## Done

2026-11-15, the commit of the removal. The rerun of `services.yml` with `service=node-trojan-149-28-130-108` (run 43715593746, #175) failed again: "TLS for searchcreativejobsandshowcaseyourartistictalents.com at 149.28.130.108:443: tls handshake eof", as on the scheduled run 43699230156. The node left as T-174 removed `node-trojan-70-34-207-14`: its file crates/pod-services/services/node-trojan-149-28-130-108.toml; the count of the rows `node-*` in `crates/pod-services/README.md` (8 to 7). `cargo test -p pod-services --all-features` and `cargo test -p pod-services-watch --all-features` pass. The hand-off of the next tag tells podssh that the id leaves its list (PodUtils/podssh@T-574).


# T-685: `services.yml` at `98017c3`: both RDAP servers answer 403 for the `.xyz` names to the runner

**Source:** `services.yml` run 43699230156 at `98017c3` (job `watch` 132766442485).
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Three `rdap` watches failed: `gosuda-portal` (`gosunuts.xyz`), `openbare-xyz` (`openbare.xyz`) and `opentunnel` (`opentunnel.xyz`): "https://rdap.centralnic.com/xyz/domain/NAME answered HTTP 403; https://rdap.org/domain/NAME answered HTTP 403". The other `rdap` watches of the run held. The scheduled run of 2026-11-14 (43578297319) was green.

## Premise

- Read: the log of the job; `crates/pod-services-watch/src/rdap.rs` (the server of IANA's bootstrap, then `rdap.org`, which redirects to the same server; the user agent is `pod-services-watch/VERSION`).
- Measured here (2026-11-15): `https://rdap.centralnic.com/xyz/domain/openbare.xyz` answered 200 and `https://rdap.org/domain/openbare.xyz` answered 302, also with the user agent of the watch. Thus the registry refused the address of the runner (a limit or a block of the cloud range), not the request.
- Not measured: a second run from a runner. A dispatch of `services.yml` failed here: "Actions has been disabled for this user".

## Approach

Dispatch `services.yml` with `kind = rdap` (or ask for it by the issue `[gh action] requested`), and read the next scheduled run. When the 403 stays on each run: make an answer 403 of each server for a name a `skipped` line with the reason (the registry refuses the runner), in the class of call C153, with a test; or read the record of `.xyz` from a second source. A failure that is not 403 stays a failure.

## Prove

```sh
cargo run -q -p pod-services-watch -- --kind rdap
cargo test -p pod-services-watch --all-features
```

## Done

2026-11-15, no change of code. The rerun of `services.yml` with `kind=rdap` (run 43715625889, #175) passed: the RDAP servers answered the records of `gosunuts.xyz`, `openbare.xyz` and `opentunnel.xyz`. The 403 of the scheduled run 43699230156 was a refusal of the address of that runner, which a later run does not meet; the watch stays as it is. A second 403 on a scheduled run reopens this entry with a second source of RDAP.

# T-689: the plant `enter-userns-proc-no-remount` misses where the host's `/proc` has `nosuid`, `nodev` and `noexec` already

**Source:** the run of `plants.yml` 43716332294 at `3ee1988` (#177), shard 4.
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The plant `enter-userns-proc-no-remount` (T-672) leaves out the remount of the bind of the host's `/proc` that adds `nosuid`, `nodev` and `noexec` (step 29). On a runner of GitHub, `scripts/ci/userns-tests.sh` passed with the plant applied: "MISSED enter-userns-proc-no-remount (T-672): exit 0, and the output has no match of: left: Ok\(9\)". The host's `/proc` there has the three flags already, and a bind keeps them, so the test reads them with no remount. Here the plant is caught. The check of the test depends on the host.

## Premise

- Read: the log of shard 4 of the run; `scripts/plants/enter-userns-proc-no-remount.plant`; `crates/pod-enter/src/ns_proc_bind.rs`; the test of T-672 in `crates/pod-enter/tests/userns.rs`.
- Not measured: the flags of `/proc` on the runner (an inference from the result).

## Approach

Make the check independent of the host: the test reads the mount flags that the remount itself set (for example the per-mount flags of the bind in `/proc/self/mountinfo` against those of the host's proc, or a host proc that the test mounts with no flags in a mount namespace of its own before the run), or the plant attacks the remount in a way that a host with the flags also shows (the remount refused, which removes the bind: the test sees no `/proc`). Run the plant here and on a runner.

## Prove

```sh
sh scripts/plant-each.sh enter-userns-proc-no-remount
```

## Done

2026-11-15, commit fc85198 (call C671). The test of the run as a caller
with no capabilities (`a_caller_with_no_capabilities_enters_the_namespace_rung`
of `crates/pod-enter/tests/userns.rs`) starts its binary again through the
new `pod_testkit::nobody::again_over_a_bare_proc`: as root and as another
user, the child is pid 1 of its own user, mount and PID namespaces, over a
fresh proc with no `nosuid`, `nodev` or `noexec`; the user namespace maps
the uid and the gid to themselves, so the exec clears each capability.
The test first asserts that its proc has none of the three flags, so a
host cannot hide the check again. The cause of the miss is confirmed
here: in a mount namespace whose `/proc` is remounted
`nosuid,nodev,noexec`, `plant-each.sh enter-userns-proc-no-remount` at
`299b798` gives "MISSED ... exit 0"; at fc85198 it gives "caught (exit
1)", on the host proc and on the flagged proc. `sh
scripts/ci/userns-tests.sh` passes as root; the binary of
`tests/userns.rs` as uid 65534 (the path of a user that is not root, as
on a runner) passes 3 of 3 on both procs. `cargo test -p pod-enter
--all-features`: 220 passed. The run on a runner is the next run of
`plants.yml` (shard of the group `enter`).

# T-736: the plant `enter-userns-dev-no-remount` misses where the host's `/dev` has `nosuid` and `noexec` already

**Source:** `plants.yml` run 43771492486 at `0d6421c` (shard 2 of 6): "MISSED enter-userns-dev-no-remount (T-688): exit 0".
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The test `devices_of_the_host` of `crates/pod-enter/tests/userns.rs` checks that the bind of `/dev/urandom` in the payload is `nosuid` and `noexec` (exit 8 of its script). A bind keeps the flags of its source mount. Where the `/dev` of the host has `nosuid` and `noexec` already, the check passes without the remount of T-688, so the plant that drops the remount is not caught. The plant was caught at `7ef7aab` (`plants.yml` 43730541043). T-689 had the same defect for `/proc`, and its fix ran the check over a fresh `proc`. A device node cannot be mounted fresh by a user with no root, and a user namespace locks the flags of the host's mounts.

## Premise

- Not read: the mount options of `/dev` on the runner at each run (the job does not print them).

## Approach

Print the options of the host's `/dev` in the step of `scripts/ci/userns-tests.sh`. Where the script has root (it uses `sudo` already), run the test of the devices again as uid 65534 from a root parent in a private mount namespace where `/dev` is remounted with no `nosuid` and no `noexec`, so the check of the flags sees the work of the remount alone (as `nobody::again_over_a_bare_proc` does for `/proc`). Where the host's `/dev` holds the flags and no root is there, the test says that the check is not seen here, and the plant runs only where it is seen.

## Prove

```sh
sh scripts/ci/userns-tests.sh
sh scripts/plant-each.sh enter-userns-dev-no-remount
```

## Done

2026-11-16, commit ee2f4159 (call C718). The cause is confirmed here: in a mount namespace whose `/dev` is remounted `nosuid,noexec`, `plant-each.sh enter-userns-dev-no-remount` at `e4e0374f` gives "MISSED ... exit 0", as shard 2 of run 43771492486. The runner image of that shard was `ubuntu-24.04` 20261111.370.1; the shard that caught the plant at `7ef7aab` (run 43730541043, shard 1) ran on 20261111.127.1. The logs do not show the options of `/dev` (an inference: the newer image mounts `/dev` with `noexec` too). The fix: `pod_testkit::nobody::again_over_a_bare_dev` starts the test binary again as uid 65534 from a root parent in a private mount namespace whose `/dev` is remounted (`MS_REMOUNT | MS_BIND`, with `rdonly` and `nodev` kept) with no `nosuid` and no `noexec`; `nobody::mount_options` reads the options of a mount. As root, `a_caller_with_no_capabilities_gets_the_devices_of_the_host` of `crates/pod-enter/tests/userns.rs` runs through it and asserts that its `/dev` has neither flag; as another user over a `/dev` with one of the flags, it writes `not seen here: ...` to its standard error and passes. `scripts/ci/userns-tests.sh` prints the options of the host's `/dev` and, as a user that is not root where `sudo -n` works (a GitHub runner), runs that test again with root; a skip line or a `not seen here` line in that run fails the script. Proof here (root): at ee2f4159 the plant gives "caught (exit 1)" with `left: Ok(8)`, over the host's `/dev` (`rw,relatime`) and over a `/dev` remounted `nosuid,noexec`. The path of a runner, with the planted binary over a `/dev` remounted `nosuid,noexec`: as uid 65534 the test writes `not seen here` and passes; as root it fails with `left: Ok(8)`. On a runner the first run is as the runner's user, and the second run with `sudo -n` catches the plant. `sh scripts/ci/userns-tests.sh` passes as root.

# T-761: The nightly at `1739b5f` is red on each cross target: the lab test of `pod-lab` expects a seccomp lab to run where its filter cannot hold

**Source:** `nightly.yml` run 43817142905 at `1739b5f` (jobs `cross` of i686 133131677560, armv7 133131677398, arm 133131677473, aarch64 133131677451, loongarch64 133131677416 and powerpc64le 133131677401).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The test binary `lab` of `crates/pod-lab/tests/lab.rs` stopped at the first case of the host classes (T-714), in `cases::ran`:

- i686 and powerpc64le: "listen-denied: the lab listen-denied does not hold: listen-refused: Refused(Verdict { ... Seen { what: "the hold", state: Fails, errno: None, detail: "on this target a socket call can go through socketcall(2), which the filter does not see" } ... })".
- armv7, arm, aarch64 and loongarch64 (qemu-user): the same, with the detail "seccomp(SET_MODE_FILTER): ENOSYS (38)".

The binary has no libtest: the cases after the first panic did not run.

## Premise

- Read: the logs of the six jobs; `crates/pod-lab/src/hold.rs` (`cannot`: a lab refuses a seccomp filter of `socket`, `listen` or `getsockname` where the target has `socketcall(2)`; `seccomp`: the error of the kernel call is the reason of the part); the job `cross` of `.github/workflows/nightly.yml` (each test binary runs on the runner, under qemu-user where the target is not x86).
- The lab answers as the crate defines it: it refuses the run, and names the hold that it cannot set. The cases of `crates/pod-lab/tests/lab/hosts.rs`, `crates/pod-lab/tests/lab/net.rs` and `crates/pod-lab/tests/lab/way_out.rs` expect each lab to run.
- qemu-user does not give the call `seccomp(2)`: `ENOSYS`.
- Measured here (x86_64 host, root): the binary for i686-unknown-linux-musl, natively, failed in the same case before the fix. With the case of `listen-denied` refused, it failed next in `proxy-only` (`net.rs`) and in `proxy-only` with a way out (`way_out.rs`), with the same socketcall detail.
- Not measured: a run under qemu-user (no QEMU on this host).

## Approach

`crates/pod-lab/tests/lab/filter.rs`, `refused_here`, for each case of the three files: on a target with `socketcall(2)`, a lab that filters a socket call must refuse with the reason of socketcall, and the case checks that and ends; a lab that does not refuse there fails the case. On a host where each part that does not hold failed at `seccomp(SET_MODE_FILTER): ENOSYS`, the case writes a skip line. The main target is not changed: a skip line fails `scripts/ci/userns-tests.sh`, and x86_64 has no `socketcall` path in the lab.

## Prove

```sh
cargo test -p pod-lab --all-features --test lab
cargo zigbuild -p pod-lab --all-features --test lab --target i686-unknown-linux-musl
```

Then run the binary for i686 from `crates/pod-lab`. The next nightly proves the targets under qemu-user.

## Done

2026-11-16, commit `6f6bb0a`. `crates/pod-lab/tests/lab/filter.rs` (`refused_here`) ends each case of `hosts.rs`, `net.rs` and `way_out.rs` (a new start of the binary exits with its own code) where the lab cannot hold its filter: a target with `socketcall(2)` must refuse, a host whose `seccomp(2)` gives `ENOSYS` gets a skip line. `cargo test -p pod-lab --all-features`: passed (x86_64, as root; uid 65534 cannot reach the scratch target here). The binary for i686-unknown-linux-musl, natively as root: it failed at `listen-denied` before the fix; after it, each case passes, with `listen-denied`, `udp-denied`, `raw-denied` and `proxy-only` (and `proxy-only` with a way out) "refused where the kernel has socketcall(2)". A planted `SOCKETCALL = false` in `crates/pod-lab/src/hold.rs` makes that binary fail: "listen-denied: no refusal where the kernel has socketcall(2)". No plant in `scripts/plants/`: the runners of `plants.yml` are x86_64, where the path is not taken. The next nightly proves the targets under qemu-user.

# T-762: The nightly at `1739b5f` is red under qemu-user: the test of the rows of `pod-hostprobe` expects no value of a mount flag that the kernel did not fill

**Source:** `nightly.yml` run 43817142905 at `1739b5f` (jobs `cross` of armv7 133131677398, arm 133131677473, aarch64 133131677451, loongarch64 133131677416 and powerpc64le 133131677401).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The binary `host_rows` of `crates/pod-hostprobe/tests/host_rows.rs` failed on each target under qemu-user:

```text
host_rows: creat($HOME/.pod-hostprobe)        OK
host_rows:     noexec unknown
assertion `left == right` failed: Outcome { verdict: Ok, errno: None, reason: "noexec unknown" }
  left: Some("unknown")
 right: None
```

## Premise

- Read: `crates/pod-hostprobe/src/probes/files.rs` (`flag`: `1`, `0`, or `unknown` where `statfs(2)` gives no `ST_VALID`); `crates/pod-hostprobe/src/sys/statfs.rs` (`Statfs::flag` gives `None` with no `ST_VALID`); `crates/pod-hostprobe/tests/host_rows/rows.rs`.
- The crate is correct: qemu-user gives no `ST_VALID`, and the row says `unknown`. The test maps `None` to no value. The row `statfs(/etc/ssl)` has the same flaw in its check of `rdonly`; the run stopped before it.
- On i686 (no emulator) the row gives `noexec 0`, and the test passes.
- Not measured: a run under qemu-user.

## Approach

The test maps the flag as the crate does: `flag_text` gives `unknown` for `None`. The checks of `noexec` of `HOME` and of `rdonly` of `/etc/ssl` use it.

## Prove

```sh
cargo test -p pod-hostprobe --all-features --test host_rows
```

The next nightly proves the targets under qemu-user.

## Done

2026-11-16, commit `6f6bb0a`. `flag_text` of `crates/pod-hostprobe/tests/host_rows/rows.rs` gives `unknown` for a flag with no `ST_VALID`, as the crate does; the checks of `noexec` of `HOME` and `rdonly` of `/etc/ssl` use it. `cargo test -p pod-hostprobe --all-features`: passed. No test fails here without the fix, and no plant: the path needs a kernel with no `ST_VALID` (qemu-user). The next nightly proves the targets under qemu-user.

# T-763: The nightly at `1739b5f` is red on windows-2025: a test of `pod-link` writes a POST on TLS and reads the answer with no flush

**Source:** `nightly.yml` run 43817142905 at `1739b5f` (job `test (windows-2025)` 133131677537).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`a_body_up_goes_into_the_link_by_parts_and_a_cut_keeps_its_part` of `crates/pod-link/tests/poll_lanes.rs` failed on windows-2025 again after the fix of T-681: "the answer in time: Elapsed(())", after 30 s, at the read of the answer of the second POST.

## Premise

- Read: the log; `tokio-rustls` 0.26.6, `Stream::poll_write`: it gives the count of the bytes that rustls took when the socket cannot take more, and the records of TLS stay in rustls until the next write or a flush; `poll_read` writes no record. The test writes the POST again (128 KiB) with `write_all` and reads the answer with no flush. Where the socket of the client takes less than the body at once (the small send buffer of Windows), the last records do not go, and the listener waits for the rest of the body.
- Measured here: a client of TLS through a pipe of 4 KiB to its socket (`connect_narrow` of `crates/pod-link/tests/common/poll.rs`) gives the same failure at the same place, after 30 s. With a flush after the write, the test passes 20 times of 20.
- The fix of T-681 (`tokio::join!` of the answer and the rest) was not the cause; it stays.

## Approach

The test flushes after the write of the POST, and its second POST goes through `connect_narrow`, so that each host shows the small send buffer of Windows. `ask_on` flushes after its write too.

## Prove

```sh
cargo test -p pod-link --all-features --test poll_lanes
sh scripts/plant-each.sh link-poll-tls-write-unflushed
```

The next nightly proves windows-2025.

## Done

2026-11-16, commits `6f6bb0a` and `a4b7c73`. The test flushes after the write of the POST again; its second POST goes through `connect_narrow` (a pipe of 4 KiB to the socket); `ask_on` flushes. Before the fix, with `connect_narrow`, the test fails here after 30 s: "the answer in time: Elapsed(())", as on windows-2025; after it, 20 runs of 20 pass. `cargo test -p pod-link --all-features`: 225 passed. `sh scripts/plant-each.sh link-poll-tls-write-unflushed`: caught (exit 101). The next nightly proves windows-2025.

# T-764: The nightly at `1739b5f` is red on windows-2025: the lower bound of a silence of `pod-link`'s split test does not allow the tick of the timer of Windows

**Source:** `nightly.yml` run 43817142905 at `1739b5f` (job `test (windows-2025)` 133131677537).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`a_silent_up_half_ends_the_session_with_up_carrier_lost_on_each_side` of `crates/pod-link/tests/split.rs` failed on windows-2025: "a silence is a loss after lost_after alone: 337.5207ms", against a lower bound of `lost_after - beat` (400 ms - 50 ms).

## Premise

- Read: the log; `crates/pod-link/src/split/run.rs` (each sender writes a beat after a sleep of `beat`; each reader takes a silence of `lost_after` from the last frame as the loss).
- The silence starts at the last frame before the cut. The test took one beat as the most time from that frame to the cut. 400 ms - 337.5 ms = 62.5 ms, which is four ticks of the timer of Windows (15.625 ms): a sleep of 50 ms there takes 62.5 ms.
- Not measured: the test on Windows; this host has a timer of 1 ms.

## Approach

The bound is `lost_after - 2 * beat` (300 ms), with the reason in a comment. It still sees a loss that comes before `lost_after`, as a plant that halves `lost_after` shows.

## Prove

```sh
cargo test -p pod-link --all-features --test split
sh scripts/plant-each.sh link-split-silence-early
```

The next nightly proves windows-2025.

## Done

2026-11-16, commits `6f6bb0a` and `3301197`. The lower bound is `lost_after - 2 * beat`, with the tick of the timer of Windows in its comment. `cargo test -p pod-link --all-features --test split`: 6 passed. `sh scripts/plant-each.sh link-split-silence-early`: caught (exit 101). No test fails here without the fix: the timer of this host has 1 ms. The next nightly proves windows-2025.

# T-765: The nightly at `1739b5f` is red on macos-15-intel: a dial of `pod-proxy` to a freed port gets a reset, not a refusal

**Source:** `nightly.yml` run 43817142905 at `1739b5f` (job `test (macos-15-intel)` 133131677450).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`a_failed_dial_keeps_the_kind_of_its_last_error` of `crates/pod-proxy/tests/egress.rs` failed on macos-15-intel: "assertion `left == right` failed; left: Some(ConnectionReset); right: Some(ConnectionRefused)".

## Premise

- Read: the log; `crates/pod-proxy/tests/closed/mod.rs` (`port`: on macOS, the port of a listener that is bound and dropped, as T-067 made it; macOS drops a SYN to a port that a socket holds with no listen); `common::dead()` of `crates/pod-warp/tests/common/mod.rs` (T-680, call C656) and `crates/pod-dns/src/tests/cuts.rs` do the same on macOS.
- A port with no socket gives a refusal. A reset of a connect needs a socket that took the connection: a listener of a parallel test of the same binary that got the freed port and closed with the connection in its queue. The helpers say that this race is seldom; this run shows it.
- Not measured: the test on macOS; this host checks the build alone.

## Approach

On macOS the closed port of the three helpers is `127.0.0.1:1`: a bind to port 0 gives a port of the ephemeral range alone, so no listener of a test can take port 1, and nothing listens there. The other hosts keep the socket that is bound with no listen (call C742).

## Prove

```sh
cargo test -p pod-proxy --all-features --test egress --test dial --test fetch --test get --test dialer
cargo test -p pod-dns --lib cuts
sh scripts/agent/check-macos.sh
```

The next nightly proves macOS.

## Done

2026-11-16, commit `6f6bb0a` (call C742). On macOS `closed::port` of `pod-proxy`, `common::dead()` of `pod-warp` and the refused port of `cuts` of `pod-dns` are `127.0.0.1:1`. `cargo test --all-features`: `pod-proxy` 210 passed, `pod-dns` 77, `pod-warp` 209. `sh scripts/agent/check-macos.sh`: 0 faults. No test fails here without the fix, and no plant: the change is in the path of macOS alone. The next nightly proves macOS.


# T-766: The nightly at `1739b5f` is red on riscv64: two time limits of tests that the emulation of QEMU outruns

**Source:** nightly run 43817142905, job 133131677430 (`cross (riscv64gc-unknown-linux-musl, riscv64)`), beside T-761 and T-762.
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

Under qemu-user for riscv64, two tests failed that pass on each other
target of the run:

- `crates/pod-guest/src/ready/tests.rs`,
  `a_black_screen_or_the_first_screen_again_is_not_ready`: "pointer: each
  frame was read, and the last one again". The wait has 300 ms; under the
  emulation the stand-in of the screen gives fewer dumps than the frames
  and one more in that time.
- `crates/pod-egress/tests/vless.rs`, `a_session_goes_over_a_websocket_and_each_mode_of_xhttp`
  and `http1_alone_carries_packet_up_and_stream_up_on_two_connections`:
  "the bytes within the limit: Elapsed(())" (the echo of the session in
  `LIMIT * 2`).

The nightly run 43842885011 at `b6b1f47` (job 133215680847) failed the
same two, and two more of the same class:

- `crates/pod-link/tests/poll.rs`,
  `a_proxy_that_cuts_idle_connections_holds_no_push_back` (the chunked
  form): "the dial: CarrierError { carrier: \"chunked\", kind:
  Io(BrokenPipe), text: \"the acceptance of the listener at ...\" }". The
  proxy of the test cuts a connection that is idle for 2 s
  (`dialing.hold`); under the emulation the TLS handshake of the dial
  holds the connection idle longer.
- `crates/pod-relay/tests/http_road_one_h2.rs`,
  `the_one_form_carries_4_mib_each_way_on_one_stream_of_h2`: "the
  session's end in time: Elapsed(())" (4 MiB each way in its limit).

armv7 (T-789) passed in that run.

## Premise

- Read: the log of the job (the two tests, the panics at
  `ready/tests.rs:217` and `vless.rs:57`); the tests.
- Not measured: the time that each step takes under the emulation of
  riscv64; the tests pass on x86_64 here.

## Approach

Measure each step under qemu-user for riscv64 (`scripts/binfmt.sh`).
The screen test waits for a count of dumps, not for a fixed 300 ms: the
deadline grows until the stand-in gave each frame and one more, with a
bound. The VLESS test takes its limit from one setting of the test kit
for a slow target (the scale of the timers of QEMU, as the other cross
jobs read it), with the reason in a comment. No check of the main target
grows weaker. The same for the h2 road of `pod-relay` (its limit) and
the hold of the cutting proxy of the poll test of `pod-link` (the hold
grows with the scale, so that a handshake under the emulation is not a
cut).

## Prove

```sh
cargo test -p pod-guest --all-features --lib -- ready::tests
cargo test -p pod-egress --all-features --test vless
cargo test -p pod-link --all-features --test poll
cargo test -p pod-relay --all-features --test http_road_one_h2
```

# T-779: `pod-identity`: one stack of curve25519 and of SHA-2: take `ed25519-dalek` 3

**Source:** The study of zmux (`docs/research/zmux.md`, lesson 6; the dependency list of `pod-link` that the study captured).
**Category:** chore
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

The default build of `pod-link` (no feature) has two stacks of each
curve and hash crate, both through `pod-identity`: `ed25519-dalek` 2.2.0
takes `curve25519-dalek` 4.1.3, `sha2` 0.10.9 and `digest` 0.10.7;
`x25519-dalek` 3.0.0 takes `curve25519-dalek` 5.0.0; `pod-identity`
itself takes `sha2` 0.11.1 and `hkdf` 0.13 (`digest` 0.11.3). Each binary
with `pod-identity` (each consumer) compiles and links both. The
workspace pins `ed25519-dalek` 2 (`Cargo.toml`), while `ed25519-dalek`
3.0.0, on `curve25519-dalek` 5 and `sha2` 0.11, is in `Cargo.lock`
already through `russh`.

## Premise

- Measured (2026-11-16, at `567a56c`): `cargo tree -p pod-link -e normal
  -d` names `curve25519-dalek` 4.1.3 and 5.0.0, `sha2` 0.10.9 and
  0.11.1, `digest` 0.10.7 and 0.11.3; the study's list at `62b8223` has
  the same.
- Read: `crates/pod-identity/Cargo.toml`; the entries of `ed25519-dalek`
  in `Cargo.lock`; `deny.toml` (`multiple-versions = "warn"`).
- Not measured: the size that a release binary saves; the API changes
  of `ed25519-dalek` 3 in `pod-tls` and `pod-relay`.
- The RSA verifier of `pod-tls` (feature `rsa`) keeps `digest` 0.10
  through `rsa` 0.9: out of the default build, not part of this entry.

## Approach

Set the workspace dependency `ed25519-dalek` to 3, with the features of
today. Change the calls of `pod-identity`, `pod-tls` and `pod-relay` to
its API (the keys from bytes, the `rand_core` 0.10 traits, the PKCS#8
features where a crate reads or writes a key file). The key files and the
signatures do not change: Ed25519 is the same algorithm. Run the tests of
each crate that takes `pod-identity`, the static probe, and the
adoption trials.

## Prove

```sh
cargo tree -p pod-link -e normal -d
cargo test -p pod-identity --all-features
cargo test -p pod-tls --all-features
cargo test -p pod-relay --all-features
python3 -I scripts/check-probe.py
```

The first command names no second version of `curve25519-dalek`,
`sha2` or `digest`.

# T-780: A test reads the pipes of its child only after the child ends: a child that writes more than a pipe holds stops (`pod-term` calibration)

**Source:** The study of zmux (`docs/research/zmux.md`, lesson 7; a failure of podssh that the study measured).
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

The study ran the tests of podssh away from CI. The test of
`podssh doctor` (`PodUtils/podssh:crates/podssh-cli/tests/doctor_upgrade.rs`)
spawns the command with its stdout and stderr piped, then polls
`try_wait()` and reads nothing until the child ends. `podssh doctor`
wrote 87,463 bytes; the pipe holds 65,536; the child waited in
`pipe_write` by the third second, and the test failed at its limit of
120 s. The same form is in podcrates:
`crates/pod-term/tests/calibration.rs:171` polls `try_wait()` on a tmux
child whose pipes it reads only after the end (`wait_with_output`). A
pane of tmux is small today, so it passes; a larger pane, a verbose
tmux or a pipe of 16 KiB (the first size on macOS) makes it a stop at
the limit with a message that names the time, not the pipe.

The study also found a test of podvm that cleared the proxy variables of
`https://` and `all_proxy` for its child but not `http_proxy`: the child
took the host's proxy. `pod-proxy` reads eight variables in four lists
(`PROXY_VARS`, `HTTP_PROXY_VARS`, `NO_PROXY_VARS` and the orders of
podnet in `crates/pod-proxy/src/env.rs`); `pod-lab` keeps its own list of
eight for its test. No helper gives a child an environment with no way
out.

## Premise

- Measured by the study (2026-11-16): the numbers of podssh above.
- Read: `crates/pod-term/tests/calibration.rs`,
  `crates/pod-testkit/src/lib.rs`, `crates/pod-proxy/src/env.rs`,
  `crates/pod-lab/tests/lab/way_out.rs`; the other calls of
  `try_wait` in `crates/` (`pod-guest`, `pod-egress`, `pod-testkit`)
  give the child no pipe or a file.
- Not measured: the calibration test with a pane of more than 64 KiB.

## Approach

1. `pod-testkit` gets a function that runs a `Command` within a limit
   (scaled for an emulator, as `host::scaled` does): it spawns the child
   with both pipes, reads each pipe on a thread while it waits, and at
   the limit kills the child and gives an error with the limit and the
   bytes read until then. The calibration test of `pod-term` takes it.
2. `pod-testkit` gets a function that removes from a `Command` each
   variable that names a proxy or its bypass (the eight of `pod-proxy`).
   A test of `pod-proxy` checks that the list of `pod-testkit` holds each
   name of `PROXY_VARS`, `HTTP_PROXY_VARS` and `NO_PROXY_VARS`.
3. `docs/development.md` names both functions in the rules of a test
   that runs a child.

## Prove

```sh
cargo test -p pod-testkit
cargo test -p pod-proxy --all-features
cargo test -p pod-term --all-features --test calibration
```

A test of `pod-testkit` runs a child that writes 1 MiB to stdout and
1 MiB to stderr, and gets both whole within the limit; a child that
sleeps past the limit gives the error with the limit named.

# T-781: The findings of the study of zmux for the tests of podssh, podnet and podvm, by COORD issues

**Source:** The study of zmux (`docs/research/zmux.md`, lesson 8 and section 3).
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

The study ran the test gates of the consumers on one host away from CI
(2026-11-16) and found tests that encode the environment of CI. A
session of podcrates does not edit a consumer (`AGENTS.md`, section 1),
so the evidence goes to each session:

- podssh (at `b551245`):
  `PodUtils/podssh:crates/podssh-cli/tests/doctor_upgrade.rs`, test
  `the_three_failures_of_the_upgrade_are_named_apart`,
  failed with "podssh doctor still ran after 120 s (refuse:403)". The
  test polls `try_wait()` with both pipes piped and unread; the command
  wrote 87,463 bytes against a pipe of 65,536, and waited in
  `pipe_write`.
- podnet (at `830c722`):
  `PodUtils/podnet:tests/pod_seccomp_diff.rs`, test
  `seccomp_errnos_match_the_host`, failed on a
  host with no route: "udp connect: host "errno 101", pod "0"". The
  parity test needs a route to `192.0.2.10` and does not say so.
- podvm (at `8f88ff1`): `PodUtils/podvm:crates/podvm/src/broker/upstream_tests.rs`
  resolves `example.com` and fails with `EAI_AGAIN` with no DNS;
  `the_store_root_prefers_podvm_store_and_never_falls_to_tmpdir_silently`
  fails when `XDG_DATA_HOME` is set, which `Store::default_root` reads
  first; `PodUtils/podvm:crates/podvm-image/tests/proxy_pull.rs` removes `https_proxy`,
  `all_proxy` and `no_proxy` (both cases) for its child but not
  `http_proxy`, and the pull took the host's proxy (403). The run was
  green after the four changes of the environment and `sshd` on `PATH`.

## Premise

- Read: the logs of the study (the report and its data are not in this
  repository; `docs/research/zmux.md`, section 3, has the facts); the
  list of the proxy variables of podvm's test through the forge mirror
  (2026-11-16).
- Not measured: the same tests on the CI of each consumer; the trees of
  the consumers after the pins of the study.

## Approach

One COORD issue for each consumer (`(to podssh)`, `(to podnet)`,
`(to podvm)`, `docs/maintenance.md`, "Talk between sessions"), each with
its findings above, the log line, the mechanism, and the offer of the
helpers of T-780 (a child within a limit with its pipes read; a child
with no way out) after their release. Map each issue in `TODO/issues.md`
and `TODO/github.txt`. Close each one on the answer of its session.

## Prove

```sh
cargo todo check
python3 -I scripts/check-github-numbers.py
```

Each of the three issues is in `TODO/issues.md` with its state.

# T-788: The nightly at `f309b07` is red on ppc64le: a test of `pod-enter`'s `pass_clear` reads a closed number that another test thread can take

**Source:** nightly run 43829527325, job 133171228001 (`cross (powerpc64le-unknown-linux-musl, ppc64le)`).
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`crates/pod-enter/src/pass_clear/tests.rs`,
`a_root_on_a_passed_number_moves_above_each_passed_number`: "the copy
stayed open". The test drops `Moved` and then checks with `fstatat` that
the number of the copy is closed. Another thread of the test binary can
open a descriptor between the close and the check, and take the same
number. T-739 fixed the same class in
`the_kind_names_a_directory_and_a_pipe` (it reads a closed number over
900).

## Premise

- Read: the log of the job (the panic at `pass_clear/tests.rs:44`); the
  test.
- Not measured: the failure on x86_64; the test passed in each full run
  here.

## Approach

The test checks the close of the copy by an identity that a reuse cannot
fake: it keeps the device and inode of the copy before the drop (or opens
the copy as a file with a mark), and after the drop it checks that the
number is closed or holds another file. The other tests of the module that
check a closed number get the same form.

## Prove

```sh
cargo test -p pod-enter --all-features --lib -- pass_clear
```

## Done

2026-11-16. The test opens a directory of its own
(`pod-enter-pass-clear-moves-PID` of the temporary directory), keeps the
device and inode of the root, and after the drop of `Moved` checks that
the number of the copy no longer names that directory, closed or reused
by another thread, which cannot open it. The other tests of the module
read no closed low number (`the_kind_names_a_directory_and_a_pipe` reads a
number over 900). Plant `enter-pass-clear-copy-kept` (the drop leaves the
copy open). Here: `cargo test -p pod-enter --all-features --lib --
pass_clear` (3 passed). The next nightly checks ppc64le.


# T-789: The nightly at `f309b07` is red on armv7: two tests of the MQTT board of `pod-wormhole` against the stand-in

**Source:** nightly run 43829527325, job 133171228262 (`cross (armv7-unknown-linux-musleabihf, arm)`).
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

`crates/pod-wormhole/tests/board.rs`, on armv7 under qemu-user, with no
Mosquitto ("did not run: no Mosquitto here"), against the stand-in:

- `a_board_puts_takes_and_clears_a_note`: "the stand-in keeps no note
  after the clear" fails: the stand-in still holds a retained note after
  the clear of the board.
- `a_bad_topic_or_payload_is_refused_before_a_connection`: the put of a
  payload of `PAYLOAD_MOST` (64000 bytes) to a broker that does not answer
  is not `Failed::Unreachable`.

Both pass on each other target of the run. The earlier nightly at
`1739b5f` stopped other binaries of armv7 before this one.

## Premise

- Read: the log of the job (the panics at `board.rs:55` and `board.rs:94`).
- Not measured: the tests under qemu-user for armv7; the order of the
  clear and the check in the stand-in; what the put of `PAYLOAD_MOST`
  gives there.

## Approach

Run the test binary under qemu-user for armv7 (`scripts/binfmt.sh`), and
print what the put gives and what the stand-in holds after the clear. A
cause in the crate (a size or a time limit of 32 bits, an order of the
retained message and the clear) gets its fix; a cause in the stand-in or
the test (a check before the stand-in read the clear) gets a wait for the
event, not a fixed sleep.

## Prove

```sh
cargo test -p pod-wormhole --all-features --test board
```

# T-790: `pod-services`: the Syncthing relay of the device DVWOU27 moved to 1.168.112.83

**Source:** `services.yml` run 43842385221 (scheduled, at `b6b1f47`).
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The watch `http-json` of `syncthing-relay` gave: "MOVED syncthing-relay
http-json: services/syncthing-relay.toml endpoint relay-dvwou27 is
1.168.114.174; the service gives host "1.168.112.83" (source: url
https://relays.syncthing.net/endpoint/full, read 2026-11-12)".

## Premise

- Read: `https://relays.syncthing.net/endpoint/full` on 2026-11-16 through
  `fetch.web.pkgforge.dev`: the relay of the device DVWOU27 (the same id,
  the same global rate of 3000000 bytes/s, provided by the same operator)
  is at `relay://1.168.112.83:443`.

## Approach

Change the host of the endpoint `relay-dvwou27` in
`crates/pod-services/services/syncthing-relay.toml`, with the new read
date of its source and of its device id.

## Prove

```sh
cargo test -p pod-services --all-features
cargo run -q -p pod-services-watch -- --service syncthing-relay
```

## Done

2026-11-16. The endpoint `relay-dvwou27` is `1.168.112.83:443`, read
2026-11-16; the fact of its device id has the same read date. The consumers
read it at the next tag. Here: `cargo test -p pod-services --all-features`
passes. The next scheduled run of `services.yml` checks the watch.


# T-793: `check-sources.py` reads a file that a part take names at the commit of the take of its dir: `pod-egress` shows 3 changes

**Source:** `upstream.yml`, job `sources`, red each day since 2026-11-13 (runs 43712799629 and 43872758011, at `a2ee305`).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The job `sources` prints:

```
pod-egress: changed PodUtils/podssh:crates/podssh-ws/src/shadowsocks/ws.rs -> crates/pod-egress/src/shadowsocks/ws.rs
pod-egress: changed PodUtils/podssh:crates/podssh-ws/src/vless/packet.rs -> crates/pod-egress/src/vless/packet.rs
pod-egress: changed PodUtils/podssh:crates/podssh-ws/src/vless/xhttp.rs -> crates/pod-egress/src/vless/xhttp.rs
check-sources: pod-egress: PodUtils/podssh at 2ffb7cd, taken at 3f6a9fe (22 paths), taken at 94ee6ac: 91 files, 3 changes
```

The part take of `crates/pod-egress/SOURCES` at `3f6a9fe` names the three
files, because that take changed them. The take of the whole block at
`94ee6ac` is later. The check reads each `dir` at the commit of the dir's
take (`94ee6ac^`) and reads a file of a dir only from that read: the
three files come from `94ee6ac^`, which holds the change of `3f6a9fe`.

## Premise

- Read: `compare` in `scripts/check-sources.py`; the blob ids of the three
  files in a clone of podssh at `2ffb7cd`, `3f6a9fe^`, `3f6a9fe` and
  `94ee6ac^`. At `3f6a9fe^` each blob is the recorded one; at `94ee6ac^`
  each is the blob of `3f6a9fe`. No commit of podssh between `2ffb7cd`
  and `3f6a9fe^` touches them.
- Not measured: a run of the check through the mirror after the fix (the
  rev of each path is the same in the two readers).

## Approach

In `compare`, read a file of a dir again at its own commit when its take
differs from the take of its dir. A file that is not at that commit is
`removed`. Plant: the old read of the file from its dir.

## Prove

```sh
python3 -I scripts/check-sources.py --clone PodUtils/podssh=CLONE pod-egress
sh scripts/plant-each.sh sources-part-take-in-dir
```

The first command exits 0 with a clone of podssh.

## Done

2026-11-16, commit `d757d9e1`. `compare` reads a file of a dir again
at the commit of its own take when that differs from the take of its dir;
a file that is not at that commit is `removed`. Before, with clones of
podssh, podnet, podvm and podrunner: `pod-egress` 3 changes. After:
`check-sources: pod-egress: PodUtils/podssh at 2ffb7cd, taken at 3f6a9fe
(22 paths), taken at 94ee6ac: 91 files, 0 changes`, and each of the 59
blocks has 0 changes (exit 0). The mirror answered 403 here; the job
reads clones. Plant `sources-part-take-in-dir`: caught, exit 1
(`plant-each: 1 caught, 0 missed or stale`; `--apply`: 0 stale).


# T-794: `pod-manual`: podnet took the crate at `9217b0b`; the `taken` line is not in `SOURCES`

**Source:** `upstream.yml`, job `sources` (run 43872758011, at `a2ee305`).
**Category:** chore
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The job `sources` prints 9 changes of the block of podnet of
`crates/pod-manual/SOURCES`: PodUtils/podnet:src/manual/model.rs,
`pager.rs`, `roff.rs`, `text.rs` and PodUtils/podnet:scripts/gen-llms.py
are removed; PodUtils/podnet:src/manual/mod.rs, `facts.rs`,
`inventory.rs` and PodUtils/podnet:tests/cli/manual.rs changed, by
`9217b0b`, `13b1514`, `3aa1ee7` and `dd903e3` of podnet.

## Premise

- Read: the log of podnet from `78a3911` on these paths, in a clone.
  `9217b0b` (PodUtils/podnet@T-359, 2026-11-15) adds `pod-manual` at tag
  `v0.54.0` to its `Cargo.toml` and removes `model.rs`, `text.rs`,
  `roff.rs` and `pager.rs`: it is the take. `13b1514`, `3aa1ee7` and
  `dd903e3` are after it. No commit between `78a3911` and `9217b0b^`
  touches the paths. `TODO/PROGRESS.md` says that the help and the manual
  of podnet come from `pod-manual`.
- `adoption/podnet/` holds no trial of `pod-manual`.

## Approach

Add `taken 9217b0b` to the block of podnet (`docs/maintenance.md`, "The
take of a consumer"). T-700 stays partial: the takes of podssh, podvm and
podrunner wait.

## Prove

```sh
python3 -I scripts/check-sources.py --clone PodUtils/podnet=CLONE --clone PodUtils/podssh=CLONE2 pod-manual
```

The command exits 0 with the clones.

## Done

2026-11-16, commit `d757d9e1`. The block of podnet has `taken
9217b0b` with its comment. Before: 9 changes. After:
`check-sources: pod-manual: PodUtils/podnet at 78a3911, taken at 9217b0b:
9 files, 0 changes`. No trial file of `pod-manual` was in `adoption/`.
T-700 stays partial for the takes of podssh, podvm and podrunner.


# T-795: `pod-manual`: the text fixes of podssh in `data.rs` and `flags.rs` change no form that the crate took

**Source:** `upstream.yml`, job `sources` (run 43872758011, at `a2ee305`).
**Category:** chore
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The job `sources` prints 2 changes of the block of podssh of
`crates/pod-manual/SOURCES`:
`PodUtils/podssh:crates/podssh-cli/src/flags.rs` (`8a8e72cc`) and
`PodUtils/podssh:crates/podssh-cli/src/man/data.rs` (`d0ef9738`,
`8a8e72cc`), both of PodUtils/podssh@T-790.

## Premise

- Read: `git diff cfc3ae3 5340fbb` of the two files in a clone of podssh.
  Each changed line is in a string of podssh's own tables: no work id, and
  no sentence past 25 words (the rules of `pod-manual`). The form that the
  crate took (`Code` of `data.rs`; `FlagKind` and `FlagRow` of
  `flags.rs`) did not change. The crate holds no text of these tables.
- podssh did not take the crate: its `Cargo.toml` has no `pod-manual`,
  and `man/model.rs`, `roff.rs`, `text.rs` and `pager.rs` are still there.
  PodUtils/podssh@T-790 is partial.

## Approach

No port. Move the block of podssh to `5340fbb` with the new blob ids of
the two files, and say in its comment that only the form of the two files
is frozen (call C763).

## Prove

```sh
python3 -I scripts/check-sources.py --clone PodUtils/podnet=CLONE --clone PodUtils/podssh=CLONE2 pod-manual
```

The command exits 0 with the clones.

## Done

2026-11-16, commit `d757d9e1`. No port: each changed line is text of
podssh's own tables. The block of podssh moved to `5340fbb`, with the
blobs `581b5b7` (`flags.rs`) and `31d7949` (`data.rs`); its comment names
the frozen form (call C763). Before: 2 changes. After:
`check-sources: pod-manual: PodUtils/podssh at 5340fbb: 10 files, 0
changes`.

# T-816: The dig study and the ingest study of 2026-11-16 as research documents, with their corrections

**Source:** the dig study of 2026-11-16 (an archive of the owner); the ingest study of 2026-11-16 (an archive of the owner).
**Category:** docs
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The findings of the two studies are only in archives outside the repository. The ingest study has seven defects of its own (a count of keys that differs from its CSV, a trial count of 50,000 labelled 200,000, a table of residual loss that is a binomial model and not a run of the codec, a test run of `pod-link` with no `--all-features` in which 17 test binaries ran no test, stale claims about T-777 and T-778).

## Premise

- Read: both archives (the reports of two agents of this session); `docs/research/` and its rule (readings at a time).

## Approach

Write two documents under docs/research, one for each study: what each measured and how, the numbers with their corrections, the limits, and the entry of each finding (T-812 to T-832).

## Prove

```sh
cargo todo check
```

## Done

2026-11-17. `docs/research/dig-study.md` and
`docs/research/ingest-study.md`: what each archive measured and how, the
numbers (the ingest study's with its seven corrections), the limits, what
the repository has (no measurement of the rate of a relay; the default relay
`tcp.ssh.relay.ajam.dev` first; its caps still hold each session), and the
entry of each finding.

# T-817: A call on references under GPL or AGPL: their designs, never their code

**Source:** the ingest study of 2026-11-16 (an archive of the owner), section 9.
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The ingest study reads a GPL-3.0 proxy router and an AGPL-3.0 mesh VPN; the project is 0BSD (`AGENTS.md`, section 6). No call says what an entry may take from such a reference.

## Premise

- Read: `AGENTS.md` section 6; section 9 of the ingest study.

## Approach

Record the call in `docs/decisions.md`.

## Prove

```sh
cargo todo check
```

## Done

2026-11-17. Call C772 of `docs/decisions.md`: from a reference under GPL or AGPL, an entry takes the design, the wire facts and the constants that a document of the reference states, never its code; the entry names the reference and says so.

# T-854: `pod-services`: GitHub Actions has two more hosts (the watch of 2026-11-17)

**Source:** the scheduled run 44067490108 of `services.yml` (2026-11-17): `MOVED github-actions http-json "full-domains"`.
**Category:** chore
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`domains.actions_inbound.full_domains` of GitHub's meta API has two more hosts: `hosted-compute-request-orchestrator-gccus02-01.githubapp.com` and `-02`, after `hosted-compute-request-orchestrator-prod-eus-02.githubapp.com`. The fact `full-domains` of `services/github-actions.toml` lacks them, so a runner behind a list of allowed hosts cannot reach them.

## Premise

- Read: `https://api.github.com/meta` through the mirror (220 full domains; the wildcards hold); `crates/pod-services/services/github-actions.toml`; `crates/pod-services/tests/actions.rs`.

## Approach

Add the two hosts in GitHub's order, with the date read; the test holds a date of a reading on or after the first one.

## Prove

```sh
cargo test -p pod-services --all-features
```

## Done

2026-11-17, 3aee6da: the two hosts in GitHub's order (the list equals `actions_inbound.full_domains` of the meta API, 220 hosts), `read = 2026-11-17`. `tests/actions.rs` held each date at 2026-11-11; it now holds a date on or after it. `cargo test -p pod-services --all-features` passes. The consumers that read the list (podrunner) get the hosts with the next tag.

# T-855: `pod-services`: the node `node-trojan-167-88-61-175` fails the TLS handshake

**Source:** the scheduled run 44067490108 of `services.yml` (2026-11-17): `FAILED node-trojan-167-88-61-175 proxy-banner: TLS ... at 167.88.61.175:443: tls handshake eof`.
**Category:** chore
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The public Trojan node `node-trojan-167-88-61-175` ends the TLS handshake with no answer. A node that is gone stays in the list of podssh (PodUtils/podssh@T-574).

## Premise

- Read: the log of the job; crates/pod-services/services/node-trojan-167-88-61-175.toml; the removal of `node-trojan-149-28-130-108` (the same failure, removed after a second run); `docs/maintenance.md`, "A red run of `services.yml`".
- The dispatch of `services.yml` with `service = node-trojan-167-88-61-175` (2026-11-17).

## Approach

Read the run of the dispatch. When the failure stays, remove the node as `node-trojan-149-28-130-108` left: its file, the count of the rows `node-*` in `crates/pod-services/README.md`, and the id in the next hand-off for podssh.

## Prove

```sh
cargo test -p pod-services --all-features
cargo test -p pod-services-watch --all-features
```

## Done

2026-11-17, 79e6514: the dispatch of `services.yml` with `service=node-trojan-167-88-61-175` (run 44068342066) failed again: "TLS for blog.browseseniorrolesandenhanceyourcareertoday.com at 167.88.61.175:443: tls handshake eof", as the scheduled run 44067490108. The node left as `node-trojan-149-28-130-108` did: its file crates/pod-services/services/node-trojan-167-88-61-175.toml; the count of the rows `node-*` in `crates/pod-services/README.md` (7 to 6). `cargo test -p pod-services -p pod-services-watch --all-features` passes. The hand-off of the next tag tells podssh that the id leaves its list (PodUtils/podssh@T-574).

# T-862: `pod-broker`: names mode still resolves each name on the host, so a host whose names resolve only through its proxy refuses each CONNECT (PodUtils/podrunner#215)

**Source:** PodUtils/podrunner#215 (COORD-078 of podrunner, its finding F1 of a user QA of `v0.1.0-beta.7`; a blocker of its release).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

On a host with an authenticated proxy that intercepts TLS and no resolver for the user (`EAI_AGAIN`), each fetch of podrunner through `pod-broker` in names mode failed with 502 "system resolver: failed to lookup address information: Try again; DNS over HTTPS: not asked for this name". `Broker::check` resolved each name, also with `names_upstream`, and a failed resolve was a refusal, though the upstream resolves the name it gets.

## Premise

- Read: PodUtils/podrunner#215; `crates/pod-broker/src/serve.rs` (`check`, `resolve_once`, `connect`), `src/names.rs`; `tests/names.rs`.

## Approach

In names mode, a failed or empty local resolve does not refuse a name that passed each deny of a name and the policy: the name goes to the upstream, and the audit line says so. Address mode keeps the refusal. Call C777. A test; a plant.

## Prove

```sh
cargo test -p pod-broker --all-features --test names
```

## Done

2026-11-17, afbb70c9f: in `check`, a refusal of `resolve_once` (rule `resolve`: a failure, no address, or a timeout) in names mode gives no address, and the connection's note sets `upstream_resolved`; `connect` sends the name as before, and the audit line holds `"resolved":"upstream"` with no `addr`. Test: `names_mode_sends_a_name_that_only_the_upstream_resolves` (with a resolver that gives no address: names mode makes a tunnel to the name and says `resolved: upstream`; a name that the policy does not allow, and `localhost`, are still 403 with no dial; address mode is 502 with no dial). The tests of `pod-broker` pass. Plant `broker-names-mode-needs-a-local-resolver`, caught. Call C777.

# T-870: CI: each crate builds alone with its default features

**Source:** T-869 (a `cfg` of the feature `plain-ws` moved onto the module of `OpenError::lines`).
**Category:** chore
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The lint and test jobs of `ci.yml` build the workspace with `--all-features`, and only `pod-core` and `pod-tls` also build with no default feature. A consumer takes each crate with its default features. A `cfg` of a feature on the wrong item passes each check of CI and breaks each consumer: in T-869, `mod lines` took the `#[cfg(feature = "plain-ws")]` of `mod plain`, and `pod-relay` did not build with its default features; the check of `pod-cloudflare`, which takes `pod-relay` with fewer features, found it before a push.

## Premise

- Read: `.github/workflows/ci.yml` (the job `lint`).

## Approach

`scripts/ci/each-crate.sh`: `cargo check --locked -p CRATE` for each member of the workspace, with its default features; a step of the job `lint` runs it.

## Prove

```sh
sh scripts/ci/each-crate.sh
```

## Done

2026-11-17, 488cf44df: `scripts/ci/each-crate.sh` runs `cargo check --locked -p CRATE` for each member of the workspace (from `cargo metadata`), with its default features, and names each crate that does not build; the job `lint` of `ci.yml` runs it after the crates with no tokio. It passes on the workspace. The slip of T-869 fails it: `pod-relay` has no default feature, so `cargo check -p pod-relay` builds with no `plain-ws`, where `mod lines` was gone.


# T-914: `pod-cloudflare`: the reader of Cap'n Proto multiplies with overflow on a 32-bit target (the nightly at `62ce717`)

**Source:** nightly run 44067754895 at `62ce717`, the jobs `cross (armv7-unknown-linux-musleabihf, arm)`, `cross (arm-unknown-linux-musleabihf, arm)` and `cross (i686-unknown-linux-musl)`.
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** open

## Problem

`fuzzing::tests::each_target_survives_mutations` of `pod-cloudflare`
panics on each 32-bit target: "attempt to multiply with overflow" at
`crates/pod-cloudflare/src/cloudflare/capnp/reader.rs:132`, in
`get_struct_list`. The product `count * size` is a `usize`: a count of
2^30 and a size of 8 words overflow 32 bits. The edge sends this message,
so a peer controls both values. A release build wraps the product, and the
range check then reads a short range.

## Premise

- Read: the logs of the three jobs; `get_struct_list` (`count` is 30 bits
  of the tag, `size` is two 16-bit counts).
- Not measured: a release build on a 32-bit target with the planted
  message.

## Approach

Compute the range in `u64` with `checked_mul`, and refuse an overflow as
`Class::Protocol` before `check_range`. Read each other product and sum of
the reader in the same way. Keep the fuzz test as the proof, and add the
planted message of the overflow as a seed of the fuzz target
`capnp_message`.

## Prove

```sh
cargo test -p pod-cloudflare --all-features --lib fuzzing
cargo zigbuild --target i686-unknown-linux-musl -p pod-cloudflare --all-features --tests
```

# T-915: `pod-cloudflare`: the test of the quick tunnel stand-in does not build on Windows and counts two registrations on macOS (the nightly at `62ce717`)

**Source:** nightly run 44067754895 at `62ce717`, the jobs `test (windows-2025)`, `test (windows-11-arm)` and `test (macos-15-intel)`.
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

- Windows: `crates/pod-cloudflare/tests/quick_tunnel_standin.rs` uses
  `std::os::unix::fs::PermissionsExt` with no `#[cfg(unix)]`
  (`quick_tunnel_standin_checks_the_auth_user`): "cannot find `unix` in
  `os`", so no test of the nightly job runs on Windows.
- macOS: `quick_tunnel_standin_serves_a_visitor_over_http2` finds two
  registrations at the stand-in edge, not one
  (`crates/pod-cloudflare/tests/quick_tunnel_standin.rs:45`).

## Premise

- Read: the logs of the three jobs.
- Not measured: why the tunnel registers a second connection on macOS (a
  second leg of the race, or a retry after a reset of the first).

## Approach

Put the mode of the file behind `#[cfg(unix)]`. On macOS, read the second
registration (its `conn_index`): when it is a second leg by design, the
test holds each registration to the issued tunnel; when it is a retry,
find the reset. Run `sh scripts/agent/check-windows.sh` and
`sh scripts/agent/check-macos.sh` before the push.

## Prove

```sh
cargo test -p pod-cloudflare --all-features --test quick_tunnel_standin
sh scripts/agent/check-windows.sh
```

# T-916: The gate of no blocking call and eight time bounds fail under the emulation of QEMU (the nightly at `62ce717`)

**Source:** nightly run 44067754895 at `62ce717`, the jobs `cross` of armv7, arm, aarch64, i686, loongarch64, powerpc64le and riscv64gc.
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

- `pod_testkit::switches::count` (T-838) counts the voluntary switches of
  the thread. Under qemu-user a computation makes hundreds:
  `switches::tests::a_sleep_counts_and_a_computation_does_not` (6 jobs),
  `a_seal_and_an_open_block_on_nothing` of `pod-e2e` (6 jobs, "369
  voluntary switches in 1000 seals and opens") and
  `a_turn_of_the_arq_blocks_on_nothing` of `pod-link` (7 jobs, "735
  voluntary switches in 1000 turns").
- Time bounds that the emulation outruns: `pod-iroh` `road.rs`
  (`a_session_through_a_proxy_and_a_relay_carries_20_mib_each_way`, 3
  jobs), `share.rs` (2), `tickets.rs` (2, "no stream to the node:
  connection lost"); `pod-relay` `caller_headers.rs` and
  `http_road_one_h2.rs`; `pod-ws` `h2_road.rs` (eight sessions on one TLS
  connection); `pod-egress` `vless.rs` (2); `pod-session`
  `session_move.rs`; `pod-guest` `ready/tests.rs` (riscv64).

## Premise

- Read: the logs of the seven jobs.
- Not measured: the counts of switches of a computation under each
  emulator; which bounds of the eight tests are scaled by
  `pod_testkit::scaled`.

## Approach

`switches::count` gives `None` when `pod_testkit::emulated()` is true, and
the tests skip with the reason, as a skip of a missing capability does. Each time
bound of the eight tests goes through `pod_testkit::scaled`. A test whose
bound is scaled and still fails gets its cause found, not a larger bound.

## Prove

```sh
cargo test -p pod-testkit --all-features --lib switches
cargo test -p pod-e2e --all-features --test allocations
cargo test -p pod-link --all-features --test arq_machine
cargo test -p pod-iroh --all-features --test road --test share --test tickets
```
