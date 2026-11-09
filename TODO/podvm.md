This file holds the entries of milestones M6 and M7: the crates of podvm
(`docs/design.md`, section 4, M6 and M7). Each crate follows the
procedure of `docs/design.md`, section 8.

# T-055: `pod-hostprobe`: the host probes of podvm

**Source:** `docs/design.md`, section 4 (M6); the owner, 2026-11-06.
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The owner, 2026-11-06: each generic crate of podvm moves to podcrates.
Source: `PodUtils/podvm:crates/podvm-probe` (the part that is generic); `PodUtils/podnet:crates/podnet-netns/src/doctor.rs` probes the same way.

## Premise

- Read: podvm `2c03085` (its `main`, after the splits of `b917f97`):
  `crates/podvm-probe/{src,tests}` and
  `crates/podvm-image/src/{space.rs,exec_probe.rs}`,
  `tests/space_precheck.rs`; the plan of
  `docs/research/podvm/m6-imports.md`, section 1.
- Measured: the tests of the crate on this host (root, user namespaces
  permitted); the static probe on `x86_64-unknown-linux-musl`.
- Not measured here: the other targets (CI builds them); the trial in
  podvm.

## Approach

The procedure of `docs/design.md`, section 8: import, edition, adapt,
tests, README with the adoption map of podvm, the trial of podvm.

## Decision

Calls of the import, each with its reason (`docs/decisions.md`, C22):

- A failure of `space` and `exec` is a `pod_core::Failure` at a new
  `Stage::Host` (rule 9); podvm maps the class to its own errors.
- `Settings` has `child_flag`, `child_name` and `directory_setting`. The
  scratch names of the probes are not settings: each lives for one probe.
- `run_child` writes its line to a writer, and a name that is not a probe
  gives a skip line and status 2 (rule 8: no output of the crate).
- The crate reads no variable: `interp::detect` takes a function,
  `writable::candidates` takes the named directories. The probe child
  gets the environment of its parent as it is.
- The list of fds to close after a fork (`sys::close_in_children`) stays
  process state, against rule 7: a descriptor belongs to the process, and
  a child between `clone` and `execve` can read no value of a caller.
- `exec::try_exec` tries `ETXTBSY` again within a bound: a fork of another
  thread holds a copy of the descriptor that wrote the file.
- The fixtures of the namespace tests map the ids of the caller to 0, so
  they run as another user too. `need_root!` is not added: no test of the
  crate needs root.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
python3 -I scripts/check-sources.py --clone PodUtils/podvm=PODVM_CHECKOUT pod-hostprobe
cargo zigbuild --release --target x86_64-unknown-linux-musl -p static-probe
sh scripts/check-static.sh target/x86_64-unknown-linux-musl/release/static-probe
```

## Start condition

podvm gave the commit of its rename from `podbox-*` to `podvm-*` (PodUtils/podvm#194).

## Done

2026-11-06, partial. Commits `6e96c0f` (the import of 29 files of podvm
at `2c03085`, with `SOURCES`), `9f48e70` (the adaptation), `88f1e30`
(the tests and the guards `need_userns!` and `need_tool!` of
`pod-testkit`), and the commit of this Done section (README, the record).

- Imported: 26 files of `PodUtils/podvm:crates/podvm-probe` (all but `exit.rs`,
  `json.rs`, `report.rs`, `report/`, `nongoals.rs`, which stay in podvm),
  `PodUtils/podvm:crates/podvm-image/src/space.rs` (as `src/space.rs`),
  `exec_probe.rs` (as `src/exec.rs`) and `tests/space_precheck.rs`.
  podvm had split each file to 500 lines; no other split was necessary.
  The crate has 7,171 lines in 30 `.rs` files.
- Tests: 92 unit tests (90 of podvm; `banner_cell`'s test stays in
  podvm; 2 new: the setting in the message of `space`, and `ETXTBSY`),
  10 integration tests, and `tests/children.rs`, which runs the 66 rows
  (here: 54 ok, 9 denied, 3 skipped: the two qemu legs and the squash
  fixture). `inode_pressure_on_a_fresh_tmpfs_refuses_a_modest_ask` takes
  its own skip arm here (a roomy tmpfs). pod-testkit: 5 guard tests.
- Checks: fmt, clippy of the workspace, `cargo todo check`,
  `check-size.py`, `check-sources.py` (0 changes), the static probe on
  `x86_64-unknown-linux-musl`.

Remains:

- The adoption trial of podvm: the patch `pod-hostprobe.patch` of
  `adoption/` (podvm) replaces the moved modules of `podvm-probe` and
  `space.rs` and `exec_probe.rs` of `podvm-image` with the crate (README,
  Adoption map); gives `Settings { child_flag: "--probe-child",
  child_name: "podvm", directory_setting: Some("$PODVM_STORE") }`; reads
  `PODVM_STORE` and `writable::VARIABLES` for `writable::candidates`;
  passes `|v| std::env::var_os(v).is_some()` to `interp::detect`; maps
  `pod_core::Failure` of `space` and `exec` to `Error::NoSpace` and
  `Error::Store`; prints the line of `run_child` to stdout; keeps
  `BANNER_ROWS`, `banner_cell` and its test with `report`; and changes
  the paths of the moved tests in `PodUtils/podvm:crates/podvm-gate`.
  Then the release.
- `sys::statfs` calls `statfs` with a buffer of `struct statfs64`. On a
  32-bit target (i686, arm) the kernel writes the narrow `struct statfs`
  there, so the values are wrong; the fix calls `statfs64` with the size
  on those targets. It came from podvm as it is.

2026-11-06, partial. The adoption trial of podvm at `b32b19c`
(`adoption/podvm/pod-hostprobe.patch`: 50 files, 253 lines in, 8,024
out; 39 files of `crates/`). podvm removes 26 files: 24 of
`PodUtils/podvm:crates/podvm-probe` (each but `lib.rs` and `select.rs` of `src/` and of
`src/sys/`, `src/probes/`, `src/select/tests.rs`, and `tests/`) and
`PodUtils/podvm:crates/podvm-image/{src/exec_probe.rs,tests/space_precheck.rs}`, whose
new `exec_probe.rs` is a call. `podvm-probe` keeps `exit`, `json`,
`report`, `nongoals`, and re-exports `binfmt`, `child`, `identity`,
`interp`, `machine`, `mounts`, `probes`, `supervise`, `sys`, `verdict`,
`writable`, `Findings` and `Settings` of the crate, so each other crate
of podvm keeps its `podvm_probe::X` paths. Its `select.rs` re-exports
`pod_hostprobe::select` and keeps `BANNER_ROWS`, `banner_cell` and the
test of the banner cell. Its `lib.rs` gives `settings()`
(`--probe-child`, `podvm`, `Some("$PODVM_STORE")`), `candidates()`
(`PODVM_STORE`, then `writable::VARIABLES`), `interpreter()` (the
variables of the process), `confinement_key(&id)`, `run()` and
`run_child(name)` (a name that is not a probe keeps podvm's message on
stderr and 2; else the line goes to stdout). `podvm-image`'s `space.rs`
and `exec_probe.rs` call the crate with podvm's settings and map a
`Failure` to podvm's errors: an I/O cause to `Io` (its kind and path),
`Class::Unavailable` to `NoSpace`, the rest to `Store`. Each podcrates
tag moves to `v0.6.0`. Changed in podvm besides: the plant pins of the
three moved test files (`podvm-gate`), the `Rung` line of
`document-state`, the rows of `PodUtils/podvm:docs/runtime-state.md` (`linux-raw-sys`
leaves, `pod-hostprobe` comes), and 58 citations of the moved files in
`TODO/`, `experiments/260-multiarch.sh` and
`PodUtils/podvm:vendor/userland-execve`,
which name `pod_hostprobe::MODULE` (the record gate refuses a path that
git does not track). No test or fixture of podvm pins `podvm-probe/1`.
New tests in podvm: the settings, the order of the candidates, the
mapping of each class and of an I/O failure; the other tests of the moved
files are the crate's. The `.run` runs the tests of each user of
`podvm-probe`, store_gates (the exec probe and the noexec refusal that
names `PODVM_STORE`), the probe children through the binary (`podvm
probe --rows`: 66 rows, 54 ok, 9 denied, 3 skipped, none "the child for";
`--probe-child` speaks `pod-hostprobe/1`), clippy, rustfmt, the
runtime-state rows, the record gate, the toolchains and the static
`node` and `podvm`. It skips `abi::tests::resolve_past_a_denied_listing`
of podvm-enter: it runs the test binary as nobody, which cannot read a
target directory under a home of mode 0700. Each step passed. podvm took the crate in podvm `8899c2e` (PodUtils/podvm@T-1681, PodUtils/podvm#210) at
`v0.6.0` (release 42261756998): 26 files of `podvm-probe` left podvm, and
the `SOURCES` of the crate has its `taken` line, so
`scripts/check-sources.py` reads no change for `pod-hostprobe`. `v0.7.0`
gives podvm's glue `Failure::io_cause` and `Class::NotFound` (T-081).


# T-056: `pod-oci`: the OCI registry client of podvm

**Source:** `docs/design.md`, section 4 (M6); the owner, 2026-11-06.
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

The owner, 2026-11-06: each generic crate of podvm moves to podcrates.
Source: `PodUtils/podvm:crates/podvm-image` (the part that is generic; the content store stays).

## Premise

- Not read yet: the source at the commit of podvm's rename.

## Approach

The procedure of `docs/design.md`, section 8: import, edition, adapt,
tests, README with the adoption map of podvm, the trial of podvm.

## Prove

```sh
cargo test -p pod-oci
```

## Start condition

podvm gave the commit of its rename from `podbox-*` to `podvm-*` (PodUtils/podvm#194), and T-055 is done.

# T-057: `pod-extract`: the rootless apply of a layer

**Source:** `docs/design.md`, section 4 (M6); the owner, 2026-11-06.
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

The owner, 2026-11-06: each generic crate of podvm moves to podcrates.
Source: `PodUtils/podvm:crates/podvm-extract`.

## Premise

- Not read yet: the source at the commit of podvm's rename.

## Approach

The procedure of `docs/design.md`, section 8: import, edition, adapt,
tests, README with the adoption map of podvm, the trial of podvm.

## Prove

```sh
cargo test -p pod-extract
```

## Start condition

podvm gave the commit of its rename from `podbox-*` to `podvm-*` (PodUtils/podvm#194), and T-055 is done.

# T-058: `pod-rootfs`: the completion of a root file system

**Source:** `docs/design.md`, section 4 (M6); the owner, 2026-11-06.
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

The owner, 2026-11-06: each generic crate of podvm moves to podcrates.
Source: `PodUtils/podvm:crates/podvm-complete`.

## Premise

- Not read yet: the source at the commit of podvm's rename.

## Approach

The procedure of `docs/design.md`, section 8: import, edition, adapt,
tests, README with the adoption map of podvm, the trial of podvm.

## Prove

```sh
cargo test -p pod-rootfs
```

## Start condition

podvm gave the commit of its rename from `podbox-*` to `podvm-*` (PodUtils/podvm#194), and T-055 is done.

# T-059: `pod-enter`: the entry of a payload

**Source:** `docs/design.md`, section 4 (M6); the owner, 2026-11-06.
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

The owner, 2026-11-06: each generic crate of podvm moves to podcrates.
Source: `PodUtils/podvm:crates/podvm-enter`.

## Premise

- Not read yet: the source at the commit of podvm's rename.

## Approach

The procedure of `docs/design.md`, section 8: import, edition, adapt,
tests, README with the adoption map of podvm, the trial of podvm.

## Prove

```sh
cargo test -p pod-enter
```

## Start condition

podvm gave the commit of its rename from `podbox-*` to `podvm-*` (PodUtils/podvm#194), and T-055 is done.

# T-060: `pod-supervise`: detached payloads

**Source:** `docs/design.md`, section 4 (M6); the owner, 2026-11-06.
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

The owner, 2026-11-06: each generic crate of podvm moves to podcrates.
Source: `PodUtils/podvm:crates/podvm-supervise`.

## Premise

- Not read yet: the source at the commit of podvm's rename.

## Approach

The procedure of `docs/design.md`, section 8: import, edition, adapt,
tests, README with the adoption map of podvm, the trial of podvm.

## Prove

```sh
cargo test -p pod-supervise
```

## Start condition

podvm gave the commit of its rename from `podbox-*` to `podvm-*` (PodUtils/podvm#194), T-055 is done, and podvm fixed its #215 (`stop -t 0`) in `podvm-supervise`.

Met on 2026-11-09: podvm fixed #215 in `063a1ed` (PodUtils/podvm@T-1691,
PodUtils/podvm#218). The import reads podvm `main` at `063a1ed` or later.
# T-061: `pod-guest`: a disposable QEMU guest

**Source:** `docs/design.md`, section 4 (M6); the owner, 2026-11-06.
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

The owner, 2026-11-06: each generic crate of podvm moves to podcrates.
Source: `PodUtils/podvm:crates/podvm-windows`.

## Premise

- Not read yet: the source at the commit of podvm's rename.

## Approach

The procedure of `docs/design.md`, section 8: import, edition, adapt,
tests, README with the adoption map of podvm, the trial of podvm.

## Prove

```sh
cargo test -p pod-guest
```

## Start condition

podvm gave the commit of its rename from `podbox-*` to `podvm-*` (PodUtils/podvm#194).

# T-062: The studies of the crates of the widened scope of podvm

**Source:** `docs/design.md`, section 4 (M7); the owner, 2026-11-06.
**Category:** research
**Milestone:** M7
**Priority:** P3
**Effort:** L
**Status:** open

## Problem

The owner: "podvm will make everything that sand-net does, but without
root, in the environment we target". `pod-microvm`, `pod-sud`,
`pod-confine`, `pod-netlab`, `pod-pcap` and `pod-sandprobe` need a study
each: the API, the C on each target, the static link, the sources.

## Premise

- Read: the topics PodUtils/podvm#166 to PodUtils/podvm#189 and their
  index PodUtils/podvm#193.

## Approach

One study for each crate, as the studies of M4; each gives the entry of
its crate.

## Prove

```sh
cargo todo check
```

## Start condition

The crates of M6 are released.

# T-072: `pod-hostprobe`: `sys::statfs` gives wrong values on 32-bit targets

**Source:** the import of `pod-hostprobe` (T-055, `cbb0525`).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`sys::statfs` calls the system call `statfs` with a buffer of the shape of
`statfs64`. On a 32-bit target (i686, arm) the kernel writes the narrow
`statfs` there, so the sizes and the counts that the probes read are wrong.
The fault came with the source (`PodUtils/podvm:crates/podvm-probe/src/sys/fs.rs`).

## Premise

- Read: `crates/pod-hostprobe/src/sys/fs.rs` and `sys/arch.rs`.

## Approach

On the 32-bit targets, call `statfs64` with the size of the buffer, as the
kernel asks; a test checks the block size and the counts of a known
directory against `df`'s view of the same file system. podvm gets the fix
as a COORD issue (the file is frozen in podvm).

## Prove

```sh
cargo test -p pod-hostprobe --lib sys
cargo zigbuild --tests -p pod-hostprobe --lib --target i686-unknown-linux-musl  # then run the test binary
```

## Done

2026-11-06. On a 32-bit target `sys::statfs` calls `statfs64` with the size
of the buffer (`SYS_STATFS64`); a 64-bit target keeps `statfs`, whose struct
is the wide one. `statfs_reads_the_wide_struct_on_each_width`, run as an
i686 musl binary on this host: without the change it failed with
`f_ffree 17729625001984, f_files 1097445294077`; with it, it passes, and on
x86_64 too. clippy is clean for x86_64, i686 and armv7.

# T-080: `pod-hostprobe`: a failed system call reads as a success on a 32-bit target

**Source:** the nightly run 42242987001 at `68a714b` (job `cross (i686-unknown-linux-musl)`).
**Category:** defect
**Milestone:** M6
**Priority:** P0
**Effort:** S
**Status:** done

## Problem

`a_negative_return_decodes_as_the_errno_and_not_as_a_value` failed on
i686: `close(-1)` gave `Ok(4294967287)`, not `Err(EBADF)`. Each failed
system call read as a success, so the tests of `space`, `writable` and
`tests/children.rs` failed too. The fault came with the source
(`PodUtils/podvm:crates/podvm-probe/src/sys/raw.rs`).

## Premise

- Read: `syscall6` casts the `usize` of `syscalls::raw::syscall6` to `i64`;
  on a 32-bit target that cast keeps the 32-bit word positive, and `split`
  finds no value in `-4095..=-1`.

## Approach

The cast goes through `isize`, which keeps the sign of the 32-bit word.

## Prove

```sh
cargo zigbuild --tests -p pod-hostprobe --target i686-unknown-linux-musl  # then run each test binary
```

## Done

2026-11-06. On this host, as i686 musl binaries: the unit tests 93
passed, `space_precheck` 2, `namespace` 3, `namespace_base` 5, and
`children` (66 rows: 53 ok, 10 denied, 3 skipped); before the change the
nightly's job failed as above. clippy is clean for x86_64 and i686. podvm
has the same fault in its frozen `PodUtils/podvm:crates/podvm-probe/src/sys/raw.rs`.


# T-107: `pod-hostprobe`: a JSON report with a published schema (GitHub #35)

**Source:** GitHub #35 (A-V-10, A-X-25)
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

A-V-10 and A-X-25 need a stable JSON of the host probe: the `caps` of cfb (Landlock, seccomp, userns,
memfd, SUD, `openat2`, `/dev/kvm`, `/dev/fuse`, `/dev/net/tun`, cgroup).

## Premise

- Read: `crates/pod-hostprobe/src/` and the asks.
- Not measured: the JSON.

## Approach

Serde types behind a feature. A `schema` field. A JSON Schema file in `schemas/`. The value `unknown` where
a filter answers `EPERM`.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
```

# T-108: `pod-confine`: Landlock, a seccomp deny filter, no_new_privs and rlimits from one profile (GitHub #35)

**Source:** GitHub #35 (A-V-01)
**Category:** feature
**Milestone:** M7
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

A-V-01 is P0 for cfb. `pod-confine` waits for the study of T-062 after M6. The study and the crate go in
one entry.

## Premise

- Read: the ask of A-V-01; `PodUtils/podnet:crates/podnet-netns/src/seccomp.rs` builds a seccomp filter
  (user notification, not a deny list).
- Not measured: Landlock on the targets.

## Approach

The study, then the crate: Landlock ABI 3 or newer for file rules and ABI 4 or newer for TCP rules, a
seccomp deny filter of the list of A-V-01, `no_new_privs`, rlimits, one profile file. The crate refuses by
name where Landlock is absent. The filter builder of podnet is a source.

## Prove

```sh
cargo test -p pod-confine --all-features
```

# T-109: Research: the hold of each connect of a payload, from the `podnet-netns` of podnet, for podvm (GitHub #35, #36)

**Source:** GitHub #35 (A-V-02) and GitHub #36 (G-04)
**Category:** research
**Milestone:** M7
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

podvm (A-V-02) and podrunner (G-04) must hold each connect of a payload. podnet has two backends that do
it. `docs/design.md` keeps `podnet-netns` in podnet.

## Premise

- Read: the asks. Not read: `PodUtils/podnet:crates/podnet-netns` (9,447 lines).

## Approach

Read `PodUtils/podnet:crates/podnet-netns`. Decide one crate (`pod-hold`) or a part of `pod-sud`. Write the
entry of the crate. Open a COORD issue to podnet for its view.

podnet's view (PodUtils/podnet#414, 2026-11-09): podnet agrees to share the
hold through podcrates, on two terms. The import comes after podnet's own
split of the pod backends (PodUtils/podnet@T-040) and its fixes of the
seccomp backend (PodUtils/podnet@T-142 to PodUtils/podnet@T-158); podnet
writes a COORD issue when that point comes. The crate takes the generic
core only: the loop of the seccomp user notification, the read of the
memory of the child, the table of descriptors and the reply, and the netns
hold of a `connect`. The pod CLI, the rules of upstreams and the routes stay
in `podnet-netns`. The seam is the ioctl of the reply; a memfd can stand in
for the memory of the child in tests.

## Prove

```sh
cargo todo check
```

# T-119: `pod-netlab`: the CI matrix of restricted hosts (GitHub #35)

**Source:** GitHub #35 (A-X-22)
**Category:** feature
**Milestone:** M7
**Priority:** P3
**Effort:** L
**Status:** open

## Problem

A-X-22 asks one CI matrix: IPv6 only (NAT64, DNS64), HTTP proxy only, no DNS, no UDP, aarch64. Each cell
gives a smoke test or one `not supported` code.

## Premise

- Read: the ask. podnet covers several cells.
- Not measured: a cell.

## Approach

The study of `pod-netlab` in T-062 first. Then the cells.

## Prove

```sh
cargo test -p pod-netlab --all-features
```

## Start condition

The study of `pod-netlab` in T-062 is done.
