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
(the patch pod-hostprobe.patch of adoption/podvm, in git at `5535a27`: 50 files, 253 lines in, 8,024
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
**Status:** partial

## Problem

The owner, 2026-11-06: each generic crate of podvm moves to podcrates.
Source: `PodUtils/podvm:crates/podvm-image` (the part that is generic; the content store stays).

## Premise

- Read: `PodUtils/podvm:crates/podvm-image` at podvm `main` `6cd3442`
  (`docs/research/podvm/m6-imports.md`, "Current files").

## Approach

The procedure of `docs/design.md`, section 8: import, edition, adapt,
tests, README with the adoption map of podvm, the trial of podvm.

## Prove

```sh
cargo test -p pod-oci
```

## Start condition

podvm gave the commit of its rename from `podbox-*` to `podvm-*` (PodUtils/podvm#194), and T-055 is done.

## Done

2026-11-10, partial. Commits `94b44ea` (the import of 24 files of podvm
at `6cd3442`, with `SOURCES`), `7bb8637` (the adaptation and the tests),
`3a7193f` (README, C38), `dde816c` (the trial, the record) and `6e45a5f`
(the trial on podvm `cb57d4b`).

- Imported: `reference`, `digest`, `oci` (as `spec`), `platform`,
  `credentials` and its tests, `registry` and `registry/{http,wire,tests}`,
  `error`, `transport`, `pull` and `pull/{run,tests}`, `layout`, `tls` (as
  `trust`), `net`, `lib`, `tests/registry_fixture.rs`,
  `tests/proxy_pull.rs` and `tests/common` of
  `PodUtils/podvm:crates/podvm-image`. Of `error`, `transport`, `pull`,
  `pull/run`, `layout`, `tls`, `net` and `lib` a part stays in podvm;
  `layout/tests.rs` stays whole. The crate has 4,990 lines in 25 `.rs`
  files, each under 500. pod-core gets `Stage::Registry`.
- Tests: 89 unit tests (the 78 of the moved parts, each with its name,
  and 11 new) and 6 integration tests (`registry_fixture` 1, `proxy_pull`
  2, `client` 3, new). The same 95 pass as i686 musl binaries.
- The trial of podvm at `cb57d4b`, after its take of `pod-supervise`
  (`adoption/podvm/pod-oci.patch`: 33 files, 260 lines in, 4,046 out;
  the imported files are the same as at `6cd3442`): `podvm-image` keeps
  the store and podvm's glue (`registry::client`, `transport::resolve`,
  `platform::wanted`, the credential paths, `tls` over `pod_oci::trust`,
  `Store` as `Blobs`, `Error::Image` with `From<Failure>`); 7 files leave
  podvm; the citations of the moved files name `pod-oci`; each tag moves
  to `v0.10.0`. The patch applies alone. It shares the manifests,
  `PodUtils/podvm:docs/runtime-state.md` and `PodUtils/podvm:TODO/gate.md`
  with the patches of `pod-guest` and `pod-enter`, and
  `PodUtils/podvm:TODO/cli.md` and two files of
  `PodUtils/podvm:crates/podvm/src` with `pod-enter`; after both, a 3-way
  apply conflicts only in the tag lines, the `pod-*` rows and two
  citation lines (`pod-oci.run` names each). Each step of `sh
  scripts/adoption.sh podvm pod-oci` passed: podvm-image 88 tests,
  podvm-extract 47, podvm-complete 53, podvm-supervise 8,
  `podvm --bin` 255, `store_gates` 4, `qol` 8, `curated_refusals` 6,
  clippy, rustfmt, the runtime-state rows, podvm's record gate, the
  toolchains, and the static `podvm` for x86_64 musl.
- Checks: `sh scripts/agent/check.sh` (fmt, clippy, doc, record, size,
  paths, static, test), `check-sources.py` (23 files, 0 changes),
  `check-probe.py`, `secrets_like.py` on the patch (no line).

Remains:

- The release `v0.10.0`, then podvm's take of the patch, after the
  patches of `pod-guest` and `pod-enter` (a merge of the tag lines, the
  `pod-*` rows and the citations of `PodUtils/podvm@T-1684`). Then the
  `taken` line in `SOURCES`.
- A live pull from a public registry, in a live workflow.

# T-057: `pod-extract`: the rootless apply of a layer

**Source:** `docs/design.md`, section 4 (M6); the owner, 2026-11-06.
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** partial

## Problem

The owner, 2026-11-06: each generic crate of podvm moves to podcrates.
Source: `PodUtils/podvm:crates/podvm-extract`.

## Premise

- Read: podvm `e8e5beb` (its `main` on the day of the import):
  `PodUtils/podvm:crates/podvm-extract/src` (12 files); its callers in
  `PodUtils/podvm:crates/podvm/src` and
  `PodUtils/podvm:crates/podvm-complete/src/write`; the plan of
  `docs/research/podvm/m6-imports.md`, section 3 and "Current files".
  podvm's `main` then moved to `2616bd7` (the takes of `pod-guest` and
  `pod-enter`) with no change in `PodUtils/podvm:crates/podvm-extract`; the
  trial reads `2616bd7`.
- Measured: the tests of the crate on this host as x86_64 and i686 musl
  binaries, and as aarch64, armv7 and powerpc64le musl binaries under QEMU
  user mode (no `openat2` there: each extraction test runs the
  `O_NOFOLLOW` walk); the trial of podvm; the static `podvm` for
  `x86_64-unknown-linux-musl`.
- Found: a whiteout `.wh...` at the root of a layer removed the entries of
  the directory above the destination in podvm's code (its name `..` had no
  check), and `../.wh.x` escaped through the walk.

## Approach

The procedure of `docs/design.md`, section 8: import, edition, adapt,
tests, README with the adoption map of podvm, the trial of podvm.

## Decision

Calls of the import, each with its reason (`docs/decisions.md`, C39):

- The cut: the crate takes all of `PodUtils/podvm:crates/podvm-extract` but
  `paths(store, digest)` and the glue over the `Store` of podvm-image.
- The caller gives each layer as a `Layer` (digest, media type, size, blob
  path) and the place as a `Place`, so the crate takes no OCI type and does
  not depend on `pod-oci`.
- `Settings`: the settings of `pod-hostprobe` for the space message, the
  headroom, the `Report`; the line of each layer is the event
  `extract.layer`.
- Failures are `pod_core::Failure` at `Stage::Host`, with the typed causes
  `Refused` and `Mismatch`.
- The `openat2` probe keeps one answer for each process.
- A whiteout gets the lexical check of each entry; a whiteout of `.` or
  `..` refuses the layer.

## Prove

```sh
cargo test -p pod-extract
python3 -I scripts/check-sources.py --clone PodUtils/podvm=PODVM_CHECKOUT pod-extract
sh scripts/adoption.sh podvm pod-extract
cargo zigbuild --release --target x86_64-unknown-linux-musl -p static-probe
sh scripts/check-static.sh target/x86_64-unknown-linux-musl/release/static-probe
```

## Start condition

podvm gave the commit of its rename from `podbox-*` to `podvm-*` (PodUtils/podvm#194), and T-055 is done.

## Done

2026-11-10, partial. Commits `f940f32` (the import of 12 files of podvm at
`e8e5beb`, with `SOURCES`), `98a70ed` (the adaptation and the tests),
`a40d45a` (the check of whiteouts), `eede16e` (README, the static probe,
C39) and the commit of the trial and this record.

- Imported: the 12 files of `PodUtils/podvm:crates/podvm-extract/src`. Of
  `lib.rs` a part stays in podvm (`paths` and the glue over its `Store`).
  The crate has 2,771 lines in 13 `.rs` files, each under 500.
  `pod_hostprobe::sys` gets `EIO`, `E2BIG`, `ENOTDIR`, `EISDIR` and
  `ELOOP`.
- Tests: 50 unit tests: each of the 47 of podvm with its name, and 3 new
  (`each_layer_gives_one_event_to_the_caller`,
  `a_whiteout_of_dotdot_removes_nothing_outside_the_destination`,
  `a_whiteout_under_dotdot_is_refused`; the last two fail on the code
  before `a40d45a`). The same 50 pass as i686 musl binaries, and under
  QEMU as aarch64, armv7 and powerpc64le musl binaries (the `openat2` test
  skips under the emulator).
- The trial of podvm at `2616bd7` (`adoption/podvm/pod-extract.patch`: 27
  files, 127 lines in, 2,977 out; `pod-extract.commit`):
  `PodUtils/podvm:crates/podvm-extract` keeps podvm's glue (`paths`,
  `extract` over the `Store`, `is_extracted`, `remove_extracted`, an
  `Error` of the extraction and of the store) and re-exports the crate; 10
  files leave podvm; the citations of the moved files name `pod-extract`;
  each tag moves to `v0.10.0`. The patch applies alone; after
  `pod-oci.patch`, a 3-way apply conflicts in four files
  (`pod-extract.run` names each), and the workspace with both passes
  clippy, the tests of podvm-extract and podvm-complete and podvm's
  path-safety experiment. Each step of `sh scripts/adoption.sh podvm
  pod-extract` passed: podvm-extract 1 test, podvm-complete 53,
  podvm-enter, podvm-windows and podvm-supervise 15, `podvm --bin` 266,
  `store_gates` 4, `detached_stdio` 3, `curated_refusals` 6, clippy,
  rustfmt, the path-safety experiment (A to F), the runtime-state rows,
  podvm's record gate, the toolchains, and the static `podvm` for x86_64
  musl.
- Checks: `sh scripts/agent/check.sh` (fmt, clippy, doc, record, size,
  paths, static, test), `check-sources.py` (12 files, 0 changes),
  `secrets_like.py` on the patch (no line).

Remains:

- The release `v0.10.0`, then podvm's take of the patch, after
  `pod-oci.patch`. Then the `taken` line in `SOURCES`.
- podvm's `.wh...` escape is in its own `PodUtils/podvm:crates/podvm-extract`
  until the take: the take fixes it.

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
**Status:** done

## Problem

The owner, 2026-11-06: each generic crate of podvm moves to podcrates.
Source: `PodUtils/podvm:crates/podvm-enter`.

## Premise

- Read: podvm `6cd3442` (its `main`): `PodUtils/podvm:crates/podvm-enter/src`
  (27 files) and `tests/tty_refusal.rs`, with the memfd probe that
  executes a trampoline and the link set of the interposer (podvm's
  commits after `0249527`); its callers in `PodUtils/podvm:crates/podvm/src`
  and `PodUtils/podvm:crates/podvm-supervise/src/launcher`; the plan of
  `docs/research/podvm/m6-imports.md`, section 5 and "Current files".
  `PodUtils/podvm:crates/podvm-enter` changed between `27ae993` (the
  commit of `adoption/podvm/commit`) and `6cd3442`, so the trial read
  `6cd3442`. The trial reads podvm `main` at `cb57d4b` now (T-136).
- Measured: the tests of the crate on this host as x86_64 and as i686
  musl binaries; the trial of podvm at `6cd3442`; the static `podvm` for
  `x86_64-unknown-linux-musl`.
- Not measured here: an entry over a real FUSE session (no host here
  holds `/dev/fuse` and grants the mount), the cover of ptmx (this host
  refuses the maps of the user namespace that the test needs).

## Approach

The procedure of `docs/design.md`, section 8: import, edition, adapt,
tests, README with the adoption map of podvm, the trial of podvm.

## Decision

Calls of the import, each with its reason (`docs/decisions.md`, C37):

- The cut: the crate takes all of `PodUtils/podvm:crates/podvm-enter` but the
  re-export of docker's codes of `podvm-probe`. podvm keeps 125 and
  `GUEST_EXE_VAR`, which the crate does not use.
- `Names`: the mode request and the active mode variables, the cache
  variable, the variable of the path maps, the two device variables, the
  cache marker, the memfd label and the word at the start of a line.
  `Plan` carries them; each function that writes or names a variable
  takes `&Names`. The crate reads no variable.
- `Error::Runtime(pod_core::Failure)` at `Stage::Host`, with a class from
  the errno; `CannotInvoke` and `NotFound` stay variants, with 126 and 127
  in `status`; `code_or` takes the caller's code of a runtime failure.
- The memfd probe keeps one answer for each process.
- The ELF reader checks each offset, for a hostile image and for a
  32-bit target.
- `PodUtils/podvm:crates/podvm-enter` stays in podvm as the glue:
  `NAMES`, `runtime`, the trait `ExitCode`, and each function that takes
  the names with its podvm signature.

## Prove

```sh
cargo test -p pod-enter
python3 -I scripts/check-sources.py --clone PodUtils/podvm=PODVM_CHECKOUT pod-enter
sh scripts/adoption.sh podvm pod-enter
cargo zigbuild --release --target x86_64-unknown-linux-musl -p static-probe
sh scripts/check-static.sh target/x86_64-unknown-linux-musl/release/static-probe
```

## Start condition

podvm gave the commit of its rename from `podbox-*` to `podvm-*` (PodUtils/podvm#194), and T-055 is done.

## Done

2026-11-09, partial. Commits `bccf3f7` (the import of 28 files of podvm
at `6cd3442`, with `SOURCES`), `ef7c76f` (the adaptation and the tests),
`c8ce099` (README, C37), `1fe6114` (the trial, the record) and `2a3c82a`
(the doc links).

- Imported: the 27 files of `PodUtils/podvm:crates/podvm-enter/src` and
  `tests/tty_refusal.rs`. Of `lib.rs` a part stays in podvm (the
  re-export of docker's codes). The crate has 8,025 lines in 35 `.rs`
  files, each under 500.
- Tests: 136 unit tests and 2 integration tests: each of the 137 of
  podvm with its name, and 1 new (`a_hostile_offset_is_a_refusal_and_not_a_panic`:
  on the old code it fails with "capacity overflow"). The same pass as
  i686 musl binaries (135 unit tests; the trampoline test is for x86_64
  and aarch64). `need_root!` joins `pod-testkit`.
- The trial of podvm at `6cd3442` (`pod-enter.patch` of `v0.9.0`: 52
  files, 258 lines in, 9,101 out; it does not need the patches of
  `pod-supervise` and `pod-guest`): `PodUtils/podvm:crates/podvm-enter`
  keeps podvm's glue and re-exports the crate; 27 files leave podvm; each
  `Plan` gets `names: NAMES`; a runtime error of podvm is
  `podvm_enter::runtime`; the gate loses the pin of `tty_refusal`;
  document-state names `Mode` in `pod-enter`; the citations of the moved
  files name `pod-enter`; each tag moves to `v0.9.0`. Each step of `sh scripts/adoption.sh
  podvm pod-enter` passed: podvm-enter 2 tests, podvm-supervise 15,
  `podvm --bin` 255, `store_gates` 4, `detached_stdio` 3, `qol` 8,
  `curated_refusals` 6, clippy, rustfmt, the runtime-state rows, podvm's
  record gate (305 entries), the toolchains, and the static `podvm` for
  x86_64 musl.
- Checks: `sh scripts/agent/check.sh` (fmt, clippy, doc, record, size,
  static, test), `check-sources.py` (28 files, 0 changes),
  `secrets_like.py` on the patch (no line).
- Found: `SYS_GETUID` and `SYS_GETGID` of `pod-hostprobe` are the 16-bit
  calls on i686 and arm; the FUSE mount of this crate and
  `identity::read` use them (a task outside T-059).

Remains:

- The release `v0.9.0`, then podvm's take of the patch. After the
  patches of `pod-supervise` and `pod-guest`, the tag lines and the
  `pod-*` rows of `PodUtils/podvm:docs/runtime-state.md` of this patch
  need a merge.
  Then the `taken` line in `SOURCES`.
- An entry over a real FUSE session, on a host with `/dev/fuse` and a
  granted mount.

podvm took the crate in podvm `423e817` (PodUtils/podvm#220): it applied
`pod-guest.patch` and then `pod-enter.patch` of `v0.9.0` at podvm `e37decf`
with `git apply --3way`; the code applied clean, three conflicts of
podvm's record were joined by hand; each line of the `.run` exited 0. The
`SOURCES` of the crate has `taken 423e817`; `check-sources.py` reads 28
files, 0 changes. podvm's notes on the crate are T-137.

# T-060: `pod-supervise`: detached payloads

**Source:** `docs/design.md`, section 4 (M6); the owner, 2026-11-06.
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The owner, 2026-11-06: each generic crate of podvm moves to podcrates.
Source: `PodUtils/podvm:crates/podvm-supervise`.

## Premise

- Read: podvm `27ae993` (its `main`, with the fix of PodUtils/podvm#215 of
  `063a1ed`): `PodUtils/podvm:crates/podvm-supervise/src` (all but `table/memo.rs`) and the
  `Lock` of `PodUtils/podvm:crates/podvm-image/src/store/files.rs`; the plan of
  `docs/research/podvm/m6-imports.md`, section 6 and "Current files".
- The plan names `pod-enter` as a dependency (the plan and the spawn of
  the launcher). The import does not need it: the entry of the payload is
  a function of the caller, so T-060 does not wait for T-059.
- Measured: the tests of the crate on this host; the trial of podvm at
  `27ae993`; the static `podvm` for `x86_64-unknown-linux-musl`.
- Not measured here: the live clauses of podvm's lifecycle tests (no
  image could be pulled: HTTP 429 of `public.ecr.aws`); CI runs them.

## Approach

The procedure of `docs/design.md`, section 8: import, edition, adapt,
tests, README with the adoption map of podvm, the trial of podvm.

## Decision

Calls of the import, each with its reason (`docs/decisions.md`, C33):

- `launcher::start` takes the entry of the caller: `enter(&Container<X>,
  Launch { pass, log }) -> Result<Payload { pid, keep }, String>` runs in
  the launcher after the lock, the control socket and the stdio of the
  payload, and before `running`. podvm keeps its probe, its plan of
  `podvm-enter`, the image hold, the devices and the memo there. No
  dependency on `pod-enter`.
- A record is `Container<X>`: the fields of the crate, and the fields of
  the caller flattened into the same JSON object (`of`, with `Deref`), so
  that podvm's table file keeps its form. `table::Any` keeps each key.
- `Settings`: `root`, `table_lock` (podvm: the store's own lock, invariant
  I1), `table_key` (podvm: `podvm_containers`), `name_prefix`,
  `list_command`, `failed_code` (docker's 125). The crate reads no
  variable and prints nothing.
- `Lock` of podvm-image moves to `pod_supervise::lock`; `try_acquire_noting`
  gives podvm's PodUtils/podvm@T-0215 instrument its hook at the refusal. The words
  "already holds N locks" stay: an experiment of podvm counts them.
- Each failure is a `pod_core::Failure` at `Stage::Host`; podvm keeps its
  `Error(String)` with `From<Failure>`.
- `tests/lifecycle.rs` has no libtest: the launcher forks from the test
  process, which must have one thread.
- `pod_core::clock::rfc3339` writes the times (the form of podvm's
  `clock::now`).

## Prove

```sh
cargo test -p pod-supervise
python3 -I scripts/check-sources.py --clone PodUtils/podvm=PODVM_CHECKOUT pod-supervise
sh scripts/adoption.sh podvm pod-supervise
cargo zigbuild --release --target x86_64-unknown-linux-musl -p static-probe
sh scripts/check-static.sh target/x86_64-unknown-linux-musl/release/static-probe
```

## Start condition

podvm gave the commit of its rename from `podbox-*` to `podvm-*` (PodUtils/podvm#194), T-055 is done, and podvm fixed its #215 (`stop -t 0`) in `podvm-supervise`.

Met on 2026-11-09: podvm fixed #215 in `063a1ed` (PodUtils/podvm@T-1691,
PodUtils/podvm#218). The import reads podvm `main` at `063a1ed` or later.

## Done

2026-11-09, partial. Commits `8b46a62` (the import of 8 files of podvm at
`27ae993`, with `SOURCES`), `16eb5a1` (the adaptation and the tests),
`2c3863c` (README, C33), and the commit of this Done section (the trial,
(the doc links).

- Imported: 7 files of `PodUtils/podvm:crates/podvm-supervise/src` (all but
  `table/memo.rs`, which stays in podvm) and
  `PodUtils/podvm:crates/podvm-image/src/store/files.rs` (as `src/lock.rs`;
  only `Lock` stays). The crate has 2,292 lines in 11 `.rs` files, each
  under 500.
- Tests: 23 unit tests (13 of podvm with their names; 10 new: the lock,
  the table key, the reconcile, the containment, the verbs,
  `exit_status`), and `tests/lifecycle.rs` (5 steps with real payloads:
  `stop -t 0`, an exit code, a signal, a killed launcher, a failed entry;
  30 runs in a loop passed). The `stop -t 0` step fails on the wait of
  podvm before #215. pod-core: 1 new test (`rfc3339`).
- The trial of podvm at `27ae993` (the patch `pod-supervise.patch`:
  22 files, 353 lines in, 1,718 out): `podvm-supervise` keeps podvm's glue
  (`settings`, `Detail`, the verbs with a `&Store`, the entry of the
  launcher, the memo); `launcher/supervise.rs` leaves podvm; `podvm-image`
  re-exports `pod_supervise::lock::Lock`; each tag moves to `v0.8.0`; six
  citations of podvm's record follow the moved lines. Each step of `sh
  scripts/adoption.sh podvm pod-supervise` passed: podvm-supervise 8 tests,
  podvm-image 146 unit and 15 integration tests, `podvm --bin` 251,
  `store_gates` 4, `detached_stdio` 3, `qol` 8 (their live clauses skipped
  here: HTTP 429 of `public.ecr.aws`), clippy, rustfmt, the runtime-state
  rows, podvm's record gate, the toolchains, and the static `podvm` for
  x86_64 musl. The trial of `pod-hostprobe` leaves `adoption/podvm/`
  (podvm took it at `8899c2e`), and `adoption/podvm/commit` moves to
  `27ae993`.
- Checks: `sh scripts/agent/check.sh` (fmt, clippy, doc, record, size,
  static, test), `check-sources.py` (8 files, 0 changes),
  `secrets_like.py` on the patch (no line).

- Release and take: `v0.8.0` is released. podvm took the patch in `989f35b`
  (PodUtils/podvm#219): each line of the `.run` exited 0. The block of
  podvm in `SOURCES` has `taken 989f35bb7b17aeb9bb84dbd5da8f1df7909e49eb`
  (`check-sources.py`: 8 files, 0 changes), and the trial leaves
  `adoption/podvm/`.

Remains:

- `PodUtils/podvm:crates/podvm-release/src/prove_t0211.rs` mutates the fork and exec
  defences of `Lock` in podvm's source; after the take they are in
  `pod_supervise::lock`. A plant in podcrates for the two defences can
  take that place.
# T-061: `pod-guest`: a disposable QEMU guest

**Source:** `docs/design.md`, section 4 (M6); the owner, 2026-11-06.
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The owner, 2026-11-06: each generic crate of podvm moves to podcrates.
Source: `PodUtils/podvm:crates/podvm-windows`.

## Premise

- Read: podvm `6cd3442` (its `main`): `PodUtils/podvm:crates/podvm-windows/src`
  (12 files), its callers in `PodUtils/podvm:crates/podvm/src/windows`
  and `PodUtils/podvm:crates/podvm/src/machine/ssh.rs`; the plan of
  `docs/research/podvm/m6-imports.md`, section 7 and "Current files". The
  files of `PodUtils/podvm:crates/podvm-windows`, `PodUtils/podvm:crates/podvm/src/windows`,
  `Cargo.toml` and `Cargo.lock` are equal at `27ae993`, the commit of
  `adoption/podvm/commit`, so the trial starts there.
- Measured: the tests of the crate on this host; the trial of podvm at
  `27ae993`; the static `podvm` for `x86_64-unknown-linux-musl`.
- Not measured here: a real guest. This host has no `/dev/kvm` and no
  QEMU; the test with a real emulator skips with its reason.

## Approach

The procedure of `docs/design.md`, section 8: import, edition, adapt,
tests, README with the adoption map of podvm, the trial of podvm.

## Decision

Calls of the import, each with its reason (`docs/decisions.md`, C36):

- The cut: the crate takes the boot plan and the accelerator, the overlay
  and the commit of `qemu-img`, the FAT16 mailbox, the `cmd.exe` agent and
  the DOS flavour, QMP and its key table, the spawn in a process group and
  the bounded stop, the per-run directory, and the bounded store of a base
  image from a `Read`. podvm keeps the download (Q3 of the plan),
  `base_cache` of both flavours and `cap_status` (docker's 0 to 124).
- `Settings`: the root and the prefix of the per-run directories, `keep`,
  the marker and the OEM name of the mailbox. The crate reads no
  variable; podvm gives `XDG_RUNTIME_DIR`, `PODVM_WINDOWS_KEEP`,
  `podvm-windows-`, `podvm` (`podvm-dos`) and `PODVM`.
- Each failure is a `pod_core::Failure` at `Stage::Host`. A run at its
  limit carries `Timeout { after, stop }` (`timeout_of`); a refusal of the
  store carries `fetch::Refusal` (`fetch::refusal`).
- `agent::nonce` takes `pod_core::entropy` and fails, not panics.
- The FAT16 reader bounds a chain that the guest wrote.
- `PodUtils/podvm:crates/podvm-windows` stays in podvm as the glue: each function keeps
  its podvm signature and calls the crate with `settings()`.

## Prove

```sh
cargo test -p pod-guest
python3 -I scripts/check-sources.py --clone PodUtils/podvm=PODVM_CHECKOUT pod-guest
sh scripts/adoption.sh podvm pod-guest
cargo zigbuild --release --target x86_64-unknown-linux-musl -p static-probe
sh scripts/check-static.sh target/x86_64-unknown-linux-musl/release/static-probe
```

## Start condition

podvm gave the commit of its rename from `podbox-*` to `podvm-*` (PodUtils/podvm#194).

## Done

2026-11-09, partial. Commits `f09dcd4` (the import of 12 files of podvm
at `6cd3442`, with `SOURCES`), `04193cc` (the adaptation and the tests),
`add7a85` (README, C36), and the commit of this Done section (the trial,
(the doc links).

- Imported: the 12 files of `PodUtils/podvm:crates/podvm-windows/src`. Of
  `lib.rs` and `scratch.rs` a part stays in podvm (`base_cache`,
  `cap_status`). The crate has 3,171 lines in 13 `.rs` files, each under
  500.
- Tests: 65 unit tests: 58 of the 60 of podvm with their names (the tests
  of the caches and of docker's range stay in podvm); 7 new: the OEM name,
  a chain that the guest broke (it panics on the bounds of podvm), a dry
  random source, a key for each printable character, the name and mode of
  a per-run directory, a line with no key, and a real emulator with QMP
  (it skips here: no `qemu-system-x86_64`).
- The trial of podvm at `27ae993` (`pod-guest.patch` of `v0.9.0`: 21
  files, 252 lines in, 3,411 out; it applies at `6cd3442` too):
  `podvm-windows` keeps podvm's glue (`settings()`, the functions with
  their podvm signatures, `RunGuard`, `base_cache`, `dos::base_cache`,
  `dos::stage` with `podvm-dos`, `cap_status`) and re-exports the crate;
  10 files leave podvm; `PodUtils/podvm:crates/podvm/src/windows` reads a timeout with
  `timeout_of` and a failed nonce as a runtime error; each tag moves to
  `v0.9.0`; the citations of PodUtils/podvm@T-1641 and the code map
  follow. Each step of `sh scripts/adoption.sh podvm pod-guest` passed:
  podvm-windows 4 tests, `podvm --bin` 251, clippy, rustfmt, the
  runtime-state rows, podvm's record gate (305 entries), the toolchains,
  and the static `podvm` for x86_64 musl.
- Checks: `sh scripts/agent/check.sh` (fmt, clippy, doc, record, size,
  static, test), `check-sources.py` (12 files, 0 changes),
  `secrets_like.py` on the patch (no line).

Remains:

- The release `v0.9.0`, then podvm's take of the patch (after the patch of
  `pod-supervise`: the tag lines then move from `v0.8.0`), and its
  `taken` line in `SOURCES`.
- A run of a real guest under KVM or TCG in CI, where QEMU and a base
  image are present.

podvm took the crate in podvm `423e817` (PodUtils/podvm#220), after the
patch of `pod-enter`'s neighbour `pod-guest.patch` applied clean at podvm
`e37decf`; each line of the `.run` exited 0. The `SOURCES` of the crate has
`taken 423e817`; `check-sources.py` reads 12 files, 0 changes. The run of a
real guest in CI is T-138.

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

# T-128: `pod-hostprobe`: `waitid` and `ppoll` read the kernel layouts of a 64-bit target on a 32-bit target

**Source:** the nightly run 42640135438 at `dada663` (jobs `cross (i686-unknown-linux-musl)`, `cross (arm-unknown-linux-musleabihf)`, `cross (armv7-unknown-linux-musleabihf)`).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`tests/lifecycle.rs` of `pod-supervise` (T-060) failed on the 32-bit
targets: a payload that SIGKILL ended gave the exit code 128, not 137.
`sys::waitid_pidfd` read `si_status` at byte 24 of `siginfo_t`, which is
right for a 64-bit target. On a 32-bit target the union starts at byte 12
and `si_status` is at byte 20; byte 24 is zero. `sys::ppoll` also put the
timeout in a `timespec` of two `i64`, but the kernel reads two 32-bit
words there.

## Premise

- Read: the preamble of `siginfo_t` is `3*sizeof(int)` and then the union,
  aligned to `long`. That is 16 on 64-bit and 12 on 32-bit.

## Approach

`si_status` is at 24 on a 64-bit target and at 20 on a 32-bit target.
`Timespec` has two `isize` words.

## Prove

```sh
cargo zigbuild --tests -p pod-supervise --target i686-unknown-linux-musl  # then run `lifecycle`
```

## Done

2026-11-09. `lifecycle` passes as an i686 musl binary ("5 steps passed");
before the change it failed with 128 and 137 as in the nightly. The
nightly dispatched after the landing is the proof for the arm targets.

# T-132: `pod-hostprobe`: the id calls are the 16-bit forms on x86 and arm

**Source:** found by the import of `pod-enter` (T-059).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

On i686 and arm, `getuid`, `getgid`, `setuid`, `setgid`, `getgroups`,
`setgroups`, `chown` and `lchown` are the 16-bit forms of the kernel. An
id above 65535 reads as 65534 or is refused. `identity::read` and the FUSE
mount options of `pod-enter` take the ids through them.

## Premise

- Read: i386 and arm EABI have `getuid32` 199, `getgid32` 200,
  `getgroups32` 205, `setgroups32` 206, `setuid32` 213, `setgid32` 214,
  `chown32` 212 and `lchown32` 198. `fchownat` takes 32-bit ids on each
  target.
- Read: the other targets have only the 32-bit form under the plain name.

## Approach

`sys.rs` and `sys/arch.rs` choose the `*32` number by `target_arch`
(`x86`, `arm`). The probe rows print `chown32` and `lchown32` there.

## Prove

```sh
cargo zigbuild --tests -p pod-hostprobe --target i686-unknown-linux-musl  # then run `pod_hostprobe`
```

## Done

2026-11-09. Commit: the commit that names T-132. The i686 musl test binary
of `pod-hostprobe` passes (96 tests), with `an_id_above_65535_reads_whole`
(a child as root sets the id 100000 and reads it back). `pod-hostprobe` and
`pod-enter` build for `arm` and `armv7`. The nightly is the proof for the
arm runs.

# T-134: `pod-hostprobe`, `pod-enter`: the open flags are the x86 values on arm, aarch64 and powerpc

**Source:** nightly run 42703485489 (v0.9.0), jobs `cross` of aarch64, armv7, arm and powerpc64le.
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`sys::O_DIRECTORY` (`0o200000`) and `sys::O_NOFOLLOW` (`0o400000`) are the
values of x86. On arm, aarch64 and powerpc the kernel has `0o40000` and
`0o100000`. `RootDir::open` of `pod-enter` opens a directory with these
flags, so it failed on a real directory: the test
`a_symlinked_rootfs_is_refused_and_names_its_target` failed.

Two other tests of `pod-enter` failed on aarch64 under QEMU user mode:
`the_probe_answers_from_this_host` and
`a_memfd_sealed_against_exec_reads_as_no_exec`. The child of the probe execs
a memfd, and the emulator answers `ENOENT`. That is no defect of the code.

## Premise

- Read: `fcntl.h` of arm, aarch64 and powerpc has `O_DIRECTORY` `0o40000`
  and `O_NOFOLLOW` `0o100000`. The other targets have the values of x86.
- Read: the emulator answers `ENOENT` for the exec of a memfd.

## Approach

- `sys.rs` chooses the two flags by `target_arch`. A test makes the kernel
  say what each flag does (a file is `ENOTDIR`, a link is `ELOOP`).
- The two memfd tests return with a printed reason when
  `pod_hostprobe::interp::detect` finds an emulator.

## Prove

```sh
cargo test -p pod-hostprobe -p pod-enter
cargo zigbuild --tests -p pod-enter -p pod-hostprobe --target aarch64-unknown-linux-musl  # run with qemu-aarch64
```

## Done

2026-11-09. Commit: the commit that names T-134. The tests of `pod-enter`
and `pod-hostprobe` built for aarch64, armv7 and powerpc64le pass under
`qemu-user`, except `resolve_past_a_denied_listing`, which needs the
`binfmt_misc` registration that the nightly makes. The nightly is the proof.

# T-136: Port of PodUtils/podvm@T-1646 into `pod-enter`: the library path of the interposer

**Source:** podvm `284f429` (PodUtils/podvm@T-1646), PodUtils/podvm#219;
`scripts/check-sources.py` on `pod-enter`.
**Category:** chore
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm changed two imported files of `pod-enter` after the import at
`6cd3442`: `userland.rs` and `userland/tests.rs` of podvm-enter.
`host_env` pushes a variable with the library directories of the image,
and drops the value of the caller. The interposer reads it for the
loader of each dynamic grandchild, because a payload can change
`LD_LIBRARY_PATH`.

## Premise

- Read: the diff of podvm-enter between `6cd3442` and `284f429`. The
  variable is `PODVM_LIB_PATH`, the const is `LIB_PATH_VAR`.
- The crate names no variable of podvm: the name comes from `Names`.

## Approach

`Names` has a new field `lib_path` (`POD_ENTER_LIB_PATH` in
`Names::DEFAULT`). `host_env` takes `&Names` as its first argument, drops
each value of `names.lib_path` and pushes the list of `LD_LIBRARY_PATH`
under that name. The test of podvm follows with its checks. The README
has the field and the new signature. The snapshot of `SOURCES` moves to
`284f429` with the new blob ids. The trial of `pod-enter` gives
`NAMES.lib_path = "PODVM_LIB_PATH"` and a `host_env` of podvm's
signature.

## Prove

```sh
cargo test -p pod-enter
python3 -I scripts/check-sources.py --clone PodUtils/podvm=PODVM_CHECKOUT pod-enter
sh scripts/adoption.sh podvm pod-enter
```

## Done

2026-11-09. Commit: the commit that names T-136. `cargo test -p pod-enter`
passes. `check-sources.py` reads `pod-enter` at `284f429`: 28 files, 0
changes. The trial of `pod-enter` is made again on podvm `cb57d4b` (T-059).

# T-137: `pod-enter`: the notes of podvm's PodUtils/podvm@T-1649 and PodUtils/podvm@T-1650 (PodUtils/podvm#220)

**Source:** PodUtils/podvm#220 (COORD-014 of podvm; PodUtils/podvm@T-1649, PodUtils/podvm@T-1650, podvm `e37decf`)
**Category:** chore
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

podvm now routes a top-level `#!` payload to its interpreter inside the
rootfs (PodUtils/podvm@T-1649). Four texts and helpers of `pod-enter`
follow the old behaviour: the module text of `src/userland.rs` says that
scripts are refused; `ScriptRefusal::message` of `src/memfd.rs` and its
comment are stale (podvm reads only `.shebang`); `ladder::guest_exe`
returns the resolved path, while the kernel passes the unresolved join of
`PATH` as `$0`; `abi::interp_of` answers "static" when a program header
lies past the bytes, which is right for a whole file and wrong for a head
of a file (podvm's `complete/capability.rs` keeps its own bounded scan).

## Premise

- Read: the notes of PodUtils/podvm#220.
- Not read: the code of podvm at `e37decf`.

## Approach

Correct the two texts. Add `shebang_parts(bytes) -> Option<(String,
Option<String>)>`, so that podvm and its interposer stop keeping their own
parsers of `#!`; `guest_invoked(rootfs, argv0, path_dirs)` that gives the
`$0` the kernel would give. The variant of `interp_of` that is safe on the
head of a file is T-139. A test for each. Tell podvm in the hand-off of the release.

## Prove

```sh
cargo test -p pod-enter
```

# T-138: `pod-guest`: the run of a real guest in CI

**Source:** T-061 (its remaining clause); the owner's ruling KVM of 2026-11-09
**Category:** chore
**Milestone:** M6
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

The test of `pod-guest` with a real emulator (`qemu-system-x86_64 -M none
-accel tcg`, over QMP) skips where QEMU is absent, so no real guest has
run in CI. The owner lifted the KVM deferral (2026-11-09).

## Premise

- Read: the guarded test of `pod-guest`; the runners of GitHub (ubuntu has
  `/dev/kvm` on the larger runners only).
- Not measured: a run.

## Approach

A job that installs QEMU on an ubuntu runner and runs the guarded tests of
`pod-guest` (TCG; KVM where `/dev/kvm` exists), with a timeout on each
guest, one guest at a time, and a check that no emulator stays. A dispatch
of it gives the run id.

## Prove

```sh
cargo test -p pod-guest
```

The run id of the dispatched job goes in the Done section.

# T-139: `pod-enter`: classify and stage a payload over 128 MiB with bounded memory (PodUtils/podvm#222)

**Source:** PodUtils/podvm#222 (COORD-016 of podvm; PodUtils/podvm@T-1647, PodUtils/podvm#90, P1)
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

On a host that denies chroot, podvm refuses a payload over 134217728
bytes (128 MiB) before it runs (exit 125): `node` of node:alpine has
149234536 bytes, a zig toolchain binary 172641672. Three paths of
`pod-enter` read the whole file into memory: `Elf::read` (`src/abi.rs`),
`ladder::payload_bytes` (`src/ladder.rs`), and `memfd::stage`
(`src/memfd.rs`, one contiguous buffer). No `doctor` or `probe` row names
the cap, so the image is pulled first and the run fails after.

## Premise

- Read: the ask of PodUtils/podvm#222; T-137 (its note 4 asks the same
  head-safe `interp_of`).
- Not measured: the memory of a stage of 200 MiB.

## Approach

Classify from a bounded read of the ELF header and its program headers
(a head-safe `interp_of` that says "unknown" when a header lies past the
bytes read; it serves podvm's `complete/capability.rs` too). Stage the
memfd family's payload by a copy from the file into the memfd in bounded
chunks, with the digest computed during the copy where one is needed.
Bound the bytes held in memory, not the size of the file; where a size
limit stays, expose it as a constant so that podvm reports it in `doctor`
and `probe` and refuses before the pull. A test classifies and stages a
sparse fixture of 200 MiB through a bounded buffer. The case of a static
runtime that finds its tree through `/proc/self/exe` stays out (PodUtils/podvm@T-1648 and
PodUtils/podvm@T-1652).

## Prove

```sh
cargo test -p pod-enter
```

# T-140: `pod-oci`: the test of the host name fails where Rust and OCI say the same word

**Source:** nightly run 42731628165 (v0.10.0), jobs `cross` of arm, armv7 and riscv64gc.
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`the_host_is_an_oci_name_and_never_rusts` asserted that `Platform::host().arch`
differs from `std::env::consts::ARCH` on each target. On arm and riscv64
Rust and OCI use the same word (`arm`, `riscv64`), so the test failed there.
The code is right: `HOST_ARCH` is `arm` and `riscv64`.

## Premise

- Read: the image-index of OCI uses Go's `GOARCH`: `arm` and `riscv64`.

## Approach

- The test holds the table of Rust names and OCI names, and asserts the OCI
  name of the target. It asserts that the names differ only where the table
  says so.

## Prove

```sh
cargo test -p pod-oci platform
cargo zigbuild --tests -p pod-oci --target riscv64gc-unknown-linux-musl  # run with qemu-riscv64
```

## Done

2026-11-10. Commit: the commit that names T-140. The test passes on
x86_64, and under `qemu-user` on riscv64 and armv7. The other failures of
the first local run (`pod-enter` `resolve_past_a_denied_listing`, `children`
of `pod-hostprobe`) need the `binfmt_misc` registration that the nightly
makes. The nightly is the proof.
