This file holds the entries of milestones M6 and M7: the crates of podvm
(`docs/design.md`, section 4, M6 and M7). Each crate follows the
procedure of `docs/design.md`, section 8.

# T-055: `pod-hostprobe`: the host probes of podvm

**Source:** `docs/design.md`, section 4 (M6); the owner, 2026-11-06.
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** partial

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

podvm gave the commit of its rename from `podbox-*` to `podvm-*` (PodUtils/podvm#194), and T-055 is done.

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
