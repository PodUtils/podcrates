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

- Read: `crates/pod-core/src/files/socket.rs`. The stage path is the
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
**Status:** done

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

Write the ruling in `docs/decisions.md`, and make `AGENTS.md` and
`docs/maintenance.md` agree with it.

## Prove

```sh
cargo todo check
```

## Done

2026-11-25, the commit of this entry (no code). The owner answered Q16 yes:
the rule of PodUtils/podrunner holds here. `docs/decisions.md` has the
ruling "Comment footer"; `AGENTS.md`, section 3, and `docs/maintenance.md`,
"Tools", state that a comment on GitHub may end with the "Generated by"
line of the harness and that a commit, a pull request title or body, and a
file have no attribution line. `cargo todo check`: 0 faults.

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
**Status:** partial

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

- 2026-11-24, `nightly.yml` run 45452272145 at `f39b8e2` (job 138652267213, riscv64): `a_black_screen_or_the_first_screen_again_is_not_ready`, `a_session_goes_over_a_websocket_and_each_mode_of_xhttp`, `http1_alone_carries_packet_up_and_stream_up_on_two_connections`, `a_proxy_that_cuts_idle_connections_holds_no_push_back` and `the_one_form_carries_4_mib_each_way_on_one_stream_of_h2` all passed (the vless tests and the h2 test ran for over 60 s and passed).
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

## Done

2026-11-25 (partial): `nightly.yml` run 45504345202 at `6f5f4951`: the job `cross (riscv64gc-unknown-linux-musl, riscv64)` passed `pod-guest`, `pod-egress` (`vless`) and `pod-link` (`poll`), the tests of this entry, in each of the last two nightlies (45452272145, 45504345202). In this run `pod_guest` took 58 s, `vless` 147 s and `poll` 44 s. Set to `partial`; set to `done` when a third nightly passes.

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
**Status:** partial

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

- 2026-11-24, `nightly.yml` run 45452272145 at `f39b8e2` (job 138652267160): `a_board_puts_takes_and_clears_a_note` and `a_bad_topic_or_payload_is_refused_before_a_connection` passed on armv7, as at `0f5dad6`. Two nightly runs in a row pass; the cause is not found.
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

## Done

2026-11-25 (partial): `nightly.yml` run 45504345202 at `6f5f4951`: the job `cross (armv7-unknown-linux-musleabihf, arm)` passed `pod-wormhole` (`board`), the two tests of this entry, in each of the last two nightlies (45452272145, 45504345202). Set to `partial`; set to `done` when a third nightly passes.

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
**Status:** done

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

## Done

2026-11-18, 30a5557: `get_struct_list` computes the words of the elements in `u64` (`count` of 30 bits times a size of 17 bits fits), and refuses a list whose elements overrun the word count of its pointer ("the elements of a struct list overrun its pointer"), before `check_range`. Each other product and sum of the reader was read: the counts of `decode` and `bytes` are `u64` or bounded by the 29 bits of a list count, and `split_segments` saturates. The test `a_struct_list_whose_size_overflows_is_an_error` (2^29 elements of 8 words, 2^32 words) and the seed `fuzz/seeds/capnp_message/struct_list_overflow`. On `i686-unknown-linux-musl` (a static test binary of `cargo zigbuild`, run on the x86_64 host): without the fix the test panics with "attempt to multiply with overflow" at `reader.rs:132`; with it, `capnp` 10 passed and `fuzzing` 2 passed.

# T-915: `pod-cloudflare`: the test of the quick tunnel stand-in does not build on Windows and counts two registrations on macOS (the nightly at `62ce717`)

**Source:** nightly run 44067754895 at `62ce717`, the jobs `test (windows-2025)`, `test (windows-11-arm)` and `test (macos-15-intel)`.
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

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

## Done

2026-11-18, 537405b: the mode of the pass file of `quick_tunnel_standin_checks_the_auth_user` is behind `#[cfg(unix)]`. The two registrations of macOS are by design: `Config::default()` has `ha_connections: 2`, and the leg of `conn_index` 1 registers in the background (`tunnel.rs`, "The other HA legs register in the background"); a slow host shows it before the test reads the list (the log of the job: `conn_index` 0 and 1, the same tunnel and secret, both accepted). The test now takes one registration for each leg up to `ha_connections`, each with its own index, index 0 among them, and each with the issued account, secret and tunnel. A second Windows fault of the same class, which the first hid: `small_frames_of_slow_visitors_do_not_end_the_connection` (`crates/pod-cloudflare/src/edge_h2/tests.rs`) bound a `tokio::net::UnixListener`; its forward target is now TCP on the loopback, so it runs on each system (1 passed). `sh scripts/agent/check-windows.sh`: 0 faults. The macOS half is proven by the next run of `nightly.yml`.

# T-916: The gate of no blocking call and eight time bounds fail under the emulation of QEMU (the nightly at `62ce717`)

**Source:** nightly run 44067754895 at `62ce717`, the jobs `cross` of armv7, arm, aarch64, i686, loongarch64, powerpc64le and riscv64gc.
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** M
**Status:** partial

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
- On a native x86_64 host with a full workspace build and test run beside
  it, `a_seal_and_an_open_block_on_nothing` counted 2 voluntary switches
  once (2026-11-18), and 0 in each of 9 runs again. A switch that is not
  the code's own (a fault of a page of the binary that waits for the disk,
  a lock of the allocator between the threads of the test binary) fails
  the gate.

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
On a native host, count the major page faults (`ru_majflt`) beside the
switches, and run the closure once before the count, so that a switch of
a page fault does not count as a block of the code.

The rule of the ruling "Emulated nightly jobs" (2026-11-25): measure first.
Run step 2 of T-978 (the `opt-level` of the dependencies in an emulated job)
before any skip. Skip a network test of `pod-iroh` on an emulated job
(riscv64, loongarch64, powerpc64le) only where a native runner runs the
same test.

## Prove

```sh
cargo test -p pod-testkit --all-features --lib switches
cargo test -p pod-e2e --all-features --test allocations --test blocks
cargo test -p pod-link --all-features --test arq_machine --test arq_blocks
cargo test -p pod-iroh --all-features --test road --test share --test tickets
```

## Done

2026-11-18, partial (the proof is a run of `nightly.yml` on the cross jobs):

- `pod_testkit::switches::count` (14de10e) gives `None` under an emulator (`host::emulated()`), and subtracts the major page faults of the thread (`ru_majflt`) from its voluntary switches; `a_sleep_counts_and_a_computation_does_not` checks the `None` under an emulator.
- The bounds that guard a hang go through `pod_testkit::host::scaled`: `pod-iroh` `tests/common/mod.rs` (`limit()`, 60 s; `road.rs`, `share.rs`, `tickets.rs`, `proxy_env.rs`), `pod-relay` `http_road_one_h2.rs` (the carry), `pod-egress` `vless.rs` (each timeout and the deadline of a dial), `pod-guest` `ready/tests.rs` (the two deadlines of 300 ms).
- Not time bounds (the read of the logs of the seven jobs): `pod-relay` `caller_headers.rs` took the last head, which the polls of its open serial session can be (also on a native host): it takes the last head of the one form, and counts no poll as a request. `pod-ws` `h2_road.rs`: under the emulator the bytes of the first sessions delayed a later CONNECT past the wait of the road (`ANSWER_WAIT`), which retires the connection by design: the eight sessions open before the bytes flow. `pod-session` `session_move.rs`: the margin of 256 KiB before the cap is too short for the handshake at the scaled pace: 512 KiB under an emulator. `pod-iroh` `tickets.rs`: a node closed the new connection with its refusal before the first stream opened, and `Dialer::link_as` gave `Failed("no stream to the node: connection lost")`, so a refused client also got `Next::Retry`: `link_as` now reads the close reason (as `refused()`, `CLOSE_WAIT`) and gives `Refused`. The message of `refuses_a_stranger` names the result and the count of targets.
- Not done: run the closure once before the count (`count` takes `FnOnce`; the subtraction of the major faults covers a fault of a page of the binary).
- The proof run, `nightly.yml` 44275593101 at `2b11314`: the cross jobs of armv7, arm, aarch64, loongarch64 and powerpc64le pass. Left: on i686, which runs natively (no emulator, so `count` counts), `a_turn_of_the_arq_blocks_on_nothing` of `pod-link` counts 3 voluntary switches in 1000 turns, as in the run before (cause not measured: a lock of musl's allocator shared with the other test threads, or a cold first pass); on riscv64, `pod-iroh` `road.rs` breaks at about 120 s (`early eof`, a timer of the stack ends the session, not the bound of the test, which is 300 s there) and `share.rs` still meets its bound of 300 s (likely a stall, not a slow emulator). The riscv64 job met its limit of 90 minutes (T-938). The other failures of that run are new and have their entries (T-933 to T-937).


- 2026-11-19, the nightly 44505235227 at `8389e1c`: two more time bounds of the same class fail under QEMU. `pod-iroh`'s `two_sessions_of_one_process_share_one_connection_on_two_streams` (`tests/share.rs`: "each session's bytes within the limit: Elapsed" on loongarch64; "early eof" at `share.rs:52` on riscv64) and `a_session_whose_client_moves_on_the_relay_alone_resumes_whole` (`tests/new_address.rs` on riscv64: the far end acknowledged 491520 of 4685824 bytes in 103 s, each `Stall` line a slow reader, no lost byte). The same run had `a_session_through_a_proxy_and_a_relay_carries_20_mib_each_way` (loongarch64, powerpc64le, riscv64) and `a_turn_of_the_arq_blocks_on_nothing` (i686), each named above.
- 2026-11-20, the nightly 44739339618 at `8a2cdd9`: the same three tests, no new one of this class. `a_turn_of_the_arq_blocks_on_nothing` on i686 (job 136298711820: "3 voluntary switches in 1000 turns"); `pod-iroh`'s `a_session_through_a_proxy_and_a_relay_carries_20_mib_each_way` and `two_sessions_of_one_process_share_one_connection_on_two_streams` on loongarch64 (job 136298712051: `Elapsed` at the bound of 300 s, `road.rs:94` and `share.rs:155`) and on riscv64 (job 136298712144: "early eof" at `road.rs:47` after 99 s and at `share.rs:52` after 78 s). `main` at `783a657c` has no change of `pod-link`, `pod-iroh` or `pod-testkit`'s `switches.rs` after `8a2cdd9`. The armv7 failure of the run is T-1043.

- 2026-11-21, the nightly run 44792996234 at `62b8039`: the same three tests, no new test of this class. `a_turn_of_the_arq_blocks_on_nothing` on i686 (job 136485246572: "4 voluntary switches in 1000 turns"; the one failed binary of the job). On riscv64 (job 136485246660) the two failed binaries are `pod-iroh`'s `road.rs` ("early eof" at `road.rs:47`, a broken pipe of the writer at `road.rs:43`, 158 s) and `share.rs` (`Elapsed` at `share.rs:155`, 306 s); `pod_job` passed 25 there. On loongarch64 (job 136485246683) `share.rs` gave `Elapsed` at `share.rs:155` (304 s), and `road.rs` gave a new form at `road.rs:98`: "Layer(Ended { end: Closing, received: 20971520, sent: 20971520 })", in 290 s. Each byte crossed; then the far end's answer to the `CLOSE` of this side did not come within `LINGER` (10 s, `crates/pod-session/src/pump.rs`), a time of the session that the emulation outruns, not a bound of the test.
- 2026-11-21, the nightly run 44848003241 at `a7fb465`: the same three tests, no new test of this class. On i686 (job 136672368047) `a_turn_of_the_arq_blocks_on_nothing` counted "3 voluntary switches in 1000 turns", the one failed binary of the job. Its cause is found and fixed in T-1082: the other test threads of the binary hold the lock of the address space while musl's allocator unmaps and maps for the gate; each gate is now in a test binary of its own. On riscv64 (job 136672368153) the two failed binaries are `pod-iroh`'s `road.rs` ("early eof" at `road.rs:47`, a broken pipe of the writer at `road.rs:43`, 140 s) and `share.rs` ("early eof" at `share.rs:52`, 119 s). The "early eof" came after 78, 99, 119, 140 and 158 s in three runs, so the end comes from a timer of the stack under a slow clock, not from one fixed limit; `new_address.rs` passed in 607 s. Not reproduced on this host: no `qemu-riscv64`. Left for T-916: the two `pod-iroh` tests on the emulated targets (T-978 part 2 measures an optimised build of the dependencies first).

- 2026-11-24, the nightly run 45452272145 at `f39b8e2`: on riscv64 (job 138652267213) the two failed binaries of this class are `pod-iroh`'s `road.rs` (`a_session_through_a_proxy_and_a_relay_carries_20_mib_each_way`: "early eof" at `road.rs:47`, a broken pipe of the writer at `road.rs:43`, after 159.81 s) and `share.rs` (`two_sessions_of_one_process_share_one_connection_on_two_streams`: "early eof" at `share.rs:52`, after 119.62 s). The jobs of loongarch64, aarch64, armv7, arm and powerpc64le passed these two binaries, and i686 passed `a_turn_of_the_arq_blocks_on_nothing` (T-1082). The failures of the same run in other binaries are new entries: T-1295 to T-1298.

- 2026-11-25, `nightly.yml` run 45504345202 at `6f5f4951`: on riscv64 (job 138835537702) the two failed binaries are `pod-iroh`'s `road.rs` (`a_session_through_a_proxy_and_a_relay_carries_20_mib_each_way`: "early eof" at `road.rs:47`, a broken pipe of the writer at `road.rs:43`, after 221 s) and `share.rs` (`two_sessions_of_one_process_share_one_connection_on_two_streams`: "early eof" at `share.rs:52`, after 115 s). On loongarch64 (job 138835538233) `share.rs` gave `Elapsed` at `share.rs:155` ("each session's bytes within the limit") after 303 s; its `road.rs` passed in 189 s. The jobs of aarch64, armv7, arm and powerpc64le passed both binaries, and i686 passed `a_turn_of_the_arq_blocks_on_nothing`. No new test of this class.

2026-11-25, seen: `nightly.yml` run 45603454952 at `cd7cb5a`.
- `cross (loongarch64-unknown-linux-musl, loong64)` (job 139168091791): only `two_sessions_of_one_process_share_one_connection_on_two_streams` fails (`tests/share.rs:52`, `Custom { kind: UnexpectedEof, error: "early eof" }`, after 59 s). `road.rs` passes on loongarch64 in this run.
- `cross (riscv64gc-unknown-linux-musl, riscv64, 150)` (job 139168091852): `road.rs` (265 s, `early eof` at `road.rs:47`), `new_address.rs` (758 s: `Lost` after 30 s with no acknowledgement, "the session hung after the move"), `share.rs` (310 s, "each session's bytes within the limit: Elapsed") fail in `pod-iroh`. Two more tests fail only in this run, as the same class (a time of the emulation): `edge_h2::tests::small_frames_of_slow_visitors_do_not_end_the_connection` of `pod-cloudflare` (`edge_h2/tests.rs:165`, "an answer: Elapsed", 38 s) and `a_resolver_that_picks_http2_shares_the_connection` and `a_link_over_dns_over_https_through_a_proxy_carries_streams` of `pod-link` (`tests/dns_doh.rs:70`: 12 and 6 connections over HTTP/2, where 1 is due; 12.6 s and 22.7 s for the dial; 106 and 113 requests). Each of the three passed in the run 45504345202 (`dns_doh` took 6 s, `new_address` 383 s). The runner was slower in this run: `modes` of `pod-term` took 1900 s (816 s before), `pairing` 968 s (846 s before). The cause of the extra connections of `dns_doh` is not measured (a connection that the slow emulation lets time out, or the change of `pod-h2` of `aa52569d`); the ruling "Emulated nightly jobs" applies: measure first.

2026-11-27, seen: `nightly.yml` run 45806031483 at `f2f8e3f5`.
- `cross (powerpc64le-unknown-linux-musl)` (job 139861405326): the one failed binary is `pod-iroh`'s `road.rs`. `a_session_through_a_proxy_and_a_relay_carries_20_mib_each_way` fails at `road.rs:98` with "Layer(Ended { end: Closing, received: 20971520, sent: 20971520 })" after 238 s: each byte crossed, then the answer to this side's `CLOSE` did not come within `LINGER` (10 s). It is the form of loongarch64 of 2026-11-21, now on powerpc64le.
- `cross (riscv64gc-unknown-linux-musl, riscv64, 150)` (job 139861405165): cancelled at the time limit of the job (T-938).
- The other failed jobs of the run are T-1600 (Windows and macOS) and T-1601 (arm, armv7, i686).

# T-917: The copies of product code that `codegraph.py dups` finds: one place for each

**Source:** the first run of `python3 -I scripts/codegraph.py dups` (the ruling "Shared code").
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

Nineteen function bodies of product code are in two files or more, token
for token (`scripts/dups-baseline.txt`, the keys `clone:`). A fix in one
copy misses the others. Among them:

- `listen` of six carriers of `pod-link` (`dns.rs`, `h2.rs`, `quic.rs`,
  `tcp.rs`, `udp.rs`, `ws.rs`), and `serve` of `h2/listen.rs` and
  `tcp/listen.rs`;
- `read_packet` and `write` of the MQTT board and broker of `pod-wormhole`;
- `parse` of the Trojan and VLESS links, `dial` of Trojan, VLESS and
  VMess, and `random_below` of meek and WebTunnel in `pod-egress`;
  `check_answer` of WebTunnel beside `webtunnel_takes` of `pod-ws`;
- `pem` of `pod-lab` and `pod-warp`; `attr` of `pod-hold` and `pod-lab`;
  `tcp_packet` of `pod-warp` and `pod-wg`; the base64 value of
  `pod-cloudflare` and `pod-turn`; the PEM text of the key file of
  `pod-tpm` (`KeyFile::to_pem`, `KeyFile::from_pem`) does the same work as
  `pem` with code of its own, and takes the one place too;
- `carry_heard` of `pod-opentunnel` and `pod-piping`; `poll_read` of
  `pod-opentunnel` and `pod-session`; `all` of `pod-piping` and
  `pod-relay`; `status` of `pair.rs` and `pair/renew.rs`; `parse` of
  `h2_road.rs` and `quic_road/mod.rs` in `pod-relay`;
- `detail` of `machine.rs` and `supervise.rs` in `pod-hostprobe`;
- `trusted` of `scripts/agent/watch.py` and `scripts/issue-sync.py`.

## Premise

- Read: the output of `codegraph.py dups` on `main`.
- Not measured: whether each pair must stay the same (a copy that must
  differ later is no clone to merge).

## Approach

For each key: move the body to the crate that both copies take (the
lowest crate of `docs/design.md`, section 4, often `pod-core`), or to one
module of the crate, and call it from each place. A pair that must stay
two copies gets a comment that says why, and its key stays in the
baseline with that reason. Remove each key from the baseline in the
commit that removes the copy.

## Prove

```sh
python3 -I scripts/codegraph.py dups
cargo test --workspace --all-features
```

# T-918: The helpers that the tests copy from file to file: one place for each

**Source:** the first run of `python3 -I scripts/codegraph.py dups` (the ruling "Shared code").
**Category:** chore
**Milestone:** M5
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

Seventy-one function bodies of test code are in two files or more, token
for token (`scripts/dups-baseline.txt`, the keys `test-clone:`): the
scratch directory of a test, the reader of a root file, the `until` of a
wait, the harness of the mailbox of `pod-relay` and `pod-wormhole`, the
harness of `pod-e2e` and `pod-relay`, the far end of the tests of
`pod-session`, and more. Most are in `pod-relay` (29 copies), `pod-link`
(24), `pod-session` (17), `pod-egress` (12), `pod-iroh` and `pod-e2e`
(11 each).

## Premise

- Read: the output of `codegraph.py dups` on `main`.
- Not measured: which helpers differ only by a value that a parameter can
  carry.

## Approach

A helper of more than one crate goes to `pod-testkit` (behind a feature
when it needs a dependency); a helper of more than one test file of one
crate goes to a shared module of its tests (`tests/common/mod.rs`). A test
keeps its name and its assertions. Remove each key from the baseline in
the commit that removes the copy.

## Prove

```sh
python3 -I scripts/codegraph.py dups
cargo test --workspace --all-features
```

# T-919: A code graph, a check of repeated work and a check of narrative history in the documents

**Source:** the owner's ask of the housekeeping (a code graph, and mechanical checks that find work that a session did twice or missed); the rulings "Shared code" and "Writing".
**Category:** chore
**Milestone:** M0
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

A session writes a capability that podcrates has already (the ruling
"Shared code"), because no tool answers "what exists for X" across 53
crates and 750 entries. No check finds a function body that a session
copied into a second file, an entry that repeats another, a paragraph
that two documents repeat, or a row of `docs/design.md` whose pod
dependencies drifted from the code (ten rows had drifted). The documents
of the current state collect narrative history (the take of each
consumer with its commit, `state:` lines, "Before, ..."), against the
rule "Writing".

## Premise

- Read: the scripts of `scripts/` (`check-loopback.py` has the scanner of
  Rust that `check-private-replace.py` already takes), `TODO/INDEX.md`,
  the documents of `check-history.py`, section 4 of `docs/design.md`.
- Measured: a scan of the workspace reads 30577 items, 11140 bodies of 6
  lines or more, 751 entries and 740 paragraphs in about 4 s.

## Approach

`scripts/codegraph.py` with `scripts/codegraph_scan.py` (standard
library; the code view of `check-loopback.py`): `similar`, `find`,
`crate`, `graph`, `build`; `dups` against `scripts/dups-baseline.txt`
(each known finding with the entry that removes it: T-917, T-918); `new`,
the hint of a change. `scripts/check-history.py` with an exception list
that gives a reason for each line. Both run in `ci.yml` and in
`scripts/agent/check.sh`, each with its self-test and its plant.

## Prove

```sh
python3 -I scripts/codegraph.py --self-test
python3 -I scripts/codegraph.py dups
python3 -I scripts/check-history.py --self-test
python3 -I scripts/check-history.py
sh scripts/plant-each.sh codegraph-clone history-in-a-document
```

## Done

2026-11-18, f0013623: `scripts/codegraph.py` and `scripts/codegraph_scan.py` (`similar`, `find`, `crate`, `graph`, `build`, `dups`, `new`), `scripts/dups-baseline.txt` (90 keys: 19 with T-917, 71 with T-918), `scripts/check-history.py`; both in `ci.yml` and `scripts/agent/check.sh`. `codegraph.py --self-test` and `check-history.py --self-test` pass; `dups`: 90 known findings, no new one; `check-history.py`: 67 documents, no narrative history; `plant-each.sh codegraph-clone history-in-a-document`: 2 caught. `similar file lock` gives `pod_core::lock::FileLock` first. In the next commit, `new` compares the words of names (two words or more, a share of 0.66) and reads only `.rs`, `.py` and the entries of `TODO/`: against `v0.66.0` it prints 101 look-alikes for 727 new items, and no clone.

# T-920: `pod-guest`: a monitor socket path longer than `sun_path` waits for the whole bound, then fails with a wrong reason

**Source:** a run of `scripts/agent/check.sh` with a long `TMPDIR` (2026-11-18): 14 tests of `pod-guest --lib` failed.
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

A Unix socket path holds at most 107 bytes on Linux (`sun_path`, 104 on
macOS). A per-run directory under a long root gives a QMP monitor path
past that limit. Then the emulator cannot bind the monitor, and
`qmp::Monitor::connect` waits for the whole bound (120 s in
`provision`), then says "the QMP socket ... never appeared". That line is
not true: the path can never be bound. The tests bind their stand-ins
under `TMPDIR`, so a long `TMPDIR` fails 14 of them with "path must be
shorter than SUN_LEN".

## Premise

- Read: `crates/pod-guest/src/qmp.rs` (the wait), `plan.rs` and `dos.rs`
  (the argument `unix:PATH,server=on,wait=off`), the test helpers in
  `evidence/tests.rs` and `ready/tests.rs`.
- Measured: with `TMPDIR` at a path of 89 bytes the monitor path is 123
  bytes and the 14 tests fail; with `TMPDIR=/tmp` each of the 177 tests
  passes.

## Approach

Check the length of the monitor path when the plan is made, before the
emulator starts. Refuse a path past the limit of the target as
`Class::Config` at `Stage::Host` with one line that names the path, its
length and the limit. In the tests, take the stand-in sockets under a
short directory when `TMPDIR` is too long. Read each other socket path
that a crate of podcrates binds under a directory of the caller (a test
in the same class).

The emulator binds the monitor socket, not a crate of podcrates, so
`pod_core::files::bind_staged` (T-1114) does not apply here; the binds
of a crate at a long path are in T-1115.

## Prove

```sh
TMPDIR=/tmp/$(printf 'x%.0s' $(seq 90)) cargo test -p pod-guest --lib
cargo test -p pod-guest --lib
```

# T-932: A check of the temp files that the crates of podcrates make (PodUtils/podvm#567, lesson 5)

**Source:** PodUtils/podvm#567 (COORD-180 of podvm, "lesson 5"; ask to podcrates, podssh and podnet).
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

podvm's outside tester found faults of temp files in crates of podcrates (T-872: a temp file that a sweep saw before its rename; T-873: a probe file with a fixed name; T-891: a scratch path of a probe that was not new). podvm's lesson 5 asks each project to check each temp file of its own code: a new name (the pid and a nonce), an exclusive create, no link followed, a lock where another process can see it, and a removal on each path out.

## Premise

- Read: PodUtils/podvm#567; T-872, T-873, T-891.
- Not read: each place in `crates/*/src` that makes a temp file or a scratch directory (`tempfile`, `create_dir`, `.tmp`, `std::env::temp_dir`).
- T-1343 (PodUtils/podnet#711, COORD-119) is the shared helper of the Approach below: `pod_core::files::new_temp_dir` and `new_temp_file`, with the list of the places of this repository that name a path with the pid. T-932 does its audit and its check of CI on that list after T-1343.

## Approach

List each place with `python3 -I scripts/codegraph.py similar temp file scratch` and a search. Hold each to the five rules, or to one shared helper of `pod-core` where two or more copies make the same file (the ruling "Shared code"). A script of CI that fails on a new `temp_dir().join(` with a fixed name in product code, with its plant.

## Prove

```sh
cargo test --workspace --all-features
sh scripts/agent/check.sh
```

# T-933: `pod-cloudflare`: three tests fail on Windows, where they ran for the first time (nightly 44275593101)

**Source:** nightly run 44275593101 at `2b11314`, the jobs `test (windows-2025)` and `test (windows-11-arm)` (T-915 let the tests of the crate build there).
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

- `origin::tests::no_tls_verify_needs_the_configuration_of_the_consumer` fails at `crates/pod-cloudflare/src/origin.rs:360`: its `UnixTls` half needs Unix; on Windows `dial_unix` gives `Class::Unsupported`, not `Policy`.
- The process of the lib tests overflows its stack (`STATUS_STACK_OVERFLOW`), so 8 tests have no result: the thread of 256 KiB of `yaml::tests::a_deep_document_is_an_error` (`src/yaml/tests.rs:22-24`) is too small for 64 levels of a debug build on Windows. The depth cap holds; the stack of the test does not.
- `quick_tunnel_standin_falls_back_to_http2_when_udp_is_dropped` fails at `tests/quick_tunnel_standin.rs:233` ("no QUIC packet came"); the assertion drops the error of `recv_from`, so the log cannot tell a timeout from another error.

## Premise

- Read: the logs of the two jobs (an agent's read of run 44275593101).
- Not measured: the error of `recv_from` on Windows.

## Approach

Put the `UnixTls` half behind `#[cfg(unix)]`; give the thread of the deep document a stack that a debug build of each system holds (the test measures the cap, not the stack); print the `Err` of `recv_from`, then find its cause on Windows.

## Prove

```sh
cargo test -p pod-cloudflare --all-features
sh scripts/agent/check-windows.sh
```

## Done

2026-11-18, partial (the proof is the next run of the Windows jobs of `nightly.yml`): the `UnixTls` half of `no_tls_verify_needs_the_configuration_of_the_consumer` expects `Policy` on Unix and `Unsupported` where the system has no Unix socket; the thread of `a_deep_document_is_an_error` has 2 MiB (3000 levels with no cap still overflow it; the 64 levels of the cap fit a debug build of Windows); `quick_tunnel_standin_falls_back_to_http2_when_udp_is_dropped` reads the silent socket up to 20 times of 100 ms and names the last error. `cargo test -p pod-cloudflare --all-features --lib origin::tests` and `yaml::tests` (6 and 6 passed), `--test quick_tunnel_standin udp` (1 passed) on Linux; `sh scripts/agent/check-windows.sh`: 0 faults. Open: the cause of the UDP read on Windows, if the next run still fails there with its error.

2026-11-20: the run 44505235227 still fails there, with `WSAETIMEDOUT` (10060) of the test's own read: no datagram came. The cause is T-990: the endpoint on `[::]:0` is IPv6 only on Windows and sends nothing to `127.0.0.1`.

2026-11-20, the proof: the nightly run 44696810225 at `68fbc96`, jobs `test (windows-2025)` 136155413723 and `test (windows-11-arm)` 136155413649 (both green). On each job the three tests ran and passed, with no skip line: `origin::tests::no_tls_verify_needs_the_configuration_of_the_consumer`, `yaml::tests::a_deep_document_is_an_error` (the lib tests of `pod-cloudflare` end with no `STATUS_STACK_OVERFLOW`) and `quick_tunnel_standin_falls_back_to_http2_when_udp_is_dropped` (with the fix of T-990).

# T-938: The riscv64 job of `nightly.yml` meets its limit of 90 minutes

**Source:** nightly run 44275593101: `cross (riscv64gc-unknown-linux-musl, riscv64)` was cancelled at 90 minutes, in `pod-proxy` `blocking_streamed`; 365 of about 448 test binaries ran. The run of 44067754895 took 88 minutes.
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** partial

## Problem

The job has no margin under `timeout-minutes: 90` (`.github/workflows/nightly.yml:199`), so a slow test of one night cancels the rest and hides their results.

## Premise

- Read: the job logs of both runs.

## Approach

Split the test binaries of the emulated targets into two jobs by crate, or raise the limit with the time of each binary in the log; keep each job under its limit with a margin of a quarter.

The rule of the ruling "Emulated nightly jobs" (2026-11-25): measure first.
Run step 2 of T-978 (the `opt-level` of the dependencies in an emulated job)
before any skip. Skip a network test of `pod-iroh` on an emulated job
(riscv64, loongarch64, powerpc64le) only where a native runner runs the
same test.

## Prove

The next run of `nightly.yml`: each cross job ends, none cancelled.

## Done

2026-11-18, partial (the proof is the next run of `nightly.yml`: each cross job ends, none cancelled): the job `cross` takes its limit from its row (`timeout-minutes: ${{ matrix.minutes || 90 }}`), and the row of riscv64 has 150 minutes, a margin of a quarter over the time that the two runs show it needs. The step of the tests prints the seconds of each binary (`== N s: EXE`), so the next limit, or a split of the job by crate, comes from measured times. `check-workflows.py`: 0 faults.
- 2026-11-20, the nightly 44739339618 at `8a2cdd9`: the riscv64 job ended in 89 minutes (13:11 to 14:40), not cancelled, under its limit of 150; it failed on the two tests of `pod-iroh` of T-916. The loongarch64 job ended in 56 minutes.
- 2026-11-21, the nightly 44848003241 at `a7fb465`: the riscv64 job (136672368153) ended in 111 minutes (01:25 to 03:16), not cancelled, under its limit of 150; each of the 484 test binaries ran, and it failed on the two tests of `pod-iroh` of T-916.

- 2026-11-24, the nightly run 45452272145 at `f39b8e2`: the riscv64 job (138652267213) was cancelled at its limit of 150 minutes (17:52 to 20:22). It ran 414 of 545 test binaries; the 414th, `session_resume_twice` of `pod-session` (one test, `two_resumes_in_quick_succession_after_the_open_links_are_cut_move_bytes_both_ways`, with timeouts of 60 s in its code), did not end, and the 131 binaries after it did not run. The longest binaries before it: `modes` 1886 s, `pairing` 950 s, `new_address` 720 s. The step "Run the tests" of `nightly.yml` gives each binary a limit of 2700 s (`timeout -k 30 2700`), so that one hung binary is a failed binary and the others run. Open: find why this test does not end under its 60 s timeouts (a blocking call in a task of the runtime, or a thread that does not end); read the next riscv64 job.

- 2026-11-25, `nightly.yml` run 45504345202 at `6f5f4951`: the riscv64 job (138835537702) ended in 127 minutes (22:48 to 00:55; the step "Run the tests" 117 minutes), not cancelled, under its limit of 150. Each of the 557 test binaries ran and printed its seconds; `session_resume_twice` passed in 91 s (the binary that hung at the last nightly), and the job failed on the two binaries of `pod-iroh` of T-916. The longest binaries: `pairing` 846 s, `modes` 816 s, `new_address` 383 s, `blocking_streamed` 251 s, `streamed` 235 s; none met the limit of 2700 s. The margin of the job is 23 minutes: a slower runner can still cancel it. The loongarch64 job ended in 48 minutes, aarch64 in 36, arm in 48, armv7 in 46 and powerpc64le in 47. The hang of `session_resume_twice` did not come back (one run); keep the entry `partial` until a second run passes.

2026-11-25, seen: `nightly.yml` run 45603454952 at `cd7cb5a`: `cross (riscv64gc-unknown-linux-musl, riscv64, 150)` (job 139168091852) was cancelled at its limit of 150 minutes in `session_handover` of `pod-e2e`, after 374 of 557 test binaries (the run 45504345202 ran 557 in the limit). The runner was slow: `modes` of `pod-term` took 1900 s (816 s before), `pairing` of `pod-offline` 968 s, `new_address` of `pod-iroh` 758 s. The margin of 150 minutes does not hold on a slow night; the step has no limit for a binary, so one binary of 1900 s takes a share of the job. Next: measure the seconds of the 20 longest binaries of the two runs and decide on a split of the job by crate or a limit for a binary (`scripts/ci/test-limited.sh` runs the binaries of the jobs `test` only).

# T-943: The static job of `aarch64-apple-darwin` in `ci.yml` gets no runner, four runs in a row

**Source:** `ci.yml` at `353e23f` (44311135449), `a743937` (44319538339), `71ea36e` (44332544463) and `e51772e` (44338630652): the job `static (aarch64-apple-darwin, macos-15, false)` ended `cancelled` or "abandoned" with no runner (`runner_name` empty), after 16 minutes in the queue; the job `all` then fails. The job `static (x86_64-apple-darwin, macos-15-intel, false)` of the same runs got a runner and passed.
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`ci.yml` is red for a reason that is not in the code: no runner of the label `macos-15` (arm64) takes the static job. HANDOFF names this as a fault of the host ("not acquired by Runner") and says to run the job again; a fault in each run of a day is no longer a single fault, and each push waits on it.

## Premise

- Read: the job lists of the four runs (the arm64 job has no runner; the Intel job passes).
- Not read: the annotations of the cancelled jobs; whether the macOS jobs of `nightly.yml` and `ci.yml` share a limit of concurrent macOS runners of the organization.

## Approach

Read the annotations of a cancelled job. If the pool of arm64 runners is the cause, build the static probe of `aarch64-apple-darwin` on the Intel runner (`--target aarch64-apple-darwin`; the check of the binary reads its Mach-O header, so it needs no arm64 host), and keep an arm64 run in `nightly.yml`. The job `all` then waits on no arm64 runner.

## Prove

Three runs of `ci.yml` in a row with each job green.

## Done

2026-11-18: a fifth run (`32773e4`, 44346004330) showed the cause: the arm64 job stayed queued with no runner while the Intel job of the same run got one in 25 s. The matrix of the job `static` builds `aarch64-apple-darwin` on `macos-15-intel` (`--target aarch64-apple-darwin`); `scripts/check-static.sh` reads the Mach-O file with `otool -L`, which reads an arm64 file on an Intel host. The tests on arm64 stay in `nightly.yml` (`test (macos-15)`). `check-workflows.py`: 0 faults. The proof: `ci.yml` at `25f9938` (44347947644), `3882ae2` (44355702336) and `d40abaf` (44365173268), three runs in a row, each job green.

# T-949: `pod-link`: the gate of E2-1 at 20 % drop divides by a clean time of some 50 ms, so the noise of one run moves the factor past its bound (nightly 44388345504)

**Source:** the nightly run 44388345504 at `2b01acf`, job `gate`
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`twenty_percent_loss_with_nodelay_stays_within_the_factor_of_the_clean_time` of `crates/pod-link/tests/udp_loss.rs` failed: "E2-1, nodelay, 8388608 bytes each way: clean 47.491015ms, 20 % drop 380.223537ms, 8.01 times", against a bound of 8 (call C284). T-520 measured 3.7 to 4.0 times on a local host and 5.3 times on a GitHub runner. One run of each side gives the factor; a clean time of 47 ms makes the factor follow the noise of the scheduler of the runner.

## Premise

- Read: the log of the job; the test; T-520 and call C284.
- Not measured: the spread of the clean time and of the lossy time on a GitHub runner.

## Approach

Run each side three times and divide the median of the lossy times by the median of the clean times; print each time. Keep the bound of call C284. Measure the factor in ten runs of the job `gate`, and write the spread in the Done section.

## Prove

```sh
cargo test --release -p pod-link --all-features --test udp_loss -- --include-ignored --nocapture twenty_percent_loss_with_nodelay_stays_within_the_factor_of_the_clean_time
```

## Done

2026-11-18, commit "pod-link: the gate of E2-1 at 20 % drop divides the medians of five runs, bound 12 times (T-949, call C787)". The medians of three runs did not hold the bound of 8 (8.02 times once in three runs on a host of 4 cores); the medians of five gave 4.4 to 8.2 times in six runs. The factor divides the median of five lossy runs by the median of five clean runs, prints each time, and has the bound of 12 (call C787, a change of call C284). Three runs after the change: 5.68, 5.73 and 4.25 times. The test: `crates/pod-link/tests/udp_loss.rs::twenty_percent_loss_with_nodelay_stays_within_the_factor_of_the_clean_time`.

# T-950: `pod-link`: when one half of a split session goes silent, the end that sees the far end close first names the other half as lost (windows-2025)

**Source:** the nightly run 44388345504 at `2b01acf`, job `test (windows-2025)`
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

`a_silent_up_half_ends_the_session_with_up_carrier_lost_on_each_side` of `crates/pod-link/tests/split.rs` failed on windows-2025: "the dialer: down-carrier lost: a beat: broken pipe", where the up half went silent. The listener sees the silence of the up half and ends the session; its down carrier closes. The next beat of the dialer on the down half then fails with a broken pipe before the dialer's own reader sees the silence of the up half, so the dialer names the down half. The user of the dialer reads the wrong cause. The timer of Windows (15.625 ms ticks) makes the order more likely; any host can give it.

## Premise

- Read: the log; the test; T-764 (an earlier fault of the same test on windows-2025: a bound of the time).
- Not read: `crates/pod-link/src/split/run.rs` for the order of the beat writer and the silence reader on one end.

## Approach

A write error of a half after the far end closed is not the first cause: when the beat of one half fails, the end waits up to `lost_after` for the reader of the other half to name a loss, and takes the reader's cause when it comes. Or the far end sends its cause on the half that still works before it closes. A test with a far end that closes the down half at once after a silent up half: each side names the up half.

## Prove

```sh
cargo test -p pod-link --all-features --test split
```

## Done

2026-11-18, commit "pod-link: a failed write or read of a split half waits for the LOST frame that names the lost half (T-950)". `split/run.rs`: a write that fails, a read that fails, or a carrier that ends is a cause that the close of the far end also gives; it waits the grace (a beat, at most 500 ms) for the reader to take a `LOST` frame that came before the close (`Shared::lose_later`). A silence, a `LOST` frame and a wrong frame end the session at once, as before. Not taken: a wait of `lost_after` or of two beats (30 s or 20 s with the beats of production before a reset ends the session). A reset that drops the `LOST` frame leaves the cause of the failed half. Two unit tests on the paused clock; the plant `link-split-write-error-names-its-half`. `tests/split.rs` passed in each of 8 runs here; it takes 37 s in place of 20 s, as each of the 240 rounds of its race waits one beat of the test.

# T-954: The release binaries of podcrates are not on ghcr.io (the owner's ruling "ghcr.io", PodUtils/podrunner#312)

**Source:** the owner's ruling "ghcr.io" (2026-11-19, relayed in PodUtils/podrunner#312); call C789
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

Each release of podcrates attaches `podlab`, `pod-fake-github` and `pod-fake-githome` for x86_64 and aarch64 musl as GitHub release assets. The ruling "ghcr.io" asks each project to publish them on `ghcr.io/podutils/NAME` too, so that a consumer (podrunner pins the two fakes) takes each from one registry by digest.

## Premise

- Read: PodUtils/podrunner#312; PodUtils/podnet#656 (podnet's form since its issue 160); `.github/workflows/release.yml` (the assets, `SHA256SUMS`, the keyless cosign of `SHA256SUMS`); call C789.
- Not read: the job `image` of podnet's `release.yml`.
- Not measured: a push to ghcr.io from the release workflow of this repository.

## Approach

`release.yml`, on the tag: for each of the three binaries, one image for each architecture by the form of call C789 (`FROM scratch`, `/usr/local/bin/NAME`, the OCI labels), built from the files of `dist/` with `docker buildx` or `crane` (no build of the binary again), pushed under one index at `ghcr.io/podutils/NAME:TAG` with `packages: write`; `actions/attest` with `push-to-registry` attests the provenance of each index (`attestations: write`), as podnet's `release.yml` does (PodUtils/podnet#656). The release notes of `docs/releases/` (written before the tag) cannot hold the digests: the run on the tag writes them to the body of the GitHub release, and the hand-off COORD issue names them. The dry run builds the images and pushes nothing. A check pulls each index after the push and compares the binary with the asset by SHA-256.

## Prove

```sh
python3 -I scripts/check-workflows.py
```

The run of `release.yml` on the next tag: each index pulls, and its binary has the SHA-256 of its asset.

## Done

2026-11-19, commit `583a10c`, partial (the push waits for the run of `release.yml` on the next tag):

- `images/release/Dockerfile`: one stage for each binary (`podlab`, `pod-fake-github`, `pod-fake-githome`), `FROM scratch`, `COPY --chmod=0755 NAME-${TARGETARCH} /usr/local/bin/NAME`, `ENTRYPOINT ["/usr/local/bin/NAME"]`. One stage for each binary, because an exec-form `ENTRYPOINT` takes no build argument.
- `release.yml`: the job `release` uploads the six binaries and `SHA256SUMS` as the artifact `dist`. The job `image` (`needs: release`, a matrix of the three names; `contents: read`, `packages: write`, `id-token: write`, `attestations: write`) checks the files against `SHA256SUMS`, builds both platforms with `docker/build-push-action` (no provenance and no SBOM of buildx, so the index holds the two images only) to files, and compares each binary with its asset in each run. On the tag only: the push of `ghcr.io/podutils/NAME:TAG` with the labels `source`, `version` and `revision`; a pull of each platform by its digest and a compare with the asset; `actions/attest` v4.2.2 with `push-to-registry` (the pin of podnet's job `image`); `gh attestation verify` with `--signer-workflow`; a warning when the index does not pull with no login. The job `notes` (on the tag, `contents: write`) appends the section "The images, by digest" to the body of the release: the index digest and each platform digest, beside the asset and its SHA-256. A second run replaces the section.
- Proven here: `python3 -I scripts/check-workflows.py` (20 files, 0 faults). With docker 29.10 and buildx 0.39: each stage builds for `linux/amd64` and `linux/arm64` with the file of its architecture, mode 0755, the entrypoint and the labels. The step "The pushed index, pulled by digest", run as it is against a local CNCF distribution 3.1.2 registry, compares both platforms of each of the three indexes and writes 9 rows. The step of the job `notes`, run twice with a fake `gh`, gives one section.
- Waits for the next tag: the dry run on GitHub (the builder of `docker/setup-buildx-action`), the push to ghcr.io, the attestation in the registry and its check, the visibility of the three new packages, and the section in the body of the release. Then set this entry to done.

The run on the tag `v0.75.0` (44469848600) proved the rest: the jobs `image` of the three binaries pushed `ghcr.io/podutils/podlab`, `pod-fake-github` and `pod-fake-githome` at `v0.75.0`, each with `linux/amd64` and `linux/arm64`; each binary pulled back by digest has the SHA-256 of its asset; each index has its provenance attested and verified; each index pulls with no login (the packages are public); and the job `notes` wrote the section "The images, by digest" in the body of the release. Q17 needed no action.

# T-955: Dependabot of 2026-11-19: coturn 4.19.0, TruffleHog 3.99.9, rand 0.10.4 and rustls-pki-types; simple-dns 0.13.0 declined (GitHub #232, #233, #234, #235)

**Source:** GitHub #232, #233, #234, #235 (Dependabot)
**Category:** chore
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Four updates of Dependabot wait for a judgement (the ruling "Dependabot": each judged update is a commit of its own on `main`, and the pull request closes with a link to it).

## Premise

- Read: the diff of each pull request; the index digests of `coturn/coturn:4.19.0` and `trufflesecurity/trufflehog:3.99.9` at Docker Hub (they agree with the diffs); `crates/pod-iroh/Cargo.toml` (`simple-dns` is the version that `iroh-dns` builds with, so that no crate is added).
- The ci, deny and secrets runs of each pull request.

## Approach

Apply #232 (with the three lines of `crates/pod-turn/README.md` that name coturn 4.18.0), #233 and #234 as commits of their own after green checks. Decline #235: `iroh-dns` 1.4 builds `simple-dns` 0.12, and 0.13 adds a second copy of the crate (`Cargo.lock` +13); it moves with `iroh-dns`. Close each pull request with its reason.

## Prove

```sh
sh scripts/agent/check.sh
```

## Done

2026-11-19. #232: coturn 4.19.0 by the digest of its index, with the three lines of `crates/pod-turn/README.md` (`0bf30d0`); its coturn tests passed. #233: TruffleHog 3.99.9 by the digest of its index (`ac6a74b`); its secrets scan passed. #234: `rand` 0.10.4 and `rustls-pki-types` 1.16.0 by `cargo update` on the current `Cargo.lock` (`6ed0b21`), as the branch was from an older `main`; the full checks passed. #235 declined: `iroh-dns` 1.4 builds `simple-dns` 0.12, and 0.13 adds a second copy. Each pull request closed with its commit or its reason.

# T-968: The fuzz run of 2026-11-19 cannot build `russh`: the pinned nightly of 2026-11-01 meets a cycle in its query of async drop on `ssh-key` 0.7.0-rc.11

**Source:** the scheduled run 44519034117 of `fuzz.yml` at `a184eed`
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Each target of the fuzz run failed to build: `russh` 0.65.1 does not compile with `error[E0391]: cycle detected when computing when ssh_key::public::key_data::KeyData needs async drop` (`ssh-key` 0.7.0-rc.11). `russh` came into the fuzz workspace with `pod-devtunnel` (T-378, the target `devtunnel_connect_info`) after the last green fuzz run (43209945321 at `45b145a`). The fuzz run builds with `nightly-2026-11-01` (`scripts/fuzz.sh`), the address sanitizer and the release profile; a plain build of the same nightly and the stable build of `ci.yml` compile it.

## Premise

- Read: the job log of run 44519034117; `scripts/fuzz.sh`.
- Measured: `RUSTFLAGS="--cfg fuzzing -Zsanitizer=address -Cdebug-assertions -Ccodegen-units=1" cargo +nightly-2026-11-01 build --manifest-path fuzz/Cargo.toml --target x86_64-unknown-linux-gnu --release -p russh` fails with the same error; the same command with `nightly-2026-11-18` passes.

## Approach

Pin `nightly-2026-11-18` in `scripts/fuzz.sh`, and build each target of the fuzz workspace with it and the flags of cargo-fuzz before the push. Run `fuzz.yml` by hand after the push.

## Prove

```sh
RUSTFLAGS="--cfg fuzzing -Zsanitizer=address -Cdebug-assertions -Ccodegen-units=1" cargo +nightly-2026-11-18 build --manifest-path fuzz/Cargo.toml --target x86_64-unknown-linux-gnu --release --bins
```

## Done

2026-11-19, this commit. `scripts/fuzz.sh` pins `nightly-2026-11-18`. The command of the Prove section built each of the 72 targets of the fuzz workspace with it, the address sanitizer and the release profile, `russh` and `ssh-key` 0.7.0-rc.11 included. `fuzz.yml` runs by hand after the push. The run by hand (44527726675 at `af79a26`) built each target and met the limit of 75 minutes of its step "Each target": the targets grew from 39 (the last green run) to 72, and 72 runs of 60 s take 72 minutes before the build. `fuzz.yml` runs the targets in two jobs (`matrix.shard`), and `scripts/fuzz.sh` keeps the targets I, I+N, ... of `FUZZ_SHARD=I/N` (a bad value exits 64); the planted defect runs in shard 0. `shellcheck` 0.11.0 passes on the script.

# T-969: `upstream.yml` is red: the sources of `pod-job`, `pod-manual` and `pod-testkit` changed in podrunner and podssh

**Source:** the scheduled runs of `upstream.yml` 44096639741 at `6c8d8dd`, 44313451109 at `353e23f` and 44537712046 at `9ee2097`, job `sources`.
**Category:** chore
**Milestone:** M5
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

`scripts/check-sources.py` reports changes after the pin of three blocks:

- `pod-job`: 7 files of PodUtils/podrunner (`PodUtils/podrunner:crates/podrunner-schema/src/secret.rs`, `PodUtils/podrunner:crates/podrunner-slots/src/status.rs`, `PodUtils/podrunner:crates/podrunner-source/src/event.rs`, `PodUtils/podrunner:crates/podrunner-source/src/lib.rs`, `PodUtils/podrunner:crates/podrunner-source/src/tests.rs`, `PodUtils/podrunner:crates/podrunner-source/src/wire.rs`, `PodUtils/podrunner:docs/contracts/source-driver.md`).
- `pod-manual`: 8 files of PodUtils/podssh (`PodUtils/podssh:crates/podssh-cli/src/flags.rs`, `PodUtils/podssh:crates/podssh-cli/src/man/data.rs`, `PodUtils/podssh:crates/podssh-cli/src/man/mod.rs`, `PodUtils/podssh:crates/podssh-cli/src/man/model.rs`, `PodUtils/podssh:crates/podssh-cli/src/man/roff.rs`, `PodUtils/podssh:crates/podssh-cli/src/man/facts/tests.rs`, `PodUtils/podssh:crates/podssh-cli/src/pager.rs`, `PodUtils/podssh:scripts/interop-man.sh`).
- `pod-testkit`: 5 files of PodUtils/podrunner (`PodUtils/podrunner:crates/podrunner-broker-fake/src/auth.rs`, `PodUtils/podrunner:crates/podrunner-broker-fake/src/lib.rs`, `PodUtils/podrunner:crates/podrunner-broker-fake/src/message.rs`, `PodUtils/podrunner:crates/podrunner-broker-fake/src/runner.rs`, `PodUtils/podrunner:crates/podrunner-fake/src/source.rs`).

The commits include the takes of podcrates tags by the consumers (podrunner `e422c23` of v0.72.0, `0fd2218` of v0.73.0; podssh `c4a2a64` of `pod-manual`, `843e3c5` of v0.65.0), features that podcrates ported (T-947: `max_units` and `draining` of status.v1; T-951: source-conformance test 15), and changes not yet read (for example podrunner `ffc9658` "broker-fake: one job for each runner session", `e7e5d4c` "job-mismatch", `033c25d` "broker-fake: rsa 0.10", `bf1dd6f` "the tolerance of cpu_s", `a817f6f` drain, `f48fdc4` podnet version in hello; podssh's T-717, T-718, T-725, T-790, T-818, T-820 and T-825 commits).

## Premise

- Read: `scripts/check-sources.py` (the lines `repo`, `dir`, `skip`, `file`, `taken`); `crates/pod-job/SOURCES`, `crates/pod-manual/SOURCES`, `crates/pod-testkit/SOURCES`; the commits of podrunner and podssh after each pin (local read-only clones); T-608.
- Measured: `python3 -I scripts/check-sources.py` with local clones of podrunner, podssh and podnet at `9ee2097` reports 9 changes of `pod-job` (2 of them removed files: `PodUtils/podrunner:crates/podrunner-source/src/host.rs` and `PodUtils/podrunner:schemas/status.v1.json`), 10 of `pod-manual` (2 removed: `PodUtils/podssh:crates/podssh-cli/src/man/json.rs` and `PodUtils/podssh:crates/podssh-cli/src/man/text.rs`) and 5 of `pod-testkit`.

## Approach

For each changed file, read each commit after the pin. A take of a podcrates tag, or a change after the take of the crate, gets a `taken` line (by path where the block goes on). A fix of a frozen file before its take is ported into the crate, with a test, and the block pin moves. A feature of the consumer that the crate does not hold (podssh's own words of its manual and its flags) is a `taken` line by path when the consumer took the crate, else the block pin moves with a note. `python3 -I scripts/check-sources.py` with the clones gives 0 changes.

## Prove

```sh
python3 -I scripts/check-sources.py --clone PodUtils/podrunner=DIR --clone PodUtils/podssh=DIR
```

## Done

2026-11-19, commit ca005b5 (calls C800 to C803). Each change was read at
its commit. No change is a fix that the crates do not have, so no code is
ported:

- `pod-job`: the podrunner block of `status.v1` and the source wire moves
  from `ca8c308` to `d001608`, the parent of podrunner's take of v0.72.0,
  with the new blobs of tests.rs, source-driver.md, status.rs and
  status.v1.json. `taken e422c23` covers crates/podrunner-source/src,
  secret.rs and the contract; `taken 0fd2218` covers status.rs and
  status.v1.json. `draining` and `max_units` (`a817f6f`, `d09a2a3`) are
  in the crate by T-947. Test 14 (`f48fdc4`) and the tolerance of `cpu_s`
  (`bf1dd6f`) are podrunner's suite and the measure of its kinds.
- `pod-testkit`: the block of the fake broker moves from `d001608` to
  `e7e5d4c`, and the block of the fake source from `b1a1a28` to `b0f8a36`.
  podrunner does not use the crate, so there is no `taken` line. One job
  for each runner session (`ffc9658`) holds in the fake GitHub; the new
  test `a_runner_that_runs_its_job_takes_no_other`
  (`crates/pod-testkit/src/github/broker.rs`) proves it. `20f5a4b` (the
  run of a job), `033c25d` (rsa 0.10), `e7e5d4c` (`run_once_into`) and
  `b0f8a36` (the plant `RefuseCancelled`) are podrunner's own.
- `pod-manual`: the podssh block moves from `5340fbb` to `d8e54f0`, the
  parent of podssh's take of the writers. `taken c4a2a64` covers mod.rs,
  model.rs, roff.rs, text.rs, json.rs, data.rs, flags.rs and
  interop-man.sh; `taken 1e5de58` covers pager.rs. facts/tests.rs is a
  block of its own at `1e5de58`, as podssh keeps its own reader of the
  variables; its changes are podssh's list of sources. Each other change
  is podssh's own text and modules. podssh's COORD-115
  (PodUtils/podssh#312) asks for `pager::Interrupts`: it is public now,
  with the test `interrupts_ignores_sigint_and_sigquit_until_its_drop`
  (`crates/pod-manual/src/pager/tests.rs`).

`python3 -I scripts/check-sources.py --clone ...` with local clones of
podrunner, podssh, podnet, podvm and gsocket: each block of each crate
reports 0 changes (exit 0). `sh scripts/agent/check.sh`: fmt, clippy,
lint, doc, record, size, paths and static gave 0. `cargo test --workspace
--all-features` with the default `TMPDIR` passed: 507 test binaries, 0
failed.

# T-970: `pod-wormhole`: the stand-in tracker test reads the announces before the `stopped` of an end arrived (macos-15)

**Source:** the nightly 44505235227 at `8389e1c`, job `test (macos-15)`
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`two_ends_pair_through_the_swarm_of_a_code_and_move_messages_each_way` (`crates/pod-wormhole/tests/tracker.rs`) failed with "the ends leave the swarm": it read the announces of the stand-in tracker right after both ends closed. An end writes its `stopped` announce at its close (`tracker/link.rs`) and does not wait for the tracker to read it; the stand-in reads each message in its own task, so on a slow runner the test looked first.

## Premise

- Read: the job log; `crates/pod-wormhole/src/tracker/link.rs` (the close); the test.

## Approach

The test waits for a `stopped` announce within its limit, then checks the announces. The client does not change: a tracker owes no answer to `stopped`.

## Prove

```sh
cargo test -p pod-wormhole --all-features --test tracker
```

## Done

2026-11-19, this commit. `until_stopped` in `crates/pod-wormhole/tests/tracker.rs` reads the announces every 20 ms until one is `stopped`, within `LIMIT`. The test passed 3 runs of 3 on Linux.

# T-974: The run on the tag `v0.78.0` failed after the signature: two steps of `release.yml` share the scratch path `$RUNNER_TEMP/changed`

**Source:** the run on the tag 44627612644 of `release.yml` at `1f82e94`, job `release`, step "The verifier checks the signed release"
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The step "SHA256SUMS signed by this workflow at the tag" writes a changed copy of `SHA256SUMS` to the file `$RUNNER_TEMP/changed` and leaves it. The step after it (T-965) makes a changed copy of `dist` with `cp -R dist "$RUNNER_TEMP/changed"`, which fails: "cannot overwrite non-directory". The verifier had passed on the release before the copy ("verified: ... 11 files checked, 0 missing"). The job stopped, so the tag `v0.78.0` exists with no GitHub release and no images. Both steps run on a tag only, so the dry run did not run them. A run on a tag reads `release.yml` at the tag, so a fix on `main` cannot release `v0.78.0`.

## Premise

- Read: the job log; `.github/workflows/release.yml`.

## Approach

Each step that makes a changed copy uses a path of its own (`changed-index`, `changed-sums`, `changed-verify`), removes it before the copy and after its check. Release the content of `v0.78.0` as `v0.78.1` (call C818).

## Prove

The steps in order on the signed files of `v0.77.0` with a `pod-release` of `main`, with a file at the old path `$RUNNER_TEMP/changed`: the sums step and the verify step each exit 0, and a changed copy and another tag each give 77.

## Done

2026-11-19, this commit. `.github/workflows/release.yml`: `changed-index`, `changed-sums` and `changed-verify`, each removed before and after its step. The Prove ran with the 11 files of `v0.77.0` (their `SHA256SUMS` checks) and `target/debug/pod-release` of `main`: "checked with no signature: ... 9 files checked, 0 missing" and "verified: ... 9 files checked, 0 missing; signed by .../release.yml@refs/tags/v0.77.0"; a changed index, a changed `podlab-install.sh` and the tag `v0.0.0` each gave 77. `shellcheck` 0.11.0 passes on the three step bodies.

# T-978: The findings of the report of the hard problems that the record does not have

**Source:** `docs/research/hard-problems.md`
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** M
**Status:** partial

## Problem

`docs/research/hard-problems.md` gives the open problems of the record (H-01 to H-18), with the questions that research must answer. It has four findings that no entry has:

1. The step `Test pod-gsr (Windows ARM64, open defect)` of `nightly.yml` has no `always()`: when the step `Test` fails, the step is skipped, and T-131 has no measurement.
2. The workspace `Cargo.toml` sets no `opt-level` for the dependencies in the dev profile: the crypto dependencies of the emulated jobs of `nightly.yml` (riscv64, loongarch64, powerpc64le) build at opt-level 0. The "early eof" of `pod-iroh` in these jobs (T-916, T-938) can come from that. No run measured it (H-01).
3. The sharded `fuzz.yml` (T-968) has no run yet (H-14).
4. Three questions for the owner, answered on 2026-11-25 (the rulings "Assets of other licences", "Emulated nightly jobs" and "Sigstore roots"): a GPL-2.0 kernel (`ARCH=um`, T-975) and `runsc` (T-977) as assets of a release; the slow emulated targets skip the network tests of `pod-iroh`; a newer set of Sigstore roots for `pod-release` that TUF signs (H-07).

## Premise

- Read: `docs/research/hard-problems.md`; `.github/workflows/nightly.yml`; `Cargo.toml`.

## Approach

1. Give the step `if: always() && matrix.runner == 'windows-11-arm'`.
2. Measure one emulated job of `nightly.yml` with `[profile.dev.package."*"] opt-level = 2` against the same job with no change: the time of the tests of `pod-iroh` and the result. Keep the change if the job passes and is faster.
3. Run `fuzz.yml` by hand, and read both shards.
4. Write the questions in `TODO/PROGRESS.md`, "Questions for the owner", each with a recommendation.

The rule of the ruling "Emulated nightly jobs" (2026-11-25): measure first.
Run step 2 above (the `opt-level` of the dependencies in an emulated job)
before any skip. Skip a network test of `pod-iroh` on an emulated job
(riscv64, loongarch64, powerpc64le) only where a native runner runs the
same test.

## Prove

The next run of `nightly.yml` shows a result for the step `Test pod-gsr (Windows ARM64, open defect)`; one run of `fuzz.yml` with both shards.

## Done

2026-11-20, parts 1 and 4: the step has `always()`; the three questions went to the owner (answered 2026-11-25).

# T-1043: `pod-link`: the EDNS test of `dns_resolver` compares two counts that the resolver still moves (armv7)

**Source:** nightly run 44739339618 at `8a2cdd9`, the job `cross (armv7-unknown-linux-musleabihf, arm)` (136298712181).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`a_query_with_edns_gets_an_answer_within_its_size`
(`crates/pod-link/tests/dns_resolver.rs:185`) fails under QEMU on armv7:
"assertion `left == right` failed, left: 344, right: 346". The test
compares the count of answers with an OPT record to the count of all
answers. The stand-in resolver still carries the polls of the link after
the transfer, on another thread, and it adds to `answers` before
`with_opt`. The two loads are at two times, so a slow host reads two more
answers than answers with OPT. The armv7 job passed this test in the run
before.

## Premise

- Read: the log of the job; the stand-in resolver of the test.
- Not measured: the test under QEMU for armv7 (the next run of the cross
  job proves it).

## Approach

The resolver counts the answers with no OPT record; the test expects 0 of
them and more than 0 with one. Both counts only grow, so the read has no
race.

## Prove

```sh
cargo test -p pod-link --all-features --test dns_resolver
```

## Done

2026-11-20, this commit. `Seen` has `without_opt`; the stand-in resolver
adds each answer to `with_opt` or to `without_opt`;
`a_query_with_edns_gets_an_answer_within_its_size` expects no answer
with no OPT record and at least one with one. On Linux: `cargo test -p
pod-link --all-features --test dns_resolver` passes 3 (the three tests of
the file), and `cargo test -p pod-link --all-features` passes.

The proof under QEMU: the nightly run 44792996234 at `62b8039`, the job
`cross (armv7-unknown-linux-musleabihf, arm)` (136485246519):
`a_query_with_edns_gets_an_answer_within_its_size ... ok`, and "test
binaries that failed: 0".

# T-1044: `check-windows.sh` and `check-macos.sh` compile no doc test, so a doc test with an item of Unix fails only in the nightly

**Source:** T-1040 (a finding: the doc test of `pod-control` on Windows).
**Category:** defect
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

`scripts/agent/check-windows.sh` and `scripts/agent/check-macos.sh` run
`cargo check --tests`, which compiles no doc test. The doc test of
`crates/pod-control/src/conformance.rs` called `Client::connect_unix`; the
check of Windows gave "0 faults", and the failure came first in the
Windows jobs of `nightly.yml` 44739339618. `cargo test --doc --no-run`
is refused ("can't skip running doc tests with --no-run").

## Premise

- Measured (`x86_64-pc-windows-gnu`, `pod-control` alone): `cargo test
  --doc --target x86_64-pc-windows-gnu` with
  `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=true`, the zig wrapper of the
  script as the linker (`CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER`) and a
  wrapper `x86_64-w64-mingw32-dlltool` that runs `zig dlltool` (the
  `raw-dylib` of `getrandom` needs it) in `PATH` compiles and links the
  doc tests, and `true` runs none. It catches the E0599 of T-1040.
- Not measured: the whole workspace (the time, the disk, a crate whose
  link with zig fails); the same for `x86_64-apple-darwin`.

## Approach

A step `--doc` of each script with the recipe above, over the workspace.
Measure its time and disk; when it fits, `check.sh` names it beside the
two checks. A doc test that runs on the host stays the work of the
nightly.

## Prove

```sh
sh scripts/agent/check-windows.sh --doc
sh scripts/agent/check-macos.sh --doc
```

# T-1052: `pod-link`: the gate of E2-1 at 20 % drop divides by a clean time that the CPU of the host sets, so a fast runner gives a factor above its bound (nightly 44792996234)

**Source:** the nightly run 44792996234 at `62b8039` (the release `v0.80.0`), job `gate` (136485246532)
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`twenty_percent_loss_with_nodelay_stays_within_the_factor_of_the_clean_time`
of `crates/pod-link/tests/udp_loss.rs` failed: "E2-1, nodelay, 8388608
bytes each way: clean [20.052128ms, 22.385896ms, 23.112201ms,
23.189506ms, 38.222414ms], 20 % drop [232.420026ms, 295.108791ms,
298.374761ms, 434.663145ms, 449.453173ms], 12.91 times", against the
bound of 12 (call C787). The memory pair has no delay. The clean run is
bound by the CPU (some 20 ms on that runner, some 65 ms on a host of 4
cores), and the lossy run by the RTO of KCP (some 300 ms on each host).
The medians of five runs (T-949) remove the noise of one run, but not the
speed of the host: a faster runner makes the clean time smaller and the
factor larger. Row E2-1 of PodUtils/podnet#468 asks for "a 20% drop run
within 3 times the clean time", a factor of a path, where the clean time
is bound by the round trip.

## Premise

- Read: the log of the job; the test; T-949, T-520; calls C272, C284 and
  C787; row E2-1 in C272 and in T-515 (`TODO/relay.md`).
- Measured: the old gate here gave 4.1 to 6.1 times in 6 runs (clean
  medians 61 to 68 ms); the runner gave 12.91 times with a clean median of
  23 ms and a lossy median of 298 ms, the same lossy time as here.

## Approach

Each run of the gate crosses a path with a delay of 10 ms each way (a
round trip of 20 ms) over the memory pair, below the `Lossy` shim. The
window of 1024 segments of 1 KiB then holds 8 round trips for 8 MiB, so
the clean time is bound by the path and not by the CPU. The gate checks
that the clean median is at least one round trip for each window, so the
denominator cannot be small again. The bound of the factor goes back to
8, from the measured spread. A plant with no fast resend of KCP proves
that the gate still fails a slow recovery of loss.

## Prove

```sh
cargo test --release -p pod-link --all-features --test udp_loss -- --include-ignored --nocapture twenty_percent_loss_with_nodelay_stays_within_the_factor_of_the_clean_time
```

## Done

2026-11-21, commit "pod-link: the gate of E2-1 at 20 % drop crosses a path of 10 ms each way, bound 8 times (T-1052, call C878)". `tests/udp_loss.rs`: `Delayed` (a datagram end whose sends arrive 10 ms later, in their order, by one task for each end) under the `Lossy` shim of a memory pair with a queue of 8192 datagrams; `path_ends`; the gate asserts a clean median of at least 160 ms (8 windows times the round trip of 20 ms), then the factor of the medians of five runs at most 8 (`GATE_FACTOR`, call C878). Measured in a release build on a host of 4 cores, 18 runs: 4.05 to 6.18 times (clean medians 221 to 260 ms, each clean run 215 to 315 ms; lossy medians 1.03 to 1.37 s); 4 more runs with 4 busy loops on the 4 cores: 4.34 to 4.40 times. Delays of 1, 5 and 20 ms each way gave 4.7 to 6.4, 5.0 to 5.3 and 4.5 to 5.6 times. The gate takes some 7.5 s in place of 2 s. The plant `link-udp-no-fast-resend` (no fast resend of KCP) fails it: "11.61 times the clean time, above 8". A lossy run of 20.2 s in one of 50 runs at 20 ms each way: T-1053. The test: `crates/pod-link/tests/udp_loss.rs::twenty_percent_loss_with_nodelay_stays_within_the_factor_of_the_clean_time`.

# T-1053: `pod-link`: a transfer of KCP with `nodelay` at 20 % drop can take ten times its median, as the RTO of a segment grows by half at each timeout

**Source:** T-1052 (a finding of its measurement)
**Category:** measurement
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

In the measurement of T-1052, one lossy run of 8 MiB each way at 20 %
drop over a path of 20 ms each way took 20.24 s; the other 49 lossy
runs at that delay took 1.7 to 3.3 s.
With `nodelay`, the crate `kcp` 0.6.0 (`flush`) adds half of the RTO of
a segment at each timeout of that segment (ikcp's `nodelay` 1; ikcp's
mode 2 adds half of `rx_rto`, which the crate does not have), and a fast
resend stops after `fastlimit` (5) sends. While the oldest segment
waits, the window of 1024 segments fills and the stream stops. A user of
`nodelay` on a path with much loss then sees a stall of seconds. The
median of five runs of the gate holds it, so the gate does not show it.

## Premise

- Read: `kcp` 0.6.0 `src/kcp.rs` (`flush`: the growth of `rto`, the
  limit of the fast resend, `dead_link`).
- Measured: one run of 20.24 s in 50 lossy runs (a release build,
  the test path of T-1052 at 20 ms each way). The cause is not measured:
  the count of sends of the oldest segment in that run is not known.

## Approach

A test run that writes, for each lossy run, the time and the largest
count of sends of one segment (from the stats of the stream, or a
counter of the shim for each sequence number). Measure the tail in 200
runs at 10 and 20 ms each way. When the tail comes from the growth of
the RTO, compare a growth of half of `rx_rto` (ikcp's mode 2, a patch of
the crate in `vendor/`) and a `fastlimit` of 0; record the tail and the
fairness of each, and take a change only with a measured gain.

## Prove

```sh
cargo test --release -p pod-link --all-features --test udp_loss -- --include-ignored --nocapture twenty_percent_loss_with_nodelay_stays_within_the_factor_of_the_clean_time
```

# T-1082: The gate of no blocking call counts the waits that the other test threads of its binary cause (i686, nightly 44848003241)

**Source:** nightly 44848003241 job 136672368047 (`cross (i686-unknown-linux-musl)`), at `a7fb465`; the same failure in the nightlies 44505235227, 44739339618 and 44792996234 (T-916)
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

On i686, which runs natively (no emulator, so `switches::count` counts),
`a_turn_of_the_arq_blocks_on_nothing` of `pod-link` counts voluntary
switches: "3 voluntary switches in 1000 turns" (`arq_machine.rs:383`).
The 11 other tests of the binary `arq_machine` run on other threads of
the same process at the same time. musl's allocator unmaps and maps its
groups while the gate runs (strace: 11 `mmap2` and 16 `munmap` of the
gate thread in one failed run, and no futex wait). Each of these calls,
and a fault of a new page, takes the lock of the address space of the
process; when another test thread holds it, the gate thread sleeps, and
the kernel counts a voluntary switch that the ARQ did not make.
`a_seal_and_an_open_block_on_nothing` of `pod-e2e` has the same form:
it shares its binary `allocations` with two other tests.

## Premise

- Measured on this x86_64 host with the i686 musl build (`cargo zigbuild
  --target i686-unknown-linux-musl`): the binary `arq_machine` failed 33
  of 40 runs (1 to 14 switches); the gate alone (a filter of the one test)
  0 of 20; the whole binary with `--test-threads=1` 0 of 3.
- strace of a failed run: in the window of the count, the gate thread
  made `mmap2`, `munmap` and `mremap` calls only, many of them
  `<unfinished ...>` while other test threads ran.

## Approach

Each gate goes in a test binary of its own, with no other test:
`crates/pod-link/tests/arq_blocks.rs` (the helpers of the ARQ tests in
`tests/arq_common/mod.rs`, shared with `arq_machine.rs`) and
`crates/pod-e2e/tests/blocks.rs`. The documentation of
`pod_testkit::switches` gives the rule and its cause. The bound stays 0.

## Prove

```sh
cargo test -p pod-link --all-features --test arq_blocks --test arq_machine
cargo test -p pod-e2e --all-features --test blocks --test allocations
cargo test -p pod-testkit --all-features --lib switches
```

On i686: `cargo zigbuild -p pod-link -p pod-e2e --all-features --test
arq_blocks --test blocks --target i686-unknown-linux-musl`, then each
binary 100 times.

## Done

2026-11-21, this commit. `a_turn_of_the_arq_blocks_on_nothing` is
alone in `crates/pod-link/tests/arq_blocks.rs`, and
`a_seal_and_an_open_block_on_nothing` alone in
`crates/pod-e2e/tests/blocks.rs`; the bound stays 0 switches. The
helpers of the ARQ tests are in `crates/pod-link/tests/arq_common/mod.rs`.
The documentation of `pod_testkit::switches` gives the rule. On this host
with the i686 musl build, 100 runs of each binary: `arq_blocks` 0
failed, `arq_machine` 0 failed (33 of 40 before), `blocks` 0 failed,
`allocations` 0 failed (1 of 100 before: "1 voluntary switches in 1000
seals and opens"). The proof on the runner is the next run of
`nightly.yml` (job `cross (i686-unknown-linux-musl)`).

# T-1114: `pod-core`: `bind_staged` binds at the path when the stage does not fit in `sun_path`, and macOS refuses a path in its temporary directory (PodUtils/podssh#343)

**Source:** PodUtils/podssh#343 (COORD-136 of podssh, from its T-857; author COLLABORATOR): the gate of podssh failed on macOS (its run 45155620101).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The issue: "`relay serve --cover`'s control socket in macOS's `$TMPDIR`
(`/var/folders/xx/<28>/T/...`) was 105 to 109 bytes. macOS's `sun_path`
holds 104 bytes with its NUL, and Linux's 108. The bind was refused and
the role exited 78." The fix of podssh at its commit `cae0bdd6`
(`PodUtils/podssh:crates/podssh-relay/src/serve/control_link.rs`): "a path that does not
fit is bound through a temporary symbolic link to its directory under
`/tmp` (else `/var/tmp`). The link goes only in a directory that is
sticky or ours, under a random name, and is removed right after the bind.
The kernel follows the link, so the socket lands at the path the owner
named, and podssh checks that it did. A file name that alone is too long
gives one line and 78." The ask: `pod_core::files::bind_staged` binds
directly at the path when its parent is over 80 bytes, so it meets the
same refusal. podssh then moves its control socket onto `bind_staged` and
removes `control_link.rs`.

## Premise

- Read: `crates/pod-core/src/files/socket.rs`. A parent of more than 80
  bytes skips the stage: a bind at the path (refused past `sun_path`),
  then a `chmod` (the window of T-196).
- Read: `crates/pod-supervise/src/table.rs` (`control_via_fd`): Linux
  takes a bind through `/proc/self/fd/N/NAME`.
- Not read: the commit `cae0bdd6` of podssh. Its branch `main` at
  `35d5188` does not have it, and GitHub does not give it.

## Approach

The stage stays for each path. When the stage socket `STAGE/s` does not
fit in 103 bytes, the bind goes through a short path to the stage:

1. Linux and Android: `/proc/self/fd/N/s`, with `N` a descriptor of the
   stage. The way is taken only when `/proc/self/fd/N` is the stage (the
   same device and inode): a `/proc` of another pid namespace names
   another directory. No file goes into a shared directory, and no other
   process can change the descriptor.
2. Else: a symbolic link to the stage, under a random name, in `/tmp`
   (else `/var/tmp`). The directory is of this user or of root, and only
   its owner can write it or it is sticky: no other user can remove or
   rename the link. A guard removes the link right after the bind, on
   each path out.

After the bind, the socket must be in the stage; else the bind fails
with a clear error. The mode, the `link(2)` to the path and the removal
of the stage stay the same. `link(2)` takes a path of any length, so the
file name of the socket has no bound of `sun_path`: only a file name of
more than 255 bytes (`NAME_MAX`) fails, as `InvalidInput` with one line.
The function keeps `std::io::Result`: its two callers (`pod-control`,
`pod-broker`) and podssh take an `io::Error`.

## Prove

```sh
cargo test -p pod-core --lib files::socket
cargo test -p pod-broker --all-features --test control
sh scripts/agent/check-macos.sh
```

## Done

2026-11-23, this commit (call C911). `bind_staged` takes a short path
to the stage when `STAGE/s` does not fit (`files/socket/alias.rs`), checks
that the socket is in the stage, and refuses a file name of more than 255
bytes with one line. The tests in `files/socket/tests.rs`:
`a_directory_of_120_bytes_takes_the_socket` (it failed before the change:
"path must be shorter than SUN_LEN"),
`a_client_reaches_a_socket_at_a_long_path` (Linux),
`a_file_name_longer_than_sun_path_binds`,
`a_file_name_past_255_bytes_fails_with_one_clear_line`,
`the_link_goes_after_a_bind_and_after_a_failure` and
`a_directory_that_others_can_write_takes_no_link`. `cargo test -p
pod-core --all-features`: each test passes (98 in the lib); `pod-broker
--test control` and `pod-control`: each test passes. Clippy of
`pod-core` is clean. `codegraph.py new`: 0 clones or twins.
`check-windows.sh`: 0 faults. `check-macos.sh`: `pod-core` and its tests
build; its 8 faults are the call of `openpty` in
`crates/pod-offline/tests/common/mod.rs`, which `main` fixes. The plant `broker-bind-at-the-path` is made again on the
new code; the new plant `core-bind-long-at-the-stage` binds the stage at
its long path. The README of `pod-core` names `bind_staged`. The other
binds of a long path are T-1115; T-920 notes that the emulator binds its
own monitor socket.

# T-1115: Two more copies of the bind of a Unix socket at a long path: `pod-link` (`bind_private`) and `pod-supervise` (`control_via_fd`)

**Source:** the search for other binds of a long path in T-1114 (the ruling "Shared code").
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

`pod_core::files::bind_staged` binds a Unix socket with its mode from the
first instant at a path of any length (T-1114). Two crates keep their own
copy of a part of that work:

- `crates/pod-link/src/unix.rs`, `bind_private`: a stage of mode 0700
  beside the path, a move to the path with no replace, and a bind at the
  path when the stage does not fit in `sun_path` or the move gives
  `EXDEV` (a Landlock domain with no `REFER`). The carrier refuses an
  address of more than 107 bytes (`MAX_PATH`), so a long path in the
  temporary directory of macOS cannot listen.
- `crates/pod-supervise/src/table.rs`, `control_via_fd`, and
  `launcher::listen`: a bind through `/proc/self/fd/N/ctl.sock` with no
  check that `/proc/self` is this process, and a mode of the umask until
  the caller changes it.

`bind_staged` has no fallback for `EXDEV`: a `link(2)` to another
directory under Landlock with no `REFER` gives `EXDEV` too.

## Premise

- Read: the two functions above and `crates/pod-core/src/files/socket.rs`.
- Not measured: the `link(2)` of `bind_staged` under a Landlock domain
  with no `REFER`.

## Approach

Move the `EXDEV` fallback of `pod-link` (a bind at the path with the mode
set before the bind, by `fchmod` on Linux) into `bind_staged`, with a test
under Landlock where the host gives it. Then `bind_private` calls
`bind_staged` (with `UnixListener::from_std`), and `MAX_PATH` bounds only
the connect. `launcher::listen` calls `bind_staged` at the control path
with the mode 0600; `control_via_fd` stays for the connect. Each plant of
the old copies (`link-unix-mode-after-bind`, `link-unix-listen-named-bind`)
is moved onto the new code.

## Prove

```sh
cargo test -p pod-core --lib files::socket
cargo test -p pod-link --all-features unix
cargo test -p pod-supervise --all-features
sh scripts/plant-each.sh --apply
```

# T-1161: `pod-control`: `a_call_back_meets_one_deadline_for_its_write` fails on riscv64 under the emulation of QEMU (the nightly at `0f5dad6`)

**Source:** `nightly.yml` run 45212967290 at `0f5dad6`, the job `cross (riscv64gc-unknown-linux-musl)`, step "Run the tests".
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The test binary `crates/pod-control/tests/server.rs` fails on riscv64 only
(loongarch64 and powerpc64le pass):

```text
thread 'a_call_back_meets_one_deadline_for_its_write' panicked at crates/pod-control/tests/server.rs:365:23:
the connection did not end after 0 bytes: Resource temporarily unavailable (os error 11)
```

The client socket of `ask` has a read timeout of 10 s. The server must
build and write an 8 MiB line first; under the emulator it can need more
than 10 s for its first byte, so the read ends with `EAGAIN` and the test
reports a fault of the server that is not one. The same test passes on
each native job. It is a time bound of the class of T-916.

## Premise

- Read: the log of the job (one failed binary of three: `pod-iroh` `road`
  and `share` are T-916 and T-938), `crates/pod-control/tests/server.rs`
  (`ask`, the test), `pod_testkit::scaled` and `emulated`.
- Not measured: the time of the first byte under riscv64 (the log has no
  time of the test; the binary ends after 10.07 s).

## Approach

The read timeout of `ask` and the bound `started.elapsed() < 10 s` of this
test go through `pod_testkit::scaled`: they guard a hang, not a time that
the test checks. The deadlines of the server (`write_timeout` 1 s) stay as
they are: they are the subject of the test. Where the test still fails
after the scaling, find the cause, not a larger bound (T-916).

## Prove

```sh
cargo test -p pod-control --all-features --test server
```

## Done

2026-11-24 (partial). `crates/pod-control/tests/server.rs`: the read
timeout of `ask` and the bound of the loop of
`a_call_back_meets_one_deadline_for_its_write` go through
`pod_testkit::host::scaled` (50 s under an emulator, 10 s on a kernel).
The deadlines of the server stay as they are. Native:
`cargo test -p pod-control --all-features --test server` passes (16
tests). Open: the job `cross (riscv64gc-unknown-linux-musl)` of the next
`nightly.yml` run. When it passes, set the entry to `done`; when the test
still fails, find the cause (T-916), not a larger bound.

2026-11-24 (done). The job `cross (riscv64gc-unknown-linux-musl, riscv64, 150)`
(138652267213) of `nightly.yml` run 45452272145 at `f39b8e2` ran the
binary `server` of `pod-control`: `a_call_back_meets_one_deadline_for_its_write`
and `a_call_back_meets_one_deadline_for_blank_lines` passed, with 17 tests
passed in 21.76 s. The scaled bounds of the test hold under QEMU. The job
itself was cancelled at its limit in another binary (T-938).

# T-1163: Two more readers of a `Retry-After` value: `pod-cloudflare` and `pod-oci` keep their own copy of `pod_core::clock::retry_after_secs`

**Source:** the search for a reader of `Retry-After` in T-1034 (the ruling "Shared code").
**Category:** chore
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`pod_core::clock::retry_after_secs` reads a `Retry-After` value (RFC 9110):
a number of seconds, or the seconds to its IMF-fixdate (T-1034; `pod-acme`,
`pod-opentunnel` and `pod-services-watch` take it). Two crates keep their own reader:

- `crates/pod-cloudflare/src/quicktunnel/retry.rs`, `parse_retry_after`: a
  number or a date, with its own parse of the date (from podnet,
  `crates/pod-cloudflare/SOURCES`).
- `crates/pod-oci/src/registry/wire.rs`, `parse_retry_after`.

## Premise

- Read: `python3 -I scripts/codegraph.py similar retry after` (each reader
  above), the two functions and their callers.
- Not measured: whether a host of these crates sends a date.

## Approach

Each crate takes `pod_core::clock::retry_after_secs` with the time of its
clock, and keeps its own bound of the wait. A block of `SOURCES` whose file
a consumer has not taken stays frozen: change that file after the `taken`
line, or port the change to the consumer by its COORD issue. A test of each
crate holds a date.

## Prove

```sh
cargo test -p pod-cloudflare -p pod-oci --all-features
python3 -I scripts/codegraph.py similar retry after
```

## Done

2026-11-24, `be0b20a4`. `pod_core::clock::retry_after(value, now)` is new: the
wait of a `Retry-After` value at a wall time, by the rules of
`retry_after_secs`. `pod_cloudflare::quicktunnel::parse_retry_after` and the
`parse_retry_after` of `crates/pod-oci/src/registry/wire.rs` call it with the
clock of the system; the own parse of the date of `pod-cloudflare` goes. Each
crate keeps its bound of the wait (`PROVISION_BUDGET` of `pod-cloudflare`, the
`BACKOFF_CAP` of `pod-oci`). `pod-opentunnel` (`retry::retry_after_secs`) and
`pod-services-watch` (`net::limit_wait`) call it in place of their own read
of the clock. Neither file is frozen: podnet took `pod-cloudflare` at
`00b54ff`, podvm took `pod-oci` at `20ea781`.

Tests: `quicktunnel::tests::a_retry_after_date_out_of_range_is_refused` fails
on the old `pod-cloudflare` ("Sun, 32 Nov 1994 08:49:37 GMT" gave `Some(0ns)`;
an hour of 25 and a long year that overflowed passed too);
`registry::tests::a_retry_after_is_seconds_or_a_date_and_anything_else_falls_back`
(the old `a_retry_after_that_is_not_a_number_falls_back_rather_than_waiting_forever`)
fails on the old `pod-oci` (a date gave `None`), and holds that `BACKOFF_CAP`
bounds the wait of a far date. The vectors of `quicktunnel::tests::retry_after_values`
hold with the shared reader. `pod_core::clock::tests::a_retry_after_is_seconds_or_a_date`
holds `retry_after`. `cargo test` of `pod-core`, `pod-cloudflare`, `pod-oci`,
`pod-opentunnel`, `pod-services-watch` (with the three crates of T-1197) with
`--all-features`: 969 passed, 0 failed. `cargo clippy --all-targets
--all-features -D warnings`, `cargo doc --no-deps --all-features` with
`RUSTDOCFLAGS="-D warnings"` and `cargo fmt --all --check` of these crates:
each exits 0. `sh scripts/plant-each.sh --apply`: 0 stale.
`codegraph.py dups`: no new finding; no baseline line removed. No new
dependency edge. No public item removed: the hand-off adds
`pod_core::clock::retry_after`, and a date of `Retry-After` now gives its wait
in `pod-oci` (podvm's pull), not the backoff.

# T-1209: `pod-core`: one probe of a Unix socket that a forked child can hold, for `pod-control`, `pod-link` and `pod-warp` (PodUtils/podnet#709)

**Source:** PodUtils/podnet#709 (COORD-117 of podnet, from PodUtils/podnet@T-534): the ask for `pod-core` to have one function `files::socket::answers(path) -> bool`, and for `pod-control`, `pod-link` and `pod-warp` to use it (the ruling "Shared code").
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

A program that probes a Unix socket with one `connect(2)` reads a stale
socket as a live one. A child that another thread forks keeps a copy of
each listener of its parent until its `exec`. A socket whose server closed
its listener still takes a connect for that short time, because the copy in
the child keeps the queue open. The probe then reports "served by another
process". The text of podnet#709:

- `gate.yml` 45309579073 and `ci` 45309573045 on podnet `9f3c8a2`: the test
  `warp::proxy::tests::a_unix_socket_that_answers_is_not_taken` failed with
  "a dead socket is taken".
- The control in podnet: its test
  `a_socket_that_a_forked_child_holds_until_its_exec_is_stale`
  (`PodUtils/podnet:crates/podnet-core/src/net.rs`; a child sleeps 40 ms
  between its fork and its exec) fails 5 of 5 runs with one connect. With 3
  connects 50 ms apart it passes 20 of 20 runs.
- The rule that podnet asks for: the socket answers only when each of 3
  connects, 50 ms apart, works. A socket that does not answer costs one
  connect. A socket that answers costs 100 ms. podnet's form is
  `podnet_core::net::socket_answers`, with its test.

Three places of this repository make the same one-connect probe:

- `crates/pod-control/src/unix.rs:27`, in `bind`: a live socket gives
  `AddrInUse`, a stale socket is removed. A false "live" refuses the bind
  of a server that has a stale socket.
- `crates/pod-link/src/unix.rs:154`, in `claim`: a live socket gives
  `InUse`, a stale socket is removed. The same fault.
- `crates/pod-warp/src/find.rs:112`, `answers`, used by `running`: the
  first socket of the list that answers is the running daemon. A false
  "live" names a dead socket as the daemon.

`crates/pod-core/src/files/socket.rs` has the bind of the same sockets
(`bind_staged`) and no probe. `python3 -I scripts/codegraph.py similar
socket answers probe stale` finds no other copy. Other connects to a Unix
socket in `crates` are real connections (`pod_control::unix::connect`, the
carrier of `pod-link`), not probes.

## Premise

- Read: the three places above, `crates/pod-core/src/files/socket.rs` (165
  lines, no probe), `crates/pod-core/src/files.rs` (the `pub use` of
  `socket`), and the text of podnet#709. The author is of the team
  (COLLABORATOR).
- Read: `PodUtils/podnet:crates/podnet-core/src/net.rs`, `socket_answers`
  and its test, is the model. Not read here: its exact text. Port it from
  the repository of podnet.
- Not measured: the 40 ms window and the 3 connects at 50 ms in this
  repository. A fork holds the copy until its `exec`; a slower host can
  hold it longer than 100 ms. The bound of 3 connects is the one that
  podnet measured on its own runners.
- Not measured: the cost of 100 ms for each live socket in
  `pod_warp::find::running`, which tries each socket of the list in order.
- T-1115 (the copies of the bind at a long path) edits the same files
  (`crates/pod-link/src/unix.rs`). The two entries do not depend on each
  other. The later one rebases.

## Approach

1. `pod-core`: add `pod_core::files::socket_answers(path: &Path) -> bool`
   (Unix only, like the module) in `crates/pod-core/src/files/socket/`
   as a new file, with the constants `PROBES` (3) and `PROBE_GAP` (50 ms),
   and re-export it in `crates/pod-core/src/files.rs`. The socket answers
   only when each connect works. The first failed connect returns `false`
   at once, so a stale socket costs one connect. The comment says why: a
   forked child that has not run `exec` holds a copy of the listener.
2. `pod-control` (`bind`), `pod-link` (`claim`) and `pod-warp` (`answers`
   of `find.rs`) call it in place of the one connect. `pod_warp::find::answers`
   stays only when a caller outside `find.rs` needs it, else it goes. Each
   crate keeps its own refusal and its own text.
3. Tests. In `pod-core`: a test that pins the class. A child made by
   `std::process::Command` with `pre_exec` sleeps 40 ms between its fork and
   its exec, and holds the listener (the listener has `CLOEXEC`, so only the
   `exec` closes the copy). The parent closes its listener while the child
   sleeps. The closed socket is not answering. A live listener answers, and
   a path with no socket does not answer. Add one test in `pod-control`
   and one in `pod-link` that the replace of a stale socket works while a
   child holds the closed listener; take the helper that holds the listener
   from `pod-testkit` when two crates need it (`codegraph.py similar`
   first).
4. Release: the next tag has the function. Answer podnet#709 by a COORD
   issue with the tag. podnet takes the tag, removes `socket_answers` and the
   same rule in `PodUtils/podnet:crates/podnet-netns/src/listen.rs`
   (`claim_unix_path`),
   and so closes its side.

## Prove

```sh
cargo test -p pod-core --all-features files::socket
cargo test -p pod-control -p pod-link -p pod-warp --all-features
python3 -I scripts/codegraph.py similar socket answers probe stale
grep -rn "UnixStream::connect(.*)\.is_ok()" crates --include=*.rs
```

## Done

2026-11-24. `pod_core::files::socket_answers(path) -> bool` (Unix only),
in `crates/pod-core/src/files/socket/answers.rs`, re-exported by
`files.rs`: the socket answers only when each of `PROBES` (3) connects,
`PROBE_GAP` (50 ms) apart, works; the first failed connect returns `false`.
`pod_control::unix::bind`, the `claim` of `pod_link::unix` and
`pod_warp::find::running` call it in place of one connect; each keeps its
own refusal and text. `pod_warp::find::answers` is removed: no caller in
this repository or in podnet (`1d446af`) uses it.

The helper `pod_testkit::held::hold_until_exec` (Unix only; `libc` of
pod-testkit is now a dependency for each Unix target, not Linux alone): a
child made by `std::process::Command` with `pre_exec` holds a copy of the
listener, takes one connect with `poll` and `accept`, then runs its exec.
Thus the first connect of a probe always lands in the window, with no sleep
of 40 ms as in podnet's test. The tests:

- `files::socket::tests::a_socket_that_a_forked_child_holds_until_its_exec_is_stale`
  (pod-core): the closed socket that the child holds does not answer; a
  live listener answers; a path with no socket does not. With the plant
  `PROBES = 1` it fails: "a closed socket that a child holds answers".
  50 of 50 runs pass after the change.
- `bind_replaces_a_stale_socket_that_a_forked_child_holds`
  (`crates/pod-control/tests/server.rs`): on the old `bind` it fails with
  `AddrInUse, "a live socket"`. In the same run, the child of this test
  held the listener of `bind_keeps_a_live_socket_and_a_file_and_replaces_a_stale_socket`
  too, and that test failed with "the socket of a dead listener is
  replaced": the fault of podnet#709 between two tests of one process.
- `unix::a_stale_socket_that_a_forked_child_holds_is_replaced`
  (`crates/pod-link/tests/carriers.rs`): on the old `claim` it fails with
  `InUse`, "... is served by another listener".
- `find::tests::a_dead_socket_that_a_forked_child_holds_is_not_running`
  (pod-warp): on the old `answers` it gives the dead socket.

After the change, `cargo test --all-features`: pod-core 123, pod-control
45, pod-link 282, pod-warp 234, pod-testkit 38 passed, 0 failed. The test
binaries of pod-control and pod-link and the `find::` tests of pod-warp:
20 of 20 runs each. Clippy and `cargo doc` with `-D warnings` of the five
crates: clean. `check-windows.sh` and `check-macos.sh`: 0 faults.
`codegraph.py dups`: no new finding. The `grep` of the Prove finds no
one-connect probe in `crates`.

For the hand-off: `pod_warp::find::answers` is removed (podnet does not
call it). The answer to podnet#709 with the tag is step 4, at the next
release; podnet then removes `socket_answers` and the rule of
`claim_unix_path`.

2026-11-25: podnet took `v0.83.0` in `a6a6ccb` (PodUtils/podnet#710, COORD-118; its T-542) and answered PodUtils/podnet#709. `podnet_core::net::socket_answers` and `podnet_netns::listen::socket_answers` re-export `pod_core::files::socket_answers`. podnet's own copy and its own rule in `claim_unix_path` are gone, and PodUtils/podnet#709 is closed. podnet did not call `pod_warp::find::answers`. No `file` line of a `SOURCES` names the removed copy, so no `taken` line is due.

# T-1213: One watcher of the Actions runs of a repository: `scripts/agent/runs.py` with podvm's surface, in place of `watch-runs.sh` and the copies of podvm and podnet

**Source:** PodUtils/podvm#661, COORD-245 (the owner's ruling "Watchers", relayed by podvm).
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The owner's ruling "Watchers" asks each session to start a watcher of the
Actions runs of its repository at its start. `watch-runs.sh` and
`runs.py` of `scripts/agent/` follow one commit and name
`PodUtils/podcrates` in the script, so podvm and podnet keep copies
(podvm's `runs-watch.py`, podnet's `watch-runs.sh` and `runs.py`).
podvm asks for one shared script with this surface:

- `--repo OWNER/NAME` and `--branch` (defaults: the repository, `main`).
- With no `--until`: a poll each `--every` seconds (at least 20), and one
  line for each run that is new or changed state. For a run that ended
  red, one line for each failed job with its failed steps. At the first
  poll, an old completed run is no event.
- `--until SHA` (a prefix is enough): wait until each run of the commit
  ended. Exit 0 when each is green (success, skipped, neutral), 1 when one
  is red, 3 when each run was cancelled, 2 at `--timeout`.
- `--once`: the state of each run of the newest commit.
- 429 and 503 wait for `Retry-After`, 502 waits with backoff, another
  error is one line on stderr while the poll goes on.

## Premise

- Read: PodUtils/podvm#661; podvm's `runs-watch.py`;
  `watch-runs.sh`, `scripts/agent/runs.py` and `watch.py` (its read of the
  mirror, `run_line`, `failed_jobs`);
  `python3 -I scripts/codegraph.py similar watch runs actions`.
- Found in podvm's script: a run that starts and ends between two polls
  after the first is no event (the first-poll rule keys on the first sight
  of a run, not on the first poll); `--once` with no answer polls with no
  end; exit 2 is also a bad argument; `--until` ends when the first
  workflow of a push ends before the others start.
- Not measured: the quota of the mirror for a poll each 60 s beside
  `watch.py`.

## Approach

One file `scripts/agent/runs.py` (standard library only) that imports
`watch.py` beside it: `watch.read` is the one read of the mirror (a
changing query; the HTTP code and `Retry-After` of a refusal), and
`watch.get` keeps its own short retry on it. `watch.py` also gives
`run_line`, `red_jobs` (the failed jobs with their failed steps), `GREEN`
and `Checks` (the lines of a self-test). `runs.py` takes podvm's surface,
with the first-poll rule on the first poll, `--min-runs N` for a gate,
exit 2 for `--once` with no list, and exit 64 for a bad argument.
`watch-runs.sh` leaves; each caller names `runs.py --until`. A
`--self-test` with a fake forge covers each exit code, the first-poll
rule, a red run's jobs and steps, 429 and 503 with `Retry-After`, and 502
with backoff. `ci.yml` and `scripts/agent/check.sh` run the self-tests of
both watchers.

## Prove

```sh
python3 -I scripts/agent/runs.py --self-test
python3 -I scripts/agent/watch.py --self-test
python3 scripts/check-workflows.py
python3 -I scripts/codegraph.py new --base main
python3 -I scripts/agent/runs.py --repo PodUtils/podcrates --once
```

## Done

2026-11-24. `scripts/agent/runs.py`: `[--repo OWNER/NAME] [--branch NAME]
[--every SECONDS] [--until SHA] [--min-runs N] [--timeout SECONDS]
[--once] | --self-test` (defaults PodUtils/podcrates, `main`, 60 s,
3600 s, 1). Exit 0 green, 1 red, 3 each run cancelled, 2 at the timeout
or for `--once` with no list or with a run of `--until` that did not end,
64 for a bad argument. `watch.py` gives `read`, `red_jobs`, `GREEN` and
`Checks`; `watch-runs.sh` is removed. The self-test passes 23 checks
(each exit code, the first-poll rule and a run that starts and ends
between two later polls, a red run's job and failed step, 429 and 503 with
`Retry-After`, a date in `Retry-After`, 502 with backoff and its end after
five reads, a 403 as one line on stderr while the poll goes on, the
options); two planted faults (no first-poll rule, no wait on 502) fail it.
`watch.py --self-test` passes. `codegraph.py new --base main`: 0 clones or
twins. The live read `runs.py --repo PodUtils/podcrates --once` got 403
from the mirror at the first try (exit 2, one line on stderr), and at the
second try printed the six runs of `5d61d40` (exit 0); the same read of
PodUtils/podvm printed four runs of `bfb3822`. Call C926 records the form.

# T-1295: `pod-extract`: `flatten` tests fail for a user that is not root: GNU tar cannot open `usr/bin/sh`, and `walk` cannot read a directory with no search mode

**Source:** `nightly.yml` run 45452272145 at `f39b8e2`, each job `cross` (all seven, also `i686`, which runs with no emulator); `ci.yml` run 45462046889 at `65fa99f`, the job `test`, step "Test".
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

Two test binaries of `pod-extract` fail on each job `cross`, and the first
fails in `ci.yml` too (the run stops there, so the second is not known
there):

```text
tar: usr/bin/sh: Cannot open: Permission denied
tar: Exiting with failure status due to previous errors
thread 'gnu_tar_with_the_arguments_of_incusd_takes_the_stream' panicked at crates/pod-extract/tests/flatten.rs:313:5:
GNU tar refused the stream

thread 'random_layer_sets_give_the_tree_of_extract' panicked at crates/pod-extract/tests/support/mod.rs:240:52:
called `Result::unwrap()` on an `Err` value: Os { code: 13, kind: PermissionDenied, message: "Permission denied" }
```

The runner is a user that is not root. `flatten.rs` (`gnu_tar_with_the_arguments_of_incusd_takes_the_stream`) and
`flatten_random.rs` (`random_layer_sets_give_the_tree_of_extract`) are the
two failed binaries; each other binary of `pod-extract` passes. The line
240 of `tests/support/mod.rs` (at `f39b8e2`) is the `symlink_metadata` of
`walk`: a directory with the mode `0o600` or `0o644` (the modes of the
generator) lets `read_dir` list its names, and a stat of a child gives
`EACCES` for a user that is not root.

## Premise

- Read: the logs of the jobs `cross (aarch64 ...)` to `cross (i686 ...)`,
  the log of `ci.yml` 45462046889, and the tests at `f39b8e2`.
- Not measured: the order of the entries in the stream at the point where
  GNU tar fails (the stream puts a directory after its children, from the
  newest layer; the directory `usr/bin` of the newest layer has the mode
  `0o555`); the same tests as a user that is not root on a developer host.

## Approach

Run both tests as a user that is not root (a user namespace with a mapped
user, or `setpriv`), print the order of the entries of the stream, and find
which one makes `usr/bin` read-only before `usr/bin/sh` is written. A cause
in `flatten` (the order of a directory after its children) gets its fix and
a test that holds without root. A cause in the test (a mode that only root
can use) gets a layer set with usable modes, or `walk` that does not stat
through a directory with no search mode. A test that needs root says so
and skips with the reason, as a missing capability does (`pod_testkit::host`).
Do not weaken a check of the tree.

## Prove

```sh
cargo test -p pod-extract --all-features --test flatten --test flatten_random
setpriv --reuid=65534 --regid=65534 --clear-groups cargo test -p pod-extract --all-features --test flatten --test flatten_random
```

## Done

2026-11-24, this commit. The cause is in the tests, not in `flatten` or
`extract`:

- `gnu_tar_with_the_arguments_of_incusd_takes_the_stream`: the stream
  starts with the newest layer (`usr/bin` 0555, `usr/bin/ls`,
  `usr/bin/dir`, `etc`, `etc/new`, `usr`, `usr/bin/sh`). GNU tar sets the
  mode 0555 on `usr/bin` when the next entry is out of it, and then a user
  that is not root cannot write the older `usr/bin/sh`. incusd runs GNU tar
  as root, where the order holds. The test now gives the newer `usr/bin`
  the mode 0775 and runs as each user. The new test
  `gnu_tar_as_root_takes_a_directory_that_a_newer_layer_closed` keeps the
  mode 0555, skips with its reason as a user that is not root, and
  `scripts/ci/root-uid1000-tests.sh` runs it as root (its `test_bin` takes
  a test target too). T-1299 has the stream for an unpacker that is not
  root.
- `random_layer_sets_give_the_tree_of_extract` (and
  `random_layer_sets_with_hard_links_give_the_tree_of_extract`): the
  generator gives a directory the mode 0644 or 0600, and `walk` of
  `tests/support/mod.rs` cannot stat below it as a user that is not root.
  `walk` now reads the mode of a directory first and then gives it `u+rwx`
  when its owner cannot read or search it (a scratch tree), so `extract`
  and `flatten` are compared on the same modes as each user.

Runs: as the user `podtest` (uid 30033, `setpriv --reuid --regid
--clear-groups`), before the change: `flatten` 19 passed, 1 failed
(`gnu_tar_with_the_arguments_of_incusd_takes_the_stream`: "GNU tar refused
the stream"); `flatten_random` 0 passed, 2 failed. After: `flatten` 21
passed (the root test prints its skip line), `flatten_random` 2 passed. As
root: `cargo test -p pod-extract --all-features` passes (each binary; the
root test passes), and `test_bin pod-extract flatten` of the root script
runs the root test: 1 passed. `cargo clippy -p pod-extract --all-targets
--all-features -- -D warnings`, `cargo fmt --all --check` and
`sh scripts/plant-each.sh --apply` (0 stale) pass.

2026-11-25, `nightly.yml` run 45504345202 at `6f5f4951`: each `flatten` binary of `pod-extract` passes in the seven jobs `cross` (`flatten` 0 to 3 s, `flatten_random` 6 to 43 s, as the runner user that is not root).

# T-1299: `pod-extract`: a stream of `flatten` that GNU tar as a user that is not root can unpack

**Source:** T-1295 (the order of the stream of `gnu_tar_as_root_takes_a_directory_that_a_newer_layer_closed`).
**Category:** feature
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`flatten` writes the newest layer first, so a child of an older layer
comes after the entry of its directory from a newer layer. When the newer
directory has no write or search mode for its owner (`usr/bin` 0555),
GNU tar as a user that is not root sets that mode when it leaves the
directory, and then cannot write the older child: "Cannot open:
Permission denied". incusd runs GNU tar as root, so its import holds. An
unpacker that is not root (a rootless runtime, a user of podvm) refuses
the stream.

## Premise

- Read: the order of the stream of T-1295 and the error of GNU tar 1.35
  as a user that is not root.
- Not measured: whether a consumer streams the tree to an unpacker that
  is not root.

## Approach

Hold each directory entry of a newer layer whose mode closes the write or
the search mode of its owner, and write it after the last layer. A
directory entry has no data: the memory is its header and its records.
Then the test of GNU tar with the mode 0555 runs as each user, and the
root step of CI no longer needs it.

## Prove

```sh
cargo test -p pod-extract --all-features --test flatten
setpriv --reuid=65534 --regid=65534 --clear-groups cargo test -p pod-extract --all-features --test flatten
```

## Done

2026-11-25, this commit. `flatten` holds each directory entry of the
image whose mode takes the write or the search mode from its owner (a
mode with `0o300` not set: `usr/bin` 0555, a directory 0644): `put` writes
its header and its records into a buffer (`write::Out` over a `Vec`), and
`entry::State::held` writes the buffers after the last layer, the hard
links, the second reads and the implied directories, and before the
entries of `after` and the end blocks. The held entries go in the order
of their components, so a parent comes before its children. The memory of
a held entry is its header and its records. Each other entry keeps its
place.

The tests. `crates/pod-extract/tests/flatten.rs`:
`gnu_tar_as_root_takes_a_directory_that_a_newer_layer_closed` (T-1295)
is now `gnu_tar_takes_a_directory_that_a_newer_layer_closed`, with no
skip: it runs as each user, with `usr/bin` 0555 and 0644, and checks
that `usr/bin` comes after `usr/bin/sh`. `scripts/ci/root-uid1000-tests.sh`
no longer runs it, and `docs/design.md` and the README of `pod-extract`
say so. `crates/pod-extract/tests/flatten_unified.rs`:
`a_closed_directory_goes_after_the_tree_and_before_the_entries_of_after`
(a held `usr/bin` and `usr/bin/x` 0500 after the tree, the parent first,
before `rootfs/.podvm`). As the user `podtest` (uid 30033, `setpriv
--reuid --regid --clear-groups`) on the code before the change (the test
with no skip): `flatten` 1 passed, 1 failed (`tar: usr/bin/sh: Cannot
open: Permission denied`, "GNU tar refused the stream"). After: as
`podtest` and as root, `flatten` 22, `flatten_links` 11,
`flatten_random` 3, `flatten_records` 10, `flatten_unified` 7 passed;
the comparisons with `extract` hold (the generator of `flatten_random`
gives directories the modes 0644 and 0600, which are now held). As root:
`cargo test -p pod-extract --all-features` passes. `cargo clippy -p
pod-extract --all-targets --all-features -- -D warnings`, `cargo doc -p
pod-extract --no-deps --all-features` with `-D warnings`, `cargo fmt
--all --check`, `cargo check --manifest-path fuzz/Cargo.toml` pass; `sh
scripts/plant-each.sh --apply`: 0 stale.

The plant `extract-flatten-closed-dir-not-held` (no directory is held)
fails `a_closed_directory_goes_after_the_tree_and_before_the_entries_of_after`
(exit 101); the run without it passes (exit 0).

For the hand-off: podvm sees one change. A stream of `flatten` puts each
directory entry that closes its owner's write or search mode after the
tree and before the entries of `after`; GNU tar as a user that is not
root unpacks it. No API changed.

# T-1296: `pod-enter`: the two title tests of the init and the relay fail under qemu-user on each emulated job (the nightly at `f39b8e2`)

**Source:** `nightly.yml` run 45452272145 at `f39b8e2`, the jobs `cross` of aarch64, armv7, arm, riscv64, loongarch64 and powerpc64le (step "Run the tests", binary `pod_enter`). The job of i686 runs with no emulator and passes.
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The two tests of the title (T-1112, T-1113) fail on each job with an
emulator and pass on each native job:

```text
title::tests::a_hidden_process_shows_its_title_and_no_environment
  crates/pod-enter/src/title/tests.rs:64:5: assertion `left == right` failed
    left: "the parse of /proc/self/stat"
   right: "a title with more than one thread"
title::tests::the_title_and_the_dump_flag_follow_the_fork
  crates/pod-enter/src/title/tests.rs:98:5: the map of this process
test result: FAILED. 268 passed; 2 failed; 1 ignored (269 passed on aarch64)
```

`hide` fails at the parse of `/proc/self/stat`, before it counts the
threads, and `Shown::of` gives no map for the relay. Under qemu-user the
file `/proc/self/stat` is written by the emulator, and its fields can differ
from the kernel's (the process name, the fields after it, the address
fields of the map).

## Premise

- Read: the logs of the six jobs (the same two failures in each), the two
  tests.
- Not measured: the text of `/proc/self/stat` under qemu-user; which field
  the parse refuses.

## Approach

Print the text of `/proc/self/stat` under qemu-user (`scripts/binfmt.sh`)
and find the field that the parse refuses. When the emulator gives a file
that a kernel never gives, the two tests skip with the reason when
`pod_testkit::host::emulated()` is true, as the test of
`PR_SET_MM_MAP` skips when the kernel refuses it. When the parse is wrong
for a field that a real kernel can give too (a name with a space or a
bracket), fix the parse and add a test with that name.

## Prove

```sh
cargo test -p pod-enter --all-features --lib title
```

## Done

2026-11-24 (partial). Cause: qemu-user writes `/proc/self/stat` itself
(`open_self_stat`, `linux-user/syscall.c`): 44 fields; the name of
`argv[0]` cut to 15 bytes; the pid, the state `R`, the parent, the group,
the number of threads, the start time and the stack; 0 in each other
field; the stack as a signed number (negative on a 32-bit guest). Fields
45 to 52 are not there, as on a kernel before 3.5. The parse of the name
up to the last `)` was right; `map_and_threads` read the map before the
count of the threads, so a text with no map gave "the parse of
/proc/self/stat" before the refusal of the threads. qemu-user also refuses
`PR_SET_MM` with `EINVAL`, so no title takes under it in any case, and the
host shows the command line of the emulator in `/proc/PID/cmdline`.
Change (`crates/pod-enter/src/title.rs`): a tolerant reader. `current_of`
reads the count of the threads first (field 20), then the map; a text with
no map is the refusal "the map fields of /proc/self/stat". `map_of` and
`threads_of` share `after_name`. The test
`title::tests::the_title_and_the_dump_flag_follow_the_fork` asserts the map
of `Shown::of` on a kernel only; under an emulator it writes the skip line
(the call is refused there). `a_hidden_process_shows_its_title_and_no_environment`
skips by the refusal of its child, as for `PR_SET_MM_MAP`. Proof here (no
qemu-user on this host): the new unit test
`title::tests::a_stat_of_qemu_user_gives_the_threads_and_no_map` takes the
exact text of qemu-user for a 32-bit guest of three threads; with the old
order it fails with `left: "the parse of /proc/self/stat"`, the text of the
nightly; the plant `scripts/plants/enter-title-qemu-stat.plant` holds that
order. With `QEMU_UNAME` set, `the_title_and_the_dump_flag_follow_the_fork`
writes the skip line. `cargo test -p pod-enter --all-features`: 272 passed,
1 ignored in the unit tests, each other binary passes. Open: the six
emulated jobs `cross` of the next `nightly.yml` run. When the two tests
pass there, set the entry to `done`.

2026-11-25, `nightly.yml` run 45504345202 at `6f5f4951`: the title tests pass in the six emulated jobs `cross` (aarch64, arm, armv7, powerpc64le, riscv64, loongarch64: the binary `pod_enter` ends in 5 to 6 s with exit 0; `the_title_and_the_dump_flag_follow_the_fork`, `a_hidden_process_shows_its_title_and_no_environment` and `a_stat_of_qemu_user_gives_the_threads_and_no_map` are `ok`) and in the job of i686. Set to `done`.

# T-1297: `pod-link`: `dns_hold` gives a hold that is longer than its query timeout under an emulator, and `a_server_that_never_answers_a_command_is_the_named_timeout` of `mail` ends with no error on arm

**Source:** `nightly.yml` run 45452272145 at `f39b8e2`: `dns_hold` in the jobs `cross` of aarch64, armv7, arm, riscv64, loongarch64 and powerpc64le; `mail` in the jobs of arm and armv7.
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

- `crates/pod-link/tests/dns_hold.rs` (T-1166): the tests
  `polls_faster_than_the_hold_stack_no_queries` and
  `a_held_poll_halves_the_idle_queries_and_cuts_the_latency_down` fail in
  six jobs, each after 96 s, in `tests/common/dns.rs:53` (`bound`):

  ```text
  bind on the loopback: CarrierError { carrier: "dns", kind: Usage, text: "DNS: a hold of 7.5s: give less than the query timeout of 2s", source: None }
  bind on the loopback: CarrierError { carrier: "dns", kind: Usage, text: "DNS: a hold of 2s: give less than the query timeout of 2s", source: None }
  ```

  The first test takes `pod_testkit::host::scaled(1500 ms)`, which is
  7.5 s under an emulator; the query timeout of the settings is not scaled,
  so the settings are refused. The second test uses a hold of 2 s with the
  same query timeout. Both bounds of the pair must go through one scale.
- `crates/pod-link/tests/mail.rs:182`
  (`a_server_that_never_answers_a_command_is_the_named_timeout`): "a failed
  stream" on armv7 and arm. The stream ended with a read of 0 bytes, where
  the error that names `UID FETCH` and "took more than" is due. The same
  test passes on aarch64, riscv64, loongarch64, powerpc64le and i686 of
  this run, and the jobs of arm passed at `0f5dad6`.

## Premise

- Read: the logs of the jobs, `tests/dns_hold.rs`, `tests/common/dns.rs`,
  `src/dns/settings.rs` (the check that prints the text), `tests/mail.rs`.
- Not measured: whether the end of the mail stream is a race of the
  emulated 32-bit targets (the command timeout is 200 ms) or a fault of the
  carrier; whether `dns_hold` passes with a scaled query timeout.

## Approach

`dns_hold`: scale the query timeout of the test settings with the same
factor as the hold, or take the hold from the query timeout, so that
`hold < wait` stays true under each factor. A test that must run at 96 s
(the same time in six jobs) takes its measure from the settings, not from a
fixed count of seconds. `mail`: run the test 50 times under qemu-user for
arm (`scripts/binfmt.sh`); when it fails, read the error of the carrier at
the end of the stream: a read that ends before the command timeout gives
the error of the timeout, not the end of the stream. A cause in the
carrier gets its fix and a test; a bound of the test that the emulator
outruns goes through `pod_testkit::host::scaled` (T-916).

## Prove

```sh
cargo test -p pod-link --all-features --test dns_hold
cargo test -p pod-link --all-features --test mail
```

## Done

2026-11-24 (partial). `dns_hold`: cause: the hold of
`polls_faster_than_the_hold_stack_no_queries` is
`pod_testkit::host::scaled(1500 ms)` (7.5 s under an emulator) and the
hold of `a_held_poll_halves_the_idle_queries_and_cuts_the_latency_down` is
four scaled poll intervals (2 s), but the query timeout stayed 2 s, so the
settings check refused both binds. The query timeout guards a lost answer
and is not the timer under test. Change (`crates/pod-link/tests/dns_hold.rs`,
`settings`): the query timeout goes through `pod_testkit::host::scaled`
too, so `hold < query_timeout` holds under each factor. Proof here: the
old binary with `QEMU_UNAME` set (the emulated flag forced on) fails both
tests after 95 s with the two texts of the nightly; the new binary with the
same variable passes 3 tests in 167 s, and passes natively.

`mail`: cause: a defect of the carriers, not a bound. `client::run` of the
IMAP, SMTP and NTP carriers owned `Far` (the far ends of the pipes) and
dropped it when it returned its error, before the spawned task set the
fault in `Ends`. A read of the link in that gap met the end of the pipe
with no fault and gave a clean end (`Ok(0)`), in place of the error of
`UID FETCH` ("a failed stream", `crates/pod-link/tests/mail.rs:182`). The
gap is short on a kernel and longer under the emulation of a 32-bit target.
Change: `crate::pipes::spawn_task` runs the task on a borrow of `Far`, sets
the fault, and then drops `Far`; the three carriers use it, and their `run`
takes `&mut Far`. The bound of 5 s of the test guards a hang and goes
through `pod_testkit::host::scaled`; the command timeout of 200 ms is the
timer under test and stays. Proof here: the new unit test
`pipes::tests::a_failed_task_never_reads_as_a_clean_end` (500 tasks, a
reader that polls with no pause on another thread) fails in each of 3 runs
with `left: Ok(0)` when `Far` drops before the fault (the plant
`scripts/plants/link-task-fault-before-end.plant`), and passes 50 runs of
50 with the fix. The IMAP test does not fail natively on the old code (0 of
200 runs) and passes 50 of 50 runs with the fix. `cargo test -p pod-link
--all-features`: 42 test binaries pass.

Open: the next `nightly.yml` run: `dns_hold` in the six emulated jobs
`cross`, and `mail` in the jobs of arm and armv7. When both pass, set the
entry to `done`.

2026-11-25, `nightly.yml` run 45504345202 at `6f5f4951`: `dns_hold` passes in the six emulated jobs `cross` (168 s each, 169 s on riscv64, where it failed after 96 s before) and in i686 and the jobs `test` (34 s); `mail` passes in arm and armv7 (1 s each) and in each other job. Set to `done`.

# T-1298: `pod-incus`: two tests of `images` hang on macOS, and the job `test (macos-15)` and `test (macos-15-intel)` meet their limit of 60 minutes

**Source:** `nightly.yml` run 45452272145 at `f39b8e2`, the jobs `test (macos-15)` (138652266705) and `test (macos-15-intel)` (138652267051), step "Test", cancelled at 60 minutes.
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

Both jobs stop in the binary `tests/images.rs` of `pod-incus`, after 8
tests start and 6 end:

```text
test an_answer_that_stalls_after_a_cut_ends_at_its_short_bound has been running for over 60 seconds
test an_upload_that_incus_closes_with_no_answer_gives_the_error_of_the_write has been running for over 60 seconds
test an_upload_that_incus_closes_with_no_answer_gives_the_error_of_the_write ... FAILED
##[error]The operation was canceled.
```

The second test fails at its limit of 60 s (`expect("the call ends")`). The
first test (`start_paused = true`, a stand-in on a real socket) never
ends: the step holds until the limit of the job. Each later test of the
workspace on macOS does not run, so the job hides their results. The two
tests are of T-1157; the macOS jobs passed at `0f5dad6`. The same tests pass
on each Linux job.

A write to a Unix or TCP socket that the peer closed gives EPIPE on Linux
(`MSG_NOSIGNAL` or a process that ignores SIGPIPE) and can give another
error or no error on macOS (`SO_NOSIGPIPE`, a different order of the reset
and the write), so the upload may wait for room in a buffer that nothing
reads.

## Premise

- Read: the logs of both jobs, the tests (`crates/pod-incus/tests/images.rs`,
  `tests/common/raw.rs`) and the Done of T-1157.
- Not measured: the error of the write and of the read on macOS; the
  behaviour of a paused clock beside a real socket on macOS.

## Approach

Run `cargo test -p pod-incus --all-features --test images` on macOS (the
job `test` with the filter, or a local run) with `--nocapture` and print
the error of the write and the state of the socket at the cut. Find the
cause in the client (`Client::answer_after_cut`: a write that blocks on
macOS, with no error after the peer closed) or in the stand-in (`Close`,
`Hold`, `Shut`: the shutdown of one side does not give EPIPE there). A fault
of the client gets its fix and a test that holds on Linux too; a fault of
the stand-in gets a form that macOS can give. Each test of a socket gets
a bound that ends the test with a named failure, not the limit of the job
(the test of `start_paused` has no bound of real time). Add a per-test
limit of real time to the step "Test" of the macOS jobs (T-938 gives the
same for the jobs `cross`), so that one hung test does not hide the later
binaries.

## Prove

```sh
cargo test -p pod-incus --all-features --test images
```

The job `test (macos-15)` and `test (macos-15-intel)` of the next
`nightly.yml` run end in less than 60 minutes.

## Done

2026-11-24, partial (this commit; the next `nightly.yml` run decides).

The cause. The log of both jobs:
`an_upload_that_incus_refuses_and_closes_gives_the_answer_of_incus` and
`an_error_envelope_over_the_bound_after_a_cut_gives_too_large_with_its_words`
(the stand-in `Close`) passed in less than 0.1 s; the two tests that use
`Hold` and `Shut` did not end. Both forms shut the read side of the
stand-in. XNU (`apple-oss-distributions/xnu`, `bsd/kern`):
`soshutdownlock_final` calls only `sorflush` for `SHUT_RD`, and
`pru_shutdown` (`uipc_shutdown`, `unp_shutdown`: `socantrcvmore` of the
peer) only for `SHUT_WR`. `sorflush` drops the receive queue, sets its
`sb_hiwat` to 0 and `SB_DROP` (a later append is dropped with no error),
and calls no `uipc_rcvd`, which is the one call that gives the send space
back to the writer and wakes it. `filt_sowrite_common` gives a Unix socket
the space `MIN(sbspace(so_snd), sbspace(so2->so_rcv))`, 0 here, and
reports EOF only for `SS_CANTSENDMORE` of the writer, which nothing sets.
So on macOS a writer that waits when its peer shuts the read side waits
for ever, and no EPIPE comes; `Shut` then shut the write side too, which
ends only the read of the client. On Linux (`net/unix/af_unix.c`,
`unix_shutdown`) a `SHUT_RD` sets `SEND_SHUTDOWN` on the peer and wakes
it: EPIPE. A close (`unp_disconnect`) sets `SS_CANTSENDMORE` and
`SS_CANTRCVMORE` on the peer on both systems, which is why `Close` passed.
The product had a part in it: the write of a request (`wire::send`) had no
bound, so a peer that reads nothing and keeps the connection open held the
call with no end on each OS, and the test with the paused clock had no
timer at all.

The change. `Client::request` writes each request through
`pod_proxy::write_limit::WriteLimit` with `Settings::write_timeout` (30 s
of no progress). A stall is a cut, as EPIPE and a reset are: the crate
reads the answer that is there within `LATE_ANSWER` and gives it, else the
`Io` error `TimedOut` of the write. An error of the reader of a payload
(`wire::from_payload`, `wire::of_payload`) keeps its kind and its text and
is no cut, so a payload that fails with `TimedOut` or `BrokenPipe` waits
for no answer. The doc of `Settings::write_timeout` and of `Client` and
the README say so. The stand-in `tests/common/raw.rs` gives each cut on
both systems: `Hold` keeps the connection and reads nothing (the write
stalls to its bound), `Shut` shuts the read side, reads the queue and
closes (EPIPE and the end of the read, no reset). Each call of the paused
tests has a bound (600 s of the paused clock), so a hang is a named
failure.

Tests (`tests/images.rs`):
`an_answer_that_stalls_after_a_cut_ends_at_its_short_bound` (the whole
answer at `write_timeout`; a stalled body is `AnswerTimeout` at
`write_timeout` + 5 s, and + 2 s with an `answer_timeout` of 2 s), the new
`an_upload_that_incus_stops_reading_ends_at_the_bound_of_the_write`
(`Io(TimedOut)`, class `Timeout`, at 7 + 5 s), and in `wire.rs`
`an_error_of_the_payload_is_not_a_cut_of_the_socket`. On the code before
(the plant `incus-request-write-unbounded`: no `WriteLimit`) the two tests
of `images` fail in 0.08 s with "the call ends at the bound of the write:
Elapsed(())": the hang of macOS, made on Linux by a peer that reads
nothing. On Linux: `cargo test -p pod-incus --all-features --test images`:
9 passed, and the binary passed 20 runs of 20 in a loop; `cargo test -p
pod-incus --all-features`: 79 passed; `cargo clippy -p pod-incus
--all-targets --all-features -- -D warnings`, `cargo doc -p pod-incus
--no-deps --all-features` with `-D warnings` and `cargo fmt --all --check`
pass; `sh scripts/plant-each.sh --apply`: 0 stale. The check of
`check-macos.sh` for `pod-incus` alone (`cargo check --tests` for
`x86_64-apple-darwin` with zig): 0 faults.

For the next nightly: the jobs `test (macos-15)` and `test
(macos-15-intel)` run the 9 tests of `images` of `pod-incus` in seconds,
and the step "Test" goes on to the later binaries. A failure of `Shut` on
macOS (an error other than `BrokenPipe`) or of `Close` names the error of
the write. T-1303 gives each test binary of those jobs a limit of real
time.

2026-11-25, `nightly.yml` run 45504345202 at `6f5f4951`: the jobs `test (macos-15)` and `test (macos-15-intel)` ran the four tests of `images` of `pod-incus` in less than 1 s each (`an_answer_that_stalls_after_a_cut_ends_at_its_short_bound`, `an_upload_that_incus_closes_with_no_answer_gives_the_error_of_the_write`, `an_upload_that_incus_stops_reading_ends_at_the_bound_of_the_write`, `an_upload_that_incus_refuses_and_closes_gives_the_answer_of_incus`: `ok`), and the step "Test" went on to each later binary: 557 binaries and the doc run ended in 21 minutes (macos-15) and 25 minutes (macos-15-intel) in place of the cancel at 60 minutes. Each macOS run of the stand-in cuts (`Shut`, `Hold`, `Close`) gave the expected error. The later binaries of those jobs show a new failure in `pod-core` (T-1359). Set to `done`.

# T-1303: The step "Test" of the jobs `test` of `nightly.yml` has no limit for each test binary, so one hung binary hides the later binaries of macOS and Windows

**Source:** T-1298: `nightly.yml` run 45452272145, the jobs `test (macos-15)` and `test (macos-15-intel)`, cancelled at 60 minutes in `images` of `pod-incus`.
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The step "Test" of the job `test` (`.github/workflows/nightly.yml`, the
rows macOS and Windows) runs `cargo test --workspace --no-fail-fast` with
only the limit of the job (`timeout-minutes: 60`). A test that does not end
holds the step until the job is cancelled: each binary after it does not
run, and the log names no result for them. In run 45452272145 one test of
`pod-incus` hid each later binary of the workspace on both macOS jobs. The
jobs `cross` have a limit for each binary (`timeout -k 30 2700`, T-938);
the jobs `test` do not. macOS has no `timeout` command in its base system.

## Premise

- Read: the step "Test" and the step "Run the tests" of `nightly.yml`;
  the logs of the macOS jobs of run 45452272145.
- Not measured: the time of the longest test binary on each runner of the
  job `test`.

## Approach

Build the tests with `cargo test --no-run --message-format=json`, as the
jobs `cross` do, and run each binary under a limit of real time: a small
portable runner (a `perl -e 'alarm N; exec @ARGV'` line on macOS, which
the runner image has, or a script of `scripts/ci/` that kills the process
group at the limit) that also prints the seconds of each binary. A binary
at its limit is a failed binary; the step runs the others and fails at the
end with the count. Take the limit from the measured times (the longest
binary, and a margin). Keep the exclusion of `pod-gsr` on Windows ARM64.

## Prove

```sh
python3 -I scripts/check-workflows.py
```

A local run of the runner on a planted binary that sleeps past the limit
gives a failed binary, and the binaries after it run. The
next `nightly.yml` run prints the seconds of each binary on the jobs
`test`.

## Done

2026-11-24, partial (this commit; the next `nightly.yml` run decides the
jobs of macOS and Windows).

The change. `scripts/ci/limit.sh SECONDS COMMAND` runs a command with a
limit of real time: GNU `timeout` (Linux, `bash` of Git for Windows), else
`gtimeout`, else perl (`fork`, `setpgrp`, `alarm`; the base system of
macOS has perl and no `timeout`). At the limit it sends TERM to the process
group, KILL 30 s later, and exits with 124. It prints the seconds and the
exit status of each command, and a line with the name at the limit.
`scripts/ci/test-limited.sh BINARY_SECONDS DOC_SECONDS ARGS` runs `cargo
test ARGS --tests` with `limit.sh` as the runner of each test binary
(`CARGO_TARGET_<HOST>_RUNNER`), then `cargo test ARGS --doc` under one
limit, and fails at the end when one run failed. The runner keeps the
filters, the environment and the working directory of each test as cargo
sets them, and `--no-fail-fast` lets the step go on after a hung binary.
The step "Test" of the jobs `test` calls it with 1200 s for each binary
and 1800 s for the doc tests, and keeps `--exclude pod-gsr` on Windows
ARM64; the step "Test pod-gsr" runs under `limit.sh` with 1200 s. The job
`cross` takes `limit.sh` (2700 s, as before) in place of its own `timeout`
and `date` lines. The limits are not measured: the log of the next nightly
has the seconds of each binary.

The local run, on Linux, with a tiny crate (a test that sleeps 600 s, a
test that passes, a doc test) and limits of 3 s and 60 s: for both tools
(`timeout`, and perl with `LIMIT_TOOL=perl`) the step printed "no end after
3 s" with the path of `a_hangs`, cargo named `--test a_hangs` as the failed
target, `b_passes` and the doc test ran and passed, and the step exited 1
with "test runs that failed: 1". The perl path returned the exit status of
a command (3), 124 at the limit, 127 for a program that does not exist, and
left no orphan of a command that started a child. `python3 scripts/
check-workflows.py`: 0 faults. `shellcheck` is not installed here: not run.

For the next nightly: the log of each job `test` shows `== N s, exit 0:
PATH` after each test binary (read the longest and set the limits from
it), and the jobs end in the time of the build and the tests. A hang
shows `limit.sh: no end after 1200 s` with the binary, the step goes on,
and it fails at the end. The Windows jobs must run `limit.sh` with the
`timeout` of Git for Windows (not `timeout.exe` of Windows: the check
`--version` selects the GNU one) and the cargo runner through `sh`
(`cygpath -m` gives the path). The macOS jobs must use the perl path.

2026-11-25, `nightly.yml` run 45504345202 at `6f5f4951`: the four jobs `test` print the line `== N s, exit RC: PATH` after each binary (one line for each of the 557 binaries and one for the doc run on macos-15, macos-15-intel and windows-2025, and 553 binaries on windows-11-arm, which skips `pod-gsr`), and the step goes on after a failed binary and ends with "test runs that failed: N" (1 in macos-15, macos-15-intel and windows-11-arm, 0 in windows-2025). The perl path of macOS and the `timeout` of Git for Windows both work. No binary met its limit, so `limit.sh: no end after` did not print: the hang path is proved only by the local run above. The longest test binary of each job: macos-15 57 s (`new_address`), macos-15-intel 70 s (`new_address`), windows-2025 52 s (`new_address`), windows-11-arm 54 s (`new_address`); the doc tests took 19 s, 37 s, 22 s and 23 s; the step of `pod-gsr` took 31 s. The limits were 1200 s for a binary and 1800 s for the doc tests. They are now 300 s and 600 s (a margin of 4 times over the longest binary), and the step of `pod-gsr` has 300 s. `python3 -I scripts/check-workflows.py`: 0 faults. Set to `done`.

# T-1325: `ci.yml`: the job `static (aarch64-apple-darwin, macos-15-intel)` met its limit of 40 minutes in "Build the probe"

**Source:** `ci.yml` run 45487233017 at `25747d3b`: the job `static (aarch64-apple-darwin, macos-15-intel, false)` was cancelled at 40 minutes while `rustc` ran in the step "Build the probe"; each other job passed.
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** partial

## Problem

The job `static` of `.github/workflows/ci.yml` has `timeout-minutes: 40`. In
run 45487233017 the cross build of the probe for `aarch64-apple-darwin` on
the Intel runner `macos-15-intel` ran 39 minutes in "Build the probe" and
the job was cancelled at the limit (the log ends with "The operation was
canceled" and two orphan processes, `cargo` and `rustc`). The job `all` then
failed, and so did the run. In run 45432751161 (green) the same job took
23.1 minutes (the step 22.5 minutes), the job of `x86_64-apple-darwin` 23.9
minutes and the jobs of Windows 19.7 and 24.5 minutes. The job of this
target has not run to its end since: each later run of `ci` on `main`
(45494344023 and the ones after it) was cancelled by the next push within
20 minutes.

## Premise

- Read: the jobs and the steps of runs 45487233017, 45494344023 and
  45432751161; the log tail of the job of the first.
- Not measured: whether the 39 minutes come from a slow runner, from a
  build that is longer than before (the commits between the two runs
  add `pod-tpm` and change `pod-extract`, `pod-link`, `pod-term`, `pod-dns`
  and `pod-turn`), or from a hang in
  `rustc`; the time of the step in the runs between 45432751161 and
  45487233017.

## Approach

Read the next `ci` run of `main` that ends. Take the seconds of "Build the
probe" of each target of macOS and Windows from the last five runs that
ended. If the step is near its limit in more than one run, raise the limit
of the job for the rows of macOS (a `timeout-minutes` that the matrix row
gives) and find the part of the build that grew. If it is one slow runner,
write the measurement here and close the entry with the times of five runs.
Do not hide a hang: a build of one probe has no reason to run 16 minutes
longer than its twin for another target of the same host.

## Prove

```sh
python3 -I scripts/check-workflows.py
```

The next five `ci` runs that end show the job `static (aarch64-apple-darwin,
macos-15-intel)` with its time; the entry closes with those times.

## Done

2026-11-25, this commit. The matrix of the job `static` in `ci.yml` has the
field `minutes`. The two rows of macOS have 60, the other rows take the
default of 40: `timeout-minutes: ${{ matrix.minutes || 40 }}`. The measures
below show a slow runner (the whole build is slower, with no stall), so the
limit is the fix. `python3 -I scripts/check-workflows.py`: 0 faults. Not
done: the next five `ci` runs that end must show the macOS rows with their
times, and the part of the build that grew (the dependencies or the last
crate) is not found.

Measures, 2026-11-25:

Seconds of "Build the probe" (job total in brackets), from the
jobs of five `ci` runs. The runner of each macOS row is `macos-15-intel`.

| Run (commit) | aarch64-apple-darwin | x86_64-apple-darwin | x86_64-pc-windows-msvc | aarch64-pc-windows-msvc |
| --- | --- | --- | --- | --- |
| 45432751161 (`47a15cd`) | 1350 (1388) | 1408 (1435) | 1355 (1470) | 1131 (1184) |
| 45487233017 (`25747d3`) | cancelled at 2358 (2422) | 1128 (1164) | 1238 (1273) | 1151 (1202) |
| 45543054726 (`45aa8e5`) | 1116 (1170) | 1270 (1308) | 862 (900) | 1223 (1281) |
| 45548340457 (`469f084`) | 1880 (1928) | 1031 (1056) | 1012 (1048) | 1103 (1154) |
| 45561305578 (`3e95e54`) | cancelled at 2353 (2431) | 1234 (1263) | 1361 (1441) | 1144 (1197) |

The step of the three other targets stays between 862 s and 1408 s in each
run. The step of `aarch64-apple-darwin` takes 1116 s to 1350 s in three
runs, 1880 s in one, and met the limit in two (runs 45487233017 and
45561305578): a spread of more than two times for the same build on the
same runner label.

The log of the job 139025410312 (run 45561305578) shows no hang. The lines
`Compiling` come at a steady rate until `pod-gsr` at 07:05:31Z (no gap of
more than 2 minutes). The probe `static-probe` starts at 07:05:37Z, 29
minutes after the step started (a green run needs 19 to 22 minutes for the
whole step), and it has no end at the cancel at 07:14:59Z. So the whole
build runs slower on that runner (the dependencies took 29 minutes), and the
last crate had run 9 minutes when the job was cancelled. The log does not
show how long the last crate needs in a green run.

# T-1326: Three readers of the fields after the name in `/proc/PID/stat`: `pod-hostprobe`, `pod-supervise` and `pod-enter`

**Source:** T-1297 and T-1296 (a finding of their work: the readers that take the fields after the last `)`).
**Category:** chore
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

Three crates read the fields of `/proc/PID/stat` after the name with code
of their own: `pod_hostprobe::probes::files::tty_nr`,
`pod_supervise::launcher::tree::parent_of` and
`pod_enter::title::after_name`. Each finds the last `)` and splits the
rest. The bodies are not token clones, so `codegraph.py dups` does not
flag them, but they do the same work (the ruling "Shared code"). A fix of
the rule (a name with `)` or spaces, the short text of qemu-user that T-1296
found) must be made three times.

## Premise

- Read: the three functions, and the Done of T-1296 (qemu-user writes 44
  fields and no map).
- Not measured: a reader of the same fields in other crates
  (`python3 -I scripts/codegraph.py similar proc stat fields`).

## Approach

One function in `pod-core` (or `pod-hostprobe`, if each user depends on
it already): the fields after the name of a stat text as a slice of
`&str`, with the name up to the last `)`. Each of the three readers takes
it and keeps its own field index and refusal text. A unit test with a
kernel text, a name with `)` and spaces, and the text of qemu-user.

## Prove

```sh
cargo test -p pod-core -p pod-hostprobe -p pod-supervise -p pod-enter --all-features
python3 -I scripts/codegraph.py similar proc stat fields
```

## Done

2026-11-24, this commit. A move with no change of behaviour.

The change. `pod_core::proc_stat::after_name` (`crates/pod-core/src/proc_stat.rs`):
the fields after the command name of a text of `/proc/PID/stat`, field 3
first, the name up to the last `)`, with no check of the count of the
fields (qemu-user and a kernel before 3.5 write 44, a new kernel 52).
`pod-core` is a dependency of each user already. The readers take it,
each with its own field index and refusal text:
`pod_hostprobe::probes::files::tty_nr` (index 4),
`pod_supervise::launcher::tree::parent_of` (index 1) and `pod-enter`'s
`title::map_of` and `title::threads_of` (its own `after_name` is
removed; `field` keeps the count from field 3). The search
(`python3 -I scripts/codegraph.py similar proc stat fields` and a search
of `rfind(')')` and `rsplit_once(')')`) found five more readers in tests,
which take it too: `crates/pod-hostprobe/tests/host_rows/rows.rs`,
`crates/pod-enter/tests/userns/pid.rs`, `crates/pod-guest/src/tests.rs`
(`gone`), `crates/pod-lab/tests/lab/hosts.rs` and
`crates/pod-lab/tests/lab/hosts_payload.rs`. No other reader of the
crates splits the text after the name. The patch of the plant
`hostprobe-tty-nr-field-shifted` has the new body of `tty_nr`.

The tests. The unit test
`proc_stat::tests::the_fields_start_after_the_last_parenthesis_of_the_name`
takes a kernel text of 52 fields, a name with a space and a `)`, the text
of qemu-user of 44 fields (field 20, the threads, is 3), a text with no
field after the name, and a text with no `)`. `cargo test -p pod-core -p
pod-hostprobe -p pod-supervise -p pod-enter --all-features`: before the
change 681 passed, 0 failed in 24 binaries; after it 682 passed (the new
unit test), 0 failed. `cargo clippy -p pod-core -p pod-hostprobe -p
pod-supervise -p pod-enter -p pod-guest -p pod-lab --all-targets
--all-features -- -D warnings` and `cargo fmt --all --check` pass. The
plant `hostprobe-tty-nr-field-shifted`: with it,
`probes::files::tests::the_field_tty_nr_follows_the_name_in_parentheses`
fails (exit 101); without it, it passes. `sh scripts/plant-each.sh
--apply`: 0 stale. `python3 -I scripts/codegraph.py dups`: no new
finding.

# T-1343: `pod-core`: one temp directory and one temp file with a random name (PodUtils/podnet#711, lesson 5 of PodUtils/podvm#567)

**Source:** PodUtils/podnet#711 (COORD-119 of podnet, PodUtils/podnet@T-545), the answer to the ask of PodUtils/podvm#567 (COORD-180, lesson 5). T-932 holds the check of the temp files of this repository and names the shared helper of `pod-core`; this entry is that helper, with the public API that podnet asks for.
**Category:** feature
**Milestone:** M5
**Priority:** P2
**Effort:** M
**Status:** partial

## Problem

A path that a program names with its pid alone is not new. In a PID namespace (a pod, a container) two processes that share a directory can have the same pid, and the second one takes or removes the path of the first. podnet has about 15 such paths: the socket of `warp run`, the `resolv.conf` of the netns backends, the probe files of `doctor` and the temp file of an atomic write of its state. `pod_core::files::create_new_private` gives `O_EXCL` with mode 0600, but `pod-core` has no public function that makes a new name: `random_name` of `crates/pod-core/src/files/socket.rs` is private.

podnet asks for two functions in `pod_core::files` and a test:

1. `new_temp_dir(parent: &Path, prefix: &str) -> io::Result<PathBuf>`: the name `PREFIX-RANDOM`, `mkdir` with mode 0700, a new name on `EEXIST` up to a bound of tries, never a name from the pid or the clock alone.
2. `new_temp_file(parent: &Path, prefix: &str, suffix: &str) -> io::Result<(PathBuf, File)>`: the same rule for the name, `O_CREAT | O_EXCL`, mode 0600.
3. A test that pins the class: two calls with the same prefix in one directory give two paths, and a path that exists is never opened or removed.

Until a tag has them, podnet writes one function in `podnet-core` with the same contract, and a check refuses `process::id()` in a path of its code. When the tag is out, podnet takes it and removes its copy.

This repository makes the same fault in its own code. The names of its temp paths:

- The pid and a counter, with no random part: `temp_name` of `crates/pod-core/src/files/replace.rs` and the temp file of `crates/pod-core/src/files/own.rs` (the create is exclusive, but two processes of one pid and one counter value make the same name); `crates/pod-actions-store/src/git.rs` and `crates/pod-hostprobe/src/writable.rs`; `crates/pod-warp/src/lib.rs`; `crates/pod-rootfs/src/write/follow.rs`.
- The pid alone: the temp file of `crates/pod-actions-store/src/store.rs` (`std::fs::write` follows a link and truncates a file that is there), `crates/pod-hostprobe/src/probes/files.rs`, `access.rs` and `cgroup.rs`.
- The pid and the clock: `crates/pod-enter/src/stage.rs` and `fuse.rs` (`mnt-PID-NANOS`), `crates/pod-supervise/src/table.rs` (`TABLE.PID.NANOS.partial`), `crates/pod-link/src/unix.rs` (a hash of the pid and the clock).
- The pid and a tag, and a directory that is there is taken: `scratch_dir` of `crates/pod-guest/src/scratch.rs` returns the path on `AlreadyExists`, so a second process of the same pid and tag shares the directory of the first (the QMP socket and the serial log of a guest go there).
- A name with a random part already: `random_name` and `make_stage` of `crates/pod-core/src/files/socket.rs` (the rule that podnet asks for, in a private copy: 8 tries, mode 0700, a new name on `AlreadyExists`) and the name of `crates/pod-hostprobe/src/exec.rs` (the pid, a counter and a nonce).
- The tests of about 40 files name a directory in `std::env::temp_dir()` with the pid alone (`pod-relay`, `pod-offline`, `pod-tls`, `pod-proxy`, `pod-testkit`, `pod-acme` and more).

## Premise

- Read: PodUtils/podnet#711 (the author is a MEMBER); `crates/pod-core/src/files/socket.rs` (`random_name`, `make_stage`), `replace.rs` (`temp_name`), `files.rs` (`create_new_private`), `entropy.rs` (`OsEntropy`); a search for `process::id()` in `crates/*/src`.
- Read: T-932 (open, the audit of the temp files of this repository, the same lesson 5). It names "one shared helper of `pod-core`" and has no API, no consumer and no test. This entry is the helper: T-932 moves the places of the list above to it and adds its check of CI.
- The search `python3 -I scripts/codegraph.py similar temp directory random name` finds no public function that makes a new name. `random_name` is the one copy of the rule.
- Not read: the code of podnet (`podnet-core`, `state.rs`, the netns backends); its 15 paths are as the issue lists them.
- Not measured: whether `getrandom` fails under a seccomp filter in a consumer. `OsEntropy` returns `NoRandom` then; see the Approach.
- `RandomState::new()` keeps its keys in a thread-local value that a `fork` without `exec` copies, so a forked child repeats the names of its parent. The ask names the hasher keys; the Approach reads the system source first for this reason. Decide at the work: the tests show which of the two the contract holds.

## Approach

1. `pod-core`: a new module `files::temp` of `pod-core` (its own source file; the source files stay under 500 lines, and `files.rs` has 344) with `new_temp_dir` and `new_temp_file`, re-exported by `files.rs`. A shared private function makes the random part: 16 hex digits from `OsEntropy`; when the system gives no bytes, the hasher keys with the pid and the clock mixed in, so that a name is never a guess that matters (the create is exclusive). `PREFIX-RANDOM` for the directory; `PREFIX-RANDOMSUFFIX` for the file. A bound of tries (8, as `NAMES` of `socket.rs`) with a new name on `EEXIST`; the last `EEXIST` is the error after the bound. The directory has mode 0700 and the file mode 0600, set whole after the create where the umask can take bits away, and `O_NOFOLLOW` on Unix. A prefix or suffix with a `/`, a NUL byte or a `..` is `InvalidInput`. A path that exists is never opened, removed or changed. On Windows the same API uses `create_new_private` and a private directory (the module `closed_win`).
2. Move the copies of this repository: `random_name` and `make_stage` of `socket.rs` use the new function (the stage keeps its short name: one prefix of 3 bytes), then the temp names of `replace.rs` and `own.rs`, `pod-hostprobe` (`exec.rs`, `writable.rs`, `probes/`), `pod-enter` (`stage.rs`, `fuse.rs`), `pod-guest` (`scratch.rs`: a directory that exists is then an error, or a new name; read its callers first), `pod-actions-store`, `pod-link` (`unix.rs`), `pod-rootfs` (`write/follow.rs`), `pod-warp` (`lib.rs`) and `pod-supervise` (`table.rs`; read T-872 first: its sweep reads the staging directory). A place whose name a reader must find again keeps the prefix and gains the random part; its reason goes in a comment. Do it in the order of the list, one crate in a commit.
3. The tests of the tests: the directories of the tests take `new_temp_dir` through `pod-testkit` where the crate depends on it (one helper that removes the directory at the end; `codegraph.py similar` first), and the check of T-932 refuses a new `temp_dir().join(` with a fixed name in product code.
4. Tests in `pod-core` (a test module of `files::temp`): (a) two calls with the same prefix in one directory give two paths; (b) a path that exists, a file, a directory and a symbolic link to a file, is never opened, truncated or removed: with a hook that gives the same random part two times, the second call takes the next name and the first path keeps its bytes; (c) a hook that gives only existing names fails with `AlreadyExists` after the bound, and no path is made; (d) the modes are 0700 and 0600 with a umask of 0; (e) a prefix with a `/` is refused; (f) two processes of the same pid in a PID namespace (a test with `unshare` that skips where it is not allowed) make two paths. A plant: the name from the pid alone makes (a) or (f) fail.
5. Release: the next tag has the functions. Answer PodUtils/podnet#711 and PodUtils/podvm#567 with the tag by a COORD issue (the hand-off of the release). podnet takes the tag and removes its copy.

## Prove

```sh
cargo test -p pod-core --all-features files::temp
cargo test --workspace --all-features
python3 -I scripts/codegraph.py similar temp directory random name
grep -rn "process::id()" crates --include=*.rs
sh scripts/agent/check.sh
```

## Done

2026-11-25 (partial). Steps 1 and 4, and the first moves of step 2.

- `9a6b9933`: the module `files::temp` of `pod-core`, re-exported by
  `files.rs`: `new_temp_dir(parent: &Path, prefix: &str) ->
  io::Result<PathBuf>` and `new_temp_file(parent: &Path, prefix: &str,
  suffix: &str) -> io::Result<(PathBuf, File)>`. The random part is 16 hex
  digits from `OsEntropy`; when the system gives no bytes, the hasher keys
  with the pid, the clock and a counter. 8 tries; the error after the bound
  has the kind `AlreadyExists`. Mode 0700 and 0600, set whole after the
  create (the directory through a descriptor with `O_NOFOLLOW`). A prefix
  or a suffix with a path separator, a NUL byte or `..` is `InvalidInput`.
  On Windows, `closed_win::create_new_private_dir` makes the directory with
  its private access list in one `CreateDirectoryW`. 10 tests of
  `files::temp`, (a) to (f) and the fallback with no random bytes; (d) runs
  a child with a umask of 0 and of 0277; (f) runs two children, each pid 1
  of a PID namespace of a user namespace, and skips where the host refuses
  the namespace (it ran on this host). The decision of the Premise: the
  system source, as the fork of a child copies the keys of the hasher.
  Plants: `core-temp-name-from-the-pid` (caught by (a) and (f)) and
  `core-temp-dir-mode-from-the-umask` (caught by (d), mode 500).
- `f934bf9b`: `random_name` and `make_stage` of `files/socket.rs` are gone:
  the stage is `new_temp_dir(parent, ".pb")`, the same name of 20 bytes,
  and the link of `socket/alias.rs` takes the same loop with `symlink(2)`.
- The commit of this Done: `temp_name` of `files/replace.rs` and the
  temporary file of `files/own.rs` take the loop of `files::temp`
  (`.NAME-RANDOM.tmp`, the mode of the replace before the first byte). A
  file at a temporary name now gives a new name, not a failed replace. Fix:
  `create_private` removed the path at its temporary name before the
  create, so it removed the file of another process of the same pid; the
  new test `a_file_at_a_name_of_the_pid_stays` fails on the old code (a
  file of the 256 planted names is gone) and passes on the new. The plants
  `core-replace-removes-a-file-it-did-not-make`, `core-store-shares-one-temp-name`
  and `core-bind-long-at-the-stage` follow the new lines; each is caught.
- Measured: `cargo test -p pod-core --all-features`: 116 lib tests and 21
  integration tests pass; clippy of pod-core for the host, and
  `cargo check -p pod-core --tests` for `x86_64-pc-windows-gnu`,
  `x86_64-apple-darwin` and `x86_64-unknown-linux-musl`, pass. FreeBSD was
  not built (no target here).

2026-11-25 (partial, the second pass). Step 2 for seven crates, one crate
in a commit, and one function of `pod-core`:

- `a345ca54`, `pod-hostprobe`: `exec.rs` takes `new_temp_file`
  (`.pod-hostprobe-exec-RANDOM`); its private name and loop are gone.
  `writable.rs`, `probes/files.rs`, `probes/access.rs` and `cgroup.rs` take
  `probes::unique` (the pid and 8 random bytes of `getrandom`), now
  visible to the crate: the raw exclusive create is the instrument of the
  probe, and `new_temp_file` sets the mode after the create, which a file
  system can refuse (a wrong verdict). The test
  `a_probe_file_never_takes_the_file_of_another` holds two names and a file
  that is there kept; the plant `hostprobe-exec-probe-overwrites` (a name
  of the pid, written over) follows and is caught.
- `05b808b6`, `pod-enter`: the tmpfs and FUSE mount directories are
  `mnt-RANDOM` (`new_temp_dir`). New test `each_mount_directory_is_new`.
- `36c7878c`, `pod-actions-store`: `Store::put` takes
  `pod_core::files::replace` (mode 0644); the old `std::fs::write` to
  `SHA.tar.tmpPID` followed a link. The fetch directory is
  `fetch-COMMIT-RANDOM`. The crate depends on `pod-core` (no default
  feature). New tests `a_put_never_writes_through_a_link_at_a_name_of_the_pid`
  (fails on the old code) and `two_fetches_of_one_commit_take_two_directories`.
- `68441371`, `pod-link`: the stage of `bind_private` is
  `.pod-link-RANDOM` (`new_temp_dir`, the same length); the private
  `nonce` is gone. New test `each_bind_takes_a_new_stage_and_leaves_none`.
- `bc98ebe1`, `pod-core`: `new_temp_with(parent, prefix, suffix, create)`,
  the names and the bound of `files::temp` with the create of a caller (a
  node by `mknodat` on a descriptor). New test
  `the_create_of_a_caller_takes_the_same_rule`.
- `86df91db`, `pod-rootfs`: the temporary device node is
  `SCRATCH_PREFIX-RANDOM` (`new_temp_with`), 8 tries; the old loop had no
  bound. New test `a_replaced_node_never_takes_a_name_of_another_run` (the
  `mknod` arm ran on this host).
- `b426dba6`, `pod-warp`: `test_dir` (tests only) takes `new_temp_dir`.
  New test `two_test_directories_of_one_tag_differ`.
- `c8cca46b`, `pod-supervise`: `create_locked` makes
  `containers.json-RANDOM.partial.new` (`new_temp_file`), locks it and
  names it `containers.json-RANDOM.partial`: the sweep of T-872 finds the
  suffix `.partial` and never sees the file unlocked. The removals before
  the create, which took a file of another writer of the same pid, are
  gone. New test `each_temp_file_of_a_write_is_new`; the plant
  `supervise-table-temp-unlocked` follows and is caught.
- Measured: `cargo test --all-features` of each crate: `pod-hostprobe` 200
  lib tests and 10 more, `pod-enter` 273 lib tests (1 ignored) and 47
  more, `pod-actions-store` 23 lib tests and 6 more, `pod-link` 52 lib
  tests and the integration tests, `pod-core` 117 lib tests and 21 more,
  `pod-rootfs` 87 and 3, `pod-supervise` 45 and 1, `pod-warp` 169 and 59;
  each passes. Clippy with `-D warnings` of each crate passes.
  `cargo check --target x86_64-pc-windows-gnu`: `pod-core --tests` and
  `pod-actions-store --tests` pass; `pod-link --lib` for
  `x86_64-apple-darwin` passes. The other crates are Linux or Unix only.

Remains:

- Step 2: `pod-guest` (`scratch.rs`): its test
  `a_scratch_directory_is_named_for_the_run_and_closed_to_others` and the
  callers in podvm take the same directory again for one tag, so a new
  name changes the contract; read the callers in podvm first.
- `probes::unique` of `pod-hostprobe` is a second copy of the rule of the
  random part (raw `getrandom`, the clock as the fallback). It names paths
  for `mknod`, `symlink` and the raw `open` of the probes; a move to
  `new_temp_with` keeps the raw calls. Decide at step 3.
- Step 3: the directories of the tests through `pod-testkit`, and the check
  of T-932.
- Step 5: the next tag carries the functions; answer PodUtils/podnet#711
  and PodUtils/podvm#567 with it.

2026-11-25, seen: `nightly.yml` run 45603454952 at `cd7cb5a`, the jobs `test (windows-2025)` (139168091494) and `test (windows-11-arm)` (139168091781), step "Test", binary `pod_core`: `files::temp::tests::a_path_that_is_there_is_never_opened_truncated_or_removed` fails at `tests.rs:91` with `Os { code: 5, kind: PermissionDenied, message: "Access is denied." }`. T-1444 holds it. The test `warp_exec_finds_warp_up_of_another_state_directory` of `pod-warp` failed on macOS because `test_dir` takes a name of 16 more hex digits than before: T-1446 (done).

# T-1359: `pod-core`: three tests of `files::socket` fail on macOS because the temporary directory is longer than their directory of 40 bytes

**Source:** `nightly.yml` run 45504345202 at `6f5f4951`, the jobs `test (macos-15)` (138835537563) and `test (macos-15-intel)` (138835541002), step "Test", binary `pod_core` (lib tests).
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Three tests of `crates/pod-core/src/files/socket/tests.rs` fail on both
macOS jobs, in less than 1 s, and pass on each Linux job (the tests are Unix only):

```text
files::socket::tests::a_file_name_longer_than_sun_path_binds
files::socket::tests::a_file_name_past_255_bytes_fails_with_one_clear_line
files::socket::tests::a_socket_that_a_forked_child_holds_until_its_exec_is_stale
thread '...' panicked at crates/pod-core/src/files/socket/tests.rs:27:60:
a temporary directory shorter than the test
test result: FAILED. 99 passed; 3 failed; 0 ignored
```

Each test calls `long_dir(name, 40)`, which makes a directory of exactly 40
bytes under `std::env::temp_dir()`. The temporary directory of macOS
(`/var/folders/XX/YYYY.../T`) and the prefix `pb-NAME-PID` are longer than 40
bytes, so `len.checked_sub(base.len() + 1)` is `None` and the `expect`
panics. On Linux `/tmp` is short. The tests came with T-1114 (2026-11-23)
and T-1209 (2026-11-24); this is the first nightly that runs them on macOS.
The 40 is not a bound under test: each test needs only a directory that is
short enough for a direct `bind` (under the 104 bytes of macOS) or that has
a file name of its own that is long.

## Premise

- Read: the two logs of the jobs, `tests.rs` (`long_dir`, `remove_long`).
- Not measured: the length of the temporary directory on the runner (the
  test of 120 bytes passes, so the directory with its prefix is under 119
  bytes).

## Approach

Add a helper `near_dir(name)` that gives the shortest directory that
`remove_long` can remove: `long_dir(name, base_len + 2)` with `base_len` the
length of `temp_dir().join("pb-NAME-PID")`. Use it in the three tests. Keep
`long_dir` for the tests that need the exact lengths 120. Check that
`dir.join("held.sock")` of the third test stays under 104 bytes on macOS.
Run `scripts/agent/check-macos.sh`.

## Prove

```sh
cargo test -p pod-core --all-features --lib files::socket
```

The jobs `test (macos-15)` and `test (macos-15-intel)` of the next
`nightly.yml` run pass the binary `pod_core`.

## Done

2026-11-25, this commit (the next `nightly.yml` run decides the jobs of
macOS).

The change. `crates/pod-core/src/files/socket/tests.rs`: `near_dir(name)`
gives `long_dir(name, N + 2)`, with `N` the length of the base
`TEMP/pb-NAME-PID` (`base_dir`), so the directory is the base and one
byte. The tests `a_file_name_longer_than_sun_path_binds`,
`a_file_name_past_255_bytes_fails_with_one_clear_line` and
`a_socket_that_a_forked_child_holds_until_its_exec_is_stale` take it in
place of `long_dir(_, 40)`. The tests of 120 bytes keep `long_dir`.
`link_dir` takes `base_dir` too.

The length. The third test binds `held.sock` directly. Its path is
`TEMP` + 1 + `pb-held-PID` (at most 15 bytes) + 2 + 9: 76 bytes for a
macOS `TEMP` of 49 bytes, 87 bytes for a `TEMP` of 60 bytes, under the
104 bytes of macOS.

The proof. On the code before, with `TMPDIR` of 60 bytes on Linux,
`cargo test -p pod-core --all-features --lib files::socket`: 6 passed,
3 failed, each at "a temporary directory shorter than the test" (the
failure of macOS). With the fix: 9 passed with `TMPDIR=/tmp`, with
`TMPDIR` of 49 bytes and with `TMPDIR` of 60 bytes; `cargo test -p
pod-core --all-features --lib`: 104 passed. `sh
scripts/agent/check-macos.sh`: 0 faults. `cargo clippy -p pod-core -p
pod-cloudflare --all-targets --all-features -- -D warnings` and `cargo
fmt --all --check` pass. Not measured: a run on macOS.

2026-11-25, seen: `nightly.yml` run 45603454952 at `cd7cb5a`: the jobs `test (macos-15)` (139168091509) and `test (macos-15-intel)` (139168093296) pass the binary `pod_core`. `cd7cb5a` holds the fix (`2f100201`). The jobs fail in other tests: T-1445 and T-1446.

# T-1360: `pod-cloudflare`: `quick_tunnel_standin_falls_back_to_http2_when_udp_is_dropped` cannot bind its UDP socket on Windows ARM64 (WSAEACCES)

**Source:** `nightly.yml` run 45504345202 at `6f5f4951`, the job `test (windows-11-arm)` (138835537711), step "Test", binary `quick_tunnel_standin`.
**Category:** defect
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

```text
test quick_tunnel_standin_falls_back_to_http2_when_udp_is_dropped ... FAILED
thread '...' panicked at crates\pod-cloudflare\tests\quick_tunnel_standin.rs:223:63:
a UDP socket on the edge port: Os { code: 10013, kind: PermissionDenied, message: "An attempt was made to access a socket in a way forbidden by its access permissions." }
test result: FAILED. 12 passed; 1 failed
```

The test binds a UDP socket on the number of the port of the stand-in edge,
which is a TCP port that `edge::listen` took with `127.0.0.1:0`. The same
test passes on `windows-2025` in this run. On Windows a bind to a port that
is in an excluded range (Hyper-V, WinNAT) or that another socket holds with
an exclusive use gives 10013, not 10048 (address in use). The ports of TCP
and UDP are separate, so a free TCP port is not a free UDP port. The cause
is not known: a reserved range of the runner image, or another socket that
holds the port.

## Premise

- Read: the log of the job, `quick_tunnel_standin.rs` (lines 216 to 230),
  `src/standin/edge.rs` (`listen`).
- Not measured: `netsh int ipv4 show excludedportrange protocol=udp` on the
  runner; how often the failure happens (one run seen); which other socket
  holds the port.

## Approach

Print the excluded UDP ranges and the owner of the port in the step when the
bind fails. Then make the test take a stand-in whose TCP port is also a free
UDP port: bind the UDP socket first (`UdpSocket::bind("127.0.0.1:0")`), take
its port, and bind the TCP listener of the edge to that port, with a retry of
the pair a few times on `AddrInUse` or `PermissionDenied`. A fault of the
runner image alone (an excluded range) skips the test with the reason, as a
missing capability does (`pod_testkit::host`), and the job prints the line.
Do not weaken the check of the leg `http2`.

## Prove

```sh
cargo test -p pod-cloudflare --all-features --test quick_tunnel_standin
```

The job `test (windows-11-arm)` of the next `nightly.yml` run passes the
binary `quick_tunnel_standin`.

## Done

2026-11-25, this commit (the next `nightly.yml` run decides the job
`test (windows-11-arm)`).

The change. `pod_cloudflare::standin::edge::listen_at(addr)` (in place
of `listen`) binds the edge on a given address and gives the error to the
caller. `Standin::start_at(edge)` starts the stand-ins with the edge on
that address and gives a bind error; `Standin::start` and
`Standin::start_here` take `127.0.0.1:0` as before. In
`crates/pod-cloudflare/tests/quick_tunnel_standin.rs`, `standin_with_udp`
binds the UDP socket first on `127.0.0.1:0` and then the edge on its
port. On `AddrInUse` or `PermissionDenied` of either bind it takes a new
pair, at most 8 times. When each try is refused, the test skips with
the errors, and on Windows it prints the excluded ranges of
`netsh int ipv4 show excludedportrange` for UDP and TCP. The check of the
leg `http2` and of the QUIC packets on the silent socket do not change.

The proof. `cargo test -p pod-cloudflare --all-features --test
quick_tunnel_standin`: 13 passed; the test
`quick_tunnel_standin_falls_back_to_http2_when_udp_is_dropped` passed 10
runs of 10 (2.1 s each). `cargo test -p pod-cloudflare --all-features`:
215 passed. `sh scripts/agent/check-windows.sh`: 0 faults. `cargo clippy
-p pod-core -p pod-cloudflare --all-targets --all-features -- -D
warnings` and `cargo fmt --all --check` pass. Not measured: the failure
on Windows ARM64 (Linux gives no 10013), and the excluded ranges of that
runner.

2026-11-25, seen: `nightly.yml` run 45603454952 at `cd7cb5a`: the job `test (windows-11-arm)` (139168091781) passes `quick_tunnel_standin`. `cd7cb5a` holds the fix (`2f100201`). The job fails in `pod-core` (T-1444) and in the doc tests of `pod-h2` (T-1447).

# T-1361: adoption/podssh/pod-tpm.run: the check of the moved names reads all of `podssh-ssh/src`, and it finds two other structs named `Out`

**Source:** `adoption.yml` run 45524129405 at `a847d777`, the job `adoption (podssh)` (138903109946), step "Each trial of the consumer".
**Category:** defect
**Milestone:** M8
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The trial `pod-tpm` of podssh failed at its sixth command, the first run of
this trial:

```text
adoption: podssh at 3001570 with pod-tpm of a847d77
adoption: $ ! grep -rnE --include='*.rs' -e '(fn|struct|enum|trait) (SoftTpm|Fault|Unmet|Out|In|Authorize|Transport)\b' crates/podssh-ssh/src
crates/podssh-ssh/src/io.rs:113:struct Out {
crates/podssh-ssh/src/server/kept_flow.rs:164:pub(in crate::server) struct Out(pub(in crate::server) Arc<Shell>);
adoption: FAILED: ! grep -rnE ... crates/podssh-ssh/src
trials: 1, failed: 1
```

The command checks that the names of the TPM module (`SoftTpm`, `Fault`,
`Unmet`, `Out`, `In`, `Authorize`, `Transport`) leave podssh. It reads the
whole of `PodUtils/podssh:crates/podssh-ssh/src`. Two structs named `Out` in `io.rs` and in
`server/kept_flow.rs` have nothing to do with the TPM, and podssh's main
has them without the patch. The patch is correct: with it applied, the
directory `PodUtils/podssh:crates/podssh-ssh/src/tpm` has no match. The step before it (the
list of the files of `tpm/`) passed. The previous green run of `adoption`
had no podssh trial: `adoption/podssh/pod-tpm.*` came with T-1222.

## Premise

- Read: the log of the job, adoption/podssh/pod-tpm.run, `scripts/adoption.sh`.
- Measured: podssh at `3001570` with adoption/podssh/pod-tpm.patch
  applied. `git apply` passes. The grep on `src/tpm` finds nothing. The grep
  on `src` finds the two structs `Out` above.
- Not measured: the commands after the grep (build, deny, clippy, the tests,
  swtpm, rustfmt, the toolchain checks, the zig build). The run stopped
  before them.

## Approach

Limit the grep to `PodUtils/podssh:crates/podssh-ssh/src/tpm`, the directory that the
module of the TPM held. Names in other directories of podssh are not part
of the module. Run the trial again in `adoption.yml` and read each command
after the grep; fix each one that fails.

## Prove

```sh
sh scripts/adoption.sh podssh pod-tpm "$TMPDIR/adoption"
```

The job `adoption (podssh)` of the next `adoption.yml` run passes the trial
`pod-tpm`.

## Done

2026-11-25, the commit that names T-1361. The trial `scripts/adoption.sh`
with adoption/podssh/pod-tpm.run is the test: it fails on the line
before the fix, and it passes that line after it. The line of the grep in
adoption/podssh/pod-tpm.run ends in `PodUtils/podssh:crates/podssh-ssh/src/tpm`. The
cause is in podcrates (the trial of T-1222, commit `732f8d13`), not in a
change of podssh and not a flake of the runner. Checked: the patch applies
to podssh `3001570`, and the narrowed grep gives no line. Not checked: the
commands after the grep; the next `adoption.yml` run proves them.

# T-1363: `adoption/podssh`: the trial `pod-tpm` does not build, because podssh's relay role gives a stream of the h2 of crates.io to `h2_road::duplex`, which takes the types of `pod-h2`

**Source:** `adoption.yml` run 45533553364 at `5a528af4`, the job `adoption (podssh)` (138934522698), step "Each trial of the consumer"; T-1095.
**Category:** defect
**Milestone:** M8
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The trial `pod-tpm` of podssh failed at `cargo build -q`:

```text
crates/podssh-relay/src/serve/http2/stream.rs:178:22
178 |     let mut stream = podssh_ws::h2_road::duplex(send, body);
expected `pod_h2::share::RecvStream`, found `h2::RecvStream`
expected `pod_h2::share::SendStream<bytes::Bytes>`, found `h2::SendStream<bytes::Bytes>`
error: could not compile `podssh-relay` (lib)
```

T-1095 made the workspace dependency `h2` the fork `pod-h2`, and
`pod_ws::h2_road::duplex` takes its types (call C934). podssh's relay
role builds its HTTP/2 server on its own `h2` (`vendor/h2` through
`[patch.crates-io]`). The trial patch did not carry the move of podssh to
`pod-h2`. The adoption map of `crates/pod-ws/README.md` said that podssh's
own h2 server stays on the h2 of crates.io, which cannot compile.

## Premise

- Read: the log of the job, `scripts/adoption.sh` (`CRATE.needs`),
  `adoption/podssh/pod-tpm.*`, the manifests of podssh at `3001570a`.
- Measured: podssh-relay and podssh-ws name `h2` directly, and
  podssh-relay has no dependency on `pod-ws`. A dependency
  `h2 = { git = .., tag = .., package = "pod-h2" }` keeps the name `h2` in
  the code. The second rewrite of `scripts/adoption.sh` points it at
  `vendor/h2` of the tree.
- Measured: a first patch moved two lines of `PodUtils/podssh:crates/podssh-relay/Cargo.toml`,
  and podssh's record cites `vendor/h2` two times; `podssh-todo remap`
  then found 12 problems.

## Approach

Add the trial `pod-h2` of podssh: `pod-h2.patch` gives podssh-relay and
podssh-ws the dependency `h2` as `pod-h2` of the tag, removes the
`[patch.crates-io]` line and the `exclude` entry of `vendor/h2`, keeps the
line count of each cited manifest, and writes the two citations of
`vendor/h2` in podssh's record as words. The first step of each `.run`
removes `vendor/h2` (a patch that deletes its 56 files carries each line
of them). `pod-tpm.needs` applies `pod-h2.patch` before `pod-tpm.patch`,
which is made again on top of it. Correct the adoption map and the hand-off.

## Prove

```sh
sh scripts/adoption.sh podssh pod-h2 "$TMPDIR/adoption"
sh scripts/adoption.sh podssh pod-tpm "$TMPDIR/adoption"
```

The job `adoption (podssh)` of the next `adoption.yml` run passes both
trials.

## Done

2026-11-25, the commit that names T-1363. The test is the trial of
`.github/workflows/adoption.yml` (`scripts/adoption.sh` with
adoption/podssh/pod-tpm.run): it fails at `cargo build -q` before the
fix. After it, `scripts/adoption.sh podssh pod-tpm` passes each step at
`869c1b3a`: the podssh record agrees (832 entries), the build, cargo-deny,
clippy, the TPM tests (4, 1, 2 with swtpm, 5 and 1 passed), rustfmt, the
toolchain and test-target checks, the zig build for x86_64 musl and
`check-static.sh`. `scripts/adoption.sh podssh pod-h2` passes each step at
`45aa8e57`: clippy of podssh-ws and podssh-relay with `serve`, `quick`,
`piping` and `plain-ws`; `h2_road` 1, `h2_fallback` 4 and `relay_serve_h2`
8 passed; the zig build and `check-static.sh`. `h2_fallback` passes with
the order of `Shape::browser`, with no change to the test. Not checked: the
run of `adoption.yml` on GitHub.

# T-1374: One session start for each harness and host: `scripts/session-start.sh` sets the identity, the hooks path and `main`, and starts both watchers

**Source:** PodUtils/podvm@2d36b23 (the shared session start of podvm).
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The start of a session works only in Claude Code. The identity and the
branch are in the Claude Code hook `.claude/session-start.sh`, and the
hook acts on the current directory when `CLAUDE_PROJECT_DIR` is not set.
Another harness (a tool that reads `AGENTS.md`) or a local machine runs
no start. The hook does not start the two watchers of the owner's ruling
"Watchers"; each session starts them by hand. Its `git fetch` has no time
limit and can ask for credentials. In a worktree of an agent, the hook
tries to check out `main`. No tool refuses an agent attribution line in a
commit outside Claude Code. `scripts/cloud-setup.sh` fails at once on a
host with no rustup, runs `rustup target add` for the toolchain of the
current directory and not of `rust-toolchain.toml`, and its check
`cargo zigbuild --version` always fails (cargo-zigbuild 0.23.4 refuses
the argument), so each run fetches or builds cargo-zigbuild again.
`AGENTS.md` and `docs/maintenance.md` state facts of one cloud host as
facts of each host (a shallow clone, the egress proxy).

## Premise

- Read: the commit PodUtils/podvm@2d36b23 (its `.claude/session-start.sh`,
  `PodUtils/podvm:scripts/session-start.sh`,
  `PodUtils/podvm:scripts/cloud-setup.sh`, `PodUtils/podvm:AGENTS.md`,
  `PodUtils/podvm:docs/hosted-sessions.md`,
  `PodUtils/podvm:scripts/qa-bundle.sh`,
  `PodUtils/podvm:scripts/common/bootstrap-env.sh`) and podvm's
  `.githooks/commit-msg` at that commit.
- Read: `.claude/session-start.sh`, `.claude/settings.json`,
  `scripts/cloud-setup.sh`, `scripts/ci/fetch-tool.sh`, `scripts/agent/`,
  `AGENTS.md`, `docs/maintenance.md`, `docs/HANDOFF.md`,
  `docs/decisions.md` (the rulings "Watchers", "Delegation", "Web
  access").
- Measured: the SHA-256 of rustup-init 1.29.3 for x86_64 and aarch64
  Linux in the `.sha256` files of static.rust-lang.org (read through
  fetch.web.pkgforge.dev) are the values of podvm's script.

## Approach

Take the parts of the commit that podcrates needs, in the form of
podcrates:

- `scripts/session-start.sh` (POSIX sh) holds the start; the Claude Code
  hook only calls it. It finds the checkout from its own path, sets the
  local identity, `commit.gpgsign false` and `core.hooksPath .githooks`,
  and changes no global setting. A worktree stops there. The lead
  checkout fetches `origin/main` with a time limit and no prompt, keeps
  the rules of the branch, runs the self-tests of both watchers, and
  starts each watcher that does not run (detached, with a log in
  `.work/`, under a lock). A watcher runs when a process has its exact
  command line and this checkout as its directory. `flock`, `setsid`,
  `pgrep` and `timeout` are used when the host has them.
- `.githooks/commit-msg` refuses an agent attribution line.
- `scripts/cloud-setup.sh` runs in the root of the repository, installs
  rustup by its pinned installer (checked by SHA-256 through
  `scripts/ci/fetch-tool.sh`, which now takes a program that is no
  archive) when the host has none, installs the toolchain of
  `rust-toolchain.toml`, links `cargo`, `rustc` and `rustup` into
  `$HOME/.local/bin`, and checks `cargo-zigbuild --version`.
- `AGENTS.md`, `docs/maintenance.md` and `docs/HANDOFF.md` name the
  shared start, the permissions and `CARGO_BUILD_JOBS` of another
  harness, the check of a shallow clone, and the egress of the current
  host. Call C937.

## Prove

```sh
sh scripts/session-start.sh          # twice, in a scratch clone
sh .claude/session-start.sh          # from another directory
sh scripts/cloud-setup.sh            # with an empty HOME, no rustup
python3 -I scripts/check-history.py
cargo todo check
```

## Done

2026-11-25, the commit that names T-1374. In a scratch clone:
`scripts/session-start.sh` run from `/` set the local identity,
`commit.gpgsign` and `core.hooksPath`, and started both watchers (exit 0);
a second run said "runs already" for each (exit 0). The watchers of
another checkout with the same command line did not count (their
directory differs). A watcher that was killed started again alone. With a
`PATH` with no `flock`, `setsid`, `pgrep` and `timeout`, the start and the
second run gave the same result. With an origin that cannot be reached,
the fetch failed with one line and the start went on. A clean branch
moved to `main`; a branch with changes stayed; a worktree kept its branch
and made no `.work/`. The hook ran from `/tmp` with no
`CLAUDE_PROJECT_DIR`. `.githooks/commit-msg` refused a message with
`Co-Authored-By:` and with a line that starts with "Generated with",
allowed "the code generated with zig" and a git comment line, and refused
a call with no message file. `scripts/cloud-setup.sh` with an empty
`HOME` and no rustup, zig or cargo on `PATH`: rustup-init 1.29.3 passed
its SHA-256, the toolchain 1.99 of `rust-toolchain.toml`, the musl target,
zig 0.16.0 and cargo-zigbuild 0.23.4 came in, and a new shell with
`$HOME/.local/bin` on `PATH` found `cargo`, `zig` and `cargo-zigbuild`.
The second run fetched nothing. A wrong SHA-256 of rustup-init failed.

Not taken from the commit, with the reason:

- The role table of models and efforts for two harnesses, and a third
  role for the hardest tasks and advisors (podvm's ruling "Hosted
  sessions and worker roles"): it changes the ruling "Delegation" of
  podcrates, so it is Q23 of `TODO/PROGRESS.md`.
- The footer of a GitHub comment for each harness: the ruling "Comment footer" settles it.
- A global git identity in a cloud session: podcrates sets the identity
  of the repository alone, and `.claude/settings.json` gives it to each
  git command of Claude Code.
- The watchers at a pinned tag of podcrates, fetched from
  raw.githubusercontent.com: podcrates runs the files of its own tree.
- `PodUtils/podvm:scripts/qa-bundle.sh` and the blind outside pass of a release: the
  outside pass is a step of podvm's release of its own binary; the
  release of podcrates has no such step.
- The managed Docker socket of
  `PodUtils/podvm:scripts/common/bootstrap-env.sh`, the KVM and cgroup
  facts of one host, podvm's `HUMAN.md` and `README.md`,
  `PodUtils/podvm:docs/hosted-sessions.md` and the record of podvm:
  podcrates has none of these files or facts.

Not verified: a run of the start in a harness other than Claude Code, on
macOS, and in a Claude Code session (the hook ran by hand).

# T-1382: `adoption/podssh`: the trials `pod-tpm` and `pod-h2` stay after podssh took `v0.84.0`, and the list of the files of `tpm/` in `pod-tpm.run` is not podssh's (PodUtils/podssh#357, ask 3)

**Source:** the run 45593943689 of `adoption.yml` at `3e480aa` is red (the job `adoption (podssh)`: the trials build podssh at `3001570` against podcrates `main`, and `Failure::io(stage, step, e)` has three arguments since T-1351, while the tests `podssh-ws/tests/vless_coalesce.rs` and `quick_edge_warp` of `podssh-relay` call it with two); PodUtils/podssh#357 (COORD-147 of podssh, the take of `v0.84.0` at `806e0a4`), ask 3, by a member of the team; T-1361, T-1363; `docs/maintenance.md`, "The take of a consumer".
**Category:** chore
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The text of the ask: "adoption/podssh/pod-tpm.run: its `ls` list of the directory no longer matches: podssh keeps `agent*.rs`, `host*.rs`, `windows.rs` and `tbs.rs`." The patch was cut at podssh `3001570`, before podssh merged the rest of its T-839 (the TPM keys of the agent, the host key of `serve`, the TBS of Windows). podssh applied four of its deletions and three files by hand.

podssh took `pod-tpm` and `pod-h2` at `806e0a4`. The procedure of a take then applies: the trial leaves `adoption/podssh/`, `SOURCES` gets the line `taken COMMIT`, and the entry of the take closes. The trial files `adoption/podssh/pod-tpm.*` and `adoption/podssh/pod-h2.*` are still in the tree.

## Premise

- Read: adoption/podssh/pod-tpm.run (the sixth command compares `ls crates/podssh-ssh/src/tpm` with `mod.rs other.rs policy_key.rs signer.rs ssh.rs testdata tests.rs unix.rs`); adoption/podssh/pod-tpm.commit and adoption/podssh/pod-h2.commit (both name `3001570a652aa9e48c6eb4dbd3ca7602f8a5854b`, so the trial runs at that commit and not at podssh's `main`); `adoption/README.md` ("When the consumer takes a crate, its trial leaves this directory"); `crates/pod-tpm/SOURCES` (the block `repo PodUtils/podssh` has no `taken` line).
- Read in podssh at `806e0a4`: `PodUtils/podssh:crates/podssh-ssh/src/tpm` holds `agent.rs`, `agent_tests.rs`, `host.rs`, `host_tests.rs`, `mod.rs`, `other.rs`, `policy_key.rs`, `signer.rs`, `ssh.rs`, `tbs.rs`, `tests.rs`, `unix.rs`, `windows.rs` and `testdata`. Each `Cargo.toml` of podssh at `806e0a4` that names a crate of podcrates names `tag = "v0.84.0"` and no other tag (the 55 `Cargo.toml` files of the tree).
- Not read: whether the trial `pod-tpm` still passes at `3001570` (the job `adoption (podssh)` of the last run of `adoption.yml` is the proof); `scripts/check-sources.py` with a clone of podssh; whether the fork `pod-h2` (the tree `vendor/h2` with `vendor/patches/h2/`) has a block of `SOURCES` that needs a `taken` line; the fork has no directory under `crates`.

## Approach

1. Run `scripts/check-sources.py --clone PodUtils/podssh=DIR pod-tpm`, with a clone of podssh at `806e0a4`, to read the drift of the files that podssh took. Each new change is a port entry (`TODO/RULES.md`).
2. Add the line `taken 806e0a4...` (the full id) to the block `repo PodUtils/podssh` of `crates/pod-tpm/SOURCES`, for the files that the take replaced (`taken COMMIT PATH...` for the glue that stays: `mod.rs`, `signer.rs`). Run the check again: 0 changes.
3. Remove `adoption/podssh/pod-tpm.*` and `adoption/podssh/pod-h2.*` (git keeps them). If `adoption/podssh/` holds no other trial, keep `repo` and `commit` as `adoption/podnet` and `adoption/podvm` do. Add the take to the list "Taken" in `adoption/README.md`.
4. If the trial of another consumer needs the list of files of a module, make the list a pattern and not a fixed string; no other trial has this today (not read).

## Prove

```sh
python3 -I scripts/check-sources.py --clone PodUtils/podssh=SCRATCH/podssh pod-tpm
sh scripts/adoption.sh podssh pod-tpm SCRATCH/adoption
```

The first command gives 0 changes. After step 3 the second command exits 66 (`pod-tpm.patch` is missing), and `ls adoption/podssh` lists `commit` and `repo` alone.

## Done

Date: 2026-11-25. Commit: see `git log` for T-1382.

Result: the trials `adoption/podssh/pod-tpm.*` and `adoption/podssh/pod-h2.*` left the tree; `adoption/podssh` holds `commit` and `repo`, and `adoption/README.md` lists the take. `crates/pod-tpm/SOURCES` has `taken 806e0a4...`. The check with a clone of podssh at `806e0a4` showed the changes of podssh's T-839 merge (`0c6e9c4`), which came before the take: T-1381 ports `flow.rs`, `flow_signed.rs`, `flow_tests.rs` and `soft.rs`; T-1380 ports `tbs.rs` and `windows.rs`; the files `agent`, `host`, `policy_key`, `ssh` and the tests stay in podssh (`skip`). The check gives 0 changes. The fork `pod-h2` has no block of `SOURCES`. No other trial lists the files of a module (step 4). The list of `ls` in `pod-tpm.run` went with the trial; no new port entry was necessary.

# T-1384: `scripts/agent/check.sh`: a full check in the main checkout runs test binaries that a worktree built in the shared target

**Source:** the session of 2026-11-25: the full check of `66c91227` failed eight tests of `pod-job` with `NotFound`; the test binary held the path of a worktree that was removed. A full check of `ffe078b8` after a `touch` of each source of the workspace passed.
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

Agents that work in parallel in `.claude/worktrees/` share the target
directory of the main checkout, because the disk does not hold one target
for each worktree (`docs/maintenance.md`, "Rules of the work", asks for
one target each). Cargo names a path crate by its path relative to the
root of the workspace, so two worktrees build the same artifact names. The
fingerprint of a path crate reads the times of its sources: an artifact
that a worktree built later than the last change of the source in the main
checkout is fresh for the main checkout. A full check then runs code of
another tree. In this session it gave eight false failures (a removed
worktree path in `CARGO_MANIFEST_DIR`), and it can give a false pass, as
the code under test is not the code of `main`. The test binaries of 400
targets held a path of a worktree.

## Premise

- Measured: `grep -l .claude/worktrees/` over the executables of
  `target/debug/deps` after the merges of the day; the check of `66c91227`
  (eight failures, `NotFound`) and of `ffe078b8` after a `touch` of each
  source (all steps green).
- Not measured: the time of a full rebuild of the crates of the workspace
  against the time of a check with stale artifacts.

## Approach

1. `scripts/agent/check.sh`: before the build, find each executable and
   rlib of `target/debug/deps` that holds a path under `.claude/worktrees/`
   and remove it (or `touch` the sources of each crate that it names), so
   that cargo builds it again from the checkout. Print the count.
2. `docs/maintenance.md`, "Rules of the work": an agent in a worktree uses
   `CARGO_TARGET_DIR` under the scratchpad when the disk holds it, and
   builds only its crates with `-p`; else the shared target with a `touch`
   of each file that the worktree changes before each cargo call; the
   coordinator runs the full check only after step 1.
3. A test of the check script: a fake artifact that holds a worktree path
   is removed and named in the output.

## Prove

```sh
sh scripts/agent/check.sh notest
```

## Done

2026-11-25, the commit that names T-1384 and T-1385.
`scripts/agent/stale_target.py` (standard library of Python) runs first in
`scripts/agent/check.sh` and prints its two counts. (1) Each unit of the
target whose dep-info (`deps/NAME-HASH.d`) records a `CARGO_MANIFEST_DIR`
in another checkout of the repository (the lead checkout or a worktree
under `.claude/worktrees/`, also a removed one) loses its artifacts and
its fingerprint dir (`.fingerprint/PKG-HASH`), in each profile and each
target triple; cargo builds it again. A unit of the registry and a unit
with no manifest dir stay. (2) Each file that another live checkout
changed against the commit of the checkout, and each file that the
checkout changed, gets a new time, so cargo builds its crate again from
the text of the checkout. Its self-test (17 checks, run by check.sh)
plants a fake target with units of a worktree, of a removed worktree, of
the lead checkout, of the registry and of a target triple, and checks what
goes and what stays, from the lead checkout and from a worktree. A dry
run from a worktree over the shared target of the lead checkout found 119
units to remove: 110 that name the removed worktrees `bearerfuzz`,
`causeonce` and `tempmove`, and the units of the lead checkout. The rule
of the target dirs of the workers is in `docs/hosted-sessions.md` ("The
brief of a worker", "Parallel work and merges") and in
`docs/maintenance.md`, "Rules of the work".

Not measured: the time of the rebuild after a removal, and a full check of
the lead checkout after a merge of agent work.

# T-1385: agent tooling: read the latest commits of the sibling repositories and take each improvement of their agent tooling (Q23)

**Source:** the owner, 2026-11-25: the answer to Q23 (the ruling "Hosted sessions and worker roles", `docs/decisions.md`), and the word that the work of T-1374 left out improvements of the sibling repositories.
**Category:** chore
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

T-1374 took one commit of podvm (`2d36b23`) and left out parts of it,
among them the role table of podvm's ruling "Hosted sessions and worker
roles", which now holds for podcrates. podrunner, podssh, podnet and podvm
change their agent tooling often: the start of a session, the watchers,
the hooks, the instructions for each harness, the checks before a push,
the role table, the QA passes. podcrates has no step that reads those
changes, so its tooling falls behind its siblings.

## Premise

- Read: the report of T-1374 (the table of the parts of
  PodUtils/podvm@2d36b23 that it took and did not take, in its Done
  section).
- Not read: the commits of podrunner, podssh, podnet and podvm after
  2026-11-24 on `AGENTS.md`, `CLAUDE.md`, `.claude/`, `.githooks/`,
  `scripts/` (their agent and session scripts) and their docs of the
  session protocol.

## Approach

1. For each sibling repository, list the commits since 2026-11-20 that
   touch the files of the agent tooling (the forge mirror first, AGENTS.md
   section 4; a shallow clone where the mirror fails).
2. For each change: what it improves, whether podcrates has it, needs it,
   or must not take it (a matter of that project alone), with the reason.
   The parts that T-1374 left out are read again first. A change that
   would alter a ruling of podcrates is a question for the owner.
3. Take the role table of "Hosted sessions and worker roles" into
   `docs/maintenance.md` ("Rules of the work") and the briefs of the
   agents; propose one shared `docs/session-protocol.md` for each
   repository of PodUtils by a COORD issue `(to all)`.
4. Apply each needed part in the form of podcrates, test each script in a
   scratch clone, and record the table of what was taken in the Done
   section.

## Prove

```sh
sh scripts/session-start.sh
cargo todo check
```

## Done

2026-11-25, the commit that names T-1385 and T-1384. Read: the clones of
podrunner (`f18c7e1`), podssh (`13623b3`), podnet (`46c7181`) and podvm
(`34cac6e`), with the commits since 2026-11-18 of their `AGENTS.md`,
`CLAUDE.md`, `.claude/`, `.githooks/`, start and setup scripts, agent
scripts, `docs/hosted-sessions.md` and their docs of the work.

| Improvement | Source | What it improves | Fate in podcrates |
| --- | --- | --- | --- |
| One start script for each harness; the hook only calls it | PodUtils/podvm@2d36b23, `PodUtils/podvm:scripts/session-start.sh` | A start in Codex and on each host | Already here (T-1374) |
| `origin/main` by an explicit refspec | PodUtils/podnet@46c7181, PodUtils/podrunner@f18c7e1 | `origin/main` moves also in a clone of another branch | Taken: `scripts/session-start.sh` |
| A branch with commits that `origin/main` does not have stays | PodUtils/podrunner@f18c7e1, `PodUtils/podnet:scripts/session-start.sh` | No unpublished work is left behind | Taken: `scripts/session-start.sh` |
| Each watcher holds a lock while it runs (`flock -n`) | PodUtils/podnet@46c7181, PodUtils/podssh@a13fef7 | A true "runs" also across PID namespaces | Taken, with the process check where no `flock` exists |
| A mask of 077 for `.work/`; a line of repair when the start fails | PodUtils/podrunner@f18c7e1, PodUtils/podvm@c5ea23c | Private logs; a clear next step | Taken: `scripts/session-start.sh`, `.claude/session-start.sh` |
| The watchers at a pinned commit, each checked by SHA-256 | PodUtils/podnet@46c7181, PodUtils/podrunner@f18c7e1 | No changed watcher runs | Taken into the text for each repository (`docs/session-protocol.md`); podcrates runs the files of its own tree |
| The shell gets the toolchain through `CLAUDE_ENV_FILE` | PodUtils/podssh@a13fef7, `PodUtils/podssh:.claude/session-start.sh` | Each command of the session finds cargo and zig | Taken: `.claude/session-start.sh`, new `scripts/cloud-env.sh` |
| `scripts/cloud-env.sh` for each shell | PodUtils/podrunner@f18c7e1, PodUtils/podnet@46c7181 | One line makes a shell ready, in each harness | Taken (PATH, `CARGO_BUILD_JOBS=4`, `CARGO_INCREMENTAL=0`) |
| The toolchain and the caches in `.work/cloud/` | PodUtils/podnet@46c7181, PodUtils/podssh@a13fef7 | A checkout that brings its toolchain | Not taken: one toolchain in `$HOME` serves the lead and each worktree; a copy in each checkout takes GB of disk |
| zig for an aarch64 host; a time limit and a lock for the setup | PodUtils/podrunner@f18c7e1, `PodUtils/podrunner:scripts/cloud-setup.sh` | Setup on ARM64; no hang | Taken: `scripts/cloud-setup.sh`, `scripts/fetch-zig.sh` (`ZIG_ARCH`; the SHA-256 of index.json of ziglang.org) |
| The setup inside each start (`cloud-setup.sh --native`) | PodUtils/podssh@a13fef7 | A ready host with no step by hand | Not taken: a download in the hook delays each start; the start says when cargo or zig is missing |
| The debug info of the dev profile cut in `cloud-env.sh` | PodUtils/podnet@46c7181, PodUtils/podssh@a13fef7 | A smaller target | Not taken now: T-1388 measures it first |
| The file creation mask of CI for the tests | PodUtils/podssh@13623b3 | A test of a file mode gives the result of CI | Taken: `umask 022` in `scripts/agent/check.sh` |
| A subreaper where PID 1 keeps exited children | PodUtils/podnet@46c7181, `PodUtils/podrunner:docs/cold-start.md` | Tests of child processes pass on such a host | Taken: `docs/hosted-sessions.md` |
| The role table of the ruling "Hosted sessions and worker roles" | PodUtils/podvm@2d36b23, `PodUtils/podvm:docs/hosted-sessions.md` | One choice of model and effort for each role | Taken: `docs/hosted-sessions.md` (podnet and podssh keep their start there too) |
| A row for reviews | `PodUtils/podrunner:docs/maintenance.md` ("Subagents") | Reviews get deeper reasoning | Taken in the row of the implementer |
| One writer of the record; tasks on disjoint files; a claim is a claim until read; the proof is the lead's | `PodUtils/podvm:TODO/RULES.md` (section 12) | Fewer conflicts and wrong counts | Taken: `docs/hosted-sessions.md` |
| A stopped agent: a WIP commit on its branch; `partial` before a long step; a read again after a compaction | `PodUtils/podssh:AGENTS.md` ("Drafting agents", "The loop") | No work lost at a usage limit | Taken: `docs/hosted-sessions.md` |
| Drafting agents that never build | `PodUtils/podssh:AGENTS.md` | One build machine for podssh | Not taken: the implementers of podcrates build under the one lock (the ruling) |
| Do not end the turn to wait for CI | `PodUtils/podnet:CLAUDE.md` ("Work without a stop") | No idle wait | Taken: `docs/hosted-sessions.md` ("The watchers"); the ruling "Work until done" is here |
| A dispatch with `gh workflow run` in Codex; read first whether a run started | PodUtils/podvm@34cac6e, PodUtils/podssh@6c4f76d | A dispatch in each harness | Taken: `AGENTS.md` section 4, `docs/maintenance.md`, `docs/hosted-sessions.md` |
| A deny of force-push in the settings | `PodUtils/podvm:.claude/settings.json` | The rule of `AGENTS.md` section 3 in the tool | Taken: `.claude/settings.json` |
| More tools allowed (worktrees, cron, artifacts) | `PodUtils/podssh:.claude/settings.json` | No prompt for a routine tool | Taken: `.claude/settings.json` |
| A global git identity in a cloud session | PodUtils/podvm@2d36b23 | The identity outside the repository | Not taken: the identity of the repository and the `env` of `.claude/settings.json` cover each commit here |
| The pre-push gate of a checked tree | PodUtils/podrunner, `PodUtils/podrunner:.githooks/pre-push` | No push of a tree that the checks did not pass | Taken: `.githooks/pre-push`; `notest` for `TODO/` and `docs/` alone; `wip/NAME` free |
| The commit-msg check with a self-test and a range, in CI | `PodUtils/podrunner:scripts/check-commit-msg.sh` | A commit made with no hook is found | Taken: `.githooks/commit-msg`, `scripts/agent/check.sh`, `.github/workflows/ci.yml` |
| A grep for failed tests that is not "failed" | `PodUtils/podrunner:scripts/agent/check.sh` | The cause of a failure shows; "0 failed" hid it | Taken (a defect of `scripts/agent/check.sh`, fixed) |
| The tests leave nothing in their temp dir | `PodUtils/podrunner:scripts/agent/check.sh` | No leak of temp files | Not taken now: T-1386 |
| A QA bundle (binary, manual, licence, `SHA256SUMS`) and the blind QA pass | PodUtils/podvm@2d36b23, PodUtils/podssh@6c4f76d, `PodUtils/podnet:scripts/agent/qa-pass.md` | An outside pass with the binary and the documents only | Taken: `scripts/qa-bundle.sh`, `docs/hosted-sessions.md` ("Blind QA"), for `podlab` and `pod-release` |
| A cold start file (`PodUtils/podvm:TODO/RESUME.md`, `PodUtils/podrunner:docs/cold-start.md`) | PodUtils/podvm@c5ea23c, PodUtils/podrunner@f18c7e1 | A short path to the work | Taken in the form of podcrates: `docs/hosted-sessions.md` ("Start") and the first steps of `docs/HANDOFF.md`; no second work order |
| The operator procedure `HUMAN.md`; the summary table and the reading receipt of a session; the task routing table | `PodUtils/podvm:HUMAN.md`, `PodUtils/podvm:docs/methodology/sessions.md`, `PodUtils/podvm:AGENTS.md` | The procedure of podvm and its corpus | Not taken: a saved summary is narrative history here (`scripts/check-history.py`); `AGENTS.md` section 8 names each document to read |
| `podcrates.sh`, `parity-snapshot.sh`, `watch-issues.sh`, `delete-stale-branches.ps1` | `PodUtils/podnet:scripts/agent/` | The view of podcrates from podnet; a refactor of podnet; an old watcher; a tool of the owner | Not taken: a matter of podnet alone (`watch.py` of podcrates replaced the watcher) |
| A merge driver or a tool for the record | none of the four | A merge of the branch of an agent with no conflict in the counts | New here: `scripts/agent/merge_todo.py`, `.gitattributes`, set by `scripts/session-start.sh` |
| A range of ids for each agent | the brief of podcrates; none of the four has a tool | No two branches take one id | Taken with a tool: `cargo todo next --from N`; the renumber steps in `docs/hosted-sessions.md` |

Also: `docs/session-protocol.md` has the section "Hosted sessions and
worker roles" for each repository; T-1387 holds the COORD issue `(to all)`
that proposes one table. `AGENTS.md` (sections 3, 4, 7, 8, 10, 11),
`docs/maintenance.md` (the start, the tools, the rules of the work, a run
of a workflow), `docs/HANDOFF.md` (the first steps) and `docs/design.md`
name the new files. A stale citation of `scripts/agent/check.sh` in
`TODO/foundation.md` points at the check of the size again. Call C938.

Tests: `python3 -I scripts/agent/merge_todo.py --self-test` (9 checks: a
real `git merge` of an agent branch with the driver ends clean with the
sorted rows and the true counts, and conflicts with no driver; two changes
of one row or one line stay a conflict); the driver over the real index:
the same file three times gives the same file, and a status on each side
plus a new row gives the counts that the rows give. `cargo test -p
pod-todo` (the test of `next --from`), `cargo clippy -p pod-todo
--all-targets -- -D warnings`. `.githooks/commit-msg --self-test` (8
checks, in `sh` and `dash`) and `--range` over the last 30 commits.
`scripts/qa-bundle.sh` with `pod-release`: the bundle passed `sha256sum
-c`, a changed licence failed it, a second run and a binary with no manual
exited 2 and 1 and left no stage dir. In a scratch clone of a bare copy of
the branch, `scripts/session-start.sh` run from `/` set the identity,
the hooks path and the merge driver, and started both watchers under their
locks (exit 0); a second run said "runs already"; a stopped watcher started
again alone; with a `PATH` with no `flock`, `setsid`, `pgrep` and
`timeout` the start and the second run worked, and the next start with
`flock` found those watchers. A clean branch at `origin/main` moved to
`main`; a branch with a local commit and a tree with changes stayed; a
worktree kept its branch and made no `.work/`; an origin that cannot be
reached gave one line. The hook from `/tmp` with no `CLAUDE_PROJECT_DIR`
wrote one line into `CLAUDE_ENV_FILE`, and a shell with an empty
environment that loaded it found cargo, with 4 jobs and no incremental
data. `.githooks/pre-push` refused a change of code with no check, passed
it after its tree was checked, refused a change of `TODO/` that was not
linted and passed it when linted, refused a linted change of
`docs/wire.md`, passed a linted change of `docs/` against the tree of the
remote ref with no checked tree, and passed a push to `wip/agent`.
`sh scripts/agent/check.sh notest`, `cargo todo check`,
`python3 -I scripts/check-history.py`, `python3 -I scripts/check-workflows.py`,
`python3 -I scripts/check-design-paths.py`, `python3 -I scripts/codegraph.py dups`.

Not verified: a start in Codex, on macOS and in a new Claude Code session
(the hook ran by hand); `cloud-setup.sh` on an aarch64 host; the step of
`ci.yml` on GitHub; a full check (with the tests) of the merged tree.

# T-1386: `scripts/agent/check.sh`: the tests run in a temp directory of their own, and an entry that a test leaves there is a fault

**Source:** T-1385: `PodUtils/podrunner:scripts/agent/check.sh` (its check `tmp-left`, podrunner's T-591).
**Category:** chore
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

A test that leaves a file or a directory in `TMPDIR` fills the disk of a
long session and of a runner, and it can make a later test pass or fail
by a stale file. No check of podcrates finds such a test. podrunner runs
its tests in a temp directory of their own and fails when an entry stays.

## Premise

- Read: `PodUtils/podrunner:scripts/agent/check.sh` (`tmp_left`, the
  tests with `TMPDIR` of their own).
- Not measured: how many tests of podcrates leave an entry in `TMPDIR`.

## Approach

1. Run the tests of the workspace with `TMPDIR` set to a new short
   directory (`mktemp -d /tmp/pc.XXXXXXXX`; a long one breaks the socket
   paths of T-920), and list each entry that stays after the run.
2. Fix each test that leaves an entry (a guard that removes its directory
   also on a panic).
3. Make an entry that stays a fault of `scripts/agent/check.sh` (a line
   for each entry, as podrunner prints them), and remove the directory.

## Prove

```sh
sh scripts/agent/check.sh
```

# T-1387: COORD `(to all)`: one `docs/hosted-sessions.md` with one role table for each repository of PodUtils

**Source:** T-1385; the owner's ruling "Hosted sessions and worker roles" (Q23): one role table for each repository of PodUtils, which podcrates takes from podvm until a shared document holds it.
**Category:** chore
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** partial

## Problem

The role table of the ruling is in `PodUtils/podvm:docs/hosted-sessions.md`
and now in `docs/hosted-sessions.md`. podnet and podssh keep a
`docs/hosted-sessions.md` with no role table, and podrunner keeps an older
table of its own in `PodUtils/podrunner:docs/maintenance.md`
("Subagents"). Each session can choose the models of its workers in a
different way. The tools of T-1385 (the merge driver of the record,
`cargo todo next --from N`, the fit of a shared target, the pre-push
gate, the check of commit messages in CI, `cloud-env.sh` through
`CLAUDE_ENV_FILE`) can help each repository.

## Premise

- Read: `docs/hosted-sessions.md`, `docs/session-protocol.md` (the
  section "Hosted sessions and worker roles"), the docs of the four
  sibling repositories at the commits of the Done section of T-1385.

## Approach

The coordinator opens this issue in podcrates after the next tag, with
the tag in place of TAG:

```text
COORD-NNN: one docs/hosted-sessions.md and one role table for each repository (to all)

The owner's ruling "Hosted sessions and worker roles" (2026-11-25) gives
one role table for each repository of PodUtils. podcrates TAG holds it in
docs/hosted-sessions.md, with the brief of a worker, and
docs/session-protocol.md, section "Hosted sessions and worker roles",
gives the text for each repository.

Ask (each repository, in its own form):
1. Keep docs/hosted-sessions.md with the role table of podcrates at TAG
   (lead, implementer, housekeeper, advisor; Claude Code and Codex), and
   the items of the brief of a worker. podrunner: replace the table
   "Subagents" of docs/maintenance.md with it.
2. Ask podcrates for a change of the table by a COORD issue
   (to podcrates); do not change it alone.

Offered, optional, each at TAG:
- scripts/agent/merge_todo.py: a merge driver of TODO/INDEX.md (the rows
  by id, the counts derived again) and TODO/PROGRESS.md, for a record of
  the form of pod-todo; .gitattributes and two lines of git config.
- cargo todo next --from N (pod-todo): the ids of one worker in a range.
- scripts/agent/stale_target.py: removes the units of a shared cargo
  target that another worktree built, before a full check.
- .githooks/commit-msg --range in CI: a commit made with no hook.
- scripts/cloud-env.sh loaded by the Claude Code hook through
  CLAUDE_ENV_FILE (podssh's form).

Answer by a COORD issue (to podcrates) with the commit of the take.
```

Map the issue in `TODO/issues.md` and `TODO/github.txt`, and close this
entry when each of the four sessions names its commit (or names why its
table differs).

## Prove

```sh
cargo todo check
```

# T-1388: The debug info of the dev and test profiles, measured for the size of the target

**Source:** T-1385: `PodUtils/podnet:scripts/cloud-env.sh` and `PodUtils/podssh:scripts/cloud-env.sh` set `CARGO_PROFILE_DEV_DEBUG` and `CARGO_PROFILE_TEST_DEBUG` (0, `line-tables-only`).
**Category:** chore
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

`target/` of podcrates grows past 20 GB, and a full check needs 22 GB
free (`AGENTS.md`, section 7; `docs/maintenance.md`, "Rules of the
work"). The full debug info of the dev and test profiles is a large part
of it. podnet and podssh cut it in the environment of a shell. In
podcrates a value in the environment of one shell alone gives another
fingerprint than CI and than a shell with no value, so each change
rebuilds the whole workspace.

## Premise

- Read: `PodUtils/podnet:scripts/cloud-env.sh`,
  `PodUtils/podssh:scripts/cloud-env.sh`, `scripts/cloud-env.sh`.
- Not measured: the size of `target/debug` after a full check with the
  default debug info and with `debug = "line-tables-only"`, and the time
  of each build.

## Approach

1. Measure both sizes and both times of a full check in a target dir of
   its own (`du -sh`, the seconds of check.sh).
2. When the gain is large, set `debug = "line-tables-only"` in
   `[profile.dev]` and `[profile.test]` of `Cargo.toml`, so that CI, each
   shell and each worker agree. A failed test still names its file and
   line in a backtrace: check one.
3. Write the result and the free space that a full check needs in
   `docs/maintenance.md`.

## Prove

```sh
sh scripts/agent/check.sh
```

# T-1401: `pod-core`: the drop of a `FileLock` only closes its descriptor, so a copy of the descriptor in a child that a spawn makes keeps the lock after the drop

**Source:** PodUtils/podssh#360 (COORD-150 of podssh), ask 3; author COLLABORATOR.
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The text of the ask: "`pod-core` `FileLock` has no `Drop`; only the close
releases the `flock`. A sibling's spawn that copied the descriptor until
its exec keeps the lock for a moment after the drop. PodUtils/podssh@T-849 saw
this once on macOS, where a lock was refused after the other holder's
drop. Ask: `impl Drop for FileLock` that unlocks explicitly before the
close. An explicit unlock releases every copy of the descriptor."

`crates/pod-core/src/lock.rs` holds the lock in a `std::fs::File`, and has
no `Drop`. A lock of `flock(2)` belongs to the open file description, not
to the descriptor: a close releases it only when no other descriptor of
that description is open. The descriptor is close-on-exec (`O_CLOEXEC`,
the default of `std`), so a child holds no copy after its exec. But a
spawn of another thread of the same process (`fork` and then `exec`)
copies each descriptor into the child, and the child closes the copy only
at its exec. A drop of the lock in that window leaves the lock held until
the exec. The root cause is that window, not a missing `O_CLOEXEC`.

## Premise

- Read: `crates/pod-core/src/lock.rs`, `crates/pod-core/src/lock_tests.rs`,
  the users of `FileLock` in the workspace (none outside `pod-core`; the
  documents of `pod-unit` name it), the text of the ask.
- `std::fs::File::unlock` is `flock(fd, LOCK_UN)` on Unix and
  `UnlockFile` on Windows.
- Not measured: the window on macOS. A `dup` of the descriptor held by the
  test is the same state as the copy of a child before its exec.

## Approach

- Add `impl Drop for FileLock` that calls `self.file.unlock()` before the
  close; ignore its error, as the close releases the lock in the other
  cases.
- Test: hold a `dup` of the descriptor of a lock (`try_clone`), drop the
  lock, and take the lock again at once through a new open of the path.
- Add a plant that removes the unlock.
- Update the README of `pod-core` and the module text of `lock.rs`.

## Prove

```sh
cargo test -p pod-core --all-features --lib lock
sh scripts/plant-each.sh core-lock-drop-keeps-a-copy
```

## Done

2026-11-25. Commit: see `git log` for T-1401.

The change. `crates/pod-core/src/lock.rs`: `impl Drop for FileLock` calls
`File::unlock` (`flock(fd, LOCK_UN)` on Unix, `UnlockFile` on Windows)
before the close, and ignores its error. The module text says the rule.
The descriptor was close-on-exec before the change (`O_CLOEXEC`, the
default of `std`): the cause is the window between the `fork` and the
`exec` of a spawn of another thread, not a missing flag.

The test. `lock_tests::a_dropped_lock_is_free_while_a_copy_of_its_descriptor_is_open`
holds a `dup` of the descriptor (`try_clone`), drops the lock, and takes
the lock again at once, for each mode. The plant
`core-lock-drop-keeps-a-copy` removes the unlock, and the test fails
there (the code before the fix).

The results:

- `cargo test -p pod-core -p pod-e2e --all-features`: 242 passed, 0
  failed.
- `cargo clippy -p pod-core -p pod-e2e --all-targets --all-features -- -D
  warnings`: clean. `cargo fmt --all --check`: clean.
- `sh scripts/plant-each.sh --apply`: 0 stale.
  `sh scripts/plant-each.sh core-lock-drop-keeps-a-copy
  e2e-target-failure-reads-as-the-channel`: 2 caught.
- `sh scripts/agent/check-macos.sh`: 0 faults.

Not measured: the window on macOS, and the test on Windows (CI runs it).
The README of `pod-core` names the unlock, and the two functions
`try_lock_existing` and `lock_until` that its row did not name.

# T-1444: `pod-core`: `a_path_that_is_there_is_never_opened_truncated_or_removed` fails on Windows: a file made at the name of a directory gives "Access is denied", not `AlreadyExists`

**Source:** `nightly.yml` run 45603454952 at `cd7cb5a`, the jobs `test (windows-2025)` (139168091494) and `test (windows-11-arm)` (139168091781), step "Test", binary `pod_core` (lib tests). Beside T-1343.
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The test fails at `crates/pod-core/src/files/temp/tests.rs:91` on both Windows runners:
`called Result::unwrap() on an Err value: Os { code: 5, kind: PermissionDenied, message: "Access is denied." }`.
The line calls `new_named(&dir, "d", "", ..., &mut create_file)` when a directory `d-A` is there. The call must take the next name (`d-C`) after `AlreadyExists`. On Windows, the exclusive create of a file at the name of a directory gives `ERROR_ACCESS_DENIED`, so `new_named` stops with the error. The function `new_temp_file` and `new_temp_with` can fail the same way when a directory has the name that was drawn. The test is new with T-1343 (`9a6b9933`), so no earlier run passed it on Windows. The 88 other tests of the binary pass.

## Premise

- Read: the log of both jobs; `files/temp.rs` (`new_named` retries only on `AlreadyExists`); `create_new_private` of `files/` for Windows.
- Not measured: the code of the error that `create_new_private` gives on Windows for a directory at the name. The cause is from the line of the test and the code, not from a run here (Windows does not run here).

## Approach

1. In the Windows `create_file` (`files/temp.rs`, `create_new_private`), when the create fails with `PermissionDenied` and `symlink_metadata(path)` says that the path is there, return `AlreadyExists` with the same text. Keep the other errors as they are.
2. Add a test that is `cfg(windows)` or runs on each system: a directory at the name, then `new_named` with `create_file` takes the next name. Check that the test fails before the change on the Windows jobs.
3. Check `sh scripts/agent/check-windows.sh` for compile.

## Prove

```sh
cargo test -p pod-core --all-features --lib files::temp
sh scripts/agent/check-windows.sh
```
The jobs `test (windows-2025)` and `test (windows-11-arm)` of the next `nightly.yml` run pass the binary `pod_core`.

## Done

2026-11-25. Commit: see `git log` for T-1444.

- `crates/pod-core/src/files/temp.rs`: the new function `as_taken` makes `AlreadyExists` of a `PermissionDenied` when `symlink_metadata` finds something at the name; the text of the error stays. A `PermissionDenied` with nothing at the name, and each other error, stays as it is. `new_named` applies it to each create, so `new_temp_dir`, `new_temp_file` and `new_temp_with` (also with a create of the caller) take a new name on each system.
- `crates/pod-core/src/files/closed_win.rs`: the Windows `create_new` (of `create_new_private` and `create_new_public`) applies it too, so `create_private` and its create in place give `Ok(false)` for a directory at the name, as on a Unix system.
- Tests: `a_denied_create_at_a_name_that_is_there_takes_a_new_name` (a create that acts as Windows: a directory at the name, then the next name; `PermissionDenied` with nothing at the name ends the tries after 1 call) and `only_a_denied_create_at_a_name_that_is_there_is_a_taken_name`. Both run on Linux.
- Plant `core-temp-denied-name-not-taken` (no `as_taken` in `new_named`): caught here, `a_denied_create_at_a_name_that_is_there_takes_a_new_name ... FAILED`. No Linux test catches the removal of the call in `closed_win.rs`: that code compiles for Windows alone.
- Verified here: `cargo test -p pod-core --all-features --lib files::temp` (13 passed); `cargo clippy -p pod-core -p pod-tpm --all-targets --all-features --target x86_64-pc-windows-gnu -- -D warnings` (zig as the C compiler, as `check-windows.sh` does): no warning.
- Not verified here: a run on Windows. The jobs `test (windows-2025)` and `test (windows-11-arm)` of the next `nightly.yml` run prove the fix.

# T-1445: `pod-tpm`: two tests of `tests/socket.rs` fail on macOS: `set_read_timeout` on a stream whose peer closed gives an error, so the call says `Unavailable`, not `Protocol`

**Source:** `nightly.yml` run 45603454952 at `cd7cb5a`, the jobs `test (macos-15)` (139168091509, one test) and `test (macos-15-intel)` (139168093296, two tests), step "Test", binary `socket` of `pod-tpm`.
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`unix::a_short_response_is_a_protocol_failure` (`tests/socket.rs:100`) and `unix::a_bad_acknowledgement_is_a_protocol_failure` (`:111`) expect the class `Protocol`; they get `Unavailable` with the text "the time limit cannot be set on the TPM socket". The stand-in answers with a cut response and closes its end. `Socket::wait` (`src/transport/socket.rs`, `fill`) sets the read and write timeouts before each read. On macOS, `setsockopt` of `SO_RCVTIMEO` and `SO_SNDTIMEO` on a Unix stream whose peer is closed fails (`EINVAL`), so the call that comes after the first bytes fails before it reads the end of the answer. On Linux the call works. The macOS job of ARM failed one test, the Intel job two: the result depends on the time of the close. The tests are new (`0cd0f488`, T-1221), so this is the first nightly that runs them on macOS.

## Premise

- Read: both logs and the code of `wait` and `Stream::timeouts`.
- Not measured: the `errno` on macOS (the cause is the known behaviour of macOS for this call; no macOS host here).

## Approach

1. In `wait`, treat a failure of `timeouts` with the kind `InvalidInput` as no failure on macOS (`cfg(target_vendor = "apple")`): a closed peer never blocks a read, so the next `read` gives the bytes that remain or the end. Keep the failure for each other error and for each other system.
2. Name the reason in a comment of one line.
3. Check `sh scripts/agent/check-macos.sh` for compile.

## Prove

```sh
cargo test -p pod-tpm --all-features --test socket
sh scripts/agent/check-macos.sh
```
The jobs `test (macos-15)` and `test (macos-15-intel)` of the next `nightly.yml` run pass the binary `socket` of `pod-tpm`.

## Done

2026-11-25. Commit: see `git log` for T-1445.

- `crates/pod-core/src/sockopt.rs` (new, public): `shut_needs_no_limit` takes the raw `EINVAL` (22) of a set of a socket option as `Ok` on `target_vendor = "apple"`. XNU `sosetopt` refuses each option on a socket that the peer shut in both directions, and such a socket never blocks: a read gives the bytes that remain, then the end; a write fails at once. Each other result stays; the `InvalidInput` of the standard library (a time of zero) has no code of the system and stays an error. This is the rule of T-1041 (`pod-control`), now in `pod-core` for each crate; T-1453 moves `pod-control` to it.
- `crates/pod-tpm/src/transport/socket.rs`: `Stream::timeouts` sets the read and the write limit through `shut_needs_no_limit`, for TCP and for a Unix stream. After the close of the stand-in, the next read gives the rest of the cut response, then the end: the class is `Protocol`.
- Tests: `sockopt::tests::only_the_einval_of_darwin_is_a_shut_socket` (the branch of Darwin runs on Linux through `shut_on(.., true)`) and `sockopt::tests::a_time_of_zero_stays_an_error`.
- Plant `core-sockopt-shut-socket-fails` (the `EINVAL` of Darwin stays an error): caught here. No Linux test catches the removal of the call in `pod-tpm`: Linux sets a limit on a closed Unix stream, so `tests/socket.rs` passes with or without it.
- Verified here: `cargo test -p pod-tpm --all-features --test socket` (10 passed); `cargo test -p pod-core --all-features --lib sockopt` (2 passed); `cargo clippy -p pod-core -p pod-tpm --all-targets --all-features -- -D warnings`, also for `x86_64-pc-windows-gnu`; `sh scripts/agent/check-macos.sh` (the workspace and its tests for `x86_64-apple-darwin`): 0 faults.
- Not verified here: a run on macOS. The jobs `test (macos-15)` and `test (macos-15-intel)` of the next `nightly.yml` run prove the fix.

# T-1446: `pod-warp`: `warp_exec_finds_warp_up_of_another_state_directory` fails on macOS: its socket path is longer than 104 bytes

**Source:** `nightly.yml` run 45603454952 at `cd7cb5a`, the jobs `test (macos-15)` (139168091509) and `test (macos-15-intel)` (139168093296), step "Test", lib tests of `pod-warp`. Beside T-1343 and T-1359.
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The test fails at `crates/pod-warp/src/find.rs:205` with "path must be shorter than SUN_LEN" when it binds `TEMP/pod-warp-find-other-RANDOM/other-run/warp.sock`. `test_dir` takes `new_temp_dir` (T-1343), whose `RANDOM` has 16 hex digits. With a `TEMP` of 49 bytes on macOS the path has 106 bytes, and macOS holds 104 (`sun_path`). The 170 other tests of the binary pass.

## Premise

- Measured: on Linux with a `TMPDIR` of 52 bytes, `cargo test -p pod-warp --lib find::` before the change: 1 failed ("path must be shorter than SUN_LEN"); after it: 171 passed.
- Not measured: a run on macOS.

## Approach

The tag of the test directory is `fo` (a path of 98 bytes for a `TEMP` of 49 bytes). Another test of this repository that binds a socket under `temp_dir()` with a name of `new_temp_dir` can meet the same limit: the next nightly of macOS shows it.

## Prove

```sh
TMPDIR=/tmp/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa cargo test -p pod-warp --all-features --lib
```
(a `TMPDIR` of 52 bytes). The jobs `test (macos-15)` and `test (macos-15-intel)` of the next `nightly.yml` run pass the lib tests of `pod-warp`.

## Done

2026-11-25, this commit. `crates/pod-warp/src/find.rs`: the tag of `test_dir` in the test `warp_exec_finds_warp_up_of_another_state_directory` is `fo`. Proof: the `TMPDIR` of 52 bytes, before: 1 failed; after: 171 passed; with `TMPDIR=/tmp`: 5 passed in `find::`. Not measured: a run on macOS.

# T-1447: `nightly.yml`: the step of the doc tests runs the doc tests of `pod-h2`, which name `h2` and not `pod_h2`, so 41 fail on each system

**Source:** `nightly.yml` run 45603454952 at `cd7cb5a`, the jobs `test (windows-2025)`, `test (macos-15)`, `test (windows-11-arm)` and `test (macos-15-intel)`, step "Test", "== doc tests": `error: 1 target failed: -p pod-h2 --doc`.
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`vendor/h2` is the package `pod-h2` with the library `pod_h2` and `doctest = false` (`aa52569d`, T-1095). A run of `cargo test --workspace` skips its doc tests, but `cargo test --doc` runs them: the explicit `--doc` does not honour `doctest = false`. Its 41 doc tests use `h2::`, so each fails with "unresolved import h2" (0 passed, 41 failed). `scripts/ci/test-limited.sh` runs `cargo test "$@" --doc`, so each job `test` of `nightly.yml` ends red on this step (the cross jobs run test binaries only).

## Premise

- Measured here (Linux): `cargo test -p pod-h2 --doc`: 41 failed. `cargo test -p pod-h2`: no doc test runs.
- With all packages but `pod-core` and `pod-h2` excluded, `--doc` fails (the doc tests of `pod-h2`); with `--exclude pod-h2` too, the doc tests of `pod-core` pass.

## Approach

`scripts/ci/test-limited.sh` passes `--exclude pod-h2` to the `--doc` run. Both callers give `--workspace`. The doc tests of the vendored code are not ours to keep (the ruling "Vendoring").

## Prove

```sh
sh scripts/ci/test-limited.sh 300 600 --locked --workspace --all-features --no-fail-fast
```
The doc step of each job `test` of the next `nightly.yml` run ends with no target failed.

## Done

2026-11-25, this commit. `scripts/ci/test-limited.sh`: `--exclude pod-h2` in the `--doc` run, with a comment of one line. Proof as in the Premise (the two runs on Linux). Not measured: the full `--doc` run of the workspace here, and the run on Windows and macOS.

# T-1463: `pod-core`: no check stops a new `to_string()` or `{}` of a `Failure` that drops its cause, and the `{}` of a `Failure` in a `format!` was not searched

**Source:** finding of T-1399 (PodUtils/podssh#360, ask 1)
**Category:** chore
**Milestone:** M5
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

`Display` of a `pod_core::Failure` writes its text alone. Since T-1351, a failure of a step keeps the error of the system as its cause and not in its text (`Failure::io`), so each `to_string()` of such a failure drops the cause: "no UDP socket for QUIC" with no cause (T-1399). T-1399 found the calls of `to_string()` by a temporary deprecated inherent method on `Failure`, on Linux only. Nothing stops a new call. A `format!("{e}")`, a `{}` and a `write!` of a `Failure` drop the cause the same way, and the temporary method does not see them.

## Premise

- Read: `crates/pod-core/src/failure.rs` (`Display` writes the text; `chain` joins each cause once); the search of T-1399.
- Not searched: each `{}` of a `Failure` in a `format!`, `write!`, `tracing` field or `panic!`; the code of Windows and macOS alone (`cfg`).

## Approach

1. A search of each `{}` of a `Failure`: a temporary build in which `Failure` has no `Display` (and `Error` gives the chain by another type) names each site as an error of the compiler; read each, and use `chain()` where the cause is lost.
2. A check in CI (`scripts/`): the temporary deprecated inherent `to_string` of T-1399 in a copy of the tree, `cargo check --workspace --all-targets` on Linux, Windows and macOS, and a failure on each new warning outside a test.

## Prove

```sh
cargo check --workspace --all-targets --all-features
cargo todo check
```

# T-1480: three crates keep their own guard that aborts a tokio task on drop

**Source:** finding of T-1405 (PodUtils/podssh#361)
**Category:** chore
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The rule "Shared code" asks for one copy of a capability. A guard that
aborts a tokio task when it drops exists three times:
`pod-relay` (`AbortOnDrop`, in `src/abort.rs` before this entry, T-1405, which replaced the
`Driver` of `http_road/one_h2.rs`), `crates/pod-cloudflare/src/edge_h2/serve.rs`
(`AbortOnDrop`, with `into_inner`) and `crates/pod-warp/src/tun.rs`
(`Abort`). `codegraph.py similar abort drop guard` finds none of them by
these words.

## Premise

- Read: the three guards; `pod-core` has the feature `async` with tokio,
  but not the feature `rt` of tokio that `JoinHandle` needs.
- Not read: each other `impl Drop` that aborts a task in the workspace
  (`pod-natprobe` `TaskPool`, `pod-relay` `blocking::Client`).

## Approach

1. One guard in `pod-core` (feature `async`, with tokio's `rt`):
   `AbortOnDrop(JoinHandle<T>)` with `into_inner`.
2. `pod-relay`, `pod-cloudflare` and `pod-warp` use it; remove the three
   copies.
3. The tests of each crate stay green; a test in `pod-core` that a drop
   aborts the task.

## Prove

```sh
python3 -I scripts/codegraph.py dups
cargo test -p pod-core -p pod-relay -p pod-cloudflare -p pod-warp --all-features
```

## Done

2026-11-26. Commit: see `git log` for T-1480.

- `pod-core`: new module `abort` (feature `async`; tokio gets the feature `rt`): `AbortOnDrop<T = ()>` with `new` and `into_inner`. The copies of `pod-relay` (`src/abort.rs`), `pod-cloudflare` (`edge_h2/serve.rs`) and `pod-warp` (`tun.rs`, `Abort`) are gone.
- Public API: `pod_core::abort::AbortOnDrop` is new.
- Tests: `abort::tests::a_dropped_guard_aborts_its_task`, `abort::tests::into_inner_lets_the_task_go_on`; the tests of the three crates pass.
- The plant `relay-t1405-operator-tasks-outlive-exchange` has its patch made again on the new import and call.
- `scripts/dups-baseline.txt` had no line for this clone.

# T-1414: Dependabot of 2026-11-26: coturn 4.19.2, Mosquitto 2.1.4, TruffleHog 3.100.0, two actions, eight crates and saphyr-parser 0.4.1; iroh-dns 1.5.0, rand 0.10.4 in `pod-h2` and simple-dns 0.13.1 declined (GitHub #264 to #271)

**Source:** GitHub #264 to #271 (Dependabot)
**Category:** chore
**Milestone:** M0
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

Eight updates of Dependabot wait for a judgement (the ruling "Dependabot": each judged update is a commit of its own on `main`, and the pull request closes with a link to it). Each branch is from an older `main`.

## Premise

- Read: the diff and the checks of each pull request; the index digests of `coturn/coturn:4.19.2`, `library/eclipse-mosquitto:2.1.4-alpine` and `trufflesecurity/trufflehog:3.100.0` at `registry-1.docker.io` (they agree with the diffs); `git ls-remote` of `actions/setup-go` `v7.1.0` (`d8cbb80`) and `docker/login-action` `v4.8.0` (`8aa5831`) (they agree with the diffs).
- The index of crates.io: `iroh` 1.4.2 and `iroh-dns` 1.4.2 build `simple-dns` 0.12; `iroh-dns` 1.5.0 and `iroh` 1.5.0 build 0.13. `pod-iroh` holds `iroh` and `iroh-dns` below 1.5.
- `vendor/h2` (`pod-h2`) is the fork of h2 0.4.20; `scripts/check-vendor.py` holds it to h2 plus its patches. `rand` 0.8.8 stays in the tree for `quickcheck` and `num-bigint-dig`.
- The CHANGELOG of saphyr-parser 0.4.0 and 0.4.1: the API is the same; the span of a block scalar starts at its `|` or `>`.
- `cargo update -p clap` on `main` locks "to highest Rust 1.63 compatible versions": `pod-h2` has `rust-version = "1.63"`, and the resolver of edition 2024 takes the lowest `rust-version` of the members.

## Approach

Apply #264 to #267 and #271 on the current `main` as commits of their own, with the documents that name a version. Apply #268 with no `iroh-dns` 1.5.0: it adds `simple-dns` 0.13 beside the 0.12 of `iroh` 1.4.2; the move to iroh 1.5 is T-1484. Set `incompatible-rust-versions = "allow"` in `.cargo/config.toml`, so that the fork does not hold each update to Rust 1.63. Decline #269 and #270 with the reason.

## Prove

```sh
sh scripts/agent/check.sh
cargo deny check
```

## Done

2026-11-26. Commit: see `git log` for T-1414.

- #264: coturn 4.19.2 by the digest of its index, with the three lines of `crates/pod-turn/README.md` and the coturn rows and source S10 of `docs/live-calls.md` (they named 4.18.0) (`bb8d36e`).
- #265: Eclipse Mosquitto 2.1.4 by the digest of its index, with its row and source S3 of `docs/live-calls.md` (`11d4ae0`).
- #266: TruffleHog 3.100.0 by the digest of its index (`26cce5c`).
- #267: `actions/setup-go` v7.1.0 and `docker/login-action` v4.8.0, each by the full commit SHA of its tag (`c60af07`); `check-workflows.py`: 0 faults.
- #268: toml 1.2.2, clap 4.6.9, bytes 1.12.2, crc32fast 1.5.6, zeroize 1.9.2, russh 0.65.4, regex 1.13.2 and plist 1.10.4, with the resolver setting of `.cargo/config.toml`; ten crates with a wide range of a dependency now take its newest version (windows-sys 0.61, syn 3, thiserror 2, indexmap 2, getrandom 0.4); no crate is added, and russh 0.65.4 drops generic-array 1.4.5 (the lock has 851 packages, it had 852) (`cf1508b`). `iroh-dns` 1.5.0 declined: `iroh` 1.4.2 builds `simple-dns` 0.12 and `iroh-dns` 1.5.0 builds 0.13, so the pair adds a second copy; T-1484 moves iroh, iroh-dns and simple-dns together.
- #269 declined: the change of `vendor/h2/Cargo.toml` is not in the patches of the fork (`check-vendor.py` fails), the test code of h2 uses the API of rand 0.8 (the job `lint` of the pull request fails in Clippy: `rand::distributions`, `thread_rng`, `gen_range`), and `rand` 0.8.8 stays for `quickcheck` and `num-bigint-dig`, so no crate goes. The fork follows its upstream.
- #270 declined: `iroh` 1.4.2 and `iroh-dns` 1.4.2 build `simple-dns` 0.12, and 0.13 adds a second copy; it moves with T-1484.
- #271: saphyr-parser 0.4.1 (the requirement `0.4.1`, with the fixes of 0.4.1), with `fuzz/Cargo.lock`, and the test `a_block_scalar_starts_at_its_indicator` of `pod-actions-workflow` (`87384bc`).

The full check and `cargo deny check` passed on the final tree (see the commit of this entry). Not verified here: the builds on Windows and macOS with the new edges of the lock file (CI), the coturn, Mosquitto and TruffleHog images in a run (CI), and the release job with the new login action (a run on a tag).

# T-1484: `pod-iroh`: the move to iroh 1.5, iroh-dns 1.5 and simple-dns 0.13 together

**Source:** finding of T-1414 (GitHub #268, #270)
**Category:** chore
**Milestone:** M0
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

`pod-iroh` holds `iroh` and `iroh-dns` below 1.5, and takes the `simple-dns` that `iroh-dns` builds (0.12), so that no crate is added. `iroh` 1.5.0 and `iroh-dns` 1.5.0 build `simple-dns` 0.13, and `n0-dns-resolver` 0.1.1 builds 0.13. Dependabot offers each part alone (`iroh-dns` 1.5.0 in GitHub #268, `simple-dns` 0.13 in GitHub #270), and each part alone adds a second copy of `simple-dns`. The workspace stays on iroh 1.4.

## Premise

- The index of crates.io: `iroh` 1.5.0 requires `iroh-dns` ^1.5.0, `iroh-relay` ^1.4.2, `noq` ^1.5.0 and `simple-dns` ^0.13; `iroh-dns` 1.5.0 requires `simple-dns` ^0.13; `n0-dns-resolver` 0.1.1 requires `simple-dns` ^0.13.
- `crates/pod-iroh/Cargo.toml`: `iroh = ">=1.4.2, <1.5"` with `unstable-net-report` (an API outside the semver of iroh), `iroh-dns = ">=1.4.2, <1.5"`, `simple-dns = "0.12"`.
- `vendor/iroh-relay` is the fork of iroh-relay 1.4.2 (`scripts/check-vendor.py`); `iroh` 1.5.0 accepts `iroh-relay` ^1.4.2.

## Approach

Move the three ranges of `crates/pod-iroh/Cargo.toml` to 1.5 and 0.13 in one commit, with `n0-dns-resolver` 0.1.1. Adapt the code of `pod-iroh` to the changes of `unstable-net-report` and `simple-dns` 0.13. Keep the fork of iroh-relay at 1.4.2 when iroh 1.5 builds with it; else rebase the fork and run the command of each of its patches (the ruling "Vendoring"). The lock file then has one `simple-dns`.

## Prove

```sh
cargo tree -i simple-dns --workspace --all-features
cargo test -p pod-iroh --all-features
sh scripts/agent/check.sh
```
`cargo tree` shows one version of `simple-dns`.

# T-1517: `pod-core`: podssh's words of an I/O error with no "(os error N)" in the files of the cache and in `text.rs` (port of podssh's T-952)

**Source:** `upstream.yml` 45838753274 (job `sources`), `scripts/check-sources.py`.
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

After T-1421, podssh changed four imported files of `pod-core`:

```text
pod-core: changed Azathothas/podssh:crates/podssh-relay/src/cache/closed_win.rs -> crates/pod-core/src/files/closed_win.rs
pod-core: changed Azathothas/podssh:crates/podssh-relay/src/cache/own.rs -> crates/pod-core/src/files/own.rs
pod-core: changed Azathothas/podssh:crates/podssh-relay/src/cache/owner.rs -> crates/pod-core/src/files/owner.rs
pod-core: changed Azathothas/podssh:crates/podssh-ws/src/text.rs -> crates/pod-core/src/text.rs
```

The commits are `07d7d9b6`, `9f788985` and `0f1f64b2` (PodUtils/podssh@T-952
and PodUtils/podssh@T-990). `07d7d9b6` adds
`podssh_ws::text::plain` and `said`, which take Rust's " (os error N)"
off a line that a user reads. `9f788985` uses `said` at each `{e}` of an
`io::Error` in the files of the cache. `0f1f64b2` adds `file_said`: the
words for a file that a flag names ("no such file", or the directory that
is missing).

## Premise

- Read: `git diff 600d59e 595568e` of podssh for the four files;
  `crates/pod-core/src/text.rs` (`plain` and `io_words` of T-1421 are
  podssh's `plain` and `said`, with the same code); each `{e}` of an
  `io::Error` in `crates/pod-core/src/files.rs` and `crates/pod-core/src/files/`.
- `codegraph.py similar file missing directory does not exist no such
  file`: no function of podcrates gives the words of `file_said`.
- `crates/pod-core/src/lock.rs` and `crates/pod-core/src/process.rs` keep
  the `io::Error` as the source of a `Failure` or of a `RunFault`:
  `chain_of` names a cause once only when the text holds its words, so
  `io_words` in such a text names the error two times. They stay.
- Not measured: the code for Windows on Windows (`closed_win.rs`,
  `replace_win.rs`); `cargo clippy --target x86_64-pc-windows-gnu`
  checks it here.

## Approach

1. Port `file_said` as `pod_core::text::file_words(&Path, &io::Error)`,
   with podssh's test, and podssh's added case of `plain`.
2. Use `io_words` at each `{e}` of an `io::Error` in the files of the cache
   (the change of `9f788985`), and at the other such lines of
   `crates/pod-core/src/files/`: `crates/pod-core/src/files.rs`
   (`store_noting`, `read_private`), `temp.rs`,
   `crates/pod-core/src/files/socket/alias.rs`, `replace_win.rs`.
3. Tests: a refused read (a name past `NAME_MAX`) and a directory that
   takes no file in `own.rs`, the owner of such a path in `owner.rs`, the
   names that are there in `temp.rs`, the refusal of each directory in
   `crates/pod-core/tests/files.rs`. Plants: `read_own` keeps the code; `file_words`
   says "no such file" for a missing directory.
4. Move the podssh block of `SOURCES` to the head of podssh, with the
   blobs of `git ls-tree`.

## Prove

```sh
python3 -I scripts/check-sources.py --clone Azathothas/podssh=PODSSH --clone PodUtils/podnet=PODNET pod-core
cargo test -p pod-core --all-features
sh scripts/plant-each.sh core-t1517-own-read-keeps-code core-t1517-file-words-no-directory
```

## Done

2026-11-26. Commit: see `git log` for T-1517. The port point is podssh
`595568e`.

- New public API of `pod-core`: `text::file_words(&Path, &io::Error)`
  (podssh's `file_said`): "no such file", "the directory DIR does not
  exist", or `io_words` of another error. `plain` and `io_words` (T-1421)
  were already podssh's `plain` and `said`; podssh's added case of `plain`
  (a code with no blank before it stays) is in the test.
- Changed text for consumers: each line of `pod_core::files` that shows an
  `io::Error` has no " (os error N)" now: `read_own`,
  `create_file_in_first`, `store_noting` (and `store_file_in_first`),
  `read_private`, the owner of a path (`owner_of`), the SID and the access
  list on Windows, the names of `temp::new_named` that were there, the
  short path of a socket, the rename on Windows. The lines of
  `crates/pod-core/src/lock.rs` and `crates/pod-core/src/process.rs` keep
  the error as their source and stay (the Premise).
- Tests: `text::tests::file_words_say_what_is_missing`,
  `files::own::tests::a_refused_read_or_directory_says_the_words_of_the_system_alone`,
  `files::owner::tests::a_refused_owner_says_the_words_of_the_system_alone`;
  new asserts in `only_names_that_are_there_fail_after_the_bound` and
  `when_no_directory_works_the_error_names_each_one`.
- The plants ran by hand in this worktree (apply the patch, run RUN, find EXPECT, revert): `core-t1517-own-read-keeps-code` and
  `core-t1517-file-words-no-directory`: each caught (exit 101).
- `crates/pod-core/SOURCES`: the podssh block is at `595568e` (blobs
  `closed_win.rs` `2fd7bc9`, `own.rs` `1e5db84`, `owner.rs` `c23782a`,
  `text.rs` `99168ec`). `check-sources.py --clone
  Azathothas/podssh=CLONE --clone PodUtils/podnet=CLONE pod-core`: 14
  files, 0 changes; 2 files, 0 changes; exit 0.
- `cargo test -p pod-core --all-features`: 152 passed (the library 131),
  0 failed. `cargo clippy -p pod-core --all-targets --all-features -- -D
  warnings` and `cargo clippy -p pod-core --target x86_64-pc-windows-gnu
  --all-features -- -D warnings` passed.

# T-1519: `pod-gzip`: podvm took `Members` at `04c8b7f`, and the block of `SOURCES` has no `taken` line

**Source:** `upstream.yml` 45838753274 (job `sources`), `scripts/check-sources.py`.
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`scripts/check-sources.py` reads two imported files of `pod-gzip` as
changed:

```text
pod-gzip: changed PodUtils/podvm:crates/podvm/src/incus/gzip.rs -> crates/pod-gzip/src/lib.rs
pod-gzip:     04c8b7f Take podcrates v0.84.0 and prepare the Incus quota proof
pod-gzip: changed PodUtils/podvm:crates/podvm/src/incus/import_tests.rs -> crates/pod-gzip/tests/members.rs
pod-gzip:     cff32d7 Support channels and process limits in saved guests
pod-gzip:     04c8b7f Take podcrates v0.84.0 and prepare the Incus quota proof
```

`04c8b7f` is podvm's take of `pod_gzip::Members` (PodUtils/podvm#666):
`gzip.rs` keeps `Upload` and `wanted` and calls `pod_gzip`; the two tests
of the writer leave `import_tests.rs`. `cff32d7` changes the tests of the
import of podvm, which stay in podvm.

## Premise

- Read: `git show 04c8b7f` and `git show cff32d7` of podvm for the two
  files; `git diff cd51868 04c8b7f^` for the two files (no change before
  the take); `docs/maintenance.md`, "After a consumer takes a crate";
  T-1382 (the form of a `taken` line).
- `adoption/podvm/` has no trial of `pod-gzip`.
- Not done here: the record of the take on the hand-off issue (a write to
  GitHub; the lead does it).

## Approach

1. Add `taken 04c8b7f...` to the podvm block of `crates/pod-gzip/SOURCES`,
   with the reason in a comment: the files are then compared with the
   parent of the take.
2. No port: the take removes podvm's copy, and `cff32d7` changes the tests
   of podvm's import alone.

## Prove

```sh
python3 -I scripts/check-sources.py --clone PodUtils/podvm=PODVM pod-gzip
cargo test -p pod-gzip --all-features
```

## Done

2026-11-26. No port. Commit: see `git log` for T-1519.

- `crates/pod-gzip/SOURCES`: the podvm block has `taken 04c8b7f4...`,
  with the reason in its comment. `check-sources.py --clone
  PodUtils/podvm=CLONE pod-gzip`: "PodUtils/podvm at cd51868, taken at
  04c8b7f: 2 files, 0 changes", exit 0.
- `cff32d7` changes the tests of podvm's import (`Stub::layers`, the space
  of the storage pool); they stay in podvm.
- No trial of `pod-gzip` in `adoption/podvm/`, so no file to remove. The
  record of the take on the hand-off issue and the list of takes in
  `adoption/README.md` are for the lead.
- No code change, so no new test. `cargo test -p pod-gzip
  --all-features`: 15 passed, 0 failed.

# T-1600: `pod-core`, `pod-relay`: two tests take the error codes and the words of Linux, and fail on Windows and macOS

**Source:** `nightly.yml` run 45806031483 at `f2f8e3f5` (schedule, 2026-11-26), the jobs `test (windows-2025)` (139861405017), `test (windows-11-arm)` (139861405515), `test (macos-15)` (139861405141) and `test (macos-15-intel)` (139861405216), step "Test".
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

Two tests make an `io::Error` with `from_raw_os_error` and a number of
Linux, then compare the words of the system with the words of Linux:

- `pod-core` `text::tests::io_words_drop_the_code_of_the_system_and_keep_its_words`
  (`text.rs:367` at `f2f8e3f5`). On Windows, code 2 gives "The system
  cannot find the file specified.", not "No such file or directory". On
  macOS, code 104 is `ENOTRECOVERABLE` ("State not recoverable"), as
  `ECONNRESET` is 54 there.
- `pod-relay` `cleartext::tests::a_mint_that_was_cut_shows_the_words_of_the_system_with_no_code`
  (`cleartext.rs:278`): code 104 gives "Cannot request exclusive
  semaphores at interrupt time." on Windows and "State not recoverable"
  on macOS, with the cut `other`, not `reset`.

The code under test is correct: it removes " (os error N)" from the words
of each system. Each of the four jobs runs all its test binaries; these
two are the only failures.

## Premise

- Read: the logs of the four jobs; `text::io_words`; `cleartext::no_answer`.
- The test of T-1517 (`file_words_say_what_is_missing`) uses code 13 and
  compares with `io_words` of the same error, so it does not depend on the
  system.
- Not measured: the words of Windows in a language other than English
  (the runners are English).

## Approach

`pod_testkit::os` (Unix and Windows) gives `RESET` and `MISSING`, each
the code and the words of this system: `libc::ECONNRESET` and
`libc::ENOENT` on Unix, WSAECONNRESET (10054) and ERROR_FILE_NOT_FOUND (2)
on Windows. A test of the module checks that `io::Error` shows each code
with these words. The two tests take the code and the words from it. The
test of `pod-relay` also checks the cut `reset`.

## Prove

```sh
cargo test -p pod-testkit --all-features --lib os::
cargo test -p pod-core --all-features --lib text::
cargo test -p pod-relay --all-features --lib cleartext
cargo clippy -p pod-core -p pod-testkit --lib --tests --target x86_64-pc-windows-gnu -- -D warnings
cargo clippy -p pod-core -p pod-testkit --lib --tests --target x86_64-apple-darwin -- -D warnings
```
The jobs `test (windows-2025)`, `test (windows-11-arm)`, `test (macos-15)`
and `test (macos-15-intel)` of the next `nightly.yml` run pass the binaries
`pod_core` and `pod_relay`.

## Done

2026-11-27, commit 37270e68 (branch `night1126`). New module
`crates/pod-testkit/src/os.rs` (`RESET`, `MISSING`, and the test
`os::tests::each_code_has_the_words_of_this_system`). The tests of
`crates/pod-core/src/text.rs` and `crates/pod-relay/src/cleartext.rs` use
it. On Linux x86_64: `cargo test -p pod-core -p pod-testkit -p pod-session
-p pod-relay --all-features`: 914 passed, 0 failed; clippy clean. `cargo
clippy -p pod-core -p pod-testkit --lib --tests -- -D warnings` for
`x86_64-pc-windows-gnu` and `x86_64-apple-darwin`: clean. `pod-relay`
does not build for Windows on this host (no C compiler for `ring`). Not
verified: a run on Windows and macOS; the next `nightly.yml` run proves it.

# T-1650: agent tooling: the start, the watchers and the role table for Gemini / Antigravity Cloud (PodUtils/podnet#799)

**Source:** PodUtils/podnet#799 (COORD-141 of podnet, author COLLABORATOR, "(to podcrates)"); the owner's ruling "Gemini / Antigravity Cloud worker roles" (2026-11-27, `docs/decisions.md`)
**Category:** chore
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

The role table of `docs/hosted-sessions.md` names Claude Code and Codex
only. A host of Gemini / Antigravity Cloud stops each process that is not
a daemon task at the end of a turn, its setup file
(`.agents/environment.json`) keeps no process, and it has no
`CLAUDE_ENV_FILE`. There, the detached watchers of
`scripts/session-start.sh` stop, and a check in a new shell does not find
the toolchain. The ask: the column Gemini / Antigravity of the role table,
and the start of podnet `48770ce` (`--no-watchers`,
`--run-watcher watch|runs`, `.agents/environment.json`, the load of
`scripts/cloud-env.sh` in `check.sh`, the fallback to the PyYAML of
`/usr/bin/python3`, `cloud-setup.sh --dev-tools`) if it helps.

## Premise

- Read: PodUtils/podnet#799; podnet `48770ce`
  (`PodUtils/podnet:scripts/session-start.sh`,
  `PodUtils/podnet:scripts/cloud-setup.sh`,
  `PodUtils/podnet:scripts/agent/check.sh`,
  `PodUtils/podnet:scripts/agent/parity-snapshot.sh`,
  `PodUtils/podnet:.agents/environment.json`,
  `PodUtils/podnet:docs/hosted-sessions.md`); podcrates `scripts/session-start.sh`,
  `scripts/cloud-env.sh`, `scripts/cloud-setup.sh`,
  `scripts/agent/check.sh`, `.claude/session-start.sh`.
- podcrates runs its watchers from `scripts/agent/` of the checkout, not
  from a pinned copy: no pin and no checksum to port.
- podcrates `cloud-setup.sh` adds the musl target and zig by default: of
  `--dev-tools`, only the tools of the optional checks are new
  (`shellcheck` for `check.sh`, `groff` and `mandoc` for
  `scripts/check-man.sh`).
- Not measured: a session on a host of Gemini / Antigravity Cloud. The
  facts of the host come from podnet's documents.

## Approach

`scripts/session-start.sh` takes `--no-watchers` and
`--run-watcher watch|runs` (call C1013); with no flag the default start
stays. `scripts/cloud-setup.sh` takes `--dev-tools`. `check.sh` loads
`scripts/cloud-env.sh`, runs `check-workflows.py` with the python3 that
has PyYAML, checks `sh -n` of `cloud-setup.sh`, the refusal of a bad flag
of the start and the JSON of `.agents/environment.json`, and runs
shellcheck on the start scripts. `.agents/environment.json` runs the
start and the setup at the start of the VM. The documents: the column and
the start of each harness in `docs/hosted-sessions.md`, `AGENTS.md`
(sections 3 and 8) and `docs/session-protocol.md`. Also read the commits
of the session tooling of podnet, podvm, podrunner and podssh after the
ones that T-1385 read, and take each improvement that helps podcrates.

## Prove

```sh
sh -n scripts/session-start.sh && sh -n scripts/cloud-setup.sh
sh scripts/session-start.sh --no-watchers
timeout 20 sh scripts/session-start.sh --run-watcher runs
python3 -I scripts/check-history.py
cargo todo check
```
In a scratch clone that is not a worktree: `--no-watchers` starts no
watcher; `--run-watcher runs` holds `.work/runs.lock` with the pid of
`runs.py`; a second `--run-watcher runs` and the default start exit 0
and find it.

## Done

2026-11-27, the commits of the branch `hosted799` that name T-1650 (the
last one sets this entry). Read: podnet `48770ce`, `d79dcc2`, `5809e66`;
podssh `2f34ecf`, `b981f5f`; podvm `5244ed7`, `5e87281`, `c5ea23c`;
podrunner `b73a374`, `1453444`, `3d69d83`, `ac7f68b`, `1f66dd3` (shallow
clones of `main`, `git log` of the start, setup, agent, hook and
document paths).

| Change | Source | Fate in podcrates |
| --- | --- | --- |
| `session-start.sh --no-watchers` | podnet `48770ce`, podssh `2f34ecf` | Taken; it prints the two commands of the daemon tasks |
| `session-start.sh --run-watcher watch\|runs` (foreground `exec flock -F`) | podnet `48770ce`, podssh `2f34ecf` | Changed: the same lock, log and command line as the detached start, so each mode finds the other; a self-test of that watcher first; exit 0 when it runs; nothing in a worktree; no `flock`: a plain `exec` |
| Auto skip on `ANTIGRAVITY_APP_DATA_DIR` or `JULES_SESSION_ID`, with an override variable | podnet `48770ce`, podssh `2f34ecf` | Taken as `PODCRATES_NO_WATCHERS` (`auto`, `0`, `1`; another value exits 64) |
| The watchers of a pinned podcrates commit, by SHA-256 | podnet `48770ce` | Not taken: podcrates runs the watchers of its own tree |
| `.agents/environment.json` | podnet `48770ce`, podssh `2f34ecf` | Changed: `;` in the place of `&&`, so a start that fails still runs the setup; `check.sh` checks its JSON |
| `check.sh` loads `cloud-env.sh` | podnet `48770ce` | Changed: each time, not only when a toolchain dir exists; the script keeps each value that is set |
| `/usr/bin/python3` for PyYAML in `check.sh` | podnet `48770ce` | Taken |
| The same load in `parity-snapshot.sh` | podnet `48770ce` | Not taken: podcrates has no such script |
| `cloud-setup.sh --dev-tools`: the musl target | podnet `48770ce` | Not taken: the script adds it by default |
| `--dev-tools`: `groff`, `mandoc`, `shellcheck` | podnet `48770ce`, `d79dcc2` | Taken; also as root (a cloud container), and with `sudo -n -E` (it keeps the proxy variables), or a `sudo` for `apt-get` alone; `--no-install-recommends`; the install also after a failed update |
| `--dev-tools`: `musl-tools` | podnet `d79dcc2`, podssh `2f34ecf` | Not taken: podcrates links musl with zig |
| `-h` / `--help` | podnet `d79dcc2` | Taken in `cloud-setup.sh` and `session-start.sh` (the header comment) |
| `CARGO_BUILD_JOBS` of `min(nproc, 4)` | podnet `d79dcc2`, podssh `2f34ecf` | Taken in `cloud-env.sh`; 4 on a host with 4 CPUs or more, as before |
| `/.agents` in the `exclude` of the package | podnet `48770ce` | Not taken: the root of podcrates is a virtual workspace |
| `flock -nF` for the detached watchers | podvm `5244ed7` | Taken (`lock_cmd`): the pid file and the lock holder are the watcher |
| `python3 -u` for the watchers | podvm `5244ed7` | Not taken: `watch.py` and `runs.py` flush each line |
| A relative `CARGO_HOME` made absolute in `cloud-env.sh` | podvm `5244ed7` | Not taken: the default of podcrates is absolute |
| `flock -o` for the cargo lock of a worker | podrunner `b73a374` | Taken: `docs/hosted-sessions.md`, `docs/maintenance.md` |
| A test temp dir of its own and the check `tmp-left` | podrunner `b73a374`, `3d69d83` | Not taken now: T-1386 has it; podcrates keeps a short `TMPDIR` (T-920) |
| `apt-ubuntu.sh` (apt with the Ubuntu sources only) | podrunner `1453444` | Not taken: `--dev-tools` installs after a failed update; the workflows of podcrates did not fail on a source |
| A pinned actionlint for each workflow | podrunner `ac7f68b` | Not taken now: T-1651 |
| The hook timeout of 1200 s, the setup in the hook | podnet `5809e66` | Not taken: the start of podcrates runs no setup (T-1385) |
| The facts of the host: pyenv `python3`, tiers of the subagent tool, a sudo for apt-get | podnet `d79dcc2`, podssh `2f34ecf` | Taken: `docs/hosted-sessions.md`, "Gemini / Antigravity Cloud" |
| The role table column and the start of each harness | podnet `48770ce`, `72dece5`, podssh `2f34ecf` | Taken: `docs/hosted-sessions.md`, `AGENTS.md`, `docs/session-protocol.md` |
| A session keeps its turn while a run goes | podssh `b981f5f` | Already here (the ruling "Work until done", "The watchers") |

Small defects fixed in place: shellcheck SC2145 in the detached start
(`"scripts/agent/$@"`) and SC2016 in `cloud-setup.sh`; `check.sh` now
runs `sh -n` of `cloud-setup.sh` and shellcheck on the start scripts.
Proof: `sh -n` and shellcheck 0.11.0 of each changed script: clean. In
the worktree: `--no-watchers`, `--run-watcher runs`, `--run-watcher
watch` and the default start each print the worktree line and exit 0; a
bad flag and `PODCRATES_NO_WATCHERS=2` exit 64. In a scratch clone that
is not a worktree: `--no-watchers` and `ANTIGRAVITY_APP_DATA_DIR=/x`
start no watcher and exit 0; the default start runs both watchers as one
process each (the pid file holds the pid of `python3`), and a second
start finds them; `--run-watcher runs` holds `.work/runs.lock` with the
pid of `runs.py`, a second `--run-watcher runs` and the default start
exit 0 and find it, and `timeout` ends it (124) with the lock free. The
`--dev-tools` block with a stub `apt-get`: `DEBIAN_FRONTEND` is set, a
failed update still runs the install with `--no-install-recommends`. The
watchers of the lead checkout ran with no change during each test.
`python3 -I scripts/check-history.py`: no narrative history; `cargo todo
check`: the record agrees. Not verified: a session on Gemini /
Antigravity Cloud, and `apt-get` through a real `sudo`.

# T-1651: CI and `scripts/agent/check.sh`: lint each workflow with a pinned actionlint

**Source:** T-1650: podrunner `ac7f68b` (podrunner's T-252, `PodUtils/podrunner:scripts/actionlint.sh`).
**Category:** chore
**Milestone:** M8
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

`scripts/check-workflows.py` checks the rules of podcrates in each
workflow (the jobs, the permissions, the names that a document gives). It
does not check the expressions, the contexts and the `run` steps of a
workflow as actionlint does (a wrong `${{ }}` type, an unknown context, a
shellcheck fault in a step). Such a fault shows only when the workflow
runs, and a release workflow runs seldom.

## Premise

- podrunner runs `sh scripts/actionlint.sh .github/workflows/*.yml` in
  its `check.sh` and in CI, with a pinned actionlint.
- Not measured: the number of faults that actionlint finds in the
  workflows of podcrates now.

## Approach

A pinned actionlint release binary for Linux x86_64 and arm64, fetched
with `scripts/ci/fetch-tool.sh` by its SHA-256, in a script
`scripts/actionlint.sh`. The job `workflows` of `ci.yml` runs it on each
workflow; `check.sh` runs it when the binary is there. Fix each fault
that it finds first, or name each kept one with its reason.

## Prove

```sh
sh scripts/actionlint.sh .github/workflows/*.yml
```
The job `workflows` of `ci.yml` passes with the step.

