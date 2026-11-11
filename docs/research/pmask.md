# pmask: study note

Subject: `Azathothas/pmask` at `5a3c972` (tag `v0.1.0-beta.3`). Read-only study. Not adopted.
Ruling: `docs/decisions.md`, 2026-11-11, "podrunner DR-6 and pmask": study it and note it; do not adopt it now.
Each line starts with Observed (read in the tree), Inferred (a conclusion) or Not read.

## 1. What it is

- Observed: a Linux `ptrace` supervisor. It runs a process tree and replaces `/proc/<pid>/cmdline`, `environ` and `comm` with a label. Each program keeps its real `argv` and `envp`.
- Observed: mechanism: at `execve` entry it swaps the argv/envp pointer arrays to the label; at exec completion it puts the real strings in an injected mapping; `prctl(PR_SET_MM_MAP)` moves the kernel's argv/env windows onto a label block; it injects `prctl(PR_SET_NAME)` through a vDSO `syscall` instruction.
- Observed: "confidential execution without trusted hardware" means: other local accounts and pasted `ps` output see the label only. With `--no-dumpable`, a same-uid peer also loses `environ`, `maps`, `fd/*`, `exe`, `mem`, `process_vm_readv`, `PTRACE_ATTACH`.
- Observed: it does not protect against root, `CAP_SYS_PTRACE`, the kernel, eBPF or audit of the supervisor's own exec. It does not encrypt memory. It does not hide that a pid exists (limits L1 to L18 in `docs/THREAT-MODEL.md`).
- Observed: second part: secret delivery. `--secret NAME=SRC` gives the value on a sealed memfd at fd 3, 4, ...; sources `file:`, `env:`, `agent:`, `stdin`, `literal:`. `pmask agent` holds an encrypted store unlocked, with leases and a hash-chained use audit. `pmaskd` runs jobs for clients and holds their environment.
- Observed: third part: `--isolate` gives user, mount, pid, net, UTS, IPC, cgroup namespaces, a private `/dev` and `/proc`, `no_new_privs` and a Landlock read/exec policy. This mode does not redact.
- Observed: language Rust (edition 2021, MSRV 1.85, CI toolchain 1.99.1), about 30,300 lines in `src/`. Dependencies: `libc`, `zeroize`, `argon2` 0.5, `chacha20poly1305` 0.10. Licence 0BSD; dependency licences MIT, Apache-2.0, BSD-3-Clause.
- Observed: state: repository made 2026-11-07; public history starts 2026-11-08 with one squashed commit; 234 commits; last commit 2026-11-10. 220 commits by one automation account.
- Observed: releases: `v0.1.0-beta.1` (2026-11-08), `beta.2` and `beta.3` (2026-11-10), all prereleases, each with a "Breaking" section. Assets: static musl `pmask` and `pmaskd` for x86_64 and aarch64, SHA256SUMS, Sigstore provenance. Not on crates.io (`crates-io = false`).
- Observed: tests: 234 unit tests; 20 shell suites and 55 per-issue QA fragments that need a ptrace host; a QEMU micro-VM gate; one fuzz target (store header). CI: fmt, clippy, unit, doc, MSRV, native suites, VM suites, cargo-deny, trufflehog. Last CI run on `main` is green. 6 open issues (b-team residuals).

## 2. Interface

- Observed: CLI `pmask [options] -- CMD`; subcommands `store`, `agent`, `get`, `read`, `probe`, `doctor`. `pmask doctor --require redact,exact` exits 0, 3 (degraded) or 4 (unavailable).
- Observed: library: `Mask` builder (`label`, `no_dumpable`, `rexec`, `namespaces`, `run_as`, `envs`, `inherit_fds`, `session_end`, ...), `run` (blocks), `spawn` -> `MaskedChild` (`try_wait`, `wait_timeout`, `kill_tree`). Also `mask_self`, `mem::SecBuf`, `mem::SealedFd`, `secret::Delivery`, `store::Store`, `caps::Caps`.
- Observed: a session owns the caller's `waitpid(-1)` domain; one global `SESSION` mutex serializes sessions in one process.
- Observed: formats: store file (magic `PMASKSTORE1`, Argon2id, XChaCha20-Poly1305, header as AAD, mode 0600); `pmaskd` wire protocol v2 (`PMK2`, stdio by `SCM_RIGHTS`, `PMKP` ping); agent line protocol (`GET <name>`); `--report=json`; `probe` JSON; `PMASK_SECRET_<NAME>=<fd>` in the child.
- Observed: host needs: Linux 5.3+, ptrace allowed for the caller (no seccomp deny, Yama scope 0 or 1). No root. No KVM (QEMU only in the test harness). `CONFIG_CHECKPOINT_RESTORE` for the exact window. Optional: user namespaces (`--unshare`, `--isolate`), Landlock 5.13+, `memfd_secret`, `SO_PEERPIDFD` 6.5+.
- Observed: without ptrace the run falls to `mode=plain`: no redaction. Secret delivery and the agent still work.
- Observed: static musl: x86_64 and aarch64 only, checked by `scripts/check-static.sh` in `release.yml`.
- Observed: `src/arch.rs` has an `imp` module for x86_64, aarch64 and x86 only, and `pub use imp::*` has no fallback. Inferred: armv7, riscv64, loongarch64, powerpc64le and s390x do not compile. i686 compiles only for a test probe; a 32-bit host is "not supported", and 32-bit tracees are refused.

## 3. Where it can fit later

Podrunner boundaries: `redirect` rewrites paths and is not a boundary; `confined` is a kernel-enforced boundary; `guest` is a microVM.

- Inferred: pmask is not a boundary. It does not move a job from `redirect` to `confined`. Its redaction hides a payload from the host's peers; it does not contain hostile code.
- Inferred: podrunner at `redirect` (DR-4): pmask can reduce one accepted residual, "read other jobs on the same uid", for `cmdline`, `environ`, `comm`, `mem` and `fd` (needs ptrace and `--no-dumpable`). It cannot stop file reads, network exfiltration or writes to the host's configuration and binaries.
- Inferred: G-03: pmask can keep the JIT config out of podrunner's own argv and environment (`--args-from-fd`, `--secret`, `mask_self`) and make the runner process non-dumpable. It cannot hide `.runner` and `.credentials` on disk from a same-uid step. G-03 stays open with pmask.
- Inferred: `guest`: nothing on the host side. In a guest, pmask can hide job metadata from other guest processes.
- Inferred: pod-supervise: add a "masked payload" option that starts the payload under the `pmask` binary (a separate process). The library cannot run inside pod-supervise next to its own reaper (the `waitpid(-1)` domain).
- Inferred: pod-enter: overlap. pmask's rexec chain (sealed memfd, `/proc/self/fd/N`, `O_TMPFILE`, temp file) and `--unshare`/`--isolate` repeat the launch ladder and the namespace rung. Use it as a reference. Do not replace pod-enter.
- Inferred: pod-hold: no shared function, and one conflict (finding 4).
- Inferred: pod-confine (T-108): a source for the Landlock ABI probe and the `no_new_privs` order. It does not replace T-108 (finding 7).
- Inferred: pod-broker: add `SecBuf` (mlock, `MADV_DONTDUMP`, `memfd_secret`), `PR_SET_DUMPABLE=0` and `RLIMIT_CORE=0` before a secret is read, delivery of a job token as a sealed memfd, and a self-mask of the broker's argv. The lease and the hash-chained audit overlap the broker's TTL and JSON audit.
- Inferred: pod-tls and pod-identity: memory hygiene for private keys (`SecBuf`), and the store format as a reference for keys at rest. Observed: no crate in podcrates calls `PR_SET_DUMPABLE`, `mlock` or `memfd_secret` today.
- Inferred: podvm: A-V-11 (`run --secret ... once`) is the closest match: sealed memfd, `--secret-burn`, `--secret-close-on-exec`, with the same limit (a burn needs a cooperating reader). On podvm hosts that deny ptrace, only plain mode works. pmask's own gap study names a second kernel (podvm machine tier, `pod-microvm`) as the only fix for L1 to L18.
- Inferred: podnet and podssh: `mask_self` at start can remove tokens from their own `cmdline` and `environ` with no ptrace. The audit record and the launch window keep the argv (finding 15). A token on an fd or in a file is better.

## 4. Risks and unknowns

- Observed: maturity: 3 days of public history, 3 prereleases with breaking changes, one maintainer and one automation account, no third-party audit, 454 `unsafe {` blocks, fuzz only on the store header.
- Observed: beta.2 changed 12 CLI rules, exit codes and the PING body; beta.3 changed the report file mode and a warning ("Breaking" sections).
- Inferred: API churn: high. Pin a commit or a tag, and read the "Breaking" section of each release.
- Observed: licence: 0BSD, and each dependency is on the podcrates allow list. No copyleft. No credit needed.
- Inferred: static build: fits the owner's rule on x86_64 and aarch64 only. The other six targets need a port of `arch.rs`, `inject.rs` and `seccomp.rs` for each architecture.
- Observed: performance: two ptrace stops per syscall until the first exec settles, then a stop at each fork, clone, exec and exit, and injected syscalls at each exec. The script launch window is about 300 us. Not measured: the cost for build-heavy CI jobs (no committed bench result).
- Observed: one tracer for each task: redact mode cannot run under or over strace, gdb or another ptrace tier. Set-uid does not elevate under ptrace: `sudo` in a job fails.

## 5. Findings

1. Inferred: the hosts that need podvm or podrunner H0 often deny ptrace. There pmask's main mechanism is not available. Measure `pmask probe` on H0 before any plan.
2. Observed: no build for armv7, riscv64, loongarch64, powerpc64le, s390x; i686 is not a supported host. A podcrates dependency on it breaks the workspace build on those targets unless gated by `cfg`.
3. Observed: the library takes the process's `waitpid(-1)` domain and a global mutex. podcrates crates use pidfds, no globals and no environment. Use it as a separate process, or one worker process for each job.
4. Inferred: `--no-dumpable` refuses `/proc/<pid>/mem` to a non-root reader of the same uid. `pod-hold` reads the task memory through `/proc/<pid>/mem` (`crates/pod-hold/src/mem.rs`). A held payload with `--no-dumpable` breaks the hold. Not measured.
5. Observed: release library code reads test fault knobs from the environment (`PMASK_FAULT_POKE`, `_NOVDSO`, `_REFUSE_MAP`, `_CLOSE`, `_NOWINDOW`; `src/mask.rs`, `Mask::to_config`) with no `cfg` or feature gate. Who sets the caller's environment can degrade the redaction. The library also writes to stderr.
6. Observed: `sudo` and other set-uid programs do not elevate under redact mode. CI jobs often call `sudo`.
7. Observed: the Landlock policy of `--isolate` is fixed (system directories and the program's parent directory). It handles only `EXECUTE`, `READ_FILE`, `READ_DIR`. No write rules, no profile file, no seccomp deny filter, no rlimits except `RLIMIT_CORE`. T-108 stays necessary.
8. Inferred: G-03 is not met by pmask for the official runner: the Worker reads its credential files from disk.
9. Observed: dependency skew: pmask uses `chacha20poly1305` 0.10; podcrates uses 0.11. A link puts two RustCrypto generations in one binary. podcrates `deny.toml` has `allow-git = []`: a git source needs a decision, or a fork in `vendor/`.
10. Observed: 20 files of `src/` have more than 500 lines (`daemon.rs` 3,327). The podcrates rule is 500 lines for each source file.
11. Observed: CI runs the suites on x86_64 only. aarch64 gets a build and a smoke run in `release.yml`. The VM harness can run aarch64 (`PMASK_VM_ARCH`); CI does not use it.
12. Observed: no committed performance result. `measurements/bench.sh` exists.
13. Observed: doc defects in pmask: `docs/research/MICROVM-GAP.md` cites `docs/research/ALTERNATIVES-honest-limits.md`, which does not exist; the `store.rs` doc example uses the `daemon:` source, which the README does not list; README and SECURITY.md say ChaCha20-Poly1305, the store uses XChaCha20-Poly1305.
14. Observed: the use of the store from the daemon is split: `pmaskd` refuses `--store`; only `pmask agent` holds the store. A consumer needs two services for "daemon plus store".
15. Inferred (verify): Rust `std::env::args()` on Linux reads the saved argv pointers at each call. After `mask_self` zeroes the argv block, a later `args()` gives empty strings, and `env::var` gives empty values. An embedder must take a snapshot first. The comment in `src/selfmask.rs` ("std copies argv at startup") is likely wrong.

## 6. Not read

- Not read: most function bodies of `tracer.rs`, `execimg.rs`, `daemon.rs`, `agent.rs`, `rexec.rs`; the shell suites; `evidence/`; `docs/research/` except `MICROVM-GAP.md`; the bodies of the GitHub issues (titles only).
- Not read: no code was built or run. No number in this note was measured by podcrates.
