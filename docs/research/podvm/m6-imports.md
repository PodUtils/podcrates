# M6: the import plan of the crates of podvm

The plan for the entries T-055 to T-061 (`docs/design.md`, section 4, M6; the procedure of section 8). The source is
`PodUtils/podvm` at `ecf61c08fdaf28db079e0939895e18e5f9440c5f` (`ecf61c0`, after the rename `98112cb`). Each path below
is a path of that repository, unless it starts with `crates/pod-`.

A change of the source: podvm split each file of these crates to 500 lines at `b917f97`, with the tests in `tests.rs`
files beside them (PodUtils/podvm#203). An import reads `b917f97` or later; the splits of section 1 to 7 are then
podvm's, and the classes of the files stay.

## 0. Conventions and the rules for all seven crates

Classes: **M** moves (generic), **S** stays in podvm, **P** splits (the row names the part that moves). "Rec" is the
count of lines that cite the record of podvm (`TODO/`, `T-NNNN`, `TOOL.md`, `experiments/`).

* **Comments.** The adapt step removes each citation of the record of podvm, of `references/` (podvm's corpus), each
  date and the markers ⭐ ⛔ ⚠. It keeps each technical fact, in ASD-STE100.
* **Names of podvm.** Each `PODVM_*` variable, each file marker and each label that names podvm becomes a setting of the
  crate. The adoption patch of podvm gives podvm's values, so podvm's files on disk and its payloads see no change.
* **Exit codes.** `crates/podvm-probe/src/exit.rs` (docker's 125 for a runtime error) stays in podvm: it is a CLI
  decision. The shell's 126 and 127 go with `pod-enter`.
* **Linux only.** The seven crates use raw Linux system calls, `/proc` and `binfmt_misc`. Each `lib.rs` is
  `#![cfg(target_os = "linux")]`. `examples/static-probe` calls them on the Linux musl targets only. podvm builds for
  Linux musl only (its `rust-toolchain.toml`).
* **Edition.** podvm is edition 2021, `rust-version` 1.85. The edition step must look at
  `std::env::set_var`/`remove_var` in tests (unsafe in 2024): `podvm-probe/src/interp.rs` (2),
  `podvm-complete/src/pkg.rs` (11), `podvm-windows/src/dos.rs` (4), `podvm-windows/src/lib.rs` (5). Each is a setting
  after the adapt step. No `static mut`, no `extern "C"`, no `gen`.
* **No C, no build script.** No crate of M6 has a `build.rs` or C code. `flate2` uses `rust_backend`. podvm's `rustls`
  feature `ring` (C and asm) is dropped: the provider is the caller's (pod-tls rule). `podvm-interpose` (`dl-stub.S`) is
  not in M6.
* **Skip guards.** `pod-testkit` has `need_loopback!` and `need_localhost!`. M6 needs three more: `need_userns!` (a user
  and a mount namespace), `need_root!`, `need_tool!(name)`.
* **podvm's gate.** `crates/podvm-gate/src/*.rs` names the paths of the moved files (`podvm-probe/src/exit.rs`, the
  `tests/` files, `podvm-complete/src/write.rs`, `podvm-supervise/src/lib.rs`). The adoption patch of podvm changes
  these checks.

### The crates.io dependencies

| Crate | podvm spec | Cargo.lock | podcrates now | Taken by |
| --- | --- | --- | --- | --- |
| `linux-raw-sys` | 0.12, `general`, `no_std` | 0.12.1 | new | pod-hostprobe |
| `syscalls` | 0.8, no default; not on mips, mips64, s390x, powerpc, powerpc64 | 0.8.1 | new | pod-hostprobe |
| `sha2` | 0.11, no default | 0.11.0 | 0.11 (`oid`, `zeroize`) | pod-oci, pod-extract, pod-guest |
| `serde` | 1, `derive` | 1.0.229 | new to the workspace | pod-oci, pod-supervise |
| `serde_json` | 1 | 1.0.151 | 1 | pod-oci, pod-rootfs, pod-supervise; pod-extract (tests) |
| `base64` | 0.23 | 0.23.1 | 0.23 | pod-oci |
| `tar` | 0.4, no default | 0.4.46 | new | pod-extract (entry API only, never `unpack()`) |
| `flate2` | 1, `rust_backend` | 1.1.10 | new | pod-extract |
| `ruzstd` | 0.9 | 0.9.0 | new | pod-extract |
| `rustls`, `rustls-pki-types`, `webpki-roots` | 0.23 (`ring`), 1, 1 | 0.23.45, 1.15.1, 1.0.9 | through pod-tls | pod-oci, pod-rootfs |
| `rustls-pemfile` | 2 | 2.2.0 | dropped | `pod_tls::bundle::pem_certificates` |
| `ureq` | 2, `tls`, `gzip` | 2.12.1 | dropped | `pod-proxy` feature `blocking` (T-063) |

## 1. T-055 `pod-hostprobe` ← `crates/podvm-probe`

| File | Lines | Class | Reason | Rec |
| --- | ---: | --- | --- | ---: |
| `src/sys.rs` | 1922 | M | raw system calls; 69 `unsafe` | 54 |
| `src/child.rs` | 395 | M | one disposable child for each probe: `clone` with no `CLONE_VM`, re-exec of `/proc/self/exe`, the errno line checked against the exit status; `CHILD_FLAG` becomes a setting | 6 |
| `src/verdict.rs` | 268 | M | the three states; `WIRE_MAGIC` `podvm-probe/1` becomes `pod-hostprobe/1` | 3 |
| `src/identity.rs` | 350 | M | the id maps, `ConfinementKey` | 13 |
| `src/mounts.rs` | 132 | M | create, attach, use | 2 |
| `src/binfmt.rs` | 221 | M | `binfmt_misc` read, `SELF_MACHINE`; adapt the `compile_error!` text | 4 |
| `src/interp.rs` | 220 | M | detection of an emulator under the probe | 5 |
| `src/machine.rs` | 436 | M | the QEMU, KVM and TCG profile; pod-guest and pod-microvm read it | 4 |
| `src/supervise.rs` | 310 | M | the seccomp-notify and pidfd legs | 3 |
| `src/probes.rs` | 1729 | M | the probe table and the bodies | 58 |
| `src/select.rs` | 620 | P | `Rung`, `Strictness`, `Rejection`, `Selection`, `Provides` move (about 540); `BANNER_ROWS`, `banner_cell` stay with the report (about 80) | 23 |
| `src/lib.rs` | 139 | P | `Findings`, `run`, `run_child` move; `run_child` writes to a writer, not `println!` | 8 |
| `src/writable.rs` | 231 | P | the write probe moves; the candidate list (`PODVM_STORE` and others) becomes a parameter | 2 |
| `src/exit.rs` | 190 | S | docker's exit codes: a CLI decision | 8 |
| `src/json.rs` | 202 | S | the writer of the report | 2 |
| `src/report.rs` | 1186 | S | podvm's banner, document and `--json` | 32 |
| `src/nongoals.rs` | 319 | S | the non-goals of podvm's specification | 7 |
| `tests/namespace.rs` | 105 | M | selection on a host that refuses namespaces | 1 |
| `tests/namespace_base.rs` | 270 | M | selection on a host that permits them | 2 |
| `podvm-image/src/space.rs` | 314 | M | `statfs` blocks and inodes, the `RLIMIT_FSIZE` ceiling, `choose`; the text names a setting, not `$PODVM_STORE` | 3 |
| `podvm-image/src/exec_probe.rs` | 210 | M | can files under a directory execute (`noexec`) | 4 |
| `podvm-image/tests/space_precheck.rs` | 151 | M | blocks and inodes on a real file system | 0 |

Counts: 15 M, 3 P, 4 S. About 7,900 lines move; about 1,980 stay.

**Splits (over 500 lines).**

* `sys.rs` → `sys/mod.rs` (`Errno`, the `E*` constants, `Sysres`, the re-exports; ~130), `sys/nr.rs` (the `nr!` macro,
  the `SYS_*` numbers, the choice of `syscalls` or `linux-raw-sys` per architecture; ~200), `sys/arch.rs` (`stat_call`,
  `KernelStat`, `entry_points`: `CHOWN`, `LCHOWN`, `MKNOD`; ~140), `sys/raw.rs` (`syscall6` for each architecture, the
  asm of powerpc64 and s390x, `split`, `sys`; ~170), `sys/consts.rs` (`CLONE_*`, `SIG*`, `MS_*`, `O_*`, `RESOLVE_*`,
  `AT_*`, `S_IF*`, `LOCK_*`, `F_*`; ~130), `sys/fd.rs` (`CBuf`, `close`, `fcntl`, `dup*`, `open`, `read`, `write`,
  `pipe2`, `chroot`, `mount`, `umount`, `unshare`, `chdir`, `getrandom`; ~230), `sys/at.rs` (the `*at` wrappers,
  `Dirent`, `getdents64`, `readlinkat`, `fstatat`; ~260), `sys/process.rs` (`FORK_CLOSE_SLOTS`, `shed_after_fork`,
  `clone_fork`, `execve`, `execveat`, `wait4`, `kill`, `setsid`, `getpid`, `exit_group`, `pidfd_open`, the signal mask
  and `signalfd`, `waitid_pidfd`, `ppoll`; ~330), `sys/fs.rs` (`Statfs`, `statfs`, `flock`, `readlink`, `prlimit`,
  `Stat`, `stat`, `getgroups`; ~200), `sys/socket.rs` (~40), `sys/tests.rs` (~160).
* `probes.rs` → `probes/mod.rs` (`Kind`, `Group`, `Probe`, `PROBES`, `MACHINE_LEGS`, `SUPERVISE_LEGS`,
  `SUPERVISION_LEGS`, `find`; source lines 1-313), `probes/helpers.rs` (314-453), `probes/census_ns.rs` (456-670:
  namespaces, mounts, ids, `chown`, `chroot`), `probes/census_exec.rs` (672-974: memfd, exec, writes, the device nodes,
  the listener, `chroot_usable`, `ptmx_usable`, `fuse_usable`), `probes/attribution.rs` (975-1340, with the `s_*`
  supervise rows), `probes/machine.rs` (1341-1509), `probes/tests.rs` (1510-1729).
* `select.rs` (~540 after the split) → `select.rs` (`Rung`, `Strictness`, `Rejection`, `Selection`; ~330),
  `select/provides.rs` (~120), `select/tests.rs`.

**Dependencies.** `linux-raw-sys`, `syscalls` (target-gated as in podvm's `Cargo.toml`), `pod-core` (typed failures). No
pod crate below it but `pod-core`. `unsafe`: `sys.rs` 69, `probes.rs` 40, `child.rs` 3, `identity.rs` 2, `interp.rs` 2;
the crate keeps `#![forbid(unsafe_op_in_unsafe_fn)]`. `pod-core` has `libc` on unix; `sys` does not use it, on purpose:
a libc wrapper (`setuid(3)` of glibc or musl) gives the library's errno, the probe needs the kernel's.

**The call on `sys`.** `sys` stays one public module, `pod_hostprobe::sys`. pod-extract, pod-rootfs, pod-enter,
pod-supervise and pod-guest take it through `pod-hostprobe`, not from a crate of their own. pod-guest needs only
`sys::kill` and `machine::Profile`.

**Adoption map (podvm).**

| Old path | New path |
| --- | --- |
| `podvm_probe::sys::*` (51 lines in 9 crates) | `pod_hostprobe::sys::*` |
| `podvm_probe::{run, run_child, Findings}` | `pod_hostprobe::{run, run_child, Findings}` |
| `podvm_probe::CHILD_FLAG` | `pod_hostprobe::Settings::child_flag`, podvm's value `--probe-child` |
| `podvm_probe::verdict::{Outcome, Verdict}` | `pod_hostprobe::verdict::*` |
| `podvm_probe::select::{Rung, Selection, Strictness, Provides}` | `pod_hostprobe::select::*` |
| `podvm_probe::select::{BANNER_ROWS, banner_cell}` | podvm, with `report` |
| `podvm_probe::probes::{chroot_usable, ptmx_usable, fuse_usable}` | `pod_hostprobe::probes::*` |
| `podvm_probe::identity::{Identity, read, ConfinementKey}` | `pod_hostprobe::identity::*` |
| `podvm_probe::binfmt::{SELF_MACHINE, machine_for, registrations, Registration, BINFMT_DIR}` | `pod_hostprobe::binfmt::*` |
| `podvm_probe::machine::{assess, Assessment, Profile}`, `mounts::Mounts`, `interp` | `pod_hostprobe::{machine, mounts, interp}` |
| `podvm_probe::{exit, report, nongoals, json}` | stay in podvm |
| `podvm_image::space::{require, read, mib, dir_bytes, file_ceiling, choose}` | `pod_hostprobe::space::*` |
| `podvm_image::exec_probe::probe_store_exec` | `pod_hostprobe::exec::probe_dir_exec` |

**Tests.** About 77 of 109 unit tests move (not those of `exit`, `json`, `report`, `nongoals`), and 9 + 2 integration
tests; `space` 9 and `exec_probe` 5 unit tests move too.

* `lib.rs` `every_child_probe_runs_and_answers_in_a_child` re-executes the test binary with `CHILD_FLAG`. libtest does
  not know the flag, so each child row is a skip and the test checks the shape only. Add a test binary with `harness =
  false` (`tests/children.rs`) that dispatches the flag first, so that the rows run.
* `tests/namespace_base.rs`, `tests/space_precheck.rs` (second half): `unshare(CLONE_NEWUSER|CLONE_NEWNS)`; they skip by
  hand now → `need_userns!`.
* The probe rows are not tests: `bind(127.0.0.1:0)+listen`, `qemu-system-x86_64 --version` and the others run only in
  `run()`.

**Overlap.** podnet `crates/podnet-netns/src/doctor.rs` (377 lines) probes in a `libc::fork` child and gives `ok: bool`
and a detail text.

| podnet row | pod-hostprobe |
| --- | --- |
| `unshare(CLONE_NEWNET)`, `unshare(NEWUSER\|NEWNET)`, then `lo` up and a TUN in the namespace (exit codes 200, 201) | not in podvm: add two Census rows with child bodies |
| `open(/dev/net/tun)` | `TUN_LEG` of `machine` |
| `ptrace(PTRACE_TRACEME)`, then `SIGSTOP` and detach | `ptrace(PTRACE_TRACEME)` row; compare the stop check |
| `seccomp(NEW_LISTENER)` after `PR_SET_NO_NEW_PRIVS` | `seccomp(NEW_LISTENER)` and its supervise row |
| write to `/proc/self/mem` | `open /proc/self/mem O_RDWR` |

podvm's model is the base: the errno, three states, and a re-exec in place of a `fork` of a process with threads (podnet
runs tokio). The README gives the parity list and the adoption map of podnet (`doctor::probe_*` → the rows by name;
`Probe { ok, detail }` → `Outcome`). podnet's `src/vnet/doctor.rs` probes the network and does not overlap.

## 2. T-056 `pod-oci` ← `crates/podvm-image`

| File | Lines | Class | Reason | Rec |
| --- | ---: | --- | --- | ---: |
| `src/reference.rs` | 360 | M | the docker.io constants come from `pod-services` | 4 |
| `src/digest.rs` | 281 | M | `Digest`, `Verifier` | 2 |
| `src/oci.rs` | 391 | M | the OCI and docker media types, `select_platform` | 6 |
| `src/platform.rs` | 367 | M | `PODVM_DEFAULT_PLATFORM`, `DOCKER_DEFAULT_PLATFORM` become a setting | 2 |
| `src/credentials.rs` | 808 | M | `~/.docker/config.json`, `containers/auth.json`, the credential helpers; the paths come from the caller | 6 |
| `src/registry.rs` | 1215 | M | the distribution client; `ureq` → `pod-proxy` `blocking` | 22 |
| `src/error.rs` | 136 | P | the enum moves; `exit_code()` and the re-export of `podvm_probe::exit` stay; `Store(String)` becomes the error of `Blobs` | 8 |
| `src/transport.rs` | 416 | P | `Policy`, `Decision`, `is_loopback`, `normalise_host` move (~270); `PODVM_INSECURE_REGISTRIES`, `PODVM_CONFIG` and the read of `registries.conf` stay (~150); `no_proxy_entries` → `pod_proxy::Environment` | 4 |
| `src/pull.rs` | 913 | P | the fetch pool (`FetchJob`, `fetch_one`, `fetch_parallel`, the ceilings, `FETCH_WORKERS`; ~330) moves behind a trait `Blobs` (`has`, `stage`, `commit`); `pull`, `pull_all`, `local`, the probe cache, the record and the provenance stay | 24 |
| `src/layout.rs` | 501 | P | the OCI layout constants, the entry filter, the shape of `index.json` (~80) move; `save`, `load`, `import` on `Store` stay | 5 |
| `src/tls.rs` | 290 | P | the search of a host bundle → `pod-tls` (~70); `client_config` → a `pod_tls` trust; `AcceptAny` and `client_config_unverified` stay | 5 |
| `src/store.rs` | 2622 | P | `Lock` (lines 1003-1218) → `pod-supervise`; the content store stays | 76 |
| `src/lib.rs` | 78 | P | `open_store` stays; the module list is new | 8 |
| `src/clock.rs` | 170 | M | to `pod_core::clock` (`iso8601`, `now`, `since`) | 1 |
| `src/contain.rs` | 160 | M | to `pod_core::files::within` | 2 |
| `src/health.rs` | 256 | S | store health, provenance | 4 |
| `src/probe_cache.rs` | 396 | S | podvm's probe cache in the store | 10 |
| `tests/registry_fixture.rs` | 109 | M | the loopback registry proves itself | 1 |
| `tests/common/registry.rs`, `mod.rs` | 236, 2 | M | the loopback registry (a candidate for `pod-testkit`) | 0 |
| `tests/acquisition.rs`, `store_digest.rs`, `parallel_layers.rs`, `across_distributions.rs` | 131, 128, 280, 268 | S | they pull into `Store` | 9 |

`space.rs`, `exec_probe.rs` and `tests/space_precheck.rs` go to pod-hostprobe (section 1). Counts (27 files): 9 M, 2 M
to pod-core, 3 M to pod-hostprobe, 7 P, 6 S. About 4,600 lines go to pod-oci, 330 to pod-core.

**Splits.** `registry.rs` → `registry/mod.rs` (`Client`, `new`, `with_policy`, the connection through pod-proxy; ~200),
`registry/manifest.rs` (`manifest`, `tags`, `next_link`; ~150), `registry/blob.rs` (`blob`, `Restart`, the ceiling;
~120), `registry/request.rs` (the auth retry, `Attempt`, `retryable`, `parse_retry_after`, `backoff`; ~250),
`registry/auth.rs` (`parse_challenge`, the token, `key`; ~120), `registry/errors.rs` (`redact`, `status_error`,
`transport_error`, `registry_message`; ~120), `registry/tests.rs` (~270). `credentials.rs` → `credentials/mod.rs`
(`Credential`, `lookup`, `lookup_in`, `basic_header`, `server_host`, `default_paths`; ~150), `credentials/auth_json.rs`
(`parse_auth_json`, `upsert_auth_json`, `delete_auth_json`, the file write; ~150), `credentials/helper.rs`
(`helper_for`, `helper_login`, `helper_store`, `helper_erase`; ~180), `credentials/login.rs` (`store_login*`,
`remove_login*`; ~100), `credentials/tests.rs` (~230).

**Dependencies.** `sha2`, `serde` (derive), `serde_json`, `base64`; `pod-core`, `pod-tls`, `pod-proxy` (feature
`blocking`, T-063), `pod-services` (the hosts of Docker Hub). Dropped: `ureq`, `rustls-pemfile`, the `ring` feature. No
`unsafe`, no C. The adapt step of T-056 waits for T-063; the import can come first. `ureq` sends `Accept-Encoding:
gzip`; pod-oci sends no `Accept-Encoding` until pod-proxy decodes it.

**Adoption map (podvm).**

| Old path | New path |
| --- | --- |
| `podvm_image::{Reference, reference::*}` | `pod_oci::reference::*` |
| `podvm_image::digest::{Digest, Verifier}`, `Digest::of` | `pod_oci::digest::*` |
| `podvm_image::oci::{Manifest, Descriptor, Config, RunConfig, Index, Fetched, MEDIA_*}` | `pod_oci::spec::*` |
| `podvm_image::platform::{Platform, OS, HOST_ARCH}` | `pod_oci::platform::*` |
| `podvm_image::registry::{Client, FetchedManifest, Restart, redact}` | `pod_oci::registry::*` |
| `podvm_image::credentials::*` | `pod_oci::credentials::*` |
| `podvm_image::transport::Policy` | `pod_oci::transport::Policy` (podvm fills it from its flags, variables and file) |
| `podvm_image::{Error, error::Error}` | `pod_oci::Error` |
| `podvm_image::error::EXIT_*` (122 uses) | podvm's `exit` module |
| `podvm_image::tls::{client_config, host_bundle_bytes}` | `pod_tls::Context` roots; `pod_tls::roots::host_bundle` |
| `podvm_image::tls::client_config_unverified` | podvm, given as `pod_tls::Trust::caller` |
| `podvm_image::clock::{now, since}`, `contain::within` | `pod_core::clock::*`, `pod_core::files::within` |
| `podvm_image::store::Lock` | `pod_supervise::lock::Lock` |
| `Store`, `Record`, `open_store`, `pull::*`, `layout::*`, `health`, `probe_cache` | stay in podvm (`Store` implements `pod_oci::Blobs`) |

**Tests.** About 74 unit tests move: credentials 22, registry 15, reference 10, digest 7, platform 7, oci 6, transport
~4, pull ~2, tls 1; and `tests/registry_fixture.rs`. Each test that binds `127.0.0.1` (registry 1, the fixture) →
`need_loopback!`. The registry tests that read `ureq::Transport` text change with T-063. The credential tests run a
helper that does not exist (`podvm-no-such-helper-xyz`, renamed): no host tool. clock 5 and contain 5 go to pod-core.

**Overlap.** pod-proxy: HTTP, the proxy variables and `NO_PROXY` (`transport::use_proxy`, `no_proxy_entries`). pod-tls:
the roots and the system bundles; pod-tls builds no insecure verifier, so `AcceptAny` stays in podvm. pod-core: `clock`,
`files` (`create_private` for `write_owner_only`; the places for `default_paths`), `backoff` (compare with 500 ms base,
30 s cap, `Retry-After`). pod-services: a new `docker-hub.toml` for `docker.io`, `registry-1.docker.io`,
`index.docker.io/v1/`, the prefix `library` (`docs/research/services/podvm.md`, section A).

## 3. T-057 `pod-extract` ← `crates/podvm-extract`

| File | Lines | Class | Reason | Rec |
| --- | ---: | --- | --- | ---: |
| `src/apply.rs` | 475 | M | the apply of one layer; 2 `unsafe` | 18 |
| `src/layer.rs` | 240 | M | compression, `HashedStream`, the size | 3 |
| `src/remove.rs` | 201 | M | whiteouts before entries | 3 |
| `src/safety.rs` | 617 | M | containment, `openat2(RESOLVE_IN_ROOT)`; pod-rootfs reads it | 10 |
| `src/sidecar.rs` | 224 | M | the ownership sidecar | 7 |
| `src/whiteout.rs` | 134 | M | basename match | 5 |
| `src/drive.rs` | 702 | M | test module; `one_layer_store` builds a podvm `Store`: the fixture becomes a blob directory with `pod_oci` types | 20 |
| `src/lib.rs` | 352 | P | the core of `extract` moves as `extract_into(blobs, manifest, rootfs, sidecar, out)`, with `Extracted`, `DONE_MARKER`, `is_extracted`, `remove_extracted` on a rootfs path; `paths(store, digest)` and the `Store` glue stay (~90) | 20 |
| `src/error.rs` | 62 | P | `Image(podvm_image::Error)` → `Oci(pod_oci::Error)` | 3 |

Counts: 7 M, 2 P, 0 S. About 2,920 lines move; about 90 stay.

**Splits.** `safety.rs` → `safety/mod.rs` (`Refusal`, `is_archive_root`, `components`, `rebase_symlink_target`; ~180),
`safety/dir.rs` (`Dir`, `Resolve`, `have_openat2`, `open_parent`, `open_parent_with`; ~320), `safety/tests.rs` (~115).
`drive.rs` → `drive/mod.rs` (`Scratch`, the layer builder, the fixture; ~200), `drive/paths.rs` (the escape tests),
`drive/layers.rs` (whiteouts, links, permissions, the sidecar).

**Dependencies.** `tar`, `flate2` (`rust_backend`), `ruzstd`, `sha2`; `serde_json` for tests only; `pod-hostprobe`
(`sys`, `space`), `pod-oci` (`Manifest`, `Descriptor`, `Digest`, `Error`). The table of `docs/design.md` lacks
`pod-oci`: add it.

**Adoption map (podvm).** `podvm_extract::{extract, Extracted, is_extracted, remove_extracted, EXTRACT_HEADROOM_BYTES}`
→ `pod_extract::{extract_into, ...}` with a rootfs path; `podvm_extract::paths` → podvm; `sidecar::FirstDropped`,
`safety::{have_openat2, open_parent_with, Resolve, Dir, Refusal}` → `pod_extract::{sidecar, safety}::*`.

**Tests.** 47 unit tests, 22 of them in `drive.rs`. None needs root, a namespace or the network. `safety` falls back
where `openat2` is absent (`have_openat2`).

**Overlap.** None in podcrates. podnet has no layer code.

## 4. T-058 `pod-rootfs` ← `crates/podvm-complete`

| File | Lines | Class | Reason | Rec |
| --- | ---: | --- | --- | ---: |
| `src/lib.rs` | 467 | M | `Options`, `Report`, `complete`; the banner text is a setting | 28 |
| `src/devices.rs` | 734 | M | device shims as regular files; `sys::getrandom`, `space::require` | 42 |
| `src/identity.rs` | 446 | M | passwd, group, nsswitch, the libc probe | 26 |
| `src/net.rs` | 374 | M | `resolv.conf`, `hosts`, `mtab` | 18 |
| `src/pkg.rs` | 1259 | M | pacman, apt, zypper, the CA bundle, the CApath; `HOST_CA_MARKER` (holds a record id) and `HOST_CA_FILE_PREFIX` become settings | 60 |
| `src/reach.rs` | 136 | M | a bounded HEAD over HTTPS: `ureq` and `podvm_image::tls` → pod-proxy `blocking` and pod-tls | 2 |
| `src/sources.rs` | 366 | M | the rewrite of `http://` package sources | 10 |
| `src/write.rs` | 793 | M | writes checked against the root, on `pod_extract::safety` | 15 |

Counts: 8 M. About 4,575 lines move, plus ~60 in pod-tls.

**Splits.** `devices.rs` → `devices.rs` (`run`, the shims; ~330), `devices/fd_links.rs` (`fd_links`, `displace_link`;
~150), `devices/tests.rs` (~250). `pkg.rs` → `pkg/mod.rs` (`find_program`, `comment_out`, `pacman`; ~190), `pkg/apt.rs`
(`apt`, `ca_bundle`; ~330), `pkg/capath.rs` (`split_pem`, `is_hash_link`, `dir_key`, `ca_hash_dir`; ~290),
`pkg/zypper.rs` (~140), `pkg/tests.rs` (~310). `write.rs` → `write/mod.rs` (`Kind`, `Wrote`, `Reach`, `Root`: `open`,
`path`, `kind`, `read`, `read_text`; ~280), `write/put.rs` (`write`, `mknod_char`; ~220), `write/follow.rs` (the
`*_following` functions, `mkdirs`, `list`, `unlink`; ~150), `write/tests.rs` (~150).

**Dependencies.** `serde_json`; `pod-core`, `pod-hostprobe` (`sys`, `space`), `pod-extract` (`safety`), `pod-tls` (a new
`roots::host_bundle(ca_file, system) -> Option<(PathBuf, Vec<u8>)>`: the path and the bytes of the first usable bundle),
`pod-proxy` (`blocking`). The table of `docs/design.md` lacks `pod-extract` and `pod-proxy`: add them.

**Adoption map (podvm).** `podvm_complete::{complete, Options, Report, Fixup, Step, Action}` → `pod_rootfs::*`. The
markers keep podvm's values in podvm's settings, so a second pass finds the files of the first.

**Tests.** 53 unit tests. `pkg.rs` tests set `SSL_CERT_FILE`: the bundle becomes a value of the options. `reach.rs`
tests use `Prober::always`: no network. No test needs root: the shims are regular files and a refused `mknod` is a named
result.

**Overlap.** pod-tls: `SYSTEM_BUNDLES` against podvm's `BUNDLE_PATHS` and `BUNDLE_ENV` (`SSL_CERT_FILE`,
`CURL_CA_BUNDLE`, `REQUESTS_CA_BUNDLE`); merge the lists, the caller reads the variables. pod-proxy: the HEAD of
`reach`. pod-services: none (the hosts of `reach` come from the image).

## 5. T-059 `pod-enter` ← `crates/podvm-enter`

| File | Lines | Class | Reason | Rec |
| --- | ---: | --- | --- | ---: |
| `src/abi.rs` | 1503 | M | the ELF reader, the libc flavour, the version predicate, `resolve_in` | 19 |
| `src/binfmt.rs` | 174 | M | foreign architectures; re-exports `pod_hostprobe::binfmt` | 4 |
| `src/device.rs` | 380 | M | `--device` by descriptor; `PODVM_DEVICE_SPEC`, `PODVM_DEVICE_FDS` → settings | 2 |
| `src/fuse.rs` | 1524 | M | the read-only FUSE server; 4 `unsafe` | 4 |
| `src/ladder.rs` | 499 | M | memfd, FUSE, tmpfs, run directory, cache; `PODVM_MODE`, `PODVM_CACHE`, `PODVM_GUEST_EXE` → settings | 4 |
| `src/memfd.rs` | 509 | M | the memfd launch; 12 `unsafe` | 8 |
| `src/plan.rs` | 400 | M | docker's rules for `Entrypoint` and `Cmd`; `PODVM_ACTIVE_MODE` → setting | 7 |
| `src/stage.rs` | 433 | M | `copy_tree`, tmpfs, cache | 1 |
| `src/userland.rs` | 644 | M | the loader plan; `MAPS_VAR` (`PODVM_MAPS`, the contract with podvm-interpose) → setting | 10 |
| `src/lib.rs` | 1495 | P | the chroot sequence, spawn, the namespace rung, `Child`, `Bounded`, the workdir check move; the re-export of `EXIT_RUNTIME_ERROR` (125) stays | 81 |
| `tests/tty_refusal.rs` | 202 | M | the `-t` refusal without `/dev/ptmx` | 1 |

Counts: 10 M, 1 P. About 7,720 lines move; about 40 stay.

**Splits.** `abi.rs` → `abi/mod.rs` (`Flavour`, `Versioned`, `Unreadable`, `Verdict`, `admits`, `union`, `newest`;
~250), `abi/elf.rs` (`Elf`, its parse, `interp_of`; source 129-408), `abi/bytes.rs` (`Bytes`, `Section`, `Sections`,
`cstr`; 809-1073), `abi/search.rs` (`libc_beside`, `search_file`, `first_match`, `ResolveKind`, `resolve_in`,
`resolve_dir_in`, `walk_in`; 564-808), `abi/tests.rs` (1074-1503). `fuse.rs` → `fuse/mod.rs` (the gate `check_node*`,
`ActiveMount`, `serve_tree`, `wait_for_server`, `release_mount`; ~400), `fuse/proto.rs` (the op codes, `put_*`, `get_*`,
the reply; ~150), `fuse/serve.rs` (the ops; ~300), `fuse/dir.rs` (the listing and its cookies; ~250), `fuse/tests.rs`
(1101-1524). `lib.rs` → `lib.rs` (the modules, `Error`, `Result`, `RootDir`, `Fds`; ~250), `child.rs` (`Child`,
`Bounded`; ~240), `spawn.rs` (`run`, `spawn`, `spawn_ladder`, `spawn_userland`, `run_*`, `spawn_selected`; ~150),
`namespace.rs` (`NsMount`, `ns_*`, `spawn_namespace`, `run_namespace`; ~200), `chroot.rs` (`spawn_with`; source
849-1235), `status.rs` (`exit_status`, `signal_name`, `invocable_but_refused`, the workdir check, 126 and 127; ~120),
`tests.rs` (1266-1495). `memfd.rs` → `memfd.rs` + `memfd/script.rs` (the shebang and the script refusal). `userland.rs`
→ `userland.rs` (~340) + `userland/tests.rs` (~300).

**Dependencies.** `pod-hostprobe` only (`sys`, `select::Rung`, `binfmt`). No crates.io dependency.

**Adoption map (podvm).** `podvm_enter::{run, spawn, spawn_selected, spawn_userland, run_ladder, run_namespace,
run_userland, check_workdir, Plan, Program, RootDir, Fds, Child, Bounded, Error, Result, ENTERED_RUNG, entered_rung,
entered_word}` and the modules `abi`, `binfmt`, `device`, `fuse`, `ladder`, `memfd`, `stage`, `userland` →
`pod_enter::*` (the same names). `podvm_enter::EXIT_RUNTIME_ERROR` → podvm's `exit`. `EXIT_CANNOT_INVOKE`,
`EXIT_NOT_FOUND` → `pod_enter::status::{CANNOT_INVOKE, NOT_FOUND}`. The variable names → `pod_enter::Names {
mode_request, mode_active, cache, guest_exe, maps, device_spec, device_fds }`, with podvm's values.

**Tests.** 120 unit tests and 3 integration tests.

* `abi.rs` `resolve_past_a_denied_listing` and its worker run a child as `nobody` and return when they cannot →
  `need_root!`.
* `tests/tty_refusal.rs`, second half: `unshare(NEWUSER|NEWNS)` → `need_userns!`.
* `memfd.rs` round trip: `memfd_create` can be refused in a sandbox → a skip on the `memfd_create` probe row.
* `fuse.rs`: the gate tests pass with and without `/dev/fuse`; the server test uses a socket pair. No mount in the
  tests.

**Overlap.** None in podcrates or podnet.

## 6. T-060 `pod-supervise` ← `crates/podvm-supervise`

| File | Lines | Class | Reason | Rec |
| --- | ---: | --- | --- | ---: |
| `src/launcher.rs` | 704 | P | the supervisor (control socket `SIG`/`WAIT`, `ppoll` on the pidfd, the readiness pipe, the signal forward; ~450) moves; the start that makes a `Plan` from `podvm_image::Record` and runs the probe selection stays (~250) | 28 |
| `src/lib.rs` | 688 | P | `create`, `kill`, `stop`, `wait`, `remove`, `list`, `get`, `referencing`, the logs, `new_id`, `generated_name` move with a state directory in place of `&Store` (~560); `start(store, want, record)` stays (~130) | 18 |
| `src/table.rs` | 725 | P | the table, `State`, `Container`, the paths, `reconcile`, `update`, `find` move (~560); the memo (`PODVM_MEMO_FD`, `EmulatedCounts`, `open_memo`, `ensure_memo_file`; the format of podvm-interpose) stays (~165) | 17 |
| `podvm-image/src/store.rs` 1003-1218 | 215 | M | `Lock`: `flock`, `try_exclusive`, `hand_to_payload` | (in store) |

Counts: 1 M (part of a file), 3 P. About 1,790 lines move; about 545 stay.

**Splits.** `lib.rs` (~560) → `lib.rs` (~300), `logs.rs` (`logs`, `follow`, `follow_from`, `tail_offset`, `follow_tail`;
~150), `tests.rs`. `table.rs` (~560) → `table.rs` + `table/tests.rs`.

**Dependencies.** `serde` (derive), `serde_json`; `pod-core` (`clock`), `pod-hostprobe` (`sys`: `pidfd_open`,
`waitid_pidfd`, `ppoll`, `signalfd_shutdown`, `kill`, `flock`), `pod-enter` (`Plan`, `RootDir`, `Fds`, `Child`,
`spawn_*`, `device`). No `podvm-image` after the split. The table of `docs/design.md` lacks `pod-enter`: add it.

**Adoption map (podvm).** `podvm_supervise::{create, kill, stop, wait, remove, list, get, referencing, logs, follow,
follow_tail, tail_offset, Error}` → `pod_supervise::*`, with a `&State` (the directory) in place of `&Store`.
`table::{State, update, log_path, ...}` → `pod_supervise::table::*`. `table::{MEMO_FD_VAR, MEMO_CHILD_FD, memo_path,
open_memo, ensure_memo_file, memo_fd_env, emulated_counts, EmulatedCounts}` and `start` → podvm.
`podvm_image::store::Lock` → `pod_supervise::lock::Lock`. `EXIT_RUNTIME_ERROR` → podvm.

**Tests.** 14 unit tests. The `lib.rs` tests open a `Store` in a temporary directory: they get a state directory.
`launcher.rs` `nothing_on_the_spawn_path_can_spawn_a_thread` reads the sources of the sibling crates from
`CARGO_MANIFEST_DIR`: its paths change to `crates/pod-enter`, `crates/pod-hostprobe`, `crates/pod-supervise`. No test
needs root.

**Overlap.** podnet `detach`, `instances` and `state` also keep detached processes; they stay in podnet
(`docs/design.md`, section 4). A later merge is possible; it is not in M6.

## 7. T-061 `pod-guest` ← `crates/podvm-windows`

| File | Lines | Class | Reason | Rec |
| --- | ---: | --- | --- | ---: |
| `src/agent.rs` | 324 | M | the `cmd.exe` agent, the mailbox names | 2 |
| `src/dos.rs` | 382 | M | the FreeDOS flavour; `PODVM_DOS_BASE` and `~/.local/share/podvm/...` → settings and `pod_core::files` places | 2 |
| `src/fat16.rs` | 558 | M | the FAT16 mailbox; the OEM name `PODVM` → setting | 1 |
| `src/fetch.rs` | 335 | M | a bounded store of a base image from a `Read`, with its digest | 3 |
| `src/plan.rs` | 452 | M | the emulator argv; the accelerator from `machine::Profile` | 2 |
| `src/qmp.rs` | 213 | M | the QMP monitor | 1 |
| `src/lib.rs` | 1142 | P | `run`, `stage`, `provision`, `stop`, `cleanup`, `RunGuard`, the scratch directory move; `cap_status` (docker's 125 rule) and its test stay (~40); `PODVM_WINDOWS_KEEP`, `PODVM_WINDOWS_BASE`, `podvm-windows-` → settings | 5 |

Counts: 6 M, 1 P. About 3,360 lines move; about 40 stay.

**Splits.** `fat16.rs` → `fat16.rs` (~300), `fat16/name.rs` (`parse_name`, `render_name`, `label_bytes`; ~120),
`fat16/tests.rs`. `lib.rs` → `lib.rs` (`Request`, `Outcome`, `Error`, `Stop`, `mailbox`, `outcome`; ~230), `emulator.rs`
(`run_steps`, `spawn_*`, `stop_*`, `kill_group`, the emulator log; ~250), `provision.rs` (`stage`, `provision`,
`type_into_console`, `keys`; ~250), `scratch.rs` (`scratch_of`, `cleanup`, `RunGuard`, `scratch_dir`, `base_cache`;
~110), `tests.rs` (source 754-1142).

**Dependencies.** `sha2`; `pod-core` (the places), `pod-hostprobe` (`sys::kill`, `machine::Profile`). One `unsafe`
(`lib.rs`). Not `pod-proxy`: the download is in podvm `crates/podvm/src/windows/setup.rs` (`ureq`), and `fetch::store`
takes a reader (question Q3). At run time the crate starts `qemu-system-x86_64` and `qemu-img`.

**Adoption map (podvm).** `podvm_windows::{run, stage, provision, cleanup, scratch_dir, scratch_of, base_cache,
RunGuard, Plan, Accel, accel_for, Ceiling, FetchError, Error}`, `fetch::store`, `agent::{AGENT_CMD, command_from_argv,
nonce}`, `dos::{stage, run, base_cache, DEFAULT_MEM_MIB, CPUS}` → `pod_guest::*`; `podvm_windows::cap_status` → podvm.

**Tests.** 60 unit tests. The `lib.rs` tests write fake emulators as `#!/bin/sh` scripts (`head`, `tr`, `sleep`) →
`need_tool!("sh")`. The `dos.rs` test expects `qemu-img` to be absent or present and checks the named failure. No test
needs KVM or QEMU.

**Overlap.** pod-core: the places of the cache. pod-services: the URLs of the base images (Validation OS, FreeDOS) are
in podvm's CLI (`docs/research/services/podvm.md`, sections A and B); they go to `pod-services` when the download moves.

## 8. Order, sizes, questions, first commits

### The dependency graph

```text
pod-core (+ clock::iso8601, files::within)
 └─ pod-hostprobe ─┬─ pod-guest
                   ├─ pod-enter ── pod-supervise
                   └─ pod-oci (+ pod-tls, pod-proxy `blocking` T-063, pod-services)
                       └─ pod-extract
                           └─ pod-rootfs (+ pod-tls, pod-proxy `blocking`)
```

### The order

1. T-055 `pod-hostprobe`. Each other crate needs `sys`.
2. T-061 `pod-guest`, T-059 `pod-enter`: they need only pod-hostprobe. Two worktrees can do them in parallel; one cargo
   build at a time.
3. T-060 `pod-supervise`, after T-059.
4. T-056 `pod-oci`: the import and the edition first; the adapt step after T-063. The `pod-core` additions (`clock`,
   `within`) are in its commits.
5. T-057 `pod-extract`, after T-056.
6. T-058 `pod-rootfs`, after T-057 and T-063.

This changes the order of `TODO/PROGRESS.md` (T-056 to T-061): the crates that do not need T-063 go first.

### The estimated sizes (lines, with tests)

| Crate | Estimate | `docs/design.md` |
| --- | ---: | ---: |
| `pod-hostprobe` | 7,900 | 4,500 |
| `pod-oci` | 4,600 | 4,500 |
| `pod-extract` | 2,900 | 3,000 |
| `pod-rootfs` | 4,600 (+60 in pod-tls) | 4,600 |
| `pod-enter` | 7,700 | 7,800 |
| `pod-supervise` | 1,800 | 2,100 |
| `pod-guest` | 3,350 | 3,400 |
| `pod-core` additions | 330 | - |

pod-hostprobe is larger than the estimate: it takes all of `probes.rs` and `sys.rs`, `machine.rs`, `supervise.rs`,
`space.rs` and `exec_probe.rs`.

### Calls made (record them in `docs/decisions.md` with the first import)

* `sys` is the module `pod_hostprobe::sys`, not a crate of its own.
* `space` and `exec_probe` go to pod-hostprobe; `clock` and `contain` go to pod-core; `Lock` goes to pod-supervise.
* docker's 125 stays in podvm; 126 and 127 go to pod-enter.
* The seven crates are Linux only (`cfg(target_os = "linux")`).
* Each name of podvm is a setting; podvm's patch gives podvm's values.
* pod-oci builds no insecure verifier; podvm gives its own configuration.
* `pod-testkit` gets `need_userns!`, `need_root!`, `need_tool!`.
* The pod dependencies of `docs/design.md`, section 4: pod-extract adds pod-oci; pod-rootfs adds pod-extract and
  pod-proxy; pod-supervise adds pod-enter; pod-guest drops pod-proxy (until Q3).

### Open questions

* **Q1 (answered).** podvm's `main` has the rename at `0249527` (PodUtils/podvm#194); `ecf61c0` is its parent, and
  `0249527` changes only `crates/podvm/src/run.rs`. The imports read `0249527`.
* **Q2 (answered, PodUtils/podvm#197).** Yes: the eight `PODVM_*` variables are settings of `pod-enter` and
  `pod-supervise` (a struct or a builder that the caller fills); podvm passes its names and values. podvm-interpose
  and the memo format stay in podvm. A crate holds no `PODVM_` name (podvm's call C8).
* **Q3 (answered, PodUtils/podvm#197).** Yes: the download of a guest base image stays in podvm's CLI for M6.
  `pod-guest` takes the mailbox, the agent, QMP, the boot plan, and the fetch with its digest check; the choice of the
  image, its terms and its source stay in podvm.

### First commit of each crate

All imports copy the files with no change, at `0249527`, and write `SOURCES` with `repo PodUtils/podvm
024952732656966ccbe087712f3e14d9ae56c237`. A file of class P is copied whole; the adapt step removes the part that
stays.

| Entry | The import commit |
| --- | --- |
| T-055 | `pod-hostprobe: import podvm crates/podvm-probe/src (all but exit, json, report, nongoals), tests/namespace*.rs, crates/podvm-image/src/{space,exec_probe}.rs, tests/space_precheck.rs at 0249527` |
| T-056 | `pod-oci: import podvm crates/podvm-image/src/{reference,digest,oci,platform,credentials,registry,error,transport,pull,layout,tls,lib}.rs, tests/registry_fixture.rs, tests/common at 0249527` (`clock.rs`, `contain.rs` into `crates/pod-core` in the same commit; `Lock` waits for T-060) |
| T-057 | `pod-extract: import podvm crates/podvm-extract/src at 0249527` |
| T-058 | `pod-rootfs: import podvm crates/podvm-complete/src at 0249527` |
| T-059 | `pod-enter: import podvm crates/podvm-enter/src, tests/tty_refusal.rs at 0249527` |
| T-060 | `pod-supervise: import podvm crates/podvm-supervise/src and the Lock of crates/podvm-image/src/store.rs at 0249527` |
| T-061 | `pod-guest: import podvm crates/podvm-windows/src at 0249527` |
