# podcrates: the hard, open and "impossible" problems

Written on 2026-11-20 from `main` at `0344ae618` of `PodUtils/podcrates`
(a clone of it is enough to follow this report). The reader is a research
agent with a clone of podcrates and of the consumers, and with no access to
the session that wrote this.

## How to read this report

- A "T-NNN" is an entry of the record: `TODO/INDEX.md` has its row, and
  the area file that the row links has its text. Find one with
  `grep -n '^# T-916:' TODO/*.md`.
- A "C NNN" is a call of the project (a decision made without a ruling):
  `docs/decisions.md`, section "Calls made without a ruling".
- A "ruling" is a word of the owner: `docs/decisions.md`, section "In
  force".
- "Read" means that I read it in this session. "Guess" means that nobody
  measured it. I mark each guess. I did not run any build or test.
- Run ids are runs of GitHub Actions in `PodUtils/podcrates`. Read a job
  log with `gh api repos/PodUtils/podcrates/actions/jobs/JOB_ID/logs`.
  The URL form is `https://github.com/PodUtils/podcrates/actions/runs/RUN_ID`.
- The rules of the project (`AGENTS.md`): the web is read through
  `https://web.pkgforge.dev/llms.txt` first; text is ASD-STE100; a flaw
  that you find goes into the record, not only into a report; a session of
  podcrates does not edit the consumers (podrunner, podssh, podnet, podvm).

## Summary

The project is a Rust workspace of about 100 `pod-` crates, with 810
entries in its record: 146 open, 27 partial, 7 blocked, 630 done
(`TODO/PROGRESS.md`). The owner's one hard rule is that each binary links
static on each target of the consumers. The owner's current order is
"podrunner first" (`docs/decisions.md`, 2026-11-18): the CI work of githome
and cfb, which is the work of podrunner, comes before other work.

What is hard now, in one paragraph each:

1. **The nightly is never green.** `nightly.yml` has failed in every
   scheduled run since 2026-11-11 and in every run since the last green run
   by hand on 2026-11-13 (run 43467939399). Seven of its 14 jobs fail in
   the newest run (44505235227). The causes are three different classes:
   time bounds under QEMU user mode (riscv64, loongarch64, powerpc64le), a
   "no blocking call" gate that counts switches on i686, and the first
   runs of the tests on Windows. A red run hides the next real defect.
2. **Windows.** Three entries (T-933, T-934, T-935) are open or partial,
   and a fourth (T-131, `pod-gsr` on Windows ARM64) has never been measured
   because the CI step that runs it is skipped after an earlier step
   fails (a finding of this reading).
3. **Services of other parties** fail from the network of GitHub runners
   and pass from other networks (T-963, T-351, T-371). The record cannot
   tell "the service is down" from "the service refuses runners".
4. **A stalled resume of `pod-session`** (T-767) cost podssh eleven
   failures of its gate. The newest three (9 to 11) point to a reader above
   the layer (podssh's own SSH client). The root cause of the first seven is
   not proved.
5. **Things that only a real machine can measure.** The cloud session of
   the agents has an egress allowlist, no `/dev/fuse`, a Landlock ABI of 7,
   no WARP, no relay. Many entries say "Not measured" for this reason (about
   115 entries have a "Not measured" or "Not read" line).
   The researcher on the owner's machine can answer many of them.
6. **Upstream limits.** The `tar` crate, `russh` with `ssh-key` on a
   nightly compiler, `kcp`, `iroh`, `arti` (SQLite), `snow`,
   `ed25519-dalek`, unmaintained `chrono` and `humantime`.
7. **The static rule** meets C code (OpenSSL in `pod-gsr`, a kernel for
   User-Mode Linux, `runsc`), macOS (no static `libSystem`), and the fuzz
   job (libFuzzer needs glibc).

## Table of the problems

Class codes: **T** flaky or timing; **U** upstream or third party;
**P** platform; **D** design; **K** unknown fact; **O** the owner's decision.
Priority is the one in `TODO/INDEX.md` (P0 correctness, P1 a consumer waits,
P2 worth doing, P3 record only).

| Id | Entries | Title | Why it is hard (one line) | Class | Priority |
| --- | --- | --- | --- | --- | --- |
| H-01 | T-916, T-938, T-766, T-789 | The emulated and cross jobs of `nightly.yml` | Emulation changes the time of every step; the causes differ per target and are not measured | T, K | P2 |
| H-02 | T-933, T-934, T-935, T-131, T-941 | Windows | Each fix needs a Windows machine; a skipped CI step hides T-131 | P, K | P2 (T-131, P1) |
| H-03 | T-963, T-351, T-371, T-970, T-419 | Services of other parties from the runners of GitHub | Cause is in a service that we do not own; no counts on a pass | U, K | P2 (T-351, P1) |
| H-04 | T-973 | The CA bundle of `pod-broker` with real clients | Four real client stacks must trust one private CA; none was run | K, D | P2 |
| H-05 | T-975, T-976, T-977 | A systemd-capable guest with no root: UML kernel, `RUNPATH`, `runsc` | A static `ARCH=um` kernel built in CI is not measured; podrunner's parity table waits | D, K, U | P2 |
| H-06 | T-106 | cfb's stand-ins as programs (A-X-18) | Waits for an answer of cfb, which is offline | O, U | P2 |
| H-07 | T-964, T-966, T-967 | Sigstore trust: Rekor v2 shards and compiled roots | The roots rotate by year; the verifier has no network | D, U | P1 (done), P2 |
| H-08 | T-692, T-702, T-700, T-703, T-146 | The discipline set: labs, installer, manual, issue check | "A profile on a runner" is not measured; no real host class run | K | P1 |
| H-09 | T-767, T-768, T-204, T-945, T-959 | A stalled resume of `pod-session` (podssh#283) | Rare, not reproduced here; the layers above hide the cause | T, K | P1 |
| H-10 | T-036, T-121, T-124 and podnet#604 | The relay role and the 409/1013 failures | The fault is in a relay server that we do not run | U, D | P2 |
| H-11 | rule 1 of `docs/design.md` | The static rule on each target | C code, macOS, Windows ARM64, libFuzzer | P, D | owner's rule |
| H-12 | AGENTS.md section 7 | The egress allowlist of a cloud session | Many facts cannot be measured in the session | K | n/a |
| H-13 | T-961, T-968, T-526, T-852, T-199, T-779, T-851 and others | Upstream limits | The fix is in code that we do not own | U | P2 to P3 |
| H-14 | fuzz.yml, upstream.yml | Two more red scheduled workflows | Unproven fix; a port review | T, U | P2 |
| H-15 | T-499, T-802, T-515, T-514, T-818, T-803 | NAT traversal and congestion control of `pod-link` | No real path was measured | D, K | P2 and P3 |
| H-16 | T-490, T-821, T-436, T-433, T-430 | Large, deferred or unmeasured feature crates | The base facts are not measured | D, K | P2 and P3 |
| H-17 | T-217 to T-220, T-832, T-617, T-538 | Blocked entries | Wait for the owner, podforge, podswarm or podvm | O | P2 and P3 |
| H-18 | Q16 and the owner actions | Questions and actions only the owner can do | An agent must not decide them | O | n/a |

The order follows the owner's ruling "podrunner first": H-01 to H-08 block
podrunner or its CI; H-09 and H-10 are the consumers' (podssh, podnet);
the rest follows.

---

## H-01. The emulated and cross jobs of `nightly.yml`

Entries: T-916 (partial), T-938 (partial), T-766 (open), T-789 (open). They
are in `TODO/upkeep.md`.

### The problem, as a fact

`nightly.yml` is the workflow that runs the full test suite on macOS,
Windows and seven more Linux targets (design section 7). A job `cross`
runs the test binaries of one target under QEMU user mode (`qemu-user`,
image `tonistiigi/binfmt:qemu-v10.2.3`, `images/binfmt/Dockerfile`,
`scripts/binfmt.sh`). i686 is not emulated: the runner runs it natively.

State of the newest scheduled run, `44505235227` at `8389e1c`
(2026-11-19, `https://github.com/PodUtils/podcrates/actions/runs/44505235227`):

| Job | Id | Result |
| --- | --- | --- |
| `cross` aarch64, armv7, arm | 135494988184, 135494988069, 135494988167 | pass |
| `cross` loongarch64 | 135494987943 | fail: `pod-iroh` `road.rs`, `share.rs` |
| `cross` powerpc64le | 135494988030 | fail: `pod-iroh` `road.rs` |
| `cross` riscv64 (limit 150 min) | 135494988182 | fail: `pod-iroh` `new_address.rs`, `share.rs`, `road.rs`; the job ran about 108 min |
| `cross` i686 (native) | 135494988262 | fail: `pod-link` `arq_machine.rs:383` |
| `test` windows-2025, windows-11-arm | 135494988195, 135494988135 | fail (H-02) |
| `test` macos-15 | 135494988353 | fail: `pod-wormhole` `tracker.rs` (fixed by T-970 in the record, not yet proved by a run) |

Evidence read in the logs of that run:

- `pod-iroh` on riscv64, `a_session_whose_client_moves_on_the_relay_alone_resumes_whole`
  (`crates/pod-iroh/tests/new_address.rs:113`): the test binary ran 758 s.
  The client said "the reader of the application waits for room for 23 s;
  this side sent 4292608 bytes, the far end acknowledged 98304; the replay
  buffer keeps 4194304 bytes, room 0". Later lines at 74 s and 103 s show
  the far end at 360448 and 491520 acknowledged bytes. Then
  `Lost { why: "the link failed: the far end answered no PING and
  acknowledged nothing for 30 s" }` at 109 s. So the far end moved about
  5 KB each second. No byte was lost.
- `pod-iroh` `road.rs:47` and `share.rs:52` on riscv64, loongarch64 and
  powerpc64le: `UnexpectedEof, "early eof"` after 54 s to 178 s, and
  `Kind(BrokenPipe)` in the other task. The test bound (`limit()`, 60 s,
  scaled by 5 under an emulator; 300 s) is not the cause. Something ends the
  session earlier. The record's reading (T-916 Done): a timer of the iroh
  stack ends the session.
- `pod-link` `a_turn_of_the_arq_blocks_on_nothing` on i686 (native, no
  emulator): "10 voluntary switches in 1000 turns". The nightly of
  2026-11-18 counted 3. The assertion needs 0.
- riscv64 per binary (the step prints `== N s: EXE`): `new_address` 759 s,
  `blocking_streamed` 354 s, `streamed` 340 s, `udp_loss` 268 s,
  `grammar_boundary` 263 s, `vless` 216 s, `h2_road` 203 s. The sum is
  about 108 minutes.

Older evidence from the entries:

- T-916: the gate `pod_testkit::switches::count` (T-838) counts voluntary
  context switches of a thread. It counted 369 for 1000 seals under
  qemu-user. A fix gives `None` under an emulator and subtracts major page
  faults. On a native x86_64 host it counted 2 once (2026-11-18) and 0 in
  nine runs.
- T-766: `pod-guest` `ready/tests.rs` (a 300 ms wait for screen dumps),
  `pod-egress` `vless.rs`, `pod-link` `poll.rs` (a proxy that cuts an idle
  connection after 2 s while a TLS handshake is slow), `pod-relay`
  `http_road_one_h2.rs`: time bounds that the emulation outruns. T-916
  scaled some of them (`pod_testkit::host::scaled`, factor 5).
- T-789: two tests of the MQTT board of `pod-wormhole`
  (`crates/pod-wormhole/tests/board.rs:55` and `:94`) on armv7 against the
  stand-in. The armv7 jobs pass in the newest run, so this may be gone.
  Nobody has measured it.
- T-938: the riscv64 job met its limit of 90 minutes. The limit is now 150
  (`nightly.yml`, `matrix.minutes`). The job needs about 108.

### What was tried, and what the record rules out

- A scale factor of 5 for bounds that guard a hang
  (`crates/pod-testkit/src/host.rs`, `EMULATOR_FACTOR`). It does not fix
  `pod-iroh` on riscv64.
- `count` returns `None` under an emulator; the closure is not run once
  before the count (`FnOnce`).
- Rule of the record (T-916 Approach): a test whose bound is scaled and
  still fails gets its cause found, not a larger bound.
- Ruled out by T-916 Done: `pod-ws` `h2_road.rs`, `pod-relay`
  `caller_headers.rs`, `pod-session` `session_move.rs` and `pod-iroh`
  `tickets.rs` were not plain time bounds; each got a code or test fix.

### What is not known (questions for the researcher)

1. Why does the far end of the iroh test on riscv64 acknowledge only about
   5 KB each second? Measure the throughput of the same test binary under
   `qemu-riscv64` on a fast machine and under native riscv64 if you have
   it. Is it the crypto (ChaCha20-Poly1305, Ed25519, QUIC) in a build of
   opt-level 0? **Read:** `Cargo.toml` line 241 to 255: the dev profile has
   `debug = "line-tables-only"` and no `opt-level`, so each dependency,
   crypto included, builds with opt-level 0 in the test jobs. This is a
   finding of this reading; nobody measured it.
2. What timer ends the iroh session at 54 to 178 s with "early eof"? Check
   the iroh QUIC idle timeout and keepalive, the relay `PING` of 15 s
   (`T-939` Done mentions it), and the `vendor/iroh-relay` fork
   (`crates/pod-iroh/README.md`, section "The fork of iroh-relay").
   Does the timer run on QEMU's virtual clock or on the host clock?
3. Which operation gives the 10 voluntary switches on i686? Candidates in
   the record: a lock of musl's allocator shared with other test threads, or
   a cold first pass. Run the closure twice and count the second pass. Use
   `perf trace -s` or `/proc/PID/task/TID/status` (`voluntary_ctxt_switches`)
   to find the call. Why did the count go from 3 to 10?
4. Is a switch gate on a multi-threaded test binary sound at all? Which
   threads can switch the measured thread? (musl `malloc` uses a lock; a
   futex wait is a voluntary switch.)
5. Does the armv7 failure of T-789 still exist? The jobs pass now. If it
   does not fail in three nightly runs, close it with the run ids.
6. How long does each cross job need after the iroh tests are fixed?
   (T-938: the record asks to split by crate if the job is near its limit.)

### Files to read

- `.github/workflows/nightly.yml` (job `cross`, lines 187 to 290),
  `scripts/binfmt.sh`, `images/binfmt/Dockerfile`.
- `crates/pod-testkit/src/switches.rs`, `crates/pod-testkit/src/host.rs`.
- `crates/pod-link/tests/arq_machine.rs` (line 383),
  `crates/pod-e2e/tests/allocations.rs`.
- `crates/pod-iroh/tests/{road,share,tickets,new_address,proxy_env}.rs`,
  `crates/pod-iroh/tests/common/mod.rs`, `crates/pod-iroh/src/home_relay.rs`,
  `vendor/` (the iroh-relay fork and its patches).
- `Cargo.toml` (profiles), `TODO/upkeep.md` (the four entries).

### Candidate approaches (guesses unless marked)

- Guess: set `opt-level = 2` or 3 for the crypto and QUIC crates in
  `[profile.dev.package.NAME]`, or run the emulated jobs with a test
  profile that optimizes. Trade-off: the build time of the cross jobs grows
  (not measured).
- Guess: a payload size that scales under an emulator (20 MiB becomes
  2 MiB) instead of a larger time bound. Trade-off: the test checks less.
- Guess: split the riscv64 job by crate (T-938 Approach), so that a
  slow binary does not hide the rest.
- Guess: do not run the switch gate where any thread other than the
  measured one runs. Trade-off: the gate gets weaker.
- Skip the three slow targets (riscv64, loongarch64, powerpc64le) for
  the `pod-iroh` tests only and run them on a real machine of the owner.
  That breaks the idea "CI proves each target" and needs the owner's call.

---

## H-02. Windows: three open defects, and one that CI hides

Entries: T-933 (partial), T-934 (open), T-935 (open), T-131 (open, P1),
T-941 (open, `PodUtils/podssh#319`).

### The problem, as a fact

The tests of `pod-cloudflare` ran on Windows for the first time in run
44275593101 (T-915 let them build). The jobs `test (windows-2025)` and
`test (windows-11-arm)` of the newest nightly (44505235227) show three
failures in two crates:

- T-935. `pod-core` `tests/files.rs:249`
  (`threads_that_store_one_name_at_once_leave_one_whole_body`): "no
  directory would take shared (...pod-core-files-threads-10648: Access is
  denied. (os error 5))". Source: `crates/pod-core/src/files/replace.rs`,
  lines 124 to 132, use `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`. A rename
  onto a name that another rename or an open handle holds gives
  `ERROR_ACCESS_DENIED`. Unix replaces atomically.
- T-934. `pod-cloudflare` `quick_tunnel_standin_goes_through_an_exec_proxy`
  (`tests/quick_tunnel_standin.rs:284`): the exec proxy ran
  `python3 C:\UsersRUNNER~1AppDataLocalTemp.tmp...pipe.py`. The cause is
  `crates/pod-proxy/src/stream.rs:134`: `ChildIo::spawn` runs the command
  with `sh -c`. Git's `sh` on Windows removes unquoted backslashes. A host
  with no `sh` cannot run an exec proxy at all. It is a user defect, not only
  a test defect.
- T-933. `quick_tunnel_standin_falls_back_to_http2_when_udp_is_dropped`
  (`tests/quick_tunnel_standin.rs:244`): "no QUIC packet came:
  `Os { code: 10060, kind: TimedOut ...}`". After the fix of the
  assertion, the error is visible: 10060 is `WSAETIMEDOUT`, the read
  timeout of the test's own socket. So no datagram reached the stand-in in
  20 reads of 100 ms. The error was not hidden by another error; the
  datagram did not arrive. The other two fixes of T-933 (the `UnixTls` half
  and the 2 MiB stack of the test thread) are in; the proof run still
  shows the UDP failure.

T-131: `cargo test -p pod-gsr --test loopback` ended with
`STATUS_ACCESS_VIOLATION` (`0xc0000005`) on `aarch64-pc-windows-msvc` in the
run 42685319279 (job 129390452858). The tests that crash run a TLS
handshake with SRP in the vendored OpenSSL, built from source with
`no-asm` on that runner (no NASM). The name of the crashing test is
unknown.

**Finding of this reading (not in the record).** `nightly.yml` lines 72 to
75 run `Test pod-gsr (Windows ARM64, open defect)` after the step `Test`,
with `if: matrix.runner == 'windows-11-arm'` and `continue-on-error: true`.
A step with an `if` that has no status function runs only when the steps
before it succeeded. The step `Test` exits with code 101 on that runner (the
`pod-cloudflare` and `pod-core` failures above), so the pod-gsr step is
skipped. In the log of job 135494988135 the step list stops after `Test`
(`##[error]Process completed with exit code 101.`). So T-131 has not been
measured since the Windows tests started to fail. Fix: `if: always() &&
matrix.runner == 'windows-11-arm'`.

T-941 (`PodUtils/podssh#319`): podssh locks `known_hosts` with one byte far
past the end of the file (`LockFileEx` at `u64::MAX - 1`) so that other
readers are never refused on Windows. `pod_core::lock::FileLock` locks the
whole file (`std::fs::File::lock`). podssh keeps its own copy until
podcrates has the mode.

### What was tried

- T-933: three fixes, in; the proof is a nightly run on Windows.
- T-935: nothing; it has an Approach only.
- T-934: nothing; an Approach only.
- A script `scripts/agent/check-windows.sh` checks Windows rules from a
  Linux host (it does not run the code).

### What is not known

1. T-935: does `SetFileInformationByHandle` with
   `FILE_RENAME_FLAG_POSIX_SEMANTICS` (`FileRenameInfoEx`) replace a file
   that another thread just renamed over, on `windows-2025` and
   `windows-11-arm` (NTFS)? Does it work when another handle is open
   without `FILE_SHARE_DELETE`? What do real tools do (Go's
   `renameio`, Rust `tempfile::persist`, `atomicwrites`)?
2. T-935: how many retries and which delays make `MoveFileExW` safe? Does
   the error come from a handle held by an antivirus scan (Windows
   Defender) on the runner? Run the failing test 200 times on a Windows
   machine and record the error codes (5 or 32).
3. T-933: does the client of the stand-in on Windows send UDP at all? Print
   the local and remote address of the stand-in socket and of the client.
   Guess: the client dials `localhost` and picks `::1` while the stand-in
   binds `127.0.0.1`; or the QUIC attempt starts only after a TCP step that
   fails first on Windows. Read `crates/pod-cloudflare/src/` (the edge
   dial) and the test at line 220 to 245.
4. T-934: how does a Windows user write an `exec:` proxy command today? Use
   the Windows command-line rules (`CommandLineToArgvW`) and no shell, or
   `cmd /C`? What does podssh do for its own `ProxyCommand`? (T-441 asks for
   `ChildIo::spawn_args` with no shell for podswarm: one fix can serve
   both.)
5. T-131: which test crashes? Run the step with `if: always()` and read
   `--nocapture --test-threads=1`. Is the crash in OpenSSL built with
   `no-asm` for ARM64 (a known problem class), in `openssl-src` options,
   or in the SRP code of our wrapper? Can we build OpenSSL with `-O0`, or
   with NASM equivalents (`armasm64`) there? Is a pure Rust SRP possible
   (the owner allows C, but the crash is in C)?
6. T-941: does `LockFileEx` at `u64::MAX - 1` behave the same on `ReFS`,
   network shares and WSL mounts? (A guess: this is rare.)

### Files to read

- `crates/pod-core/src/files/replace.rs`, `crates/pod-core/tests/files.rs`,
  `crates/pod-core/src/lock.rs`.
- `crates/pod-proxy/src/stream.rs` (line 134),
  `crates/pod-cloudflare/tests/quick_tunnel_standin.rs`,
  `crates/pod-cloudflare/src/origin.rs`, `crates/pod-cloudflare/src/yaml/tests.rs`.
- `crates/pod-gsr/` (the OpenSSL build and `tests/loopback.rs`),
  `scripts/openssl-perl.sh`, `.github/workflows/nightly.yml` (lines 40 to 80).
- `PodUtils/podssh:crates/podssh-ssh/src/known_hosts/lock.rs`.

### Candidate approaches (guesses unless marked)

- T-935 (the record's approach): a rename with POSIX semantics where the
  file system takes it, else a bounded retry on `ERROR_ACCESS_DENIED` and
  `ERROR_SHARING_VIOLATION`, with one line at the bound.
- T-934 (the record's approach): on Windows, split the command with the
  Windows rules and spawn with no shell; keep `sh -c` on Unix.
- T-131: first run the step so that it reports. Guess: build OpenSSL with
  `no-asm no-threads`, or drop SRP and use another crate for the GSRPC
  handshake of the "gs-netcat" protocol.

---

## H-03. Services of other parties fail from GitHub runners

Entries: T-963 (partial), T-351 (partial, P1), T-371 (open), T-970 (done),
T-419 (open), T-812 (partial), T-814 (partial). Files:
`TODO/relay.md`, `TODO/upkeep.md`, `docs/live-calls.md`, `docs/research/relay-rates.md`.

### The problem, as a fact

Tests that run against a public service of a third party fail from the
network of a GitHub runner and pass from a cloud session. Two classes:

1. **The Nostr relay `offchain.pub`.** `live.yml` run 44497255102 at
   `fee6f8a` (two attempts):
   `two_ends_pair_through_each_public_nostr_relay_after_a_dead_mailbox`
   (`crates/pod-relay/tests/pair_code_live.rs`) failed with "wss://offchain.pub/:
   the giver: the other end said nothing for 60 s". Both ends passed the
   greeting. `relay.primal.net` paired in 2.6 s and 4.3 s in the same runs.
   From a cloud session the same test paired at `offchain.pub` in 2.2 s.
   Run 44507180792 at `b74ffbd` showed the cause: the relay refused the
   taker's event with "Policy violated and pubkey is not in our web of
   trust." after it took the first events. The relay runs a write policy
   for keys that it does not know; each pairing draws a new key. A later
   test from a cloud session saw `offchain.pub` reset TLS.
   Step taken (call C796): `offchain-nostr` left the catalog of
   `pod-services`; `noslol-nostr` (`wss://nos.lol/`) took its rank 2.
   `relay.damus.io` answered HTTP 503 and was not taken. Run 44529873962 of
   `live.yml` at `5e13425` passed from a runner, but the pairing at
   `nos.lol` took **56.1 s** against a wait of **60 s**. The entry stays
   partial because the giver still waits its whole limit after a refusal
   (its step 4), and because 56 s is near the bound.
2. **The WebTorrent tracker `tracker.openwebtorrent.com`.** Run 44505410827
   at `e475b23`: HTTP 503 on the WebSocket upgrade (101 from a cloud
   session). Run 44507180792 paired. Also, the step ran its commands with
   `set -e`, so one service that failed hid the others; the step now runs
   both tests and fails at its end.
3. **Hosts that give no answer to a runner** (T-351). Four of seven red
   runs of `services.yml` had Trojan nodes "timed out after 29s". The
   record's rule (call C153) keeps the first "no answer" of a watch from
   failing a scheduled run; a second one in a row does. The cache of the job
   holds the state. T-371 lists three paths that give no class: the DNS
   watches, `stun-binding`, and the TLS of the HTTP client.
   `services.yml` run 44504878691 (2026-11-19) passed.

`pod-wormhole` is the crate that pairs by a code through a mailbox, a Nostr
relay or a WebTorrent tracker. The catalog of services is data in
`crates/pod-services/services/*.toml` (for example `noslol-nostr.toml`,
`primal-nostr.toml`).

### What was tried

- Counts at the failure (step 1 of T-963): the failure text names how many
  events this end sent, how many the relay accepted (`OK` true), how many it
  passed of the other key, and how many were messages of the other end.
  A pass gives no counts.
- Catalog change (C796). Test `tests/nostr.rs` in `pod-services` names
  `primal-nostr` and `noslol-nostr`.
- Class "no answer" (C153, C155).

### What is not known

1. Does `nos.lol` have a write policy that delays events of unknown keys
   (56 s)? Does `relay.primal.net` have one? Measure 30 pairings from the
   runner network and from your own network: time to pair, and the NIP-11
   document (`limitation`, `restricted_writes`, `payment_required`).
2. Is the delay on the giver's side a repeated first message (the taker
   "sends its first message again until the giver answers")? Read
   `crates/pod-wormhole/src/nostr/` and measure the interval of the retries.
3. Can the giver learn that the taker was refused? On a mailbox the
   taker's last word is visible; on a relay it is not (T-963 step 4). Can
   the taker send an event that the giver can read and that says "refused"?
   Can the giver use the `OK false` that the relay sends to the taker?
   (It cannot; it is sent to the connection that wrote.)
4. Which Nostr relays accept ephemeral events (kind 20000 to 29999) from
   a new key without proof of work, payment or a web of trust, from AWS
   and Azure address ranges? Make a ranked list with the date of each
   measurement. Which of them run `strfry` with the same policy plugin?
5. T-351: do the Trojan nodes answer from another network on the same
   day? (Not measured.) Does Quad9's DoH answer a runner on the other
   cloud? (T-371, not measured.)
6. Does a third relay on a distinct implementation (not `strfry`) exist, so
   that one policy change does not remove all of them?

### Files to read

- `crates/pod-relay/tests/pair_code_live.rs`, `crates/pod-wormhole/src/nostr.rs`
  and `src/nostr/`, `crates/pod-wormhole/tests/nostr.rs`, `tests/tracker.rs`,
  `tests/harness/nostr.rs`, `crates/pod-services/services/*nostr*.toml`,
  `crates/pod-services/tests/nostr.rs`.
- `.github/workflows/live.yml`, `.github/workflows/services.yml`,
  `crates/pod-services-watch/` (`answer.rs`, `dns.rs`, `stun.rs`, `heard.rs`),
  `docs/live-calls.md` (rows L19, L20, L21), `docs/research/relay-rates.md`.

### Candidate approaches (guesses unless marked)

- The record's step 4: the pairing that meets a refusal after the greeting
  fails with the relay's words at the taker; look for an earlier end at the
  giver.
- Guess: run the pairing at two relays at once (a race), so that the first
  that answers wins. Trade-off: more keys and more traffic; the code on
  the relay is public.
- Guess: add a relay of the owner as rank 1 (the owner runs a relay for
  podssh; the ruling "Services: the own relay"). Trade-off: a single owner
  (the ruling of 2026-11-13 asks for several operators).
- Guess: a test that is "skip with a reason" when a service answers with a
  policy refusal. Trade-off: hides a real failure.

---

## H-04. The CA bundle of `pod-broker` with real clients

Entry: T-973 (open, P2). Related: T-100 (done, `v0.78.1`), T-972, T-191,
T-194. Source: githome's ask G-35 on
`https://github.com/PodUtils/podcrates/issues/223`.

### The problem, as a fact

`pod-broker` is the egress broker for a job of podrunner. T-100 gave it a
TLS end: a CA for each job (P-256 keys), a bundle file, six variables
(`SSL_CERT_FILE`, `GIT_SSL_CAINFO`, `NODE_EXTRA_CA_CERTS`,
`REQUESTS_CA_BUNDLE`, `CURL_CA_BUNDLE`, `AWS_CA_BUNDLE`), the swap of a
placeholder credential for a real value (header, Basic, query, body), and
the scrub of answers. The tests of the TLS end use rustls clients, and the
leaf is checked with `openssl verify -x509_strict`. **No test runs curl, git,
Python `requests` or Node.js** with the bundle and the variables through
the broker, to a far end that checks the value. These clients differ in
how they take a CA: name constraints, `keyUsage`, `basicConstraints`, the
SAN form, the validity of a leaf, a CA that is not a trust root of the
system, and the order of the variables.

### What is not known

1. Does each client accept the leaf and the CA that `ca::JobCa` makes
   (a P-256 CA with a short life)? Test: curl (OpenSSL build and a
   rustls or mbedTLS build), git over HTTPS (libcurl), Python 3.x
   `requests` and `urllib3` (`certifi` is replaced by the bundle), Node.js
   (`NODE_EXTRA_CA_CERTS` adds to the trust; `--use-openssl-ca`), Go
   (`SSL_CERT_FILE`), Java (does not read these variables), `pip`, `npm`,
   `cargo` (`SSL_CERT_FILE` or `CARGO_HTTP_CAINFO`), `docker` and
   `gh`.
2. Which clients ignore the variables? What does the job see in its
   environment when it prints `env`? Does the placeholder reach a log of
   the client (a verbose curl prints the header)?
3. Certificate pinning in a client (for example `gh`, `cosign`, some
   updaters) cannot use this scheme. Is a list of such clients useful as
   a doc?
4. Does HTTP/2 or HTTP/3 (ALPN) change the swap? (The record bounds in
   C810 to C817; read them.)

### Files to read

- `crates/pod-broker/README.md` ("Placeholder credentials"),
  `crates/pod-broker/src/ca*`, `crates/pod-broker/tests/tls_swap.rs`,
  `tls_scrub.rs`, `credentials.rs`; `docs/decisions.md` C810 to C817;
  `crates/pod-lab/` (T-692); `crates/pod-tls` (`BUNDLE_VARS`).

### Candidate approaches

- The record: a lab of `pod-lab` with the broker, a stand-in far end with a
  leaf of the lab CA that echoes the `Authorization` it got, and each
  client with the bundle and the placeholder. Prove that the far end gets
  the value and the client prints the placeholder.
- Guess: use a container image of each client, pinned by digest (the
  project already does this for tools: `scripts/ci/fetch-tool.sh`). Trade-off:
  CI time and a download from third parties.

---

## H-05. A systemd-capable guest with no root: UML, `RUNPATH`, `runsc`

Entries: T-975 (open, effort L), T-976 (open), T-977 (open). Sources:
`https://github.com/PodUtils/podvm/issues/602`, `.../603`, `.../604`; the
parity table of `https://github.com/PodUtils/podrunner/issues/316`.

### The problem, as a fact

podrunner must run an image with systemd as PID 1 and services as their own
users (the parity table of podrunner#316) on a host with no KVM, no
ptrace, no namespace and no root. The only route today is the QEMU guest of
`pod-guest` under TCG: 24 to 46 s for a run.

- T-975: User-Mode Linux (UML) in seccomp mode (Linux 6.16 or later,
  `seccomp=on`, SMP from 6.19 with `ncpus=N`) needs no ptrace, no namespace,
  no KVM and no QEMU. podvm measured it (its experiment 467 at `38b766f`,
  files `experiments/467-uml-seccomp.sh` and
  `experiments/results/uml-seccomp.txt` in PodUtils/podvm): Debian 12 with
  MariaDB and PostgreSQL ran in 14.7 s against 45.6 s under TCG. No crate
  builds or starts such a kernel. `pod-microvm` is planned
  (`docs/design.md`) and does not exist.
- T-977: gVisor's `runsc` boots systemd in 7 to 12 s on a host with no
  delegated cgroup. podvm does not fetch it. A module of `pod-release` must
  fetch a pinned static tool and check its hash before each exec.
- T-976: the loader family of `pod-enter` runs a program through
  `ld.so --library-path DIRS`. It reads no `DT_RUNPATH` or `DT_RPATH`, so
  `systemctl` of systemd 261 (library only in `/usr/lib/systemd` by
  `RUNPATH`) fails on that rung.

podvm's answers on #602 (2026-11-20): x86_64 only; no patch of pocket_vm
needed (stock Linux 7.3.5, no network); exit code and stdout and stderr as
two streams like the mailbox of `pod-guest`; volumes later.

### What is not known

1. T-975: can a **static** `ARCH=um` kernel be built reproducibly in the CI
   of podcrates? Which `CONFIG_STATIC_LINK` needs a static glibc, and is
   there a musl option for UML? (UML links against the host libc by default;
   the owner's rule needs a static binary. A guess from general knowledge:
   check it.) What is the size of the kernel
   and the build time on a GitHub runner? Where does the build cache?
2. T-975: which kernel version? 7.3.5 or later is named by podvm. Does the
   SMP fault in `chan_kern.c` (`free_irq` from the SIGIO handler) and the
   deadlock of the vector network driver of the 8 patches of pocket_vm
   (`https://github.com/flyinghorse0510/pocket_vm`, Apache-2.0 and MIT)
   matter for a no-network guest? (podvm says no.) What licence and source
   offer duties does a kernel binary (GPL-2.0) bring for an asset of a
   release? The project is 0BSD and its `deny.toml` forbids GPL crates; a
   kernel is a separate work, but this needs an owner's decision (Q-new,
   below).
3. T-975: does the launcher run under a seccomp that gives EPERM to
   `ptrace`, `unshare`, `setns`, `chroot` and `mount` (the test the entry
   asks for)? Does UML's seccomp mode itself use any of them? Which
   syscalls does the "seccomp=on" host side need?
4. T-975: does it run on GitHub's runners, which have KVM only on some
   hosts? Does the user-mode kernel need `/dev/shm` or `memfd`?
5. T-977: the URL, architecture names and `.sha512` of gVisor's
   `release-20261102.0` (`runsc` for x86_64 and aarch64). Not read.
   Does `runsc` run on a host with no root, no user namespace and no
   delegated cgroup? Which platform (`systrap`, `ptrace`, `kvm`) does it
   use there? Read gVisor's documentation and podvm's measurement
   (7 to 12 s).
6. T-976: does `abi::Elf` read `DT_RUNPATH`, `DT_RPATH` and `DT_STRTAB`
   now? (Not read.) How does `ld.so` order `LD_LIBRARY_PATH`,
   `RUNPATH` of the program and of each library, `$ORIGIN`, `$LIB`,
   `$PLATFORM`? (The record gives the rule; glibc 2.35 and musl differ.)

### Files to read

- `crates/pod-guest/README.md`, `crates/pod-guest/src/plan.rs`, `qmp.rs`;
  `docs/design.md` (row `pod-microvm`); `crates/pod-enter/src/userland.rs`
  (`lib_dirs`, `LoaderPlan`), `crates/pod-enter/src/abi/closure.rs`;
  `crates/pod-release/README.md`, `scripts/ci/fetch-tool.sh`;
  `.github/workflows/guest.yml`, `guest-windows.yml`.
- `https://github.com/PodUtils/podvm/issues/602`, `.../603`, `.../604`.
- `docs/research/podvm/m6-imports.md`.

### Candidate approaches (guesses unless marked)

- T-975 (the record): a reproducible static `ARCH=um` build in CI,
  published as an asset with its SHA-256 in `SHA256SUMS` and
  `release-index.json`; a launcher in `pod-microvm`; a test under a seccomp
  that gives EPERM to the five calls.
- Guess: build UML in an Alpine/musl container with `zig cc` as the
  compiler, as the project does for other static C (zig is in CI already).
  This may need patches to the kernel (UML uses glibc symbols).
- Guess: if a static kernel is impossible, ship the kernel as a data file
  (it is not a program of the host; it runs as a process, though, so it is
  a program). Ask the owner whether a dynamic helper is allowed for a
  "guest kernel", as QEMU's firmware is data.

---

## H-06. cfb's stand-ins as programs (A-X-18, T-106)

Entry: T-106 (partial, P2). Channel:
`https://github.com/PodUtils/podcrates/issues/223` (one comment of podcrates
to cfb, 6970315549; one to githome, 6970683028).

### The problem, as a fact

cfb and podrunner need loopback stand-ins as programs so that their CI
drives real binaries with no internet. The G-10 half is done:
`pod-fake-github` (T-921) and `pod-fake-githome` (T-948) are static programs
of each tag, with images on `ghcr.io` (from `v0.73.0` and `v0.75.0`). The
A-X-18 half (the stand-ins of the edge, the relay and the WARP API as
binaries) waits for cfb. podcrates asked cfb on 2026-11-19: which
stand-ins, with which ports, fixtures and control API? cfb went offline on
2026-11-18 until the CI of podrunner and githome is complete (its ruling
R-053). So the answer is not in sight.

### What is not known

1. Which stand-ins does cfb's CI run as programs: the edge (a quick tunnel
   or a named tunnel of `pod-cloudflare`), the relay (WebSocket or HTTP
   road), the WARP API (`pod-testkit::stand::warp`)? (A question to cfb.)
2. Is T-106 needed at all for the CI of githome? (R-053 says cfb gets CI
   from githome only.) Without an answer, can podcrates mark the A-X-18
   half as "waiting for the owner", and close the entry as split? The rules
   forbid "won't fix" closes (AGENTS.md section 9); a status `blocked` with a
   Blocker section is allowed.
3. `crates/pod-testkit` stand-ins today: which of them speak only bytes and
   HTTP (design rule)? Is one already usable as a program with a trivial
   `main`?

### Files to read

`TODO/fleet.md` (T-106, T-921, T-948), `crates/pod-testkit/` (`stand/`,
`README.md`), `docs/research/fleet/asks.md` (A-X-18),
`https://github.com/PodUtils/podcrates/issues/82` (tracker of cfb's asks),
`#83` (githome's), `#223`.

### Candidate approach

Mark T-106 `blocked` with its Blocker, until cfb answers; make no program
before it does. (Guess: this is the safest use of time; a program that
nobody asked for is waste.)

---

## H-07. Sigstore trust: Rekor v2 shards and compiled roots

Entries: T-964 (done 2026-11-20, calls C820 and C821), T-966 (partial),
T-967 (open), T-104 and T-880 (done), T-965 (done). Consumers:
podrunner's T-562 (`https://github.com/PodUtils/podrunner/issues/320`),
podssh's self-update (took `pod-release` at `c9772a3`), podnet (its half
of T-966 waits for PodUtils/podnet@T-494).

### The problem, as a fact

`pod-release` verifies a cosign bundle of `SHA256SUMS` with no network. It
compiles in Sigstore's public trusted root (PEM files in
`crates/pod-release/src/sigstore/`: `fulcio.pem`, `rekor.pem`,
`rekor-log2025-1.pem`, `tsa.pem`; source `sigstore/root-signing` at
`c9bda74`, `SOURCES`). A bundle of **Rekor v2** has no signed entry
timestamp; it has an inclusion proof with a signed checkpoint, and the time
comes from an RFC 3161 timestamp. The shards of Rekor v2 change by year
(`log2025-1`; a `log2026-1` is expected). The record says (C821): "a new
shard needs a release of the crate before the signing configuration of
Sigstore sends signatures to it". So a bundle that names a shard that the
compiled roots do not have is **refused**, and each consumer that does not
take the new tag has a broken self-update.

Read today through the mirror (2026-11-20):

- `targets/signing_config.v0.2.json` of `sigstore/root-signing` main lists
  only `https://rekor.sigstore.dev` (Rekor v1).
- `targets/signing_config_rekor_v2.v0.2.json` lists
  `https://log2025-1.rekor.sigstore.dev` (API version 2, `validFor.start`
  `2026-01-01T00:00:00Z`) and Rekor v1.
- `targets/trusted_root.json` lists two tlogs (`rekor.sigstore.dev` and
  `log2025-1.rekor.sigstore.dev`, valid from 2025-09-23) and one timestamp
  authority (`sigstore-tsa-selfsigned`, from 2025-07-04).

### What is not known

1. When does the default signing configuration (`signing_config.v0.2.json`)
   move to Rekor v2? Is there a public plan? What does cosign v3.x do with
   no flag today? (The bundles of podcrates v0.76.0 have the promise, the
   proof and a timestamp, from cosign v3.1.4.)
2. What name will the next shard have, and when is it announced? Does
   Sigstore announce shards in the TUF root before they take traffic? How
   long between the root and the first bundle? (This decides how fast a
   release of podcrates must follow.)
3. The check `check-sources.py` finds a change of `trusted_root.json` in
   `upstream.yml`. Does it run daily and alert a session in time? Which
   job reads it? What is the maximum delay?
4. Is a fetch of the trusted root by TUF at run time acceptable for a
   consumer that has a network (C821 "Not taken")? A design with the
   compiled roots as a floor and an optional newer set (signed by TUF) is a
   guess; it needs the owner's call because it adds network code and a
   trust root of its own.
5. T-967: does a release with a minisign signature beside the bundle
   help consumers if Sigstore is down? (Not measured.)
6. T-966: do the trial patches of podnet's take pass its `update` tests?
   (Not measured: waits for podnet.)

### Files to read

`crates/pod-release/` (`README.md`, `SOURCES`, `src/sigstore.rs`,
`src/sigstore/{proof,stamp,rekor,chain}.rs`, `tests/fixtures/sigstore/`),
`docs/decisions.md` C793 to C807, C820, C821, `.github/workflows/upstream.yml`,
`scripts/check-sources.py`, `scripts/release-index.py`,
`.github/workflows/release.yml` (and the failure of T-974).

### Candidate approaches (guesses unless marked)

- Add an alert: a scheduled job that reads the live
  `signing_config*.json` and `trusted_root.json` and compares the log
  names with `Roots::sigstore()`. (`check-sources` already compares the
  trusted root file; whether it covers the signing config is not read.)
- Guess: accept a log that is named in the bundle's own trusted root only
  with an opt-in setting. Trade-off: a weaker check.

---

## H-08. The discipline set: labs, installer, manual and the issue check

Entries: T-692 (partial, P1, L), T-693 to T-696, T-700 (partial, P1, L),
T-702 (partial), T-703 (partial), T-704, T-705, T-146 (open, research, L).
All in `TODO/fleet.md`. The owner's ruling "Discipline" (2026-11-15) and the
rulings "The text of a command line", "Installer" and "Issues".

### The problem, as a fact

The owner's ruling says that "most of our projects appear to be toy products
rather than beta/wip of something serious and none survive a real contact
with beta testers". The answer is shared tooling:

- `pod-lab` (T-692): 16 profiles of hostile host classes; `podlab check`
  proves a profile holds with the rows of `pod-hostprobe`; `podlab run`
  starts a command in the profile. Part 1 is done. Held with no root:
  seccomp, Landlock, a tmpfs in a user namespace, inherited descriptors.
  **Not held here:** `proxy-only`, `cgroup-v1`, the arch part of
  `arm64-tcg`; and the six network profiles of podnet. Part 2 (the Actions
  step `lab/action.yml` and the machine-tier profiles) is open. The entry
  says "Not measured: a profile on a runner".
- `pod-manual` (T-700): one help and manual for each command line, with
  checks of each drift and of the text rules (ASD-STE100, no emoji, UTF-8
  only). The consumers must take it; the entry's premise says it did not
  read podnet's `src/manual/` (a 403 from the mirror) and the help of the
  other projects.
- The installer (T-702): a POSIX `sh` core exists; each project's take and
  the hand-off are left.
- The issue check (T-703): a tool writes the "state in the record" block of
  each issue (visible in the issues of the consumers).

T-146 is the plan of podrunner itself: "Not measured: the official runner
under podvm (phase 0 of the plan)". The plan is in
`docs/research/ci/podrunner-plan.md`.

### What is not known

1. Does each profile of `pod-lab` hold on a GitHub runner, on a podvm
   guest, and on the owner's machine? Which profiles fail the check there
   and why? Run `podlab check PROFILE` for all 16 profiles on a runner and
   record the verdicts. (T-693: the rows of AF_INET6 and `connect` on a
   host with no IPv6; T-694: `TMPDIR` that is not `/tmp`; T-695: cgroup v1
   and `hidepid`; T-696: UDP refusal on Landlock ABI 10, which this session
   does not have.)
2. How long does the installer take on each profile, and does it hold on
   `dash`, `busybox sh` and `bash --posix`? (The entry asks for a measured
   time with a limit.)
3. Does the official GitHub Actions runner (`actions/runner`) run under
   podvm's userland rung (phase 0)? Which syscalls and files does it
   need? (Not measured.)
4. Can the sources of `check-strings.py` see the help text inside a
   Mach-O and a PE binary? (T-705: not measured: the sections of the
   static probe.)

### Files to read

`TODO/fleet.md` (T-692 to T-705, T-146), `crates/pod-lab/`,
`crates/pod-hostprobe/`, `crates/pod-manual/`, `scripts/install/`,
`docs/install.md`, `docs/research/ci/podrunner-plan.md`,
`https://github.com/PodUtils/podrunner/issues/180`, `.../183`,
`https://github.com/PodUtils/podnet/issues/584`, `.../586`.

### Candidate approaches

The entries have Approach sections. The unmeasured items need a real
runner and a real host of each class: that is the work of a researcher
with machines.

---

## H-09. A stalled resume of `pod-session` (podssh#283)

Entries: T-767 (done, P1), T-768 (open), T-204 (open), T-939 (done),
T-945 (done), T-959 (done). Files: `TODO/relay.md`.
Issue: `https://github.com/PodUtils/podssh/issues/283` (COORD-091).

### The problem, as a fact

podssh's gate step "m6" (`cargo test -p podssh-cli --features
podssh-cli/iroh-test --test m6_exit -- --ignored --test-threads 1`) failed
eleven times since 2026-11-13. Each time one test hangs to its limit of
600 s, a different test each time, and a rerun passes. A local loop of 15
runs failed once (run 8, 600 s). podcrates cannot reproduce it: 100 runs of
three tests, half on an idle host and half with each of four cores busy,
moved the bytes whole each time.

The eleven failures, in short:

1. to 6.: a resume that the client calls done; then no byte moves for 600 s.
   The replay of the sixth was 64 KiB, below the window of 768 KiB.
7. (`the_layer_survives_a_stall_of_3_minutes`, run 43870922795): the
   node's line says resume 1, no link carried the session; the node
   wrote 7,814,432 bytes of the client to the target; its reader of the
   target had no byte in flight at the resume.
8. (`iroh::the_iroh_road_survives_a_new_address`, run 44287863366): on the
   iroh road with no UDP the client's address changed; the node says it
   sent 13,758,313 bytes, the client acknowledged 13,247,259, 511,054 bytes in
   flight. No `session.resume` line came. T-939 added a progress check
   to the heartbeat (a link is lost when a PING waits 30 s and the
   session's position does not move) and a line for the iroh relay dial.
9. (run 44452674747): the client's `Note::Stall`: "the application took no
   byte for 21 s, while 288823 bytes wait for it ... the far end
   acknowledged 20680508"; the node said no stall. So the hold is above the
   layer, at the channel of `pod-e2e`, or at SSH.
10. (run 44478407399, the layer's road, `the_layer_survives_a_stall_of_3_minutes`):
    the same sign.
11. (run 44506565889, iroh road): podssh's own watch says "SSH read nothing
    for 20 s while the channel waits to give it bytes; 16089582 bytes went
    to SSH". So the reader that stopped is **podssh's SSH client** (a read
    of its end of a pipe of 256 KiB). podssh takes it
    (PodUtils/podssh@T-780). podssh says the channel carries both
    directions at once (`crates/pod-e2e/src/channel/carry.rs`) and a cycle
    through the pipe is possible.
12. to 16. (podssh's close of #283): the hold is inside podssh's own code.
    The pump of its SSH session awaited a write while the queue of the
    channel filled, and nothing drained it. Its repair is on podssh `main`
    (`io/beside.rs`). podssh withdrew asks 2 to 4 and asks nothing more of
    podcrates, so T-767 is done.

Failures 1 to 7 are not explained. The record says (T-767): a lost wake, a
lost ACK, an offset gap, a deadlock of locks, an old link that does not
stop, and a stale waker were ruled out by reading, not by a reproduction.
The case "a first resume whose link ended before the far end's greeting, a
second resume 1.1 s later" has a test (`tests/session_resume_again.rs`,
`tests/session_resume_twice.rs`).

T-768: a spare link waits 20 s (`SPARE_QUIET`), not 1 to 3 s
(`spare_after`), when a relay host that stopped holds the link open and the
writer waits in a write of a `DATA` batch (`pump/outbox.rs`, `write`):
the question is marked only after the write ends. Seen in 4 of 100 runs
(`Spare { quiet: 20.03 s }`).

T-204: `a_session_moves_while_both_ends_send_at_full_speed`
(`tests/session_move.rs`) lost its link once in a run of the workspace
tests, and once in 200 runs under load: `[Lost { why: "the link ended with
no word from the far end" }, Resumed { resent: 65536, after: 382µs }]`.
"Not measured: the order of the end of the old link and the far end's word
at a move under load." With a line printed at each end, 0 failures in 1,120
runs (the observation changes the timing).

### What is not known

1. Does the SSH client of podssh (the russh or its own channel) read the
   pipe that `channel::around` writes? Is there a cycle: the SSH client
   waits to write to the channel; the channel waits to write to SSH; both
   pipes are full? Read podssh's `channel::around` (the glue that writes the
   256 KiB pipe) and its SSH client, and `carry.rs` in podcrates. A test with two full pipes
   and one task that does `select!` on both would show it. **podssh's close of
   #283 confirms it:** the cycle is in its own pump (failures 12 to 16).
2. Are failures 1 to 7 the same cause as 11 (a hold above the layer)? The
   far end's lines in 1 to 7 were not enough to say. Can podssh's
   log from the failed runs show "SSH read nothing"? (The watch came late.)
3. T-768: how often does a stalled relay fill the link before the first
   question is marked? (Not measured.) Does the fix (mark the question when
   the write starts) keep a slow reader from a false spare?
4. T-204: which comes first at a move under load, the end of the old link
   or the far end's word? Run the test under `rr` or with a deterministic
   scheduler (`loom`, `shuttle`, or tokio `test-util` with a paused clock
   and a seeded runtime) to find the race. Is it a defect of the test (the
   test's link drops the word) or of the code (`Keeper::take` with no
   `RETIRE` and no linger)?
5. Is a model check of the protocol of `pod-session` (the state machine of
   resume, the offsets, the window) possible with TLA+ or `stateright`? The
   record names no such work; a model would answer "can a resume be called
   done while the far end does not take the session?" for all orders.

### Files to read

- `crates/pod-session/src/{pump,keep,resume,beside,hearing,far,client}.rs`
  and `pump/{up,down,outbox,progress,stall,watch}.rs`;
  `crates/pod-session/tests/session_{resume_again,resume_twice,move,spare,heartbeat}.rs`;
  `crates/pod-session/README.md`; `docs/session-protocol.md` is for the talk
  between sessions, **not** the protocol; the wire is `docs/wire.md`.
- `crates/pod-e2e/src/channel/{carry,stall}.rs`, `crates/pod-iroh/src/`,
  `crates/pod-iroh/tests/new_address.rs`.
- `PodUtils/podssh`: `crates/podssh-cli/tests/m6_exit.rs`, and the code that
  podssh#283 names: `channel::around`, `ends::operator_told` and
  `ssh/iroh/dial.rs`.
- `https://github.com/PodUtils/podssh/issues/283` (the whole thread),
  `.../320`, `.../325`.

### Candidate approaches (guesses unless marked)

- Guess: make the channel's two directions independent tasks with a bounded
  buffer between them, so that a full pipe in one direction never stops the
  other. Trade-off: memory per session.
- Guess: write a deterministic model of the pump with `loom`.
- The record's approach for T-768: mark the question when the write starts.
- Keep the lines of T-945 and T-959 on; they found the cause.

---

## H-10. The relay role, the 409 and 1013 failures, and renew

Entries: T-036 (open, L: `pod-relay-serve`), T-121 (renew a pair), T-124
(failover on the reverse road), T-361. Issues:
`https://github.com/PodUtils/podnet/issues/604` (COORD-098),
`https://github.com/PodUtils/podnet/issues/413`,
`https://github.com/PodUtils/podcrates/issues/214`.

### The problem, as a fact

The live runs of podnet fail about one in six at the step "SSH through the
reverse relay". After a reset ("Connection reset by peer"), the relay holds
the node's lost socket for minutes, so each new connect of the node gets
HTTP 409 for at least 150 s, or the relay sends "1013 node open timeout" for
the 600 s of the resume window. The fault is in the **relay server**, which
is not in podcrates today: it is podssh's relay role (podssh T-801 at
`d8e54f0` makes a failed write let the name go at once, and a silent socket
go in 30 to 40 s). A relay of the pool keeps the fault until its operator
deploys that build. podcrates takes the role into `pod-relay-serve` (T-036,
effort L). The ask 2 (a node asks for a new pair) is done in podnet v0.65.0
(`lost_hold`, 60 s). T-121: the relay contract has no renew call; the crate has
`pair::renew::renew` (`POST /v1/renew/NAME`) that podssh's role serves and
a relay of the contract answers with 404 or 405. T-124: the reverse road must
fail over across relays of several operators (the ruling "Open questions"):
"Not measured: a node socket on host B that serves a pair minted on host A."

### What is not known

1. Which relays of the pool run a build at or after `d8e54f0`? (podssh
   names them; the status of each is not in the record.)
2. Does `live` of podnet show no 1013 after the deploy? (T-406, T-478 of
   podnet.)
3. T-124: how does a pair minted on host A reach a node socket on host B?
   Is a shared store needed, or does a node hold pairs on several relays at
   once? What is the cost to the operator?
4. T-036 (`pod-relay-serve`): which parts of podssh's role move to a
   sans-IO core that a Worker on wasm32 can run (the ruling "The relay
   Worker", Q-A7)? What is the size of the wasm binary? (Not measured.)
5. T-361: do the tests of `pod-relay` pass against the self-hosted role and
   against `workerd` in a CI job? (Not measured: the start of the role and
   of workerd in a job.)

### Files to read

`TODO/relay.md` (T-036, T-121, T-124, T-361), `docs/wire.md`,
`docs/research/relay-rates.md`, `crates/pod-relay/src/reverse/`,
the relay role of podssh (PodUtils/podssh@T-592 and T-801), and the issues
above.

### Candidate approach

Port the role from podssh as the record plans (T-036) with the fix at
`d8e54f0` and a test that resets a node socket and asserts that the next
connect is admitted at once.

---

## H-11. The static rule on each target

Source: the ruling of 2026-11-05 (`AGENTS.md` section 2),
`docs/design.md` sections 1 and 7, `scripts/check-static.sh`, the `static`
job of `ci.yml`, `examples/static-probe*`.

### The problem, as a fact

"Everything must be statically compiled." CI proves it on 14 targets:
8 Linux musl targets (x86_64, aarch64, riscv64gc, i686, loongarch64,
powerpc64le, armv7, arm), 2 Linux gnu, 2 macOS, 2 Windows (MSVC). The
rules by system (design rule 1): Linux no `NEEDED` and no interpreter;
Windows the C runtime linked in; macOS only system libraries (Apple allows
no static `libSystem`). The `static` job is green at `edbc86b` (run
44632880776). The record has no open "static" entry, so this is a standing
risk, not a defect. The places where the rule is hard:

- **C dependencies.** `pod-gsr` builds OpenSSL from source (SRP). Design
  section 7 lists known risks "proved or fixed in the study entry of the
  crate": `aws-lc-sys` on i686, arm, loongarch64 and powerpc64le musl
  (unproven; "prefer ring where the upstream allows it"); the SQLite of
  arti on loongarch64 and powerpc64le; `iroh` and `netwatch` on the targets
  that only zig builds.
- **Windows ARM64**: the OpenSSL crash (H-02, T-131).
- **macOS**: the check lists the system libraries that are allowed
  (`libSystem`, `libiconv`, `CoreFoundation`, `Security`).
- **Tools that the project ships as data**: a UML kernel, `runsc` (H-05).
- **Fuzzing**: libFuzzer needs glibc, so the fuzz job
  (`fuzz.yml`, "the runner is ubuntu-24.04, not musl") tests the code in a
  build that is not the shipped one.
- **Nightly Rust.** The owner allows it "if necessary". Fuzz pins
  `nightly-2026-11-18` (T-968).
- The adoption rule: the consumers take a git tag; a crate that cannot link
  static is a blocked entry with a question for the owner.

### What is not known

1. Is the "proved or fixed" status of `aws-lc-sys` and arti's SQLite
   recorded for each target? Search `docs/` and the crate READMEs for the
   study entries; the rule of design section 7 says the entry of the crate
   holds the proof. If it does not, the static job needs a row for each.
2. Does a binary that links `pod-tor` (arti, bundled SQLite) pass the check
   on loongarch64 and powerpc64le? `examples/static-probe` links each crate
   with each feature, so it should. (Read `scripts/check-probe.py` for the
   allowance list.)
3. A binary that is static and in-memory safe is not proof of a working
   program: does the static probe run on each target under QEMU? (It is
   built and inspected, not run? Read `ci.yml`, job `static`.)
4. For macOS: can the check also forbid a dynamic dependency on a
   framework that is not in a fresh install?

### Files to read

`docs/design.md` sections 1, 7, 9; `scripts/check-static.sh`,
`scripts/check-probe.py`, `examples/static-probe/`,
`examples/static-probe-lean/`, `.github/workflows/ci.yml` (job `static`),
`deny.toml`.

---

## H-12. The egress allowlist of a cloud session

Source: `AGENTS.md` section 7, `docs/maintenance.md` ("This sandbox").

### The problem, as a fact

A cloud session reaches GitHub, crates.io, the mirrors of zig and a few more
through an HTTPS proxy with an allowlist. WARP, the edge of Cloudflare and
the owner's relay are not reachable. The session has no `/dev/fuse`, a
Landlock ABI of 7, and cannot start KVM. The rule is: "Do not try to go
around it. Live tests run in `live.yml` on GitHub." So each fact that needs a
real network or a kernel feature stays "Not measured" until a live run or a
real machine measures it. The list (from the "Not measured" lines of the
record; there are about 115 such entries, of which these need a real
machine):

| Entry | Missing fact |
| --- | --- |
| T-339 | the live WARP session (the egress reaches no WARP endpoint) |
| T-386 | the live run of `pod-devtunnel` (needs the owner's token and a tunnel) |
| T-330 | a pair of the live relay through the facade |
| T-433 | a real FUSE mount (no `/dev/fuse`) |
| T-241 | a kernel with Landlock ABI 11 or 12 (the host has 7); T-696: ABI 10 |
| T-359 | the kernel's WireGuard on a runner (`ip link add type wireguard`) |
| T-431 | `PTRACE_SEIZE` under Docker, AppArmor, `ptrace_scope` |
| T-430 | the rungs `Cgroup`, `Pidns`, `Reaper`, `Job`, `Session`; ConPTY |
| T-436 | the `sev` crate with `crypto_nossl` on x86_64 musl; SEV-SNP hardware |
| T-434 | the `blake3` crate with C and assembly paths on 14 targets |
| T-649 | Windows mounting a FAT image as a removable medium |
| T-802 | a host with two ways out (multi-WAN) |
| T-825 | a dual-stack host |
| T-409 | the throughput of the two UDP paths (GSO, GRO) |
| T-189 | `tcp_allowed_congestion_control` for an unprivileged process; BBR gain |
| T-190 | whether a registry honors `Range` on its redirected blob URL |
| T-419 | an upload and a fetch at each file host |
| T-812 | the rate of any hosted relay; a rate limit |
| T-502 | a headscale harness |

### What is not known (the questions)

Each row above is one question that an experiment answers. The researcher on
the owner's machine can take the rows that need only a kernel, a root, or an
open network (T-241, T-696, T-433, T-359, T-431, T-430, T-409, T-189,
T-802, T-825, T-812, T-419). The rows that need an account or a token
(T-386, T-339) need the owner's secret (the ruling "Test accounts" says
"Not now" for the CI; a manual run on the owner's machine is a different
case).

---

## H-13. Upstream limits

Each row: the problem, the evidence, the question for the researcher.

### `tar` 0.4.46 (T-961, done in the crate)

`tar` reads the data of a PAX header (`x`) and of a GNU long name (`L`, `K`)
whole, with no bound: a layer whose header says 8 GiB makes the extraction
allocate 8 GiB. `PaxExtensions` cuts records at each newline byte and not
by the length, so a `SCHILY.xattr.` value with a 0x0a byte (a capability
set with bits 1 and 3 in one byte) does not parse. `pod-extract` now frames
the stream itself (`crates/pod-extract/src/pax.rs`: `PAX_MAX` 1 MiB,
`LONG_MAX` 4096, parse by length). **Question:** is there an upstream issue
or pull request at `alexcrichton/tar-rs` for these two? (`docs/upstream/`
holds only the `kcp` patch; the record does not say that the `tar` problem
was sent.) Do other `tar` users (cargo, container tools) need it? Guess: another
reader crate (for example `astral-tokio-tar`) may already bound these
headers; check.

### `russh` 0.65.1 and `ssh-key` 0.7.0-rc.11 with a nightly compiler (T-968, done)

`russh` did not compile under `nightly-2026-11-01` with the fuzz flags:
`error[E0391]: cycle detected when computing when ssh_key::public::key_data::KeyData
needs async drop`. It compiled with `nightly-2026-11-18`. The fix was to pin
the newer nightly in `scripts/fuzz.sh`. `russh` takes `ssh-key` 0.7.0
**release candidates**, so the whole stack of SSH in podcrates (the
`pod-testkit` SSH server, `pod-services-watch`, `pod-devtunnel`) depends on
a pre-release crate. **Questions:** is `ssh-key` 0.7 stable out? Is the
async-drop cycle a rustc regression that is now fixed, or does it come back?
(Find the rustc issue.) Does `russh` still pull `rsa` 0.10 rc, which is
tied to the advisory RUSTSEC-2023-0071 ignore in `deny.toml`?

### `cargo-fuzz` and libFuzzer (T-017, T-968)

The fuzz workspace has about 72 targets (`fuzz/fuzz_targets`; the job counts
72). 72 runs of 60 s do not fit in 75 minutes in one job, so the job
runs two shards (`FUZZ_SHARD=I/N`). **The sharded workflow has not been
proved by a run** (see H-14). libFuzzer needs glibc; cargo-fuzz is
installed from a release tarball (`scripts/fuzz.sh`, with its SHA-256) or
`cargo install`. **Questions:** do 36 targets of 60 s each fit in a shard
with their build? Is a corpus cache across runs possible
(`actions/cache`)? Is a continuous fuzz service (OSS-Fuzz, ClusterFuzzLite)
suitable for a 0BSD workspace of this size?

### `kcp` 0.6.0 (T-526, partial)

`kcp` is a port of ikcp; its clock (32 bits of ms) and sequence numbers wrap
by design, and with overflow checks (dev, test, fuzz) a forged segment or
true traffic after 24.8 days panics. The workspace turns the checks off for
`kcp` in the dev profile; the fuzz build keeps them. A patch
`docs/upstream/kcp-0.6.0-wrapping.patch` exists. **Owner action:** send it
upstream as a pull request (a session cannot write outside PodUtils).
**Question:** does upstream `kcp` accept it (find the maintainer and the
repository, and whether it is alive)?

### `iroh` and the fork `vendor/iroh-relay`

`pod-iroh` carries a patched fork of `iroh-relay` (`vendor/`,
`crates/pod-iroh/README.md`). `chrono` comes only through iroh
(`tokio-rustls-acme`, `wmi` of `netwatch`): unmaintained (RUSTSEC-2026-0543,
T-852). T-809: "whether a custom transport of iroh 1.4 can carry the relay's
connection" (not measured). **Questions:** which patches does the fork hold,
and has iroh upstream accepted them? Does the next iroh drop `chrono`?

### `arti` (pod-tor) and SQLite

`deny.toml` ignores RUSTSEC-2026-0459 (SQLite before 3.54.0 gives stale
memory with a crafted `-wal` file) because no arti release takes
`rusqlite >= 0.41`; also `paste` (RUSTSEC-2024-0436), and `humantime`
(RUSTSEC-2026-0542, T-852). The licences of arti's crates (MPL-2.0,
Unlicense, CC0, BSL-1.0) are allowed one by one (C766). **Question:** what
is the arti version that fixes `rusqlite`, and when?

### `snow` (T-199) and `ed25519-dalek` (T-779)

`snow` 0.10.0 pins `chacha20poly1305` 0.10 and `chacha20` 0.9; the workspace
has 0.11: two stacks in each binary that has `pod-e2e`. `pod-link` has two
stacks of the curve and hash crates (`ed25519-dalek` 2.2.0 and
`x25519-dalek` 3.0.0). **Questions:** is a snow release on the 0.11 line
announced? What is the size that a second stack costs in a release binary
(not measured; `cargo bloat` on `podssh`)?

### `cloudflared` pin (T-851)

The pin is 2026.10.7; 2026.11.0 has the same bytes in each pinned file.
**Question:** did the files that the pin does not follow
(`connection/protocol.go`, `cmd/cloudflared/tunnel/cmd.go`,
`edgediscovery/allregions/discovery.go`) change? (The check
`crates/pod-cloudflare/tools/check-upstream.py` shows it.)

### `pocket_vm` patches for UML (T-975)

See H-05, question 2.

---

## H-14. Two more red scheduled workflows

- **`fuzz.yml`**: the latest scheduled run 44519034117 (`a184eed`,
  2026-11-19) failed because `russh` did not compile on the pinned
  nightly (T-968). The fix pinned a new nightly and sharded the job. The
  run by hand 44527726675 (`af79a26`) built every target but met the 75-minute
  limit; the sharded workflow was pushed after it. **No run of the sharded
  workflow exists.** The last green run is 43209945321 (2026-11-12, `45b145a`).
  Check: dispatch `fuzz.yml` and read both shards.
- **`upstream.yml`**: the scheduled runs fail daily (latest 44537712046,
  2026-11-19, `9ee2097`). HANDOFF says: the job `sources` is red on
  `pod-manual` until the port review of podssh's changes (T-863, open,
  P2). The job `sources` compares files that came from a consumer with the
  default branch of that consumer. **Question:** how many files of how many
  consumers drift? Is one port review a day sustainable, or should the
  check allow a "reviewed at" mark per file?
- **`ci.yml`**: HANDOFF line 3 says T-943 is "partial until three runs in
  a row are green"; the record (`TODO/upkeep.md`) says T-943 is **done**
  with three green runs (44347947644, 44355702336, 44365173268). The
  documents disagree. The newest `ci.yml` run with a result at `edbc86b`
  (44632880776) is green. The run at `0344ae6` (44646534192) was still in
  progress when I read it.

---

## H-15. NAT traversal and congestion control of `pod-link`

Entries: T-499 (L), T-802, T-515, T-514 (L), T-818 (L), T-803, T-820 (L),
T-821 (L), T-496 (L), T-498. `TODO/relay.md`, `TODO/transport.md`.
Issues: `https://github.com/PodUtils/podnet/issues/521`, `.../468`,
`https://github.com/PodUtils/podcrates/issues/212`, `.../213`.

### The problem, as a fact

podnet closes issues before a complete beta, and asks for these crates:

- T-499 NAT traversal: STUN to learn the public address, candidates
  exchanged over a reverse session of the relay inside `pod-e2e`, a UDP
  punch, and a fall back to the relay. "Not read yet: the text of
  PodUtils/podnet@T-284 and its tests."
- T-802: a NAT probe of RFC 5780 (mapping, filtering, port preservation) on
  the caller's datagrams. No crate classifies a NAT today. Issue 212 is by
  a user outside the team (`Nemo-010`, label `confirmed`).
- T-515: the ARQ has no congestion window. The tests run over a memory pair
  with no rate limit. "Not measured: the ARQ over a real path with other
  flows." Start condition: the first carrier over a shared path.
- T-514: a stream-aware ARQ in which one stream's timeout does not close
  the link. T-818: forward error correction and a NACK backstop ("not
  measured: FEC on a real lossy path; the residual numbers are a binomial
  model"). T-803: a parity datagram (issue 213; "the stall of a tail loss
  on a lossy carrier" not measured).

### What is not known

1. What fraction of the NATs that users of podnet meet allow a punch (the
   classes of RFC 4787)? The answer decides T-499's value. (A guess: a
   survey of published numbers, then a measurement from the owner's
   machines on several ISPs and mobile networks.)
2. What loss and RTT do the real carriers have (WireGuard UDP, the DNS
   stream, a relay)? T-818 needs it before FEC.
3. Is NewReno (the record's first choice) right, or BBR-style pacing? The
   test for the choice is a shim with a rate limit and a queue of a fixed
   size (the Approach).
4. For T-499: do two peers behind a symmetric NAT or CGNAT ever punch? Which
   share of the owner's own targets is behind CGNAT?

### Candidate approaches

The entries' Approach sections. Guess: do T-802 first (a small, self-contained
probe with a clear test on the loopback); its result decides how much of T-499
is worth building.

---

## H-16. Large, deferred or unmeasured feature crates

- **T-490, T-821** (`pod-tls`: the server half of ECH, a ClientHello in the
  shape of a browser, a measure of the handshake; GitHub #130,
  `[DEFERRED]`). L. The measure is "none here"; a JA4 audit of the hello
  needs real browser captures.
- **T-436** (`pod-attest`: SEV-SNP report check and expected measurement).
  Needs the `sev` crate on musl and a real report.
- **T-433** (`pod-fuse`): the FUSE server with write opcodes, needs
  `/dev/fuse`. T-448 (a wrong fact in `pod-enter`: `mount(2)` of FUSE works
  in a user namespace since Linux 4.18) is a docs fix.
- **T-430, T-431** (`pod-proc`): `PTRACE_SEIZE` under Docker, AppArmor,
  `ptrace_scope` (the spec's risk K-03).
- **T-822** (a threshold CA of the grants): "Not read: a consumer that asks
  for a threshold CA" (no consumer asks).
- **T-207, T-345 to T-349** (`pod-netlab`, a network lab of the guest, L):
  needs the layout of podvm's issue 178 and the knobs. "Not measured:
  which knobs a user-space stack can hold and which need a kernel."
- **T-062** (studies of the widened scope of podvm, L).

All of these wait on the ruling "podswarm" (they help podswarm alone) or
"Priority", or on facts only a real machine gives. They are listed so a
researcher can see where a measurement is the first step. The questions are
in the "Not measured" lines of each entry.

---

## H-17. Blocked entries

| Entry | Why blocked | Who can unblock |
| --- | --- | --- |
| T-217, T-218, T-219, T-220 (`pod-git`, L) | The owner moved `pod-git` to `PodUtils/podforge` and ordered "keep it deferred". The research (T-217) compares gix, grit-lib 0.5.33 and own code | the owner (podforge) |
| T-832 (the dig study's intake for podswarm, L) | The ruling "podswarm": an ask that helps podswarm alone waits | the start of the podswarm session |
| T-617 (an NTFS and hive writer for an offline Windows setup) | podvm must first measure whether `AutoRun` fires on the x64 image (PodUtils/podvm#350, item 6) | podvm |
| T-538 (the rule on an attribution line in a GitHub comment) | Q16 | the owner |

The entries must not be closed as "out of scope" (AGENTS.md section 9).

---

## H-18. Questions and actions only the owner can answer

### Open questions (from `TODO/PROGRESS.md` and `docs/decisions.md`)

- **Q16.** Does the word of the owner of 2026-11-11 in the `AGENTS.md` of
  PodUtils/podrunner ("a comment on GitHub can end with the 'Generated by'
  line of the harness; a commit cannot") apply to podcrates too? The ruling
  "Git" of 2026-11-05 here forbids each attribution line in a comment, and
  each comment of a session of podcrates has the line. Blocks T-538.
  Recommendation of the record: yes, the same rule in each repository of
  PodUtils.
- Q1 to Q14 are answered (see `docs/decisions.md`). Q15 is the ruling
  "podswarm". Q17: the packages of ghcr.io are public, no action. The next
  free number is Q18.
- `docs/decisions.md` ends with "Questions for the owner" that points to
  `TODO/PROGRESS.md`: there is no other open question there.

### Questions that this reading raises (not in the record; for the owner)

- **Q-A.** UML and `runsc` are programs that are not Rust crates: may a
  release publish a Linux kernel (GPL-2.0) and a gVisor binary
  (Apache-2.0) as assets, and what is the source offer? (H-05; the project
  licence is 0BSD and `deny.toml` forbids GPL crates.)
- **Q-B.** May the slow QEMU targets (riscv64, loongarch64, powerpc64le)
  skip the `pod-iroh` tests in the nightly while a real machine of the
  owner runs them? (H-01.)
- **Q-C.** May the verifier of `pod-release` get a TUF-signed newer set of
  roots when a network exists? (H-07.)

### Owner actions (`TODO/PROGRESS.md`, "Owner actions")

- Send `docs/upstream/kcp-0.6.0-wrapping.patch` to the `kcp` upstream (T-526).
- Report to `Lazarus404/xsockets` that `recverr: true` is an option that no
  Erlang/OTP release takes (C588); `scripts/ci/xturn-recverr.patch`
  removes it for T-631 until xsockets drops it.
- Set the repository secrets `POD_DEVTUNNEL_TUNNEL`, `POD_DEVTUNNEL_PORT`
  (and `POD_DEVTUNNEL_ACCESS_TOKEN`), host the tunnel
  (`devtunnel host -p 22 --allow-anonymous`), and dispatch `live.yml` with
  `measure=devtunnel` (T-386, row L45 of `docs/live-calls.md`).
- Delete the merged `wip/` branches that `https://github.com/PodUtils/podcrates/issues/208`
  names (`wip/t909` too), and the Dependabot branch
  `dependabot/docker/images/trufflehog/trufflesecurity/trufflehog-3.99.8`.
  The proxy of a cloud session drops `git push origin --delete`.

---

## The facts that the record marks "Not measured" and that decide a design

These are facts where the next step of the design depends on the number.
The full list is the output of this command (115 entries mention
"Not measured" or "Not read"):

```sh
grep -n -E '^- Not (measured|read)' TODO/*.md
```

The ones that decide a design:

| Entry | Fact not measured | The decision that waits on it |
| --- | --- | --- |
| T-916 | the count of voluntary switches of a computation under each emulator; which bounds are scaled | whether the "no blocking call" gate can run in emulated jobs at all |
| T-766 | the time of each step under riscv64 emulation | the scale of each bound |
| T-935 | `FILE_RENAME_FLAG_POSIX_SEMANTICS` on both Windows runners | the replace of `pod_core::files` on Windows |
| T-131 | which test and which call crash on Windows ARM64 | fix of OpenSSL build, or a replacement of SRP |
| T-933 | the `recv_from` error on Windows | a defect of the stand-in or of the client |
| T-963 | whether the relay drops or holds events past 60 s; whether it refuses each host of the runners or two connections of one host | the choice of Nostr relays for pairing |
| T-351 | whether the nodes answer from another network on the same day | the rule of "no answer" |
| T-371 | whether Quad9's DoH answers a runner on the other cloud | the class of DNS findings |
| T-973 | each client (curl, git, Python, Node.js) with the CA bundle | whether the TLS end of `pod-broker` is usable by a real job |
| T-975 | a static `ARCH=um` kernel built in CI | whether podrunner's no-root systemd rung is possible |
| T-977 | the release files of gVisor | the pinned fetch of `runsc` |
| T-976 | whether `abi::Elf` reads `DT_RUNPATH` | the loader family of `pod-enter` |
| T-106 | cfb's list of stand-ins | the program set of `pod-testkit` |
| T-146 | the official runner under podvm (phase 0) | the whole plan of podrunner |
| T-692 | a profile on a runner; the six network profiles | the labs as gates of a release |
| T-964 | a bundle of Rekor v2 from the live service; the date that the signing configuration moves | the release rhythm of `pod-release` |
| T-966 | the trial of either consumer's `update` on `pod-release` | the hand-off |
| T-499 | the text of podnet's T-284 and its tests | the shape of `nat::Punch` |
| T-515 | the ARQ over a real path with other flows | the congestion control |
| T-818 | FEC on a real lossy path | whether FEC is worth its cost |
| T-802 | a host with two ways out | the NAT class API |
| T-207, T-349 | issue 178 of podvm (layout, knobs); which knobs a user-space stack can hold | the design of `pod-netlab` |
| T-217 | grit-lib's server code; gix's pack writer; size and package count in a musl binary | the base of a git server |
| T-121 | a renew endpoint on the relay server | the renew of a pair |
| T-124 | a node socket on host B that serves a pair minted on host A | failover across relays |
| T-812 | the rate of any hosted relay; a rate limit | the rank of relays by measured rate (T-814) |
| T-809 | whether a custom transport of iroh 1.4 can carry the relay's connection | the proxy URL for iroh's relay dial |
| T-189 | which controllers an unprivileged process can select; BBR gain on a path of PodUtils | the congestion controller on a dialed socket |
| T-190 | whether a usual registry honors `Range` on its redirected blob URL | parallel ranges for `pod-oci` |
| T-290 | the size of podnet's release binary with and without the layer | whether the layer should be a feature |
| T-102, T-103, T-105 | `systemd-analyze security` of a unit; a client against two tools; a registry | the hardening set; the control API; the registry |
| T-436, T-434 | `sev` crate on musl; `blake3` C and assembly paths on 14 targets | the build of those crates |

---

## Where to start (my ranking, with reasons)

1. **H-01 the nightly** (T-916 and the three entries with it). The CI of
   podcrates cannot show a real defect while seven jobs are red, and
   the owner's order puts the red nightly first. Start with the cheap
   measurement: how fast do the iroh tests run in a release or optimized
   build under `qemu-riscv64`?
2. **H-09 the stalled resume** (T-767, done). The consumer saw it sixteen
   times; podssh found the hold in its own pump and repaired it. T-768 and
   T-204 stay open.
3. **H-02 Windows**, starting with the skipped pod-gsr step: one line of
   the workflow gives the first measurement of T-131; T-935 and T-934 are
   small and well described.
4. **H-05 the UML kernel** (T-975). It is the largest open feature that a
   consumer (podrunner's parity table) waits for, and the static build is
   the unknown that decides it.
5. **H-03 and H-04**: the services from runners, and the CA bundle with
   real clients. Both turn guesses into measured lists.

Number of problems in this report: 18 (H-01 to H-18).
