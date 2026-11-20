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
**Status:** done

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

2026-11-10. Commits `94b44ea` (the import of 24 files of podvm
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
  (`pod-oci.patch` of `v0.10.0`: 33 files, 260 lines in, 4,046 out;
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

- Released in `v0.10.0` (`b21c193`, release 42742043211). podvm took the
  patch at `20ea781` (PodUtils/podvm#223): the 3-way apply conflicted only
  where `pod-oci.run` said, and each step of the `.run` passed there. The
  `taken` line is in `crates/pod-oci/SOURCES`, and the trial of pod-oci
  ends.
- A live pull from a public registry is T-142.

# T-057: `pod-extract`: the rootless apply of a layer

**Source:** `docs/design.md`, section 4 (M6); the owner, 2026-11-06.
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

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

2026-11-10. Commits `f940f32` (the import of 12 files of podvm at
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
- The trial of podvm at `2616bd7` (`pod-extract.patch` of `v0.11.0`: 27
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

Released in `v0.11.0` (`c660aca`, release 42755226074). podvm took the
patch at `0682c0c` (PodUtils/podvm#225; with `--3way` at podvm `72523d7`,
no conflict; each line of the `.run`, `cargo test --workspace` and podvm's
gate passed there). The `taken` line is in `crates/pod-extract/SOURCES`, and
the trial of pod-extract ends.

# T-058: `pod-rootfs`: the completion of a root file system

**Source:** `docs/design.md`, section 4 (M6); the owner, 2026-11-06.
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The owner, 2026-11-06: each generic crate of podvm moves to podcrates.
Source: `PodUtils/podvm:crates/podvm-complete`.

## Premise

- Read: podvm `2616bd7` (its `main` on the day of the import):
  `PodUtils/podvm:crates/podvm-complete/src` (14 files); its callers in
  `PodUtils/podvm:crates/podvm/src` (`complete.rs`, `complete/strict.rs`,
  `run/prepare.rs`) and podvm's gate (the pin `32d write`); the plan of
  `docs/research/podvm/m6-imports.md`, section 4 and "Current files".
- Measured: the tests of the crate on this host as x86_64 (as root) and
  i686 musl binaries, and as aarch64, armv7, powerpc64le and riscv64 musl
  binaries under QEMU user mode (no `openat2` there: each test that needs
  `RESOLVE_IN_ROOT` stops with a skip line); the trial of podvm; the
  static `podvm` for `x86_64-unknown-linux-musl`.
- Found: eleven defects of podvm's code (the README of the crate and the
  Done section list them), among them a removal and a creation of links
  on the host through a `dev` that is a symlink out of the root, and a
  stop for ever on a fifo of the image.

## Approach

The procedure of `docs/design.md`, section 8: import, edition, adapt,
tests, README with the adoption map of podvm, the trial of podvm.

## Decision

`docs/decisions.md`, C40: the crate takes the completion; podvm keeps its
flags, the record ids, the banner, the JSON and the HTTPS request of the
mirror probe; the names, the host bundle, the files of the host and the
probe come from the caller.

## Prove

```sh
cargo test -p pod-rootfs
python3 -I scripts/check-sources.py --clone PodUtils/podvm=PODVM_CHECKOUT pod-rootfs
sh scripts/adoption.sh podvm pod-rootfs
cargo zigbuild --release --target x86_64-unknown-linux-musl -p static-probe
sh scripts/check-static.sh target/x86_64-unknown-linux-musl/release/static-probe
```

## Start condition

podvm gave the commit of its rename from `podbox-*` to `podvm-*` (PodUtils/podvm#194), and T-055 is done.

## Done

2026-11-10. Commits `50c9eda` (the import of 14 files of podvm at
`2616bd7`, with `SOURCES`), `a316661` (the adaptation, the eleven fixes
and the tests), `07b043f` (README, `IDS`, the static probe, C40),
`019b897` (the trial) and the commit of this record.

- Imported: the 14 files of `PodUtils/podvm:crates/podvm-complete/src`.
  Of `lib.rs` a part stays in podvm (`Options`, the banner, the JSON); the
  HTTPS request of `reach.rs` stays in podvm. The crate has 4,347 lines in
  20 `.rs` files, each under 500. `pod_hostprobe::sys` gets `O_NONBLOCK`
  (0o200 on mips) and `S_IFIFO`, with a test.
- Fixed, each with a test that fails on a plant of podvm's line: the
  removal of a device link and the `/dev` links through a host path (a
  `dev -> /dir` removed and made files in a host directory); a read of a
  fifo of the image that waited for ever; a device node on a `nodev` file
  system that did not open (a root-only test); no rehash step for a grown
  host bundle; the C library of armv7, i686, riscv64, powerpc64le, s390x
  and loongarch64 read as `unknown`; `passwd`, `group` and
  `nsswitch.conf` behind a link inside the root replaced by the minimal
  files; a user part or an address in another form at the mirror probe
  (a loopback source was changed); a line end in an added host or the
  container name; a mount point with a space; the bytes of a CA bundle
  that are not UTF-8; a mode change by name (a race).
- Tests: 74 unit tests: 52 of the 53 of podvm with their names (the
  banner test stays in podvm), and 22 new. The same 74 pass as an i686
  musl binary, and under QEMU as aarch64, armv7, powerpc64le and riscv64
  musl binaries (51 skip there, as the emulator has no `openat2`).
- The trial of podvm at `2616bd7` (`pod-rootfs.patch` of `v0.11.0`: 34
  files, 227 lines in, 4,449 out; `pod-rootfs.commit`):
  `PodUtils/podvm:crates/podvm-complete` keeps podvm's glue (`Options`,
  `names()` with the values of earlier releases, `entry(id)`, `Fixup`,
  `Step`, `Report` with `banner`, `degradations`, `json`, `settings`, and
  `reach::probe` over `podvm_image::net`); 13 files leave podvm; the
  gate's pin of the moved write test goes; the citations of the moved
  files name `pod-rootfs`; each tag moves to `v0.11.0`. The patch applies
  alone; after `pod-oci.patch` and `pod-extract.patch`, a 3-way apply
  conflicts in seven files (`pod-rootfs.run` names the resolution), and
  the workspace with the three passes clippy, the tests of
  podvm-complete, podvm-extract and the podvm binary, the rows and
  podvm's gate. Each step of `sh scripts/adoption.sh podvm pod-rootfs`
  passed: podvm-complete 4 tests, `podvm --bin` 266, `store_gates` 4,
  `detached_stdio` 3, `curated_refusals` 6, clippy, rustfmt, the
  runtime-state rows, podvm's record gate, the toolchains, and the static
  `podvm` for x86_64 musl. podvm's experiment 85 (the completion escape)
  needs a pull from public.ecr.aws, which gave HTTP 429 here; it is not a
  step.
- Checks: `sh scripts/agent/check.sh` on `main` with T-139 (fmt, clippy,
  doc, record, size, paths, static, test: each 0), `check-sources.py` (14
  files, 0 changes), `secrets_like.py` on the patch (no line). The trial
  passed again on `main` `81439a8`.

Released in `v0.11.0` (`c660aca`, release 42755226074). podvm took the
patch at `0682c0c` (PodUtils/podvm#225; with `--3way` at podvm `72523d7`,
no conflict; each line of the `.run`, `cargo test --workspace` and podvm's
gate passed there). The `taken` line is in `crates/pod-rootfs/SOURCES`, and
the trial of pod-rootfs ends.

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
- podrunner is a consumer (PodUtils/podvm#227 section 2.7). The `microvm` unit kind of podrunner waits for `pod-microvm` or for PodUtils/podvm@T-1656. The design review asks the owner (DR-5) for a stopgap driver in podrunner.

## Approach

One study for each crate, as the studies of M4; each gives the entry of
its crate.

## Prove

```sh
cargo todo check
```

## Start condition

The crates of M6 are released.

## Correction

2026-11-11: the study of `pod-confine` went into T-108 with the crate, as
its Problem says; T-108 is done. The other five studies stay here.

2026-11-12: the study of `pod-netlab` went into T-207 with the crate
(calls C145 to C149); its rest is T-345 to T-349. The other four studies
stay here.

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
**Status:** done

## Problem

A-V-10 and A-X-25 need a stable JSON of the host probe: the `caps` of cfb (Landlock, seccomp, userns,
memfd, SUD, `openat2`, `/dev/kvm`, `/dev/fuse`, `/dev/net/tun`, cgroup).

## Premise

- Read: `crates/pod-hostprobe/src/` and the asks.
- Not measured: the JSON.
- podrunner is a consumer (PodUtils/podvm#227 section 2.7). The host qualification that the design review (DR-15) gives to `pod doctor` comes from this report, and `podrunner doctor` reads it. The podvm driver reads `podvm probe --json`.
- podswarm is a consumer (GitHub #82, podswarm P-5, read 2026-11-13): it asks for `report(&Findings) -> serde_json::Value` that validates against the schema of this entry, released with each tag. Its map to capabilities and its cache across runs are T-443; its new rows are T-444.
- The first part is done (T-485, 2026-11-13, commit `5a2b5fd`): the module `report` of the feature `report` writes `Findings` and `Selection` as a JSON object with the number `schema` (1), and reads it back (calls C242, C243; the README of the crate, "The report"). The JSON Schema file in `schemas/`, the value `unknown` and the form that podswarm asks for go on that object.

## Approach

Serde types behind a feature. A `schema` field. A JSON Schema file in `schemas/`. The value `unknown` where
a filter answers `EPERM`.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
```

## Done

2026-11-14, commit `af39707` (the first part: T-485, commit `5a2b5fd`).
`schemas/hostprobe-report.v1.schema.json` (JSON Schema 2020-12) is the
schema of the object of `report::write`, generated from its wire types
by schemars (calls C308, C309). The feature `schema` (`report` and
schemars, in `scripts/check-probe.py` ALLOWED: no binary carries it)
gives `report::schema()` and `report::SCHEMA_ID`, so podvm validates
`probe --json` against the schema of the tag with no copy
(PodUtils/podvm#239). The `$id` names the version `v1`, and the field
`schema` of the report is the constant 1. A verdict, a rung and a row
name are an `enum` of the words of the crate; a `denied` row or write
probe needs an integer `errno`; an object has no other field. A row name
that comes two times fails `read` only. No word `unknown` in the report:
a row keeps its verdict and its errno (a filter's `EPERM` is `denied`
with errno 1), and `unknown` is a value of a capability of T-443; the
form `report(&Findings) -> serde_json::Value` of podswarm goes with T-443
(call C309, the ruling "podswarm").

Tests (`cargo test -p pod-hostprobe --all-features`): the file equals the
schema of the types; the `$id` names the version that the report
carries; `tests/examples/report/` has 2 valid and 10 invalid reports, and
each gets the same verdict from `read` and from boon (the reason of each
in `tests/examples/expect.json`); the object of `write` on the fixture of
the module validates. The plants `hostprobe-schema-any-verdict` (a
verdict of any string) and `hostprobe-schema-denial-unchecked` (no
`errno` for a denial) fail the test of the examples.

# T-108: `pod-confine`: Landlock, a seccomp deny filter, no_new_privs and rlimits from one profile (GitHub #35)

**Source:** GitHub #35 (A-V-01); PodUtils/podvm#259 (podvm's ask, PodUtils/podvm@T-1658, issue 170)
**Category:** feature
**Milestone:** M7
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

A-V-01 is P0 for cfb. `pod-confine` waits for the study of T-062 after M6. The study and the crate go in
one entry.

## Premise

- Read: the ask of A-V-01; `PodUtils/podnet:crates/podnet-netns/src/seccomp.rs` builds a seccomp filter
  (user notification, not a deny list).
- Not measured: Landlock on the targets.
- podrunner is a consumer (PodUtils/podvm#227 section 2.7). It needs a proof string for a capability record with `boundary: confined`. Without `confined`, `trusted` jobs run only if the owner answers yes to DR-4.

- podvm's ask (PodUtils/podvm#259): `Spec` (Landlock read, write and
  execute paths as access sets of each path; TCP connect and bind ports,
  ABI 4 or later; a seccomp deny list); `probe() -> Option<u32>` (the ABI
  of `landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)`,
  and whether a nested ruleset loads under the host's policy);
  `apply(&Spec)`: `PR_SET_NO_NEW_PRIVS`, the ruleset,
  `landlock_restrict_self`, then a seccomp filter in mode 2, with a
  default deny list (the namespace, mount, ptrace, keyctl, bpf,
  `perf_event_open`, `kexec_*`, module and `*_handle_at` calls) and
  `EPERM`; no allocation in `apply` (it runs in the child after `fork`);
  an error names the call and the errno; where Landlock is absent, `apply`
  refuses. In `pod-enter`: `Plan.confine: Option<pod_confine::Spec>`,
  applied in the child after the limits and `no_new_privs`, before the
  exec; a spec of the chroot rung names guest paths, one of the userland
  rung host paths. Its proof: experiment 414 (`cat /etc/hostname` EACCES
  outside the read list; a connect to 22 EACCES, 443 passes;
  `NoNewPrivs 1`, `Seccomp 2`). The host of 2026-11-11 (Linux 6.18)
  answers ABI 7.
- `pod-enter` already sets the rlimits and `no_new_privs` in the child
  (T-176): `pod-confine` adds no rlimit.

## Approach

The study, then the crate: Landlock ABI 3 or newer for file rules and ABI 4 or newer for TCP rules, a
seccomp deny filter of the list of A-V-01 and of podvm's ask, `no_new_privs`, the `Spec` of podvm's ask (a profile file of the consumer maps to it), and `Plan.confine` of `pod-enter`. The crate refuses by
name where Landlock is absent. The filter builder of podnet is a source.

## Decision

Call C57 (`docs/decisions.md`): `Plan.confine` is an
`Option<pod_confine::Spec>`, compiled by the entry before the banner and
the fork. On each rung with a chroot the paths are guest paths, opened
under the root descriptor with `openat2(RESOLVE_IN_ROOT)`; on the entry
with no chroot they are host paths. A rule holds the inode, so the open
before the fork names the file that the payload sees after the root
change, and the child only issues system calls on prepared data. The
crate refuses an ABI under 3 (no confinement of `truncate(2)`).

## Prove

```sh
cargo test -p pod-confine --all-features
cargo test -p pod-enter --all-features --test confine
```

## Correction

2026-11-11. A rule opened before the fork does not cover a mount that
the child makes on its path: Landlock does not read a rule on the
directory under a mount point. On the namespace rung a write rule on
`/tmp` thus missed the tmpfs of the child (PodUtils/podvm#267). T-227
measured it and fixed it: the child opens the path of each rule after
its mounts and its root change. Call C68 corrects call C57.

## Done

2026-11-11, `8e203ee` and `271a8ec`. The crate `pod-confine` (new code; it depends on
`pod-core` and `pod-hostprobe`):

- `Spec { paths, connect, bind, deny }` with `Access` (`READ`, `WRITE`,
  `EXECUTE`) for each path, `Spec::new().allow(..).connect_to(..)
  .bind_to(..)`; `Deny` and `Deny::defaults()` (the namespace calls:
  `unshare` and `clone` with a namespace flag, `setns`, `clone3` with
  `ENOSYS`; the mount calls with the new mount API; `ptrace`,
  `process_vm_readv`, `process_vm_writev`; the key calls; `bpf`,
  `perf_event_open`; `kexec_*`; the module calls; `*_handle_at`; each
  `EPERM`); `deny::program` (the audit arch first: a foreign ABI and x32
  get `ENOSYS`).
- `check()` and `probe() -> Option<u32>`: the ABI, and an `apply` of a
  minimal ruleset and filter in a disposable child (a nested ruleset under
  the policy of the host); the caller stays free.
- `Spec::compile(abi)` (host paths) and `Spec::compile_in(abi, root)`
  (guest paths) before the fork: `O_PATH` for each path, a missing path is
  a refusal by name, no Landlock or an ABI under 3 is refused, TCP rules
  need ABI 4; `Compiled::keep_clear_of`.
- `apply(&Compiled)` in the child: `PR_SET_NO_NEW_PRIVS`, the ruleset,
  the path and port rules, `landlock_restrict_self`, seccomp mode 2; no
  allocation; `Refusal { call, index, errno }` (five bytes for a pipe).

`pod-enter`: `Plan.confine: Option<pod_confine::Spec>`, applied in the
child after the limits and `no_new_privs`, as the last step before the
exec (step 15 of the readiness pipe, with the path or the port of the
rule). `pod-hostprobe`: the Landlock calls, `connect`, the numbers of the
deny list, and the one table of the audit arch (`sys/landlock.rs`,
`sys/seccomp.rs`); `pod-hold` reads it in place of its own table.

Tests: `cargo test -p pod-confine --all-features`: 19 unit tests and 8 in
`tests/apply.rs` (a confined child: a read and a write outside their lists
`EACCES`, a TCP connect and bind outside their lists `EACCES` and a
connect inside passes, `NoNewPrivs: 1` and `Seccomp: 2`, `unshare`,
`clone` with `CLONE_NEWUSER` and `setns` `EPERM`, `clone3` `ENOSYS`, an
exec outside the execute list `EACCES`, `apply` refuses where a filter
stands in for a kernel with no Landlock); all pass on this host (Landlock
ABI 7). `cargo test -p pod-enter --all-features --test confine`: 7 tests
(the same checks through the entry with no chroot, the chroot sequence and
the namespace rung in an image of the test binary; a missing path and no
Landlock stop the entry before the fork). A host with no Landlock skips
the checks of a rule; the rungs with a chroot need root.
`sh scripts/agent/check.sh` exits 0 in each check. Plants (group
`confine`, each caught): `confine-write-unhandled`,
`confine-filter-foreign-abi-passes`, `confine-absent-runs-unconfined`,
`enter-confine-not-applied`, `enter-confine-rule-on-a-passed-number`,
`enter-pipe-on-a-passed-number`, `enter-pass-onto-the-root`,
`enter-own-number-stays-cloexec`. The static
probes call `pod-confine` and are static on `x86_64-unknown-linux-musl`;
the probe reports Landlock ABI 7 and a filter of 73 instructions here.

A defect found on the way, fixed in the same commit: a passed descriptor
(`Fds.pass`) could take the number of the readiness pipe (a failed step
in the child then seemed a start) or of the root (`fchdir` failed with
`ENOTDIR` and no cause). The pipe moves clear of each passed number, and
a pass onto the root or the memfd is refused before the fork; the two
tests fail on the code before the fix. A second one, fixed in `271a8ec`: a descriptor passed onto its own number (`(n, n)`) kept its
close-on-exec flag, as no `dup2` ran, so the payload never held it; the
child now clears the flag
(`a_descriptor_passed_onto_its_own_number_reaches_the_payload`). Open
from this work: the Landlock scopes and UDP (T-209). Breaking: `Plan` has
the field `confine`.

# T-109: Research: the hold of each connect of a payload, from the `podnet-netns` of podnet, for podvm (GitHub #35, #36)

**Source:** GitHub #35 (A-V-02) and GitHub #36 (G-04)
**Category:** research
**Milestone:** M7
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm (A-V-02) and podrunner (G-04) must hold each connect of a payload. podnet has two backends that do
it. `docs/design.md` keeps `podnet-netns` in podnet.

## Premise

- Read: the asks. Not read: `PodUtils/podnet:crates/podnet-netns` (9,447 lines).
- podrunner is a consumer (PodUtils/podvm#227 section 2.7). The hold reports a level (`enforced`, `advisory` or `none`) and a proof. The terms of PodUtils/podnet#414 look met: PodUtils/podnet@T-040 and PodUtils/podnet@T-142 to PodUtils/podnet@T-158 are done at podnet `31d040f` (read in the issue, not read here). A COORD issue to podnet asks podnet to write its hand-off.

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

## Done

2026-11-10. podnet's hand-off came (PodUtils/podnet#422, COORD-014 of
podnet; PodUtils/podnet@T-232): the terms of PodUtils/podnet#414 are met
(PodUtils/podnet@T-040 and the seccomp fixes PodUtils/podnet@T-142 to
PodUtils/podnet@T-158 are done). The decision: one crate, `pod-hold`, with
the generic core only, as #414 cuts it. The import is T-166.

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

T-347 is done: a lab of `pod-netlab` runs on a kernel.

## Correction

2026-11-12: the study of `pod-netlab` went into T-207 (the crate's knobs,
plan, derivation and judge); a cell of this entry needs a lab that runs,
so the start condition is T-347.

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
**Status:** done

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
head of a file is T-139 (done: `interp_of` itself is head-safe). A test for each. Tell podvm in the hand-off of the release.

## Prove

```sh
cargo test -p pod-enter
```

## Done

2026-11-10. Commit: 6f2c0a4. `cargo test -p pod-enter` passes (141 unit
tests); `sh scripts/agent/check.sh` exits 0.

- The module text of `src/userland.rs` and `ScriptRefusal::message` of
  `src/memfd.rs` no longer say that scripts are refused: the caller runs
  the interpreter from the image.
- `memfd::shebang_parts(bytes)` gives the interpreter and the one optional
  argument of a `#!` line as `fs/binfmt_script.c` parses it (the head is
  `memfd::SHEBANG_HEAD`, 256 bytes). It ports `shebang` of podvm's
  `lifecycle/script.rs`, with its cases as tests.
- `ladder::guest_invoked(rootfs, argv0, path_dirs)` gives the `$0` of the
  kernel: `argv0` with a slash as given, a bare name as the unresolved
  join of the first `PATH` directory where it resolves. `guest_exe` keeps
  the resolved path.
- The README has both items in the API table and in the adoption map.

# T-138: `pod-guest`: the run of a real guest in CI

**Source:** T-061 (its remaining clause); the owner's ruling KVM of 2026-11-09
**Category:** chore
**Milestone:** M6
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

The test of `pod-guest` with a real emulator (`qemu-system-x86_64 -M none
-accel tcg`, over QMP) skips where QEMU is absent, so no real guest has
run in CI. The owner lifted the KVM deferral (2026-11-09).

## Premise

- Read: the guarded test of `pod-guest`; the runners of GitHub (ubuntu has
  `/dev/kvm` on the larger runners only; run 42755557589 found it on the standard one).
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

## Done

2026-11-10. Commit: 6f2c0a4. Run 42755557589 of `guest.yml` (dispatched
on `main` at that commit) passed: 66 tests of `pod-guest`, one test
thread, and the last step reported that no emulator stays.

- `.github/workflows/guest.yml` (each week, by hand): QEMU on
  ubuntu-24.04, `cargo test -p pod-guest` under `timeout`, a check with
  `pgrep` that fails the job when a qemu process stays.
- A new test, `a_real_emulator_under_kvm_answers_qmp_and_leaves_no_process`,
  runs where `/dev/kvm` opens; the TCG test shares its helper. The
  ubuntu-24.04 runner of run 42755557589 has `/dev/kvm` (the step KVM
  printed "kvm: /dev/kvm opens"), so the KVM test ran there.

# T-139: `pod-enter`: classify and stage a payload over 128 MiB with bounded memory (PodUtils/podvm#222)

**Source:** PodUtils/podvm#222 (COORD-016 of podvm; PodUtils/podvm@T-1647, PodUtils/podvm#90, P1)
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** M
**Status:** done

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

## Done

2026-11-10. Commit: the commit that names T-139. `cargo test -p pod-enter`
passes (139 unit tests, 5 in `tests/bounded_payload.rs`, 2 in
`tests/tty_refusal.rs`); the same on i686 (138 unit tests and the 7 others).

- `abi::Elf::read` reads the ELF header, the two header tables and the
  sections of the checks with `pread` (`src/abi/image.rs`), never the whole
  file. The ceiling of 128 MiB is gone. `abi::READ_CEILING` (64 MiB)
  bounds the bytes held for one file; a file whose tables need more is an
  `Unreadable` that names the bound. A section that a read cannot give is
  a refusal, never a file with fewer symbols.
- `abi::interp_of` is head-safe: a program header table or a `PT_INTERP`
  path past the bytes gives an `Unreadable` ("unknown"), never `Ok(None)`
  ("static"). `abi::PHDR_TABLE_MAX` (64 KiB, the bound of the kernel)
  refuses a larger table, and an `e_phentsize` smaller than a program
  header is refused. `abi::interp_of_file` reads a file with `pread`.
- `ladder::payload_head` replaces `payload_bytes` and `PAYLOAD_CEILING`.
  It gives `Payload { path, len, head }` and `Payload::file()`; the head
  has at most `abi::HEAD_BYTES` (64 KiB). No size limit of a payload stays.
- `memfd::stage_file` and `memfd::stage_file_with` copy from the file into
  the memfd in chunks of `memfd::STAGE_CHUNK` (1 MiB); `stage_file_with`
  gives each chunk to the caller for a digest. `memfd::stage` stays for
  bytes in hand.
- Tests on a sparse file of 200 MiB, with a counting allocator of the
  thread (bound 4 MiB): `a_200_mib_payload_classifies_from_its_head`
  (65616 bytes held), `a_200_mib_elf_reads_its_tables_with_bounded_memory`
  (5081), `a_section_over_the_read_ceiling_is_refused_unread` (1473),
  `a_header_past_the_head_is_unknown`,
  `a_200_mib_payload_stages_through_a_bounded_buffer` (1048602; it skips
  under an emulator). Unit tests: `a_header_past_the_head_is_unknown_and_never_static`,
  `a_hostile_program_header_table_is_refused`,
  `the_bounded_read_agrees_with_the_parse_of_the_whole_file`,
  `the_payload_head_of_a_small_file_is_the_file`.
  `a_hostile_offset_is_a_refusal_and_not_a_panic` now expects "unknown"
  for a program header table past the file.
- Plant: `Elf::read`, `payload_head` and the copy of the stage that read
  the whole file. Each of the 5 tests of `tests/bounded_payload.rs` fails
  (`Elf::read` held 209716146 bytes, the stage 209715226).
- podvm at its take (README, "Adoption map"): `payload_bytes(..)` becomes
  `payload_head(..)`, and `eligible`, `has_shebang` and `script_refusal`
  take `&p.head`; `memfd::stage(&bytes)` of a payload becomes
  `pod_enter::memfd::stage_file(&NAMES, p.file())`; the refusal of 128 MiB
  goes; `doctor` and `probe` can report `HEAD_BYTES`, `READ_CEILING` and
  `STAGE_CHUNK` as bounds of memory; `complete/capability.rs` can use
  `interp_of` on its head. Then podvm runs `node` of node:22-alpine
  (PodUtils/podvm#222).

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

# T-141: `pod-extract`: port podvm's fix of the whiteout escape and its tests; remake the patch on podvm's main

**Source:** PodUtils/podvm#221 (COORD-015), PodUtils/podvm@T-1698; podvm `c89b769`.
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podvm fixed the whiteout escape in its own `PodUtils/podvm:crates/podvm-extract`
at `c89b769`, after the trial commit `2616bd7` of `pod-extract.patch`. The
fix changes four files that the patch removes (`drive.rs`, `lib.rs`,
`remove.rs`, and the new `drive/whiteouts.rs` with its sentinel tests). The
patch of `v0.10.0` does not apply at podvm's `main`.

## Premise

- Read: `c89b769` and the `repo PodUtils/podvm` block of
  `crates/pod-extract/SOURCES`.
- The crate has its own fix (T-057). podvm's tests can find a case that the
  crate's tests do not cover.

## Approach

- Compare podvm's fix with the crate's. Port each test of
  `drive/whiteouts.rs` and of `remove.rs` to the tests of the crate; port
  each case that the crate does not refuse.
- Move the SOURCES snapshot of the four files to `c89b769` or later.
- Make `pod-extract.patch` of `v0.11.0` again on podvm's `main` (after
  `c89b769`), with `pod-extract.commit`; run the trial.

## Prove

```sh
cargo test -p pod-extract
python3 -I scripts/check-sources.py --clone PodUtils/podvm=../podvm
sh scripts/adoption.sh podvm pod-extract
```

## Done

2026-11-10. Commit: the commit that names T-141.

- podvm's fix (`c89b769`, `remove::checked_dir`) checks the directory of a
  whiteout for NUL, an absolute path and `..`, and refuses a name of `.` or
  `..`. The crate's `remove::collect` gives the whole path of a whiteout to
  `safety::components` and refuses the same names: the same rule.
- Ported to `crates/pod-extract/src/drive/whiteouts.rs`: the three tests of
  podvm's `drive/whiteouts.rs` (`.wh...` at the root; `../.wh.sentinel`,
  `a/../../.wh.sentinel` and `../.wh..wh..opq`; `d/.wh..`), each with a
  sentinel beside the destination, and a test of the absolute whiteout of
  `checked_dir` (the refusal comes before each removal of the layer). podvm's
  `remove.rs` has no test of its own. The crate refuses each case: no gap.
  With the check of `collect` turned off, each of the four tests fails.
- `SOURCES`: the block of `PodUtils/podvm` reads `20ea781` (13 files; its
  change of `lib.rs` after `c89b769` is in podvm's glue, not in the crate);
  `check-sources.py --clone PodUtils/podvm=../podvm` finds no change.
- `pod-extract.patch` of `v0.11.0` is made on podvm `20ea781` (podvm
  took `v0.10.0` and `pod-oci` there; `pod-extract.commit`), and it applies
  at `e99fc9b` too: 11 files leave podvm, the citations of
  PodUtils/podvm@T-1698 name the crate, each tag is `v0.11.0`. The patch
  takes T-139 (PodUtils/podvm#222) as the adoption map of `pod-enter` says:
  `ladder::payload_head` and `memfd::stage_file` at the three call sites of
  `PodUtils/podvm:crates/podvm`; the 128 MiB refusal goes. The trial passes
  each step, with `qol` after the build of the interposer.
- With `--3way` after this patch at podvm `20ea781`, `pod-rootfs.patch` has
  two conflicts (`PodUtils/podvm:crates/podvm-image/Cargo.toml`,
  `PodUtils/podvm:docs/runtime-state.md`); `pod-rootfs.run` says so.

# T-142: `pod-oci`: a live pull from a public registry

**Source:** T-056, Done section.
**Category:** measurement
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The tests of `pod-oci` pull from a local fixture registry only. No test
pulls a manifest and a blob from a public registry through the crate.

## Premise

- `.github/workflows/live.yml` runs the live checks once a day.
- A public registry can answer 429 (rate limits); the check reads that as
  a skip, not as a failure.

## Approach

- Add a live test of `pod-oci`: resolve a small public image of each
  platform, fetch its manifest and its config blob, check each digest.
- Run it in `live.yml`.

## Prove

```sh
cargo test -p pod-oci --test live -- --ignored
```

## Done

2026-11-11. `crates/pod-oci/tests/live.rs`
(`a_pinned_index_resolves_for_two_platforms_and_a_pinned_manifest_for_one`)
reads the service `ecr-public-registry` of `pod-services`: the first
image of `images` (`docker/library/alpine:3.20`, pinned by the digest of
its index) resolves for linux/amd64 and linux/arm64, and
`guest-userland` (pinned by the digest of one manifest) for linux/amd64;
the client checks each index, manifest and config against its digest,
and the test checks the pinned digest and the platform of each config. A
429 is a skip that the log names. `live.yml` runs it.

- Measured here on 2026-11-11: the index gave the amd64 manifest
  `sha256:c64c687c...` and the arm64 manifest `sha256:45e09956...`, which
  is the fact `aarch64-image` of the service. An earlier run here got a
  429 ("Data limit exceeded") and skipped.
- Found and fixed: the first form of the test took `guest-userland` for an
  index (the run 42963084618 of `live.yml` failed on it); it is one
  manifest.

# T-144: `pod-enter`: the volume pairs in the maps table; the workdir and the first program through a guest-to-host resolver (PodUtils/podvm#224)

**Source:** PodUtils/podvm#224 (COORD-018 of podvm; PodUtils/podvm@T-1653, `-v HOST:GUEST` on the loader family).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podvm puts each `-v HOST:GUEST` as a `GUEST:HOST` pair in its maps
variable after the root pair; the interposer takes the longest prefix.
`pod-enter` knows the root only:

1. `userland_maps_table(names, rootfs)` takes the rootfs only, so podvm's
   glue checks each volume side with the root's message ("the rootfs path
   is N bytes").
2. `check_workdir`, `workdir_kind` and the userland child's `workdir_rel`
   resolve the workdir inside the rootfs: `-w` cannot name a volume.
3. `ladder::payload_head`, `userland::loader_plan` and `abi::resolve_in`
   resolve the first program inside the rootfs only: a first program
   inside a volume is refused.

## Premise

- Read: PodUtils/podvm#224.
- Not read: podvm's commit of PodUtils/podvm@T-1653 (it follows the issue).

## Approach

- A table of pairs (`Maps`: the root pair and the volume pairs, the
  longest guest prefix first), built and checked by the crate; each side
  over the length limit is refused with its own name.
- One guest-to-host resolver over the table, for the workdir and for the
  first program (`resolve_in` takes the table); podvm's
  `lifecycle::script::follow` can call it.
- A test for each: a workdir in a volume, a first program in a volume, the
  longest prefix, a symlink of a volume that leaves the volume.

## Prove

```sh
cargo test -p pod-enter
```

## Done

2026-11-10. Commit: the commit that names T-144. Read: PodUtils/podvm#224
and podvm `4ef187e` (PodUtils/podvm@T-1653: `podvm-enter`'s
`userland_maps_table(root, &volumes)`, `volume_at`, `volumes_of`, and
`lifecycle::volume::refuse_paths`, which refuses `-w` and a first program
in a volume by name).

- `pod_enter::userland::{Volume, Maps}`: `Maps::of_root(rootfs)` is the
  table of the root pair alone; `Maps::new(rootfs, &volumes)` adds the
  volumes and refuses by name a guest side that is not absolute, is `/`,
  is not a normal path or repeats one, a host side that is not absolute,
  and a NUL. `Maps::volume_at` gives the longest guest prefix.
- `userland_maps_table(&names, &maps)` (the root pair, then each volume)
  and `userland_volume_table(&names, &maps)` (no root pair, for
  `--unsafe-host-paths`) refuse each side with `:` or `,` or over
  `MAX_MAP_SIDE` (4096) bytes with its own name ("the rootfs path", "the
  volume HOST:GUEST: the guest side", "... the host side"), and a table
  over `MAX_MAP_PAIRS` (32, the bound of the interposer).
- One resolver: `abi::GuestRoot` (for `Path`, `PathBuf`, `str`, `String`
  and `Maps`). Each step of the walk maps its guest prefix through the
  table (the longest guest prefix wins); a link target is a guest path
  again, and `..` removes a guest component. So a link in a volume that
  points out of it goes to the side that holds its target (the image or
  another volume), never to the host directory beside the volume; `..`
  past the guest root is an escape, as for the root alone. A mount point
  is read with `stat`; a missing ancestor of a mount point is passed.
- Through the table: `check_workdir`, `spawn_userland` and `run_userland`
  (they take `&Maps`; the child enters the resolved directory, relative
  to the root descriptor in the image, by host path in a volume; a table
  of another root is refused), `ladder::{resolve_payload, payload_head,
  guest_exe, guest_invoked}`, `userland::{loader_plan, lib_dirs,
  invocation_path}`, `abi::{resolve_in, resolve_dir_in}`. A plain path is
  the table of the root pair alone, so each rootfs-only call compiles and
  answers as before. `invocation_path` keeps the invoked spelling only
  where the host opens the resolved file through it (the same device and
  inode).
- Tests (`crates/pod-enter/src/userland/maps/tests.rs`, 8): the table
  order, each volume side refused by its own name (an over-length host
  side and guest side, `:` and `,`, 33 pairs), the refusals of
  `Maps::new`, the longest prefix, a workdir in a volume, a first program
  in a volume (path, `PATH`, head, guest path, loader), a link out of a
  volume (relative to the host file beside the volume: absent; past the
  root: escape; absolute: the file of the image), and the userland entry
  that starts `/bin/sh` in a volume and in the image (`pwd -P`).
- Plants: the walk follows a link on the host (`canonicalize`):
  `a_link_out_of_a_volume_is_held_inside_the_table` fails (`/data/up`
  gives the host file `secret`). The first pair, not the longest: three
  tests fail. The child enters the raw guest path relative to the root:
  `the_userland_entry_enters_a_workdir_in_a_volume` fails (`ENOENT` at
  the step of the working directory).
- `cargo test -p pod-enter`: 149 unit tests, 5 and 2 integration tests,
  pass. `cargo zigbuild --tests -p pod-enter --lib` for i686, aarch64,
  armv7, arm, riscv64gc, powerpc64le and loongarch64 musl builds; the
  8 tests pass on each under qemu-user, but on loongarch64 the userland
  entry test fails as 3 older tests fail there: `RootDir::open` gets
  `ENOSYS` from `newfstatat` under qemu-loongarch64 8.2.
- README: the table section, the API, the adoption map (what podvm
  changes).

# T-145: `pod-hostprobe`: `Statfs::f_flags` (PodUtils/podvm#224)

**Source:** PodUtils/podvm#224 (COORD-018 of podvm, ask 4).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`sys::Statfs` has no `f_flags`. With it (`ST_NOEXEC`), podvm reads one
`statfs` to name a noexec volume source, not `/proc/self/mountinfo`.

## Premise

- The layout of `struct statfs` differs between the 64-bit targets and the
  32-bit ones (`statfs64` on i686, arm and armv7).

## Approach

- Add `f_flags` and the `ST_*` constants (`ST_RDONLY`, `ST_NOSUID`,
  `ST_NODEV`, `ST_NOEXEC`) on each target of the consumers; a test that
  reads the flags of a known mount.

## Prove

```sh
cargo test -p pod-hostprobe
```

## Done

2026-11-10. Commit: the commit that names T-145.

- `sys::Statfs` has `f_flags` (`u64`), read from the `struct statfs64` of
  `linux-raw-sys` on each target: a `long` on x86_64, aarch64, riscv64,
  powerpc64le and loongarch64, a `u32` on i686, arm, armv7 and s390x
  (`statfs64(2)` on a 32-bit target). `Statfs::flag(ST_*)` is `None`
  where the kernel did not set `ST_VALID`.
- `sys::{ST_RDONLY, ST_NOSUID, ST_NODEV, ST_NOEXEC, ST_SYNCHRONOUS,
  ST_VALID, ST_MANDLOCK, ST_NOATIME, ST_NODIRATIME, ST_RELATIME}`, the
  values of `include/linux/statfs.h`.
- Test `statfs_reads_the_mount_flags`: `/` has `ST_VALID`, and each mount
  of `/proc/self/mounts` has the `ro`, `nosuid`, `nodev` and `noexec` of
  its line (a skip line for a flag that no mount has). On the build host
  each of the four flags is seen. Plant: `f_flags` from `f_spare[0]`: the
  test fails ("f_flags 0x0 of / has no ST_VALID").
- `cargo test -p pod-hostprobe`: 99 unit tests pass. `cargo zigbuild --tests -p
  pod-hostprobe --lib` for i686, aarch64, armv7, arm, riscv64gc,
  powerpc64le and loongarch64 musl builds, and the two `statfs` tests pass
  on each (i686 native, the others under qemu-user). s390x has no target
  here; its layout is the one of `linux-raw-sys` (`c_uint` fields).

# T-147: `pod-services`: podvm took `pod-oci`: the SOURCES block of podvm reads the take

**Source:** `scripts/check-sources.py`, 2026-11-10 (2 changes in the podvm block of `pod-services`).
**Category:** chore
**Milestone:** M5
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podvm took `pod-oci` at `20ea781`: `PodUtils/podvm:crates/podvm-image/src/reference.rs`
left podvm (its Docker Hub names are `pod_oci::reference` now), and
`PodUtils/podvm:crates/podvm/src/images/login_extract.rs` changed two
error conversions. The check read both as changes.

## Premise

- Read: `git diff 2be31b2 20ea781` of both files in podvm. No name of a
  registry changed.

## Approach

A `taken` line for `reference.rs` at `20ea781`; the blob of
`login_extract.rs` at `20ea781`.

## Prove

```sh
python3 -I scripts/check-sources.py --clone PodUtils/podvm=../podvm pod-services
```

## Done

2026-11-10. Commit: the commit that names T-147. The check of each crate
with clones of podvm, podssh and podnet: 0 changes.

# T-148: `pod-hostprobe`: `fstatat` has no fallback to `statx` (loongarch64 before Linux 6.11)

**Source:** a run of the tests of `pod-enter` under `qemu-loongarch64` 8.2, 2026-11-10.
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`pod_hostprobe::sys::fstatat` calls `newfstatat(2)` only. A loongarch64
kernel older than 6.11 has no `newfstatat`, and `qemu-loongarch64` 8.2
has none. The call fails with `ENOSYS`. `RootDir::open` and the launch of
`pod-enter` use it, so four tests of `pod-enter` fail there (among them
`the_userland_entry_enters_a_workdir_in_a_volume`).

## Premise

- loongarch64 has `statx(2)` in each kernel. `fstat(2)` has the same gap,
  and no code of the workspace calls it: each stat call goes through
  `fstatat`, with `AT_EMPTY_PATH` for a descriptor.

## Approach

- `fstatat` calls `newfstatat` first (the other targets have it, and the
  seccomp filters expect it). On `ENOSYS` only, it calls `statx` with
  `STATX_BASIC_STATS` and the same `AT_*` flags, and it remembers the
  answer in an atomic.
- The result of `statx` is the same `Stat`. `st_dev` and `st_rdev` have
  the `makedev` encoding of the kernel.

## Prove

```sh
cargo test -p pod-hostprobe --lib statx
cargo zigbuild --tests -p pod-hostprobe -p pod-enter --lib --target loongarch64-unknown-linux-musl
```

## Done

2026-11-10. Commit: the commit that names T-148.

- `sys::statx_at(dirfd, path, flags)` and the fallback in
  `sys::fstatat` (`crates/pod-hostprobe/src/sys/statx.rs`). `SYS_STATX` is
  the number of `statx` of each target.
- Tests: `statx` gives the same `Stat` as `newfstatat` for a file, a
  directory, a character device, a symlink (with and without
  `AT_SYMLINK_NOFOLLOW`) and a descriptor (`AT_EMPTY_PATH`). The tests
  pass the check on the `statx` result alone where the host has no
  `newfstatat`. Plant: `st_ino: 0` in the conversion: three of the four
  tests fail.
- Under `qemu-loongarch64`: `pod-hostprobe` passes 103 of 103 tests, and
  `pod-enter` passes 147 of 148 tests. The failed test,
  `abi::tests::resolve_past_a_denied_listing`, runs its own binary as a
  child, which qemu-user cannot do without binfmt (`Exec format error`); T-149 records it.
  Under `qemu-aarch64`: `pod-hostprobe` passes 103 of 103 tests. x86_64
  native: 103 of 103.

# T-149: `pod-enter`: three tests fail under qemu-user only

**Source:** the runs of T-148 under `qemu-loongarch64` and `qemu-aarch64`, 2026-11-10.
**Category:** defect
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

Under qemu-user with no binfmt, `abi::tests::resolve_past_a_denied_listing`
fails on loongarch64 and aarch64: it runs `current_exe()` as a child and
the kernel answers `Exec format error`. On aarch64, `memfd::probe::tests::
a_memfd_sealed_against_exec_reads_as_no_exec` and `the_probe_answers_from_this_host`
fail: qemu-user does not enforce the memfd exec seal.

## Premise

- Read: the failures also show on the aarch64 run; none of them touches `statx`.

## Approach

- Skip the child test when the exec of `current_exe()` gives `ENOEXEC`,
  with a `skip_line` as the other host limits do.
- Skip the memfd tests where the host emulates the call (name the
  condition in the skip line), or run them natively only.

## Prove

```sh
cargo zigbuild --tests -p pod-enter --lib --target aarch64-unknown-linux-musl
qemu-aarch64 target/aarch64-unknown-linux-musl/debug/deps/pod_enter-*
```

## Done

2026-11-11. Two causes, two fixes:

- `abi::tests::resolve_past_a_denied_listing` skips with a named line when
  the exec of `current_exe()` as a child gives `ENOEXEC` (an emulator
  with no binfmt registration).
- `pod_hostprobe::interp::detect` had no evidence of a bare
  `qemu-<arch> ./binary` with no `QEMU_*` variable: the two memfd tests
  ran there and failed. Measured with `qemu-aarch64-static` 8.2: the
  kernel names the process `qemu-aarch64-st` in `/proc/self/comm`, and
  qemu does not emulate that file. `detect` now reads it
  (`interp::qemu_comm`, with the test
  `the_name_of_a_process_under_qemu_user_is_evidence`), so the memfd
  tests skip there with their line. A consumer of `detect` (podvm's
  probe) also sees this route now.

Prove: `cargo zigbuild --tests -p pod-enter --lib --target
aarch64-unknown-linux-musl`, then the binary under `qemu-aarch64-static`:
158 passed, 0 failed; the three tests print their skip lines. Natively,
`cargo test -p pod-hostprobe -p pod-enter --all-features` passes.
Not run: loongarch64. The kernel cuts a name to 15 bytes, so there the
name is `qemu-loongarch6`; the test above holds that case.

# T-155: `pod-enter`: the refusal of a missing `-w` names its volume; the reserved guest paths and the bound of pairs in `Maps::new` (PodUtils/podvm#226)

**Source:** PodUtils/podvm#226 (COORD-020 of podvm, the gaps 1 and 2 of its take of v0.12.0).
**Category:** feature
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

1. A `-w` directory that is missing inside a volume gets the refusal "is
   not a directory in the image": the message names the image, not the
   volume.
2. podvm keeps its reserved guest paths (`NEVER_MAPPED`: `/proc`, its own
   directory) and the bound of volume pairs outside `Maps::new`, so each
   caller checks them again.

## Premise

- Read: PodUtils/podvm#226; `crates/pod-enter/src/userland/maps.rs`.

## Approach

1. `check_workdir` over a `Maps` names the volume (`HOST:GUEST`) of a
   missing directory inside it, and the image otherwise; a test of each.
2. `Names` carries the reserved guest paths of the caller; `Maps::new`
   refuses a guest side under one of them, and a count over
   `MAX_MAP_PAIRS - 1`, each by name; a test of each. podvm's adoption map
   in the README drops its own checks.

## Prove

```sh
cargo test -p pod-enter
```

## Done

2026-11-10. Commit: the commit that names T-155. Read: PodUtils/podvm#226
and podvm `lib.rs` (`volume_maps`, `NEVER_MAPPED`, `MAX_VOLUMES`) at
`origin/main`.

- `GuestRoot::volume_label(guest)` (default `None`; `Maps` gives the
  volume `HOST:GUEST` of the lexical guest path). `check_workdir` and the
  entry with no chroot refuse a missing directory inside a volume as "in
  the volume HOST:GUEST is not a directory in the volume", and a missing
  directory elsewhere as before (the image). Test:
  `a_missing_workdir_in_a_volume_names_the_volume`.
- `Names::reserved` (`&'static [&'static str]`, default empty).
  `Maps::new(&names, rootfs, &volumes)` refuses a guest side that is a
  reserved path or is under one, by name, and a count over
  `MAX_MAP_PAIRS - 1` by the name of `names.maps`. Tests:
  `a_reserved_guest_path_is_refused_by_name`,
  `a_count_over_the_bound_is_refused_by_name`.
- `Maps::new` takes `&Names` first (API change). The README API section
  and the podvm adoption map are updated: podvm drops `volume_maps`,
  `MAX_VOLUMES` and `NEVER_MAPPED`.
- 2026-11-11: podvm took `v0.15.0` with this change at `d9fe511`
  (PodUtils/podvm#242, PodUtils/podvm@T-1716): `cargo test --workspace` 650 tests
  pass; `NAMES.reserved` is `["/proc", "/.podvm"]`. No defect.

# T-162: `pod-supervise`: kill the tree, a wall timeout, rusage and the count of residual processes (PodUtils/podvm#227)

**Source:** PodUtils/podvm#227 section 2.3 (COORD-021 of podvm, 2026-11-10)
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

A signal of the launcher reaches one pid. A grandchild of a payload survives `podvm stop` and `rm -f` (a measurement of the design review, M2, `05-lifecycle.log`). The ask A-V-07 of `docs/research/fleet/asks.md` (a run timeout that kills the tree, peak RSS and CPU in the report) routes to T-060, which is done, and no open entry owns the tree kill. The consumers: podrunner (it retires its per-unit shim when this lands), podvm (`PodUtils/podvm@T-1655`, the launcher of its WP-V4) and G-12 of GitHub #36 (end in 60 s). The tree kill serves podvm's `PodUtils/podvm@T-1655` and G-12 of #36 as well as podrunner. This entry waits like the other M8 entries. The owner can lift the wait.

## Premise

- Read: `crates/pod-supervise/src/launcher.rs:380` is `let _ = sys::kill(pid, sig);`, in the control handler of the launcher. It signals one pid. This agrees with the issue.
- Read: no entry of `TODO/` names a tree kill, a subreaper or a count of residual processes. This agrees with the issue.
- Read: `crates/pod-supervise/src` sets no `RLIMIT_FSIZE`.
- Not measured: the survival of a grandchild. Not run: the .NET runner under any `RLIMIT_FSIZE` (the design review measured that it dies at start below 2 TiB, M1 and M2, correction C21 of the review).
- Under "Who edits" a podvm session may change `pod-supervise` in podcrates.

## Approach

Add to `pod-supervise`:

- A kill of every descendant of a payload: a subreaper, or a cgroup where the host gives one.
- A wall timeout.
- Rusage of the tree.
- A count of the processes that stay after the kill (`residual_processes`: a number, or `unknown`).

Set no `RLIMIT_FSIZE` by default.

## Decision

The launcher is the child subreaper of the payload's tree
(`PR_SET_CHILD_SUBREAPER`): no privilege, no cgroup, no PID namespace. The
count 0 comes from `wait4(-1)` and `ECHILD`, the pids to kill from `/proc`
(the `children` files, else a scan of each `stat`). The kill happens at
the end of the payload, as the end of PID 1 of a namespace does; `stop`
and `kill` still signal the payload. The wall bound is a field of the
record (`set_wall`), so the launcher reads it at its start. A cgroup kill
(`cgroup.kill`) where the host gives one is not built: no consumer host
has a delegated cgroup today.

## Prove

```sh
cargo test -p pod-supervise --all-features
sh scripts/agent/check.sh
```

The tests that the issue names:

1. A payload forks a grandchild that calls `setsid` and ignores SIGTERM. The kill leaves 0 processes and the count says 0. A positive control: with the kill disabled, the count says 1 or more.
2. A wall timeout ends the tree within the timeout plus a stated bound.
3. When the host gives no way to find each descendant, the count is `unknown`, never 0.
4. No `RLIMIT_FSIZE` is set by default.

## Start condition

None. The owner named podrunner on 2026-11-11 (the ruling "podrunner online"; call C49: DR-1 and DR-2 are yes). Take the entry when podrunner's work needs it (`TODO/PROGRESS.md`, Work order).

Depends on no other crate.

## Done

2026-11-11. `pod-hostprobe`: `set_child_subreaper`, `reap_any`
(`wait4(-1, WNOHANG)`), `rusage_children` with `Usage`, `ECHILD`,
`SYS_GETRUSAGE`, `PR_SET_CHILD_SUBREAPER`. `pod-supervise`: the module
`end` (`End`, `Reason`, `Residual`), `launcher::tree` (`descendants`,
`parent_of`, `TREE_KILL_MS`, the sweep and the kill), `Settings::kill_tree`
and `Settings::proc`, `Container::wall_ms` and `Container::end`, the verb
`set_wall`; the launcher reaps each descendant that ends while the payload
runs, kills the tree at the end, and writes the end before it answers a
waiter. The crate sets no `RLIMIT_FSIZE`.

- Tests: 30 unit tests (new: `a_residual_is_a_number_or_the_word_unknown`,
  `an_end_keeps_each_field_through_json`,
  `the_parent_is_read_after_the_last_parenthesis`,
  `the_descendants_come_from_the_children_files`,
  `with_no_children_file_the_descendants_come_from_each_stat`,
  `with_no_proc_the_descendants_are_not_known`,
  `the_descendants_of_this_process_are_read_from_the_real_proc`),
  `tests/lifecycle.rs` (5 steps, unchanged) and the new `tests/tree.rs`
  (5 steps): the 4 tests of the issue are its steps
  `a_grandchild_that_left_and_ignores_sigterm_is_killed` (with
  `with_no_kill_the_grandchild_is_counted` as the positive control),
  `the_wall_bound_ends_the_tree` (800 ms, within the bound and
  `TREE_KILL_MS`), `with_no_proc_the_count_is_unknown_never_zero` and
  `no_limit_of_the_file_size_is_set`.
- Shown once: with the subreaper off, `tree` fails at the first step
  (`left: Unknown, right: Count(0)`).
- Next: podvm's launcher (PodUtils/podvm@T-1655) and podrunner's unit
  driver (PodUtils/podrunner#23) take it with the next tag.

# T-166: `pod-hold`: the hold of each connect of a payload, from podnet's `podnet-netns` (PodUtils/podnet#422)

**Source:** PodUtils/podnet#422 (COORD-014 of podnet; PodUtils/podnet@T-232); T-109; PodUtils/podnet#414.
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** L
**Status:** done

## Problem

podnet holds each connect of a payload with no root (`podnet pod`: the
seccomp user notification backend and the netns backend). podvm (A-V-02,
PodUtils/podvm@T-1665) and podrunner (G-04, T-160) need the same hold.
The generic core is in `PodUtils/podnet:crates/podnet-netns`.

## Premise

- Read: PodUtils/podnet#422 and #414: the cut is the generic core only:
  the loop of the seccomp user notification, the read of the memory of the
  child, the table of descriptors and the reply, and the netns hold of a
  `connect`. The pod CLI, the rules of the upstreams, the routes and `pod
  doctor` stay in `podnet-netns`. The seam is the reply ioctl; a memfd can
  stand in for the memory of the child in tests.
- Since #414 the core got PodUtils/podnet@T-150 (an accepted inbound
  connection goes to the program with no relay), PodUtils/podnet@T-151 (a
  relay buffer of 64 KiB), PodUtils/podnet@T-181 (the flow budget from
  `RLIMIT_NOFILE`, with `EMFILE`), PodUtils/podnet@T-226 (the netns egress
  loop and a bounded buffer) and PodUtils/podnet@T-109 to
  PodUtils/podnet@T-113 (detach, join, the netns backend first,
  `--host-gateway`). Import from podnet `main` at `9e8d177` or later.
- Not read: `PodUtils/podnet:crates/podnet-netns` in full.

## Approach

The procedure of `docs/design.md`, section 8: import with `SOURCES`, adapt
(settings, typed failures, no environment reads; the level of the hold:
`enforced`, `advisory` or `none`, with its proof), the tests with a memfd
for the child's memory, the README with the adoption map of podnet, the
trial of podnet (`pod-hold.patch` of `adoption/podnet`). From the import, the
files of the core in podnet take fixes only.

## Prove

```sh
cargo test -p pod-hold
sh scripts/adoption.sh podnet pod-hold
sh scripts/agent/check.sh
```

## Done

2026-11-11, done (podnet's take; see the last line). 2026-11-10, partial:
the take of podnet remained (the trial, the cross tests and the release
`v0.15.0` were done).
Read: podnet `main` at `9e8d177` (`PodUtils/podnet:crates/podnet-netns`). Commits
`7ca9a36` (the import of `seccomp/filter.rs`, `seccomp/notif.rs`,
`seccomp/mem.rs`, `seccomp/mod.rs`, `fdtable.rs`, `netns/mod.rs`,
`netns/tun.rs`, `netns/fdpass.rs`, with `SOURCES`), `11e8cad` (the
adaptation), `24c8697` (a defect of `pod-hostprobe`), `89ccffc` (the
static probe, the fuzz target `hold_sockaddr`, three plants), `759c4ef`
(README, the row of `docs/design.md`, call C48, the trial files) and the
commit of this record.

- API: `abi` (the ioctl numbers from the encoding of the target),
  `filter` (`Calls::sockets`, `program`, `install`), `spawn` (`resolve`
  with the caller's `PATH`, `Program`, `start` gives `Started { listener,
  pid, hold }`), `listen` (`Listener::recv`: poll, `NOTIF_RECV`, the open
  of the memory, `NOTIF_ID_VALID` after the open), `mem` (`Memory`,
  `Access` in place of podnet's `PODNET_NETNS_TEST_MEM`), `sockaddr`,
  `reply` (the seam `Answer`; `Recorded` for tests), `table`
  (`Table<T>`, the flow budget, `Refusals` with a `const fn new`),
  `netns` (`enter`, `announce`, `answer`, `Tun`, `create_tun`, the
  descriptor passing), `level` (`Level` `enforced`, `advisory`, `none`;
  `Hold { level, proof, not_held }`). No environment read, no global, no
  output; each failure is a `pod_core::Failure` of the stage `host`. The
  raw `prctl`, `seccomp`, `unshare` and `prlimit64` go through
  `pod-hostprobe`. 19 source files, each under 500 lines.
- Cut (call C48): the handlers of each call, the dials and the hand-off,
  the relays, the rules, the upstreams, the fake DNS, the router of the
  TUN, the reaper, `pod doctor` and the CLI stay in podnet. T-169 (the
  dial state of a held `connect`) and T-170 (the netns hold of a SYN) ask
  podnet whether two more parts are in its cut.
- Defects found and fixed, each with its test: podnet's x86_64 ioctl
  numbers of `NOTIF_ADDFD`, `NOTIF_ID_VALID` and `TUNSETIFF` are wrong on
  powerpc64le (`each_ioctl_number_follows_the_encoding_of_its_architecture`;
  the trial found the same `TUNSETIFF` in podnet's `src/warp/tun.rs`, and
  the patch fixes it there); podnet's `recv_fd` kept the extra descriptors
  of a message (`a_message_with_no_descriptor_or_with_two_is_refused`);
  `read_word` panicked past the bytes and an address past `i64::MAX`
  overflowed (`a_word_is_read_only_inside_the_bytes`,
  `a_read_past_the_memory_fails`); `pod-hostprobe`'s probe
  `seccomp(NEW_LISTENER)` put the filter pointer at offset 8 on a 32-bit
  target and got EFAULT (`the_filter_pointer_is_where_the_kernel_reads_it`
  fails on the old code as an i686 binary under qemu-i386, left 8 right 4);
  podnet's README named Linux 5.0, but `SECCOMP_ADDFD_FLAG_SEND` needs 5.14
  (the patch corrects it).
- Tests: `cargo test -p pod-hold --all-features`: 28 unit tests (podnet's
  `the_filter_covers_the_bypass_surfaces` and the four tests of
  `fdtable.rs` keep their names) and `tests/hold.rs` (no libtest: a
  refused connect fails with the errno of the reply, a socket call gets
  the descriptor of the supervisor, a namespace gives its TUN to the
  parent): each passed on this host, none skipped. The fuzz target
  `hold_sockaddr` (four seeds) passes the stable test of `fuzz/`. The
  plants `hold-reply-errno-positive`, `hold-address-read-past-length` and
  `hold-filter-foreign-abi-passes` are each caught
  (`sh scripts/plant-each.sh`). `sh scripts/agent/check.sh` exit 0 on the
  rebased tree.
- Trial (`adoption/podnet/pod-hold.{patch,run,commit}`, podnet `9e8d177`,
  tag `v0.15.0`): before the rebase onto `53dd5da`, a run passed each step
  up to `! git grep` of the written ioctl numbers (the record, size,
  layers, build, cargo-deny bans and sources, `cargo tree -i pod-hold`),
  which found `src/warp/tun.rs`. Before that run, on a scratch copy, the
  glue built with clippy clean, `cargo test -p podnet-netns` (77 + 1
  tests) passed, and the integration tests `pod_seccomp_tcp`, `_dgram`,
  `_fds`, `_handoff`, `_nonblock`, `_rules`, `_slow_dial`, `_diff`,
  `pod_backend_line`, `pod_usage`, `af_inet_kernel` and `doctor` passed
  (56 tests). The patch then changed (`src/warp/tun.rs`, the root
  `Cargo.toml`, the README of podnet-netns, a remap of `PodUtils/podnet:TODO/pods.md`); no
  run of the trial covers the patch as it is now.
- 2026-11-10: the trial of podnet passed in one clean run on the tree of
  `27a4ccd` (adoption 42872309501), after its grep for the old ioctl
  numbers read code lines only. The nightly 42872313856 ran the tests of
  `pod-hold` on the seven cross targets under qemu-user: the 28 unit tests
  pass; `hold` prints `SKIP:` for the network namespace and twice for the
  seccomp listener (`ENOSYS` under qemu-user). The release `v0.15.0` is
  at `27a4ccd`; the hand-off is GitHub #54 (COORD-029).
- 2026-11-11, done: podnet took the crate at `b759937`
  (PodUtils/podnet#424, COORD-015 of podnet; podnet's `check.sh` passed at
  `5599012`). `crates/pod-hold/SOURCES` has `taken b759937`;
  `python3 -I scripts/check-sources.py --clone PodUtils/podnet=CLONE pod-hold`
  reads 9 files, 0 changes. The trial files `adoption/podnet/pod-hold.*`
  are removed. podnet answered T-169 yes and T-170 yes in two steps.

# T-169: `pod-hold`: the dial state of a held `connect` of the seccomp hold, from podnet's `connect.rs`

**Source:** T-166 (the cut of `pod-hold`).
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

`pod-hold` (T-166) has the receive, the memory, the reply and the table,
but each consumer that holds a `connect` must write the state of a dial
again: the call waits while the supervisor dials, a second call waits for
the same dial, a nonblocking `connect` gets `EINPROGRESS` and a second
one `EALREADY`, `getsockopt(SO_ERROR)` waits for the end of the dial, a
`close` during the dial gives `ECONNABORTED`, and a dial with no waiting
call is dropped. podnet has this state in
`PodUtils/podnet:crates/podnet-netns/src/connect.rs` (`Dials`, `Waiting`,
`ErrorCall`, `finish`, `answer_error`), bound to its `Sock` entry, its
hand-off and its relays. podvm (PodUtils/podvm@T-1665) and podrunner
(T-160) need the same state with their own dial.

## Premise

- Read: `connect.rs` at podnet `9e8d177` (411 lines): the state uses only
  the reply, `notif_id_valid`, `child_fd_flags`, the memory of the task
  and the table; the dial is a task that sends a `Dialed` back to the loop.
- Not read: how `crate::handoff` and `crate::sockopt::Connected` would
  take a generic entry.
- PodUtils/podnet#414 names the loop, the memory, the table and the reply,
  not this state. podnet's view (PodUtils/podnet#424, 2026-11-11): yes, the
  state is in the cut. Import it from podnet `main` at `5599012` or later;
  from the import, `connect.rs` takes fixes only in podnet.

## Approach

Ask podnet (a COORD issue of podcrates `(to podnet)`) whether the dial
state is in the cut. If so: import `connect.rs` (its state only) as
`pod_hold::connect` with a trait of the consumer for the dial and the end
of a dial, keep each test with its name, and test the waits with a
`Recorded` reply and a memfd.

## Prove

```sh
cargo test -p pod-hold
sh scripts/adoption.sh podnet pod-hold
```

## Done

2026-11-11. `pod_hold::connect` from podnet `connect.rs` at `5599012`
(podnet's answer PodUtils/podnet#424): `Dials<K>` with `join`, `start`,
`wait_error`, `finish`, `in_progress`, `nonblocking`; `Finished` with
`answer` and `taker`; `write_error`, `write_error_to_task`; `Caller`,
`ErrorCall`, `Joined`, `Started`. The dial is the consumer's: it calls
`join`, decides, `start`s, dials, and calls `finish` at the end. `SOURCES`
lists `connect.rs` in the block at `5599012` (0 changes).

- Tests (9): `a_second_blocking_connect_waits_for_the_same_dial`,
  `a_nonblocking_connect_gets_einprogress_then_ealready`,
  `a_blocking_dial_with_no_waiting_call_is_dropped`,
  `a_nonblocking_dial_stays_with_no_waiting_call`,
  `so_error_waits_and_the_first_takes_the_error`,
  `a_connect_takes_the_error_before_so_error`,
  `a_call_that_left_gets_no_answer_and_a_failed_write_gets_efault`,
  `the_taker_is_the_blocking_connect_or_the_one_so_error`,
  `a_short_option_length_writes_that_many_bytes`, with a `Recorded` reply
  and a memfd memory. podnet's own tests of the state are its integration
  tests (`pod_seccomp_nonblock`, `_slow_dial`), which stay in podnet.
- Shown once: with each `SO_ERROR` given the error,
  `so_error_waits_and_the_first_takes_the_error` fails.
- Partial: podnet's take of the patch remains. The crate code is in
  `v0.16.0` and `v0.17.0`; podnet's `connect.rs` takes fixes only.
- 2026-11-11, the adoption trial (`docs/design.md`, section 8, step 7):
  `adoption/podnet/pod-hold-connect.{patch,run,commit}` at podnet
  `d3fad49` (`main`), each `pod-*` dependency on the tag `v0.17.0`, with
  the step 1 of T-170. podnet's `connect.rs` is glue over
  `pod_hold::connect`: podnet's `Dials` holds a
  `pod_hold::connect::Dials<Key>` and the channel of the ends of the
  dials; `handle_connect` calls `join`
  and `start`; `finish` calls `Dials::finish`, `Finished::taker` and
  `Finished::answer` with `write_error_to_task` and podnet's
  `mem_access()`. `ErrorCall` and `handoff::Caller` are the crate's types.
  `Waiting`, `answer_error` and the state code leave podnet: 71 lines of
  Rust (`connect.rs` 96 out, 30 in; `handoff.rs` 6 out, 1 in). Its
  integration tests `pod_seccomp_nonblock`, `_slow_dial`, `_handoff` stay
  and pass. `sh scripts/adoption.sh podnet pod-hold-connect` passed each
  step: the record, the size and the layers of the files, `cargo build`,
  cargo-deny bans and sources, no copy of the moved items, clippy of the
  workspace with `-D warnings`, `cargo doc` of `podnet-netns`, the tests
  of `podnet-netns` (80: the 84 of podnet less the 4 tests that moved
  with T-170) and the library tests of `podnet-core`, `podnet-serve` and
  `podnet-vnet`, the integration tests `pod_seccomp` (with the netns
  tests), `pod_seccomp_tcp`, `_dgram`, `_fds`, `_handoff`, `_nonblock`,
  `_rules`, `_slow_dial`, `_diff`, `pod_backend_line`, `pod_usage`,
  `af_inet_kernel`, `doctor`, `docs` and `api_surface`, the fuzz targets
  with their lock, rustfmt, the toolchains, and the static musl binary.
- The changes of podnet in the patch that are not glue: under the test
  hook `PODNET_NETNS_TEST_MEM=none`, an `SO_ERROR` that waits for a dial
  now gets `EFAULT` with no write: podnet's `answer_error` wrote through
  `process_vm_writev` (`write_mem_to` with the pid), against the hook.
  With no hook, the write is the same. The record of podnet: the citation
  `netns/packet.rs:62` of `PodUtils/podnet:TODO/later.md` names
  `rst_reply_for_syn` of `pod_hold::packet`; `cargo todo remap` of the
  README of `podnet-netns`. The tag `v0.17.0` needs no other change of podnet (the
  "Breaking" of `v0.16.0` and `v0.17.0` names no item that podnet uses).
- 2026-11-11, the take: podnet took the patch at `e4d4049`
  (PodUtils/podnet#426, COORD-017 of podnet; `git apply --3way` on
  `90428ed`, one comment of `podnet-netns/Cargo.toml` merged by hand, one
  citation remapped; each line of the `.run` passed but the zig build,
  which podnet's CI does). `crates/pod-hold/SOURCES` has
  `taken e4d4049` in the block of `connect.rs` and `netns/packet.rs`:
  `check-sources.py` reads 0 changes. The trial files leave `adoption/`;
  GitHub #59 is closed. podnet accepts the change under
  `PODNET_NETNS_TEST_MEM=none`.

# T-170: `pod-hold`: the netns hold of a SYN (dial first, then accept or RST), from podnet's netns router

**Source:** T-166 (the cut of `pod-hold`).
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The netns hold of `pod-hold` (T-166) gives the namespace and its TUN. The
hold of each `connect` on the TUN is in podnet's router: the stack holds
each SYN, dials the upstream first, and accepts the connection only when
the dial succeeds, else it answers a RST
(`PodUtils/podnet:crates/podnet-netns/src/netns/backend.rs`, `hold_syns`,
`accept_flows`; `netns/packet.rs`, `parse_tcp_syn`, `rst_reply_for_syn`,
`ipv4_checksum`). A consumer of the netns hold (podvm `--net`) needs the
same hold of a SYN with its own dial.

## Premise

- Read: the names of the functions of `backend.rs` (621 lines) and
  `packet.rs` (110 lines) at podnet `9e8d177`. The router runs on smoltcp
  0.12 (`medium-ip`); `pod-netstack` (T-023) is on smoltcp 0.14.
- Not read: the router in full; its ties to the rules, the flow log and the
  UDP flows of podnet.
- podnet's cut (PodUtils/podnet#414) names "the netns hold of a connect":
  T-166 read it as the namespace and the TUN. podnet's view
  (PodUtils/podnet#424, 2026-11-11): yes, in two steps. Step 1:
  `netns/packet.rs` with its tests, now. Step 2: the hold of a SYN of
  `netns/backend.rs`, after PodUtils/podnet@T-174 (the veth wire) and the move
  of its router to the smoltcp of `pod-netstack`; podnet writes a COORD
  issue when both are done.
- PodUtils/podnet#451: PodUtils/podnet@T-174 is done (podnet `295a41d`,
  `96cd1b7`, `fc97ef9`, `8e726b0`). The move of the router onto
  `pod-netstack` is PodUtils/podnet@T-249 (the device of
  `PodUtils/podnet:crates/podnet-netns/src/netns/tun.rs`, both wires, under the stack; the
  hold of a SYN and the flow table stay). Its COORD issue with the commit
  is the start of step 2.

## Approach

Ask podnet with the COORD issue of the entry above. If the hold of a SYN
is in the cut: import `packet.rs` (`parse_tcp_syn`, `rst_reply_for_syn`,
`ipv4_checksum`, with the tests `parses_a_tcp_syn` and
`the_rst_reply_answers_the_syn` of podnet's `lib.rs`) and the hold of a
SYN of `backend.rs` behind a trait of the consumer for the dial, after
the smoltcp version of `pod-netstack` is settled.

## Prove

```sh
cargo test -p pod-hold --all-features
sh scripts/plant-each.sh hold-syn-accepted-before-the-dial hold-syn-past-the-budget-to-the-stack
sh scripts/adoption.sh podnet pod-hold-syn
```

## Done

2026-11-11, partial: step 1 is done; step 2 waits for podnet's COORD
issue (PodUtils/podnet@T-174 and the smoltcp of `pod-netstack`).

- Step 1: `pod_hold::packet` (`parse_tcp_syn`, `rst_reply_for_syn`,
  `ipv4_checksum`) from podnet `netns/packet.rs` at `5599012`, with
  podnet's tests `parses_a_tcp_syn` and `the_rst_reply_answers_the_syn`
  under their names; `dns_reply_packet` stays in podnet. `SOURCES` has a
  second block `repo PodUtils/podnet 5599012...`;
  `python3 -I scripts/check-sources.py --clone PodUtils/podnet=CLONE pod-hold`
  reads 0 changes in both blocks.
- Defect found and fixed: podnet's `parse_tcp_syn` took an IHL under 5
  (it read the ports and the flags from the IP header) and a later
  fragment. Test `a_header_shorter_than_20_bytes_is_no_syn` fails on the
  old check (`IHL 0`) and passes now; `a_fragment_past_the_first_is_no_syn`,
  `a_syn_with_options_in_the_ip_header_is_read_past_them`,
  `the_checksum_of_an_odd_length_pads_with_zero`. podnet hears of the
  defect in its COORD issue.
- Fuzz target `hold_syn` (four seeds: a SYN, a SYN with IP options, an
  ACK, a header of IHL 4): `cargo test --manifest-path fuzz/Cargo.toml`
  passes.
- 2026-11-11, step 1, the adoption trial:
  the patch `pod-hold-connect.patch` of the trial of podnet (removed at the take) (podnet `d3fad49`, tag
  `v0.17.0`; with T-169) passes each step. podnet's
  `netns/packet.rs` takes `parse_tcp_syn`, `rst_reply_for_syn` and
  `ipv4_checksum` of `pod_hold::packet` and keeps `dns_reply_packet`. The
  tests `parses_a_tcp_syn`, `the_rst_reply_answers_the_syn`,
  `a_header_shorter_than_20_bytes_is_no_syn` and
  `a_fragment_past_the_first_is_no_syn` leave podnet's `lib.rs` (each is
  in `pod_hold::packet` under its name). 168 lines of Rust left podnet
  (`packet.rs` 70 out, 5 in; `lib.rs` 103 out). The netns tests of
  `pod_seccomp` (the RST of a refused SYN) pass. Partial: podnet's take
  of the patch remains.
- podnet ported the fix of `parse_tcp_syn` at `2efa720`
  (PodUtils/podnet@T-234). The second block of `SOURCES` reads `2efa720`
  now, with the blob of that commit: `check-sources.py` with a clone of
  podnet reads 0 changes in both blocks. After the take, the Prove of
  PodUtils/podnet@T-234
  (`cargo test -p podnet-netns --lib no_syn`) runs no test: the two tests
  are in `pod-hold`.
- Step 1 taken: podnet `e4d4049` (see T-169). Step 2 waits for podnet's
  COORD issue (its T-174 and the move of its router to the smoltcp of
  `pod-netstack`).

2026-11-12, step 2, partial: the import, its tests, its plants and the
trial patch are done. The run of the trial in `adoption.yml` (the lead,
after the push), the tag `v0.29.0` and podnet's take remain.

- `pod_hold::syn_hold` (the feature `syn-hold`; calls C90, C91, C100)
  from `PodUtils/podnet:crates/podnet-netns/src/netns/backend.rs` at
  `a7350b2` (podnet's PodUtils/podnet@T-249, PodUtils/podnet#451):
  `SynHold<U, E>` on a stack of `pod-netstack` (`new`, `pump`,
  `activity`, `next_flow`, `shutdown`), the dial of the consumer (`Dial`;
  a closure is one), `Syn`, `Flow` with its `Open`, `Refused`,
  `Settings`, `Stats`. The hold of each SYN, the release or the RST, the
  listeners of the ports, the match of an accepted stream to its upstream
  and the late dial came; the dial, the rules, the flow log, the copy of a
  flow, the DNS, the UDP flows and the device of the wire stay in podnet.
  `SOURCES` has a fourth block (podnet `a7350b2`: `backend.rs` and the
  tests `tcp.rs` and `netns_tcp.rs` of `PodUtils/podnet:tests/pod_seccomp`);
  `check-sources.py` with a clone of podnet reads 0 changes in each of
  the four blocks. Commits `818f581` (the import), `d02c0ce` (the plants),
  `671358e` (the trial) and the commit of this section.
- Tests (8, `crates/pod-hold/src/syn_hold/tests.rs` and
  `crates/pod-hold/src/syn_hold/tests/many.rs`), over a link to a payload
  stack of `pod-netstack`: podnet's
  `netns_backend_refuses_connect_when_the_upstream_is_dead` (one RST on
  the link, `ECONNREFUSED` in less than 3 s, no SYN for the stack),
  `netns_backend_takes_a_burst_of_connects_to_one_port` (48, no RST) and
  `netns_backend_has_no_cap_on_destination_ports` (300 ports, then six
  again) keep their names;
  `a_syn_whose_dial_succeeds_is_accepted_and_carries_bytes_both_ways` (256
  KiB both ways, a closure as the dial),
  `the_stack_answers_a_syn_only_after_its_dial`,
  `a_syn_sent_again_while_its_dial_waits_is_held_once`,
  `a_syn_past_the_budget_waits_for_room`,
  `a_syn_of_a_packet_names_its_addresses`. Each passes in 20 of 20 runs.
- Plants: `hold-syn-accepted-before-the-dial` (the hold gives a new SYN to
  the stack at once) fails `the_stack_answers_a_syn_only_after_its_dial`
  in 20 of 20 runs; `hold-syn-past-the-budget-to-the-stack` (podnet's
  behaviour past the budget) fails `a_syn_past_the_budget_waits_for_room`
  in 10 of 10 runs (held 1, late 1, flows 2).
- Defects of podnet's router at `a7350b2`, for the COORD answer to
  PodUtils/podnet#451 (the crate does not have them):
  1. Past the budget (`held + open >= MAX_TRACKED_FDS`), `hold_syns` gave
     a new SYN to the stack. The limit of the stack counts its own sockets
     only, so the stack accepted the connection with no dial, and the dial
     started after the accept: the `connect` of the program succeeded
     before its dial, and a failed dial then reset it. The comment "The
     table is full: the stack drops the SYN" does not hold. The budget also
     did not count the upstreams of the released SYNs (call C91).
  2. `accept_flows` dialed a late connection (`Source::Dial`) with
     `spawn_dial` also when its target is the gateway: the rules and the
     proxies, not `spawn_gateway_dial` and `--host-gateway` (call C100).
  3. The map of the open flows kept one entry for each address tuple: a
     connection that the stack accepted again while its first flow lived
     left the map at the end of the first flow.
  4. podnet's record: the Premise of PodUtils/podnet@T-176
     (`PodUtils/podnet:TODO/later.md`) cites `HeldSyn` and `reap_closed`
     at lines of `backend.rs` that hold other code at `a7350b2`
     (`reap_closed` is gone); the remap of the patch moves one of its
     citations and names one for a read.
- The trial (adoption/podnet/pod-hold-syn.patch, its `.commit` and its
  `.run`): podnet's `backend.rs` takes `SynHold` (666 to 452 lines);
  `TcpRoute` implements `Dial`; `netns/packet.rs` keeps `ipv4_checksum`;
  each tag moves to `v0.29.0`. Run here on the patch, with the crates of
  this tree: `podnet-todo check`, `check-size.py`, `check-layers.py`,
  clippy of `podnet-netns`, its tests (116, `raw` 1, `veth` 1), the 41
  tests of `pod_seccomp` (the 14 netns tests among them, also the 48 MiB
  download in 1.76 s), `docs`, `api_surface`, `doctor`, `pod_backend_line`,
  rustfmt of the changed files, the grep for a copy of the moved items;
  each passes, as root with no skip. Not run here (the disk): the build of
  the workspace, `cargo deny`, the clippy of the workspace, the docs, the
  other test targets, `root-netns-tests.sh`, the fuzz targets, the
  toolchains, the static musl binary. The lead runs the trial in
  `adoption.yml`.
- Checks: `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets --all-features -- -D warnings`,
  `cargo test -p pod-hold -p pod-netstack --all-features` (`pod-hold` 59,
  `tests/hold.rs` 3 and `tests/veth.rs` 1 as root with no skip;
  `pod-netstack` 61), `cargo todo check`, `check-design-paths.py`,
  `check-size.py`, `check-probe.py`, `plant-each.sh --apply`: each exits 0.
  The static probe turns `syn-hold` on and runs one refused SYN
  (`examples/static-probe/src/hold.rs`): clippy compiles it; it did not
  run here.
- Not measured: the hold under a real payload with a namespace in this
  tree (podnet's pod tests above measure it through the trial), the rate
  of a flow through the hold, and the time limits of `Settings`.

2026-11-12, step 2 done: podnet took it. `adoption.yml` run 43164667092
passed the trial at podnet `a7350b2`; `v0.29.0` is at `a4d3630`
(GitHub #87).

- podnet `98d8a71` (PodUtils/podnet#478) takes `tag = "v0.29.0"` with
  adoption/podnet/pod-hold-syn.patch of the tag (a three-way apply on its `main`;
  the conflicts were the tag lines alone) and `podnet-netns` on the
  feature `syn-hold`. podnet ran the steps of the `.run` but
  `check-consumer-toolchains.py` and the musl build (its CI builds the
  static binaries); `root-netns-tests.sh` as root and `cargo test
  --workspace --no-fail-fast` had 0 failures. The four defects of the
  router at `a7350b2` are gone with the crate (podnet's answer).
- The `taken` line of the block of `a7350b2` in `crates/pod-hold/SOURCES`:
  `python3 -I scripts/check-sources.py --clone PodUtils/podnet=CLONE
  pod-hold` gives 3 files, 0 changes. The trial files
  `adoption/podnet/pod-hold-syn.*` are gone; the adoption map of
  `crates/pod-hold/README.md` names the take.

# T-173: Fuzz targets of four parsers of untrusted input: `pod-oci` (two), `pod-extract`, `pod-enter` (PodUtils/podvm#241)

**Source:** PodUtils/podvm#241 (COORD-023 of podvm, 2026-11-11; podvm's
ruling "Fuzz targets", 2026-11-11; PodUtils/podvm@T-1684)
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podvm asks for one `cargo fuzz` target for each of four parsers that read
untrusted input, now in podcrates (the tag `v0.12.0`, `393158f`):

- `pod-oci`, `Fetched::parse(bytes, header_media_type)`
  (`crates/pod-oci/src/spec.rs`): the bytes of a manifest or an index that
  a registry sends.
- `pod-oci`, `parse_challenge(challenge)`
  (`crates/pod-oci/src/registry/wire.rs`): the `WWW-Authenticate` value of a
  registry.
- `pod-extract`, `apply_layer(..)` (`crates/pod-extract/src/apply.rs`): a
  tar layer, plain or compressed, that an image author controls.
- `pod-enter`, `Elf::parse(path, bytes)` (`crates/pod-enter/src/abi/elf.rs`):
  the ELF header and the program headers of a payload.

A workflow runs each target for a fixed time on a schedule and on dispatch,
and uploads a crash input; each crash input becomes a regression test of
the crate before its fix lands. podvm expects no change of API. podvm keeps
the targets of its interposer's ELF and shebang reader and the watch of its
own vendored sources.

## Premise

- Read: the issue; `fuzz/` holds 21 targets and `fuzz/README.md` lists
  them; `.github/workflows/fuzz.yml` and `scripts/fuzz.sh` run them.
- Not read: the four functions at the head of `main`; whether
  `apply_layer` can take a layer from memory with no file system writes.

## Approach

Add the four targets to `fuzz/` with seeds in `fuzz/seeds/` (a real
manifest and index, three `WWW-Authenticate` values, a small tar layer in
each compression, a static and a dynamic ELF), a row each in
`fuzz/README.md`, and each target in the run of `fuzz.yml`. The layer
target writes into a fresh temporary directory and removes it. Each crash
input that a run finds goes to the crate's tests first.

## Prove

```sh
sh scripts/fuzz.sh
cargo test --manifest-path fuzz/Cargo.toml
```

## Done

2026-11-11. Four targets in `fuzz/` (`src/targets/podvm.rs`), each with its
binary, its seeds and a row of `fuzz/README.md`; `fuzz.sh` and `fuzz.yml`
run each target that `cargo fuzz list` gives, so they need no change.

- `oci_manifest`: `Fetched::parse` with the `Content-Type` that the first
  byte picks; the document's own `mediaType` decides over the header; the
  digest of each descriptor reads. Seeds: an OCI index and manifest, a
  docker list and manifest named by the header only, an index whose header
  says manifest, a schema 1 document.
- `oci_challenge`: the parse of a challenge, reached through
  `pod_oci::registry::fuzz_parse_challenge` (the new feature `fuzzing` of
  `pod-oci`, or `--cfg fuzzing`; no consumer turns it on). Seeds: Docker
  Hub, a scope with a comma in quotes, Basic, `Bearer` alone.
- `extract_layer`: `remove::collect`, `remove::apply` and `apply_layer` in
  a fresh directory with the root four levels deep; no entry escapes the
  root; the run removes the directory and follows no symbolic link. Seeds:
  one layer in tar, gzip and zstd (a frame of raw blocks, written here),
  and three that climb out (`../../escape`, a link to `../../../..`, an
  absolute path).
- `elf_header`: `abi::Elf::parse` gives the same answer twice and no NUL in
  a name. Seeds: a static and a dynamic program of two lines, built with
  `cc -Os -s`, and a shell script.

`pod_enter::abi::Elf::parse`, `pod_oci::spec::Fetched::parse` and
`pod_extract::apply::apply_layer` were public: no change of API.

- Tests: `the_seeds_of_the_podvm_targets_read` (each document parses as
  its type, each compression applies and writes `etc/hostname`, each
  program parses) and `a_challenge_keeps_a_comma_in_quotes`;
  `each_target_survives_mutations` runs 2000 mutations of each seed set.
- Not run here: libFuzzer (the pinned nightly is not installed in this
  sandbox). The next run of `fuzz.yml` runs the four targets.

# T-175: `pod-supervise`: private modes for the table, the logs and each container directory (PodUtils/podvm#244)

**Source:** PodUtils/podvm#244 (COORD-026 of podvm, 2026-11-11; PodUtils/podvm@T-1715)
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The container table keeps each `-e` value of a record. At `v0.15.0` the
crate creates each file and directory with the umask of the process: with
umask 022 the table and each log are 0644 and each container directory is
0755, so another user of the host reads the environment of each payload
and what it prints. podvm creates its own store root with mode 0700; for
podrunner's store of each unit (PodUtils/podrunner#23) the modes of the
crate are the protection.

## Premise

- Read: `write` of `table.rs` (`std::fs::write` of the staging file, then
  the rename; `create_dir_all` of `staging/`), `create` of `lib.rs` and
  `start` of `launcher.rs` (`create_dir_all` of the container directory),
  the log of `start` (`OpenOptions` with no mode).
- Not changed: the locks (mode 0644): they hold no data, and podvm's image
  store takes the same `Lock`.

## Approach

The staging file of the table is a new file of mode 0600 (`create_new`,
then the rename), so the table is 0600 from its first byte; each container
directory and `staging/` are 0700 (`DirBuilder` with mode 0700); the log
opens with mode 0600. No change of API.

## Prove

```sh
cargo test -p pod-supervise --test tree
```

The step `the_state_is_private_under_umask_022` sets umask 022, runs a
payload, and reads the modes of the table, `staging/`, the container
directory and the log.

## Done

2026-11-11. `pod_supervise::private_dir` (a `DirBuilder` of mode 0700) for
each container directory and `staging/`; the staging file of the table is
a new file of mode 0600 (`create_new`, then the rename); the log opens
with mode 0600. `pod-hostprobe` gives `sys::umask` for the test. The step
`the_state_is_private_under_umask_022` of `tests/tree.rs` passes; with the
table written at mode 0644 it fails (`the table`, left 420, right 384).
Goes out with `v0.16.0`, with T-162 (PodUtils/podvm#244). podvm took
`v0.16.0` at `894cf13` (654 tests; its own test of the modes under umask
022).

# T-176: `pod-enter`: resource limits and `no_new_privs` in the child before the exec (PodUtils/podvm#245)

**Source:** PodUtils/podvm#245 (COORD-027 of podvm, 2026-11-11; PodUtils/podvm@T-1655)
**Category:** feature
**Milestone:** M6
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

`podvm run --ulimit NAME=SOFT[:HARD]` (`nofile`, `nproc`, `cpu`, `as`,
`core`, `fsize`) and `--security-opt no-new-privileges` must take effect in
the payload and in no other process. podvm cannot set them in its own
process before the fork: `fsize` would limit podvm's own writes, and
`PR_SET_NO_NEW_PRIVS` cannot be undone. The child of `pod-enter` is the
place: after the root change and the `chdir`, before the `execve`. At
`v0.15.0` `Plan` has no field for either, and the child of `spawn_with`
sets neither.

## Premise

- Read: the issue, with its tests.
- Not read: `crates/pod-enter/src/plan.rs` and `launch.rs` at the head of
  `main`; whether each entry (the chroot sequence, the namespace rung, the
  userland entry, the memfd launch) shares one child path.

## Approach

`Plan` gets `limits: Vec<Limit>` (`Limit { resource, soft, hard }`, the
resource a named `RLIMIT_*` of `pod-hostprobe`, `u64::MAX` for infinity)
and `no_new_privs: bool`, both empty or false by default. The child of
each entry calls `prlimit64` on itself for each limit, then
`PR_SET_NO_NEW_PRIVS` where asked, after the `chdir` and before the
`execve` or `execveat`, with async-signal-safe calls only. A refused call
reports its step on the readiness pipe (`setrlimit(RLIMIT_NPROC): EPERM`),
and the payload does not run. `pod-supervise`'s record carries both fields
for a detached run.

## Prove

```sh
cargo test -p pod-enter --all-features
```

The tests of the issue: `fsize` of 1 MiB gives `EFBIG` past 1 MiB in the
child while the parent writes more; `nofile = 64` shows in
`/proc/self/limits` of the child; `no_new_privs` shows `NoNewPrivs: 1` in
the child and `0` in the parent; a hard limit over the parent's hard limit
is refused by name.

## Done

2026-11-11. `pod_enter::limits` (`Limit`, `Resource`, `INFINITY`, `MOST`)
and `Plan::{limits, no_new_privs}`; `pod-hostprobe` gives
`sys::set_rlimit`, `sys::set_no_new_privs` and `sys::rlimit` (the numbers of
the kernel of each architecture, from `linux_raw_sys`). The one child path
of each entry (`launch::spawn_with`: the chroot sequence, the namespace
rung, the userland entry, the memfd launch) sets each limit after the
`chdir` and before the exec, then `no_new_privs`; a refused call reports
step 12 with the index of the limit, or step 13, and the refusal names the
call (`setrlimit(RLIMIT_NOFILE): EPERM`). More than 64 limits are refused
before the fork. `pod-supervise` builds no `Plan`: podvm's record of a
detached run (its `Detail`) keeps the two fields.

- Tests (6, `src/limits/tests.rs`): `fsize` of 1 MiB stops the write of the
  payload at 1 MiB while the caller writes 2 MiB; `nofile = 64` shows in
  `/proc/self/limits` of the payload and not of the caller; `no_new_privs`
  shows `NoNewPrivs: 1` in the payload and `0` without it and in the
  caller; a hard limit over the caller's (over `fs.nr_open` as root) is
  refused by name; more than 64 limits are refused. With the `setrlimit`
  call taken out, three of them fail.
- Goes out with the tag after `v0.16.0` (PodUtils/podvm#245).

# T-182: `pod-enter`: `lib_dirs` takes `/usr/local/lib` and the image's loader configuration (PodUtils/podvm#247)

**Source:** PodUtils/podvm#247 (COORD-029 of podvm, PodUtils/podvm@T-1719)
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

On the userland rung the image's loader runs on the host with the
library directories of `lib_dirs`. The list has no `/usr/local/lib` and
reads no loader configuration of the image. A library that the list does
not hold is then searched in the host's directories: musl reads the
host's `/etc/ld-musl-ARCH.path` or its built-in path, glibc the host's
`/etc/ld.so.cache` and system directories. podvm measured it: a library of
an alpine image in `/usr/local/lib` failed to load (exit 127), and with a
file of the same name in the host's `/usr/local/lib` the guest loader
loaded the host's file. Also, each standard directory was a plain join on
the host: an image whose `/lib` links to the absolute `/usr/lib` gave the
host's `/usr/lib` to the loader.

## Premise

- Read: the issue; `crates/pod-enter/src/userland.rs` (`lib_dirs`,
  `loader_plan`). In podcrates the loader plan gives `lib_dirs`; podvm
  builds the command line of the loader.
- A directory that no list holds is still searched in the host's default
  directories by each loader: this rung isolates nothing (the head of
  `userland.rs`). The list only makes the image's directories come first.

## Approach

1. Add `/usr/local/lib`, `/usr/local/lib64` and their multiarch forms.
2. Add each directory of the image's `/etc/ld.so.conf` with its `include`
   globs, and of `/etc/ld-musl-ARCH.path` for a musl loader, read inside
   the image through the table, with bounds of depth, files, directories
   and bytes.
3. Resolve each directory inside the image through the table.
4. `LoaderPlan::inhibit_cache`: true where the loader is glibc and its
   libc declares 2.16 or later, so that podvm gives `--inhibit-cache`.

## Prove

```sh
cargo test -p pod-enter --all-features ldconf
```

## Done

2026-11-11. `lib_dirs` gives the loader's directory, the standard ones
with `/usr/local/lib`, `/usr/local/lib64` and their multiarch forms, then
each directory of the image's loader configuration (`userland::ldconf`):
`/etc/ld.so.conf` with `include` globs (relative to the file; `*` and `?`
in the last component; sorted; no dot file unless the pattern has one),
`hwcap` and comments, an old `DIR=TYPE`; or `/etc/ld-musl-ARCH.path` for
a musl loader. The bounds: 32 files, a depth of 4, 64 directories, 64 KiB
for each file. Each file and each directory resolves inside the image
through the table (`resolve_in`, `resolve_dir_in`), so a link out of the
image reads the image's side. This also fixes the standard directories,
which were plain joins on the host. `LoaderPlan::inhibit_cache` and
`takes_inhibit_cache`: true where the loader is glibc and its libc
declares a version of 2.16 or later.

- Tests (`src/userland/ldconf/tests.rs`, 7):
  `the_glibc_configuration_of_the_image_names_its_directories`,
  `the_musl_path_file_of_the_image_names_its_directories`,
  `a_configuration_that_links_out_of_the_image_reads_nothing_of_the_host`,
  `an_include_of_itself_ends_at_the_bound`,
  `lib_dirs_resolve_in_the_image_and_take_usr_local_and_the_configuration`
  (an image whose `/lib` links to the absolute `/usr/lib`),
  `a_glob_matches_as_fnmatch_does_for_star_and_question_mark`,
  `inhibit_cache_needs_a_glibc_of_2_16_or_later` (a copy of the host's
  glibc). `cargo test -p pod-enter --all-features` passes.
- Not run: podvm's experiment 356 (its clause of PodUtils/podvm@T-1719
  comes with its take). podvm gives `--inhibit-cache` where the plan says
  so; a library that no directory of the image holds is still searched
  in the host's default directories, as the head of `userland.rs` says.
- Proved by podvm (PodUtils/podvm#249): podvm takes `v0.19.0` at
  `2c2b8d2` and gives `--inhibit-cache` where the plan says so; its
  experiment 356, clause 32, exits 0 on the chroot-denied fixture: a musl
  program with its library in `/usr/local/lib` and a glibc program with
  its library named only in `/etc/ld.so.conf.d` each load the image's
  library. PodUtils/podvm#247 is closed.

# T-190: `pod-oci`: one large blob as parallel ranges, kept only by the digest of the whole blob (PodUtils/podnet#437)

**Source:** PodUtils/podnet#437 (COORD-019 of podnet), the owner's donor
review: satmux 4. The donor: Th0rgal/satmux at
`2922ebb835d0eb939ddbdfa3b5e7e0f247a0dc0c` (MIT), bin/satmux-get.
**Category:** feature
**Milestone:** M6
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

A CI runner or a build host pulls an image with one layer of 400 MiB
through a proxy or a long path that limits each connection (a shaper for
each flow, a small window on a long round trip, or a proxy that cuts long
bodies at a round size). The pool of `FETCH_WORKERS` (4) works across
blobs only. Three workers stop when the small layers are done, and the
large layer moves on one stream. A cut at 90 percent starts the blob again
at byte zero, up to `ATTEMPTS` (3) times. The wall time is the time of one
blob, and a link that cuts wastes each retry.

The donor probes with `Range: bytes=0-0` and reads the total from
`Content-Range` (else from a `Content-Length` above 1). It uses one stream
for an object under 128 KiB or with no range support. Else it splits the
object into `min(streams, total / 64 KiB)` parts, tries each part up to 3
times, keeps a part only when its size is correct, and joins the parts in
order. It never hashes the whole file and sends no `If-Range`, so it joins
an object that changes between two parts with no error. Here the digest of
the whole blob closes that gap.

## Premise

- Read (this tree): `crates/pod-oci/src/registry.rs:301-348`.
  `Client::blob<W: Write + Restart>` streams into a sink with a
  `Verifier`. `Restart` sets the sink back for a new try (`:40-42`). The
  retry is around the whole transfer (`ATTEMPTS`, `:75`). The doc comment
  names the proxy that cuts at a round size (`:306-308`) and refuses a
  resume: `crates/pod-oci/src/registry.rs:310` says "a resume trusts". The
  review cites the method from `:305`; its doc comment starts at `:301`.
- Read: `crates/pod-oci/src/registry/http.rs:12`: `get` takes an `Accept`
  value only, so a `Range` header needs a new parameter.
  `crates/pod-oci/src/registry/http.rs:128-130` and
  `crates/pod-oci/src/net.rs:9-11`: `pod-proxy` follows the redirect of a
  blob and sends no `Authorization` header to another origin. They agree
  with the review.
- Read: `crates/pod-oci/src/pull.rs:22-26` (`FETCH_WORKERS`: the limit is
  what the registry serves) and `:186-191` (`fetch_parallel` gives the jobs
  in strides); `crates/pod-oci/README.md:114` (podvm keeps
  `FETCH_WORKERS`). They agree with the review.
- Found, not in the review: `Client::blob` and `Client::get` take
  `&mut self`, because the client keeps the token cache and the endpoints
  of plain HTTP. Parts in parallel cannot share one `Client` as it is.
  `fetch_parallel` has no count of free slots: each of `workers` threads
  takes its jobs by stride. "Spare slots only" thus needs a new shared
  bound between the pool and the client. The sink of `blob` is a `Write`,
  with no write at an offset: the ranged mode needs a file
  (`std::os::unix::fs::FileExt::write_at`), thus a new method or a new
  bound.
- Read: TODO/image.md of PodUtils/podvm at `782449a`. Its entry of a blob
  body that is cut says "A Range request is NOT the mechanism" for a
  resume. PodUtils/podvm@T-1342 gives "resuming an interrupted pull with
  `Range`, partial OCI blobs" as out of its scope. The line numbers that
  the review gives were not read.
- Not measured: a time or a gain. Open questions of the review
  (Unverified): a usual registry honors `Range` on its redirected blob URL;
  the gain shows on a path of PodUtils. The donor README (Documented): a
  shaper of the whole link bounds the parallel downloads.

## Approach

Licence: satmux is MIT. The donor is a shell script. Write new Rust code
from this description, with no donor text.

- `Client::with_ranged(max_parts)`, off by default, and a method for a
  file sink.
- The gate: a blob under a threshold (8 MiB, a choice) or a `max_parts`
  under 2 takes the path of today. A probe `GET` with `Range: bytes=0-0`
  must get 206 and a `Content-Range` total equal to the size of the
  descriptor; else the path of today runs, after one probe.
- The split: `n = min(max_parts, size / 4 MiB)` parts, and
  `file.set_len(size)`. Each part is a `GET` with its `Range`, through
  `get` with a new header parameter: each part follows its own redirect
  and sends the token to the registry only. A part needs 206, a
  `Content-Range` of `start-end/size`, and `end - start + 1` bytes written
  at `start`. A failed part is tried again alone, up to 3 times. After 3
  failures each part stops and the path of today runs. `ATTEMPTS` is not
  multiplied.
- The check: one sequential read of the file, and its SHA-256 against the
  digest of the descriptor. A mismatch truncates the file and runs the
  path of today, whose refusal is final. The doc comment gives the cost of
  that read.
- Parallel parts: each part has its own client state (a copy of the token
  cache, or a lock). The parts share the bound of `fetch_parallel`, so the
  requests in flight never pass `workers` (the comment of
  `crates/pod-oci/src/pull.rs:22-25`).
- The doc comment of `blob` keeps its rule: no resume from the bytes on
  disk. The ranged mode trusts no prefix; the digest of the whole blob
  decides.
- The consumer: the pull of podvm, the only consumer of `pod-oci`.

## Prove

```sh
cargo test -p pod-oci --all-features
```

The tests, on a fixture registry that counts the requests of each range:

- A blob of 32 MiB in 4 parts: the correct digest, and 4 different `Range`
  headers.
- Each range answered with 200 and the full body: one probe, then one
  single stream.
- One byte changed in part 2: the digest check fails, the file is
  discarded, and the single stream runs. A registry that still serves bad
  bytes gives a digest error. The plant: with no digest check, this test
  fails.
- Part 3 cut two times, then served: only part 3 is asked again, and the
  blob verifies.
- A part with another total in its `Content-Range`: the single stream
  runs, with no silent join.
- A `max_parts` under 2, or a blob under the threshold: the log of the
  requests is the log of today.
- A measurement, proposed: a loopback proxy caps each connection at a
  fixed rate; N parts take about `size / (N x rate)`. Report the host, the
  cap, N and three runs.

## Start condition

podvm, the only consumer of `pod-oci`, or the owner asks for the mode
(podvm by a COORD issue `(to podcrates)`). podvm reads its two entries on
`Range` first: they refuse `Range` for a resume, and this mode is a
different mechanism.

# T-201: `pod-hold`: the veth wire of the netns hold when the namespace has no `/dev/net/tun` (PodUtils/podnet#446)

**Source:** PodUtils/podnet#446 (COORD-028 of podnet; its T-174, commits `295a41d`, `96cd1b7`, `fc97ef9`)
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

The netns hold of `pod-hold` sets up a TUN device in the namespace
(`netns/tun.rs`). A namespace that cannot open `/dev/net/tun` has no
device, so the hold cannot run there. podnet added a second wire: a veth
pair (`eth0`, `wire0`) made with one `RTM_NEWLINK`; the parent reads
frames on a packet socket of `wire0`, answers ARP for the gateway, and
gives the IP packets to smoltcp. `pod-hold`'s `tun.rs` has no wire enum,
so podnet keeps the wire in its own `veth.rs` and calls it after its take.

## Premise

- Read: PodUtils/podnet#446; `crates/pod-hold/src/netns/tun.rs` and its
  `SOURCES` block (taken at `b759937`).
- Not read: `PodUtils/podnet:crates/podnet-netns/src/netns/veth.rs` and its
  tests; the probe row `veth wire (no TUN)` of its `doctor.rs`.
- PodUtils/podnet#451: the veth wire is done (PodUtils/podnet@T-174). No
  CI job runs its tests: a GitHub runner makes no user namespace, so
  `veth_wire_carries_a_tcp_flow` skips. PodUtils/podnet@T-250 adds a step
  that runs the netns tests as root and fails on a skip line; podnet names
  the job and its first passing run in a COORD issue. That run is the
  start of this entry.

## Approach

When podnet's tests of the wire pass in its CI (`veth_wire_carries_a_tcp_flow`
and the three unit tests of `netns::veth::tests`), ask podnet for the cut
by a COORD issue, take `veth.rs` (the setup, the ARP answer, the packet
socket) into `pod-hold` with a `Wire::{Tun, Veth}` of the netns hold, keep
the smoltcp device in the consumer, and port the tests under their names;
the trial for podnet as for `pod-hold`.

## Prove

```sh
cargo test -p pod-hold
```

## Done

2026-11-11. The start: PodUtils/podnet#451 (PodUtils/podnet@T-250, its
root step "Netns tests as root", first passing run 43062916833 at podnet
`db34bae`). Read: podnet `main` at `db34bae`. Commits `aa39288` (the
import of `PodUtils/podnet:crates/podnet-netns/src/netns/veth.rs`, its
`veth/tests.rs`, `PodUtils/podnet:crates/podnet-netns/src/netns/tun.rs`
and `PodUtils/podnet:crates/podnet-netns/tests/veth.rs`, with the third
block of `crates/pod-hold/SOURCES`), `e07a6d0` (the adaptation, the root
step of CI), `4caed55` (two plants) and the commit of this record (the
README, the trial).

- API (`pod_hold::netns`): `Wire` (`Tun`, `Veth`; `of(fd)` by `fstat`,
  `word`, `checks_checksums`, `read_buffer`), `WireChoice` (`Auto`, `Tun`,
  `Veth`: the setting in the place of podnet's process variable
  `PODNET_NETNS_TEST_VETH`), `open_wire(&Tun, choice)`,
  `create_veth(&Tun, wire)`, `take_frame(frame, gateway, mtu)`, `Frame`,
  `frame_for`, `GUEST_MAC`, `GATEWAY_MAC`, `WIRE_NAME`, `ETH_HEADER`. The
  program end takes the `configure` of the TUN (no second copy of the
  ioctls). `Hold::netns(pid, userns, wire, name)` (breaking): the proof of
  a veth hold names the veth pair, not a TUN. The smoltcp device stays in
  podnet; no process variable, no global; each failure is a
  `pod_core::Failure` at the stage host; each file under 500 lines. The
  probe row `veth wire (no TUN)` of podnet's `doctor.rs` stays with
  `pod doctor` (call C48).
- Tests, `cargo test -p pod-hold --all-features` as root on this host: 51
  unit tests (podnet's `the_wire_answers_arp_for_the_gateway`,
  `the_wire_strips_the_header_and_the_padding_of_an_ip_frame` and
  `the_newlink_request_names_both_ends` with their names; new: the names,
  a netlink answer that is no acknowledgement, IPv6 frames, the wire of a
  descriptor, a double failure); `tests/hold.rs` 3 steps;
  `tests/veth.rs` 1 step, `veth_wire_carries_a_tcp_flow` (a stack of
  `pod-netstack` in the place of the pod: a datagram to the gateway, a
  connect past it, 256 KiB out and back, no frame larger than the MTU).
  No line `SKIP:`. The step passed 50 runs in a loop (3 s). No unit test
  makes an interface in the namespace of the host.
- Shown once each: a `frame_for` to a wrong MAC fails the step (child exit
  20); no offload change fails it (144 frames larger than the MTU; before
  the count, the flow passed in 8 s and the test with it).
- Plants: `hold-veth-arp-any-address` (the unit test, exit 101) and
  `hold-veth-offloads-on` (147 frames larger than the MTU), each run by hand
  as `scripts/plant-each.sh` runs it; `--apply`: 0 stale.
- CI: a GitHub runner gives a user no user namespace, so both netns tests
  of `pod-hold` skipped there. `scripts/root-netns-tests.sh` and the step
  "Netns tests as root" of the job test of `ci.yml` run the `hold` and
  `veth` binaries as root and fail on a line `SKIP:` (as
  PodUtils/podnet@T-250). Locally as root: exit 0. No CI run covers it yet.
- Trial: the patch `pod-hold-veth` of `adoption/podnet/` (removed after
  podnet's take; it is in the tag `v0.26.0`) (podnet `db34bae`; each `pod-*` dependency at tag `v0.26.0`). podnet's
  `netns/veth.rs` is glue over `pod_hold::netns`; its `netns/tun.rs` takes
  `Wire`, the read buffer and the checksums of the crate; the three unit
  tests leave podnet; `doctor.rs` gets its doc comment of `probe_veth` back
  above it. 491 lines of Rust leave podnet, 29 come in. Local, with the
  crates of this tree: the patch applies to a clean `db34bae`; `cargo test
  -p podnet-netns`: 96 unit tests (1 ignored), `raw` 1, `veth` 1 (podnet's
  own `veth_wire_carries_a_tcp_flow` on the wire of the crate, as root, no
  skip); `cargo clippy -p podnet-netns --all-targets -- -D warnings` and
  `cargo fmt -p podnet-netns --check` clean; podnet's record agrees after a
  remap of its README (2 citations); its size and layer checks 0 faults; no
  copy of the moved items. `adoption.yml` runs the rest of the `.run`.
- The trial of `v0.26.0` failed at the build of `podnet-serve`: its
  `pick_tunnel_url` read `dashboard-host` of `pinggy`, which T-224 moved
  to `url-skip`. The patch changes that reader to `url-skip` and remaps
  podnet's record (`PodUtils/podnet:TODO/later.md`, 9 citations).
- podnet holds `PodUtils/podnet:crates/podnet-netns/src/netns/veth.rs`,
  its `veth/tests.rs`, `PodUtils/podnet:crates/podnet-netns/src/netns/tun.rs`
  and `PodUtils/podnet:crates/podnet-netns/tests/veth.rs` to fixes from
  `db34bae` until its take. For podnet's COORD issue: the probe children
  of its `doctor.rs` call `create_tun` and `create_veth`, which allocate,
  and the comment of `close_inherited` says that the child calls only
  async-signal-safe functions.
- podnet took the trial at `f107766` (`v0.26.0`, PodUtils/podnet#458): the
  `taken` line is in `crates/pod-hold/SOURCES`, and the freeze of the four
  files ends. The trial files are removed: podnet's own tests at
  `db34bae` set `StackConfig::accept_pool`, which T-232 removed. Its `doctor.rs` probes run in a new process at `e44575a`
  (PodUtils/podnet@T-257).

# T-202: `pod-supervise`: a payload that kills its launcher leaves a `Dead` record with no count of 0 (podrunner's T-063)

**Source:** podrunner's message of 2026-11-11 (its T-063, its design unit tests 25 and 26, `PodUtils/podrunner:crates/podrunner-unit/src/conformance/design.rs`)
**Category:** measurement
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podrunner found a process that escaped a tree kill on its fake driver.
Its two cases, as a check of `pod-supervise`:
1. `sh -c '(setsid env -i sleep 300 &); exec sleep 300'`: the orphan has
   no environment and a session of its own. A subreaper catches it.
2. `sh -c '(setsid env -i sleep 300 &); kill -9 $PPID; exec sleep 300'`:
   the payload kills its subreaper parent, so the orphan goes to init. No
   record may then say a count of 0.

`tests/tree.rs` covers case 1
(`a_grandchild_that_left_and_ignores_sigterm_is_killed`). No test covers
case 2: the payload kills the launcher, the lock is free, and the record
becomes `Dead` with no exit code (README rule 2). A waiter, `kill` and
`remove` must then give no `residual_processes` of 0, and the payload
itself must end (`PR_SET_PDEATHSIG`).

## Premise

- Read: podrunner's message; `crates/pod-supervise/tests/tree.rs`; the
  rules of `crates/pod-supervise/README.md`.
- Not measured: what `wait`, `kill` and `remove` give today for case 2.

## Approach

A test of case 2 in `tests/tree.rs`: the payload kills its launcher; the
record becomes `Dead`; the payload ends; no end with a count of 0 is
written; the orphan is found and killed by the test. If a verb gives a
count of 0, fix it to `Unknown`.

## Prove

```sh
cargo test -p pod-supervise --test tree
```

## Done

2026-11-11. `tests/tree.rs` has the step
`a_payload_that_kills_its_launcher_leaves_a_dead_record_and_no_count`
(podrunner's case 2; its case 1 is
`a_grandchild_that_left_and_ignores_sigterm_is_killed`).

- Defect found and fixed: `launcher::wait` read the end of the control
  stream with no answer as the bound (`Ok(None)`). After a payload killed
  its launcher, `wait` gave the stale record `Running` with no code, and
  `stop` sent SIGKILL to a launcher that was gone and answered that SIGKILL
  ended the payload. The end with no answer is now `WaitError::Gone`;
  `wait` reads the record again for up to 1 s (the kernel closes the
  socket and the lock one after the other) and gives `Dead` with no exit
  code and no end. Plant `supervise-launcher-eof-is-the-bound` (the old
  line) fails the step with `left: Running`.
- Defect found, larger: the payload outlives its launcher; no code of
  `pod-supervise` or `pod-enter` sets `PR_SET_PDEATHSIG` (T-205).
- `cargo test -p pod-supervise`: 30 unit tests, `lifecycle` 5 steps,
  `tree` 7 steps pass.

# T-205: `pod-supervise`, `pod-enter`: a payload outlives its launcher; no code sets `PR_SET_PDEATHSIG`

**Source:** T-202 (podrunner's case of its T-063)
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The rule 4 of `crates/pod-supervise/README.md` keeps the path that
forks, enters and execs on one thread, because `PR_SET_PDEATHSIG` fires at
the exit of a thread. But no code sets `PR_SET_PDEATHSIG`: not the
launcher, not `pod-enter`'s child, not the `enter` of the tests. When a
payload, or anything else, kills the launcher, the payload runs on with
no supervisor, its record reads `Dead`, and no verb can stop it. The step
of T-202 shows it: the payload lives after its launcher is killed.

## Premise

- Read: `crates/pod-supervise/src/launcher.rs` (the double fork, the
  comment on `PR_SET_PDEATHSIG`), `launcher/tests.rs`
  (`nothing_on_the_spawn_path_can_spawn_a_thread`), and
  `grep -rn PDEATHSIG crates/pod-enter/src` (no match).
- Not read: whether podvm's entry sets it on its own
  (PodUtils/podvm crates of the enter of a payload).

## Approach

`PR_SET_PDEATHSIG(SIGKILL)` in the child of the payload, after the fork
and before the exec, with the check that the parent is still the
launcher after the call (the race of a parent that ended before it).
`pod-enter` sets it in its child path (with the limits and
`no_new_privs`); `pod-supervise` names it in `Launch` for an `enter` of
the caller, and the `enter` of its tests sets it. The step of T-202 then
asserts that the payload ends with its launcher. Ask podvm by a COORD
issue whether its entry sets it today.

## Prove

```sh
cargo test -p pod-supervise --test tree
cargo test -p pod-enter
```

## Done

2026-11-11. `pod-hostprobe`: `sys::set_pdeathsig`, `sys::getppid`.
`pod-enter`: `Plan.die_with_parent` (opt-in: the signal fires at the exit
of the thread that forked, so a caller that forks from a pool of threads
keeps it false); the child sets `PR_SET_PDEATHSIG(SIGKILL)` first after
the fork and compares its parent with the pid of the caller before the
fork; a refused call is `prctl(PR_SET_PDEATHSIG)` by name.
`pod-supervise`: rule 4 of the README and the doc of `Launch` name the
rule; the `enter` of the tree tests sets the signal, and the step of T-202
asserts that the payload ends with its launcher. Tests:
`a_payload_ends_with_the_thread_that_forked_it` (pod-enter, 166 tests);
`tree` 7 steps. Plant `enter-no-death-signal` (a signal of 0) fails the
test of pod-enter. Breaking: `Plan` has the field `die_with_parent`.
podvm took `v0.22.0` at `c9d5038` (PodUtils/podvm#262): the launcher of
`podvm-supervise` sets `die_with_parent: true` (it forks from its one
thread), and each other `Plan` keeps `false`. Its experiment 437: after a
SIGKILL to the launcher of `podvm run -d`, the payload ends (state Z) and
`ps -a` names the container Dead; with `false` the payload lives (state S).

# T-206: `pod-guest`: the module set of a guest kernel from its modloop (PodUtils/podvm#260)

**Source:** PodUtils/podvm#260 (COORD-041 of podvm; PodUtils/podvm@T-1662, issue 178)
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podvm's machine tier boots Alpine's `vmlinuz-virt` (`6.12.110-0-virt`),
whose nf_tables, nft_nat, sch_netem, wireguard, tun, veth and nf_nat are
modules. They are in `boot/modloop-virt`, a squashfs with xz compression,
and `squashfs.ko` is in the modloop only, not in `initramfs-virt`. So the
guest cannot mount the modloop: the host must read the squashfs and give
the guest a module set with its `modules.dep`. `pod-microvm` (M7) needs the
same reader.

## Premise

- Read: podvm's ask and its facts of 2026-11-11 (the archive 3.22.6, 21 MB,
  the `CONFIG_*` lines).
- podvm's ceiling of its static binary is 8,000,000 bytes
  (`experiments/110-bloat-delta.sh`, `CEILING_BYTES`); the binary is
  5,162,616 bytes on 2026-11-11: a static liblzma of some hundreds of KB
  fits (podvm's amended PodUtils/podvm#260).
- Read, on 2026-11-11, `netboot/modloop-virt` of Alpine 3.22.8 x86_64
  (21,164,032 bytes): squashfs 4.0, xz, blocks of 131,072 bytes, 1,179
  inodes. The compressor options are there (flag 0x400): a dictionary of
  131,072 bytes and the x86 BCJ filter. Of its 1,016 xz streams, 8 have the
  chain x86 BCJ then LZMA2, and 1,008 have LZMA2 only. So the decoder must
  take the x86 BCJ filter. The modules are plain `.ko` files (no `.ko.gz`
  or `.ko.xz`), and `modules.dep` names them as `kernel/...`
  (`kernel/net/netfilter/nf_tables.ko: kernel/net/netfilter/nfnetlink.ko
  kernel/lib/libcrc32c.ko`).
- Read (crates.io): `xz4rust` 0.2.3 (MIT, no unsafe code, no normal
  dependency, `no_std`) decodes the BCJ filters with its feature `bcj`.
  `lzma-rust2` 0.25.0 (Apache-2.0) is a port of XZ for Java; its README
  does not name BCJ. `lzma-rs` 0.3.0 has no release since 2023. `liblzma`
  0.4.9 binds the C library. Each licence is in the list of `deny.toml`.
- Not measured: the speed and the size of `xz4rust` on the 1,016 streams.

## Approach

A reader of a squashfs v4 file (the superblock, the inode and directory
tables, the fragments, xz blocks) for the read path only, in `pod-guest`:
`modules(modloop, kernel_release, names) -> ModuleSet` (the `.ko` files
of the names and of each of their dependencies by `modules.dep`, and the
`modules.dep` of the set), to put in the guest's initramfs or a second
disk. Pure Rust if a pure decoder reads the modloop's xz with its filters;
else liblzma, static (C is allowed). Record the choice as a call. Tests on
a squashfs that the test writes, and a live test of the real modloop in
`live.yml`. A fuzz target of the superblock and the tables.

## Prove

```sh
cargo test -p pod-guest
```

## Done

2026-11-11, commit `f04b67c` (the reader, the tests, the fuzz target, the
live step) and the commit of this section (the plants, the documents,
call C58). The decoder is `xz4rust` (C58).

- `pod_guest::squashfs`: squashfs 4.0, read only, over a `Read + Seek`;
  each offset and size checked against `bytes_used`; a ceiling before each
  allocation; a typed `squashfs::Fault`. `pod_guest::modloop::modules`:
  each name and each dependency of `modules.dep`, each once, in load
  order, with the `modules.dep` of the set; a name of `modules.builtin`
  needs no file; a name in neither file is `modloop::Fault::Unknown` with
  the name.
- `cargo test -p pod-guest`: 89 passed, 1 ignored (`write_the_fuzz_seeds`);
  `tests/modloop_live.rs`: 1 ignored. 23 tests are new: 11 of `squashfs`
  (on images of the test writer, with the xz fixtures of
  `crates/pod-guest/tests/fixtures/squashfs/make.py`), 12 of `modloop`.
  Two fixes after `f04b67c`, each with a test that fails on `f04b67c`: a
  file of the release that is not UTF-8 is `Fault::NotText` with its path
  (it was "modules.dep, line 0" also for `modules.builtin`); a file
  `modules` at the root sends the search to `lib/modules` (it was
  `NotADirectory`).
- The live read of the local copy of `netboot/modloop-virt` of Alpine
  3.22.8 (`POD_GUEST_MODLOOP=PATH cargo test -p pod-guest --test
  modloop_live -- --ignored`, 8.8 s in a debug build): 1,016 xz streams
  decoded, 8 with the x86 BCJ filter; 240 directories, 939 files,
  114,162,529 bytes, the same as Python's lzma. The set of `nf_tables
  nft_nat sch_netem wireguard tun veth nf_nat` for `6.12.115-0-virt`: 20
  modules, 2,194,724 bytes, each sha256 the same as with Python's lzma.
  `live.yml` runs the test each day on the newest Alpine 3.22.
- podvm measured `xz4rust` in its binary (PodUtils/podvm#276, at
  `a714e99f`): 19.4 KiB of `.text` (0.7 %); the static binary is
  5,613,176 bytes, under its ceiling of 8,000,000 bytes.
- The fuzz target `squashfs_image` with 3 seeds:
  `cargo test --manifest-path fuzz/Cargo.toml --lib`, 9 passed. Its walk
  has a bound of 256 inodes: a listing can name its own directory. The
  walk of `f04b67c` counted lines of output, and a directory adds none:
  `a_directory_that_lists_itself_ends_the_walk` did not end there (exit
  124 at a limit of 60 s). The stable
  test of the fuzz targets took 470 s on this host (`wg_response` 239 s,
  `wg_cookie` 165 s, `e2e_proof` 64 s, `squashfs_image` 0.35 s). The dev
  profile of `fuzz/Cargo.toml` now optimizes the crates under test, and
  the test takes 6 s.
- The plants `guest-squashfs-bcj-off`,
  `guest-modloop-dependency-not-followed` and
  `guest-squashfs-read-past-the-end`: each caught
  (`sh scripts/plant-each.sh ID`).
- The static probe calls `modloop::modules`.
- `pod-broker`'s `a_socket_path_that_is_another_file_is_refused` made its
  socket under `std::env::temp_dir()`, and failed where the temporary
  directory is long (`path must be shorter than SUN_LEN`, with the
  scratchpad of this session as `TMPDIR`). It takes
  `pod_testkit::host::socket_dir` now, as the other socket tests of the
  file do, and passes there.
- Not measured: the size of `xz4rust` in podvm's binary.
- `sh scripts/agent/check.sh`: fmt 0, clippy 0, lint 0, doc 0, record 0,
  size 0, paths 0, static 0, test 0 (the workspace: 2,229 passed, 18
  ignored).

# T-207: `pod-netlab`: the network lab of podvm's guest (PodUtils/podvm#260, issue 178)

**Source:** PodUtils/podvm#260 (PodUtils/podvm@T-1662, issue 178; PodUtils/podvm@T-1663 and PodUtils/podvm@T-1664 run in the same guest)
**Category:** feature
**Milestone:** M6
**Priority:** P3
**Effort:** L
**Status:** partial

## Problem

podvm's labs need the layout and the knobs of issue 178 (NAT44, NAT66,
NAT64 with DNS64, CGNAT, IPv6 only, a firewall policy, impairment), the
expected verdict of each lab derived from its knobs, and a check that
reads the nftables counters before and after and fails on a flushed
chain. podvm blocked its PodUtils/podvm@T-1662 on this crate.

## Premise

- Read: podvm's ask; the row of `pod-netlab` in `docs/design.md`.
- Not read: issue 178 of podvm (the layout and the knobs); the guest of
  podvm's machine tier (TCG, `--podvm-cpus`, the proxy at the gateway, the
  broker forward under `restrict=on`).

## Approach

The study first (with T-062): read issue 178 and port its layout and knobs
unchanged; the derivation of the verdicts; the counter check. Then the
crate on `pod-netstack` and the module set of T-206. T-119 (the CI matrix
of cfb) reuses its cells later.

## Prove

```sh
cargo test -p pod-netlab
cargo test -p pod-netlab --test nft -- --ignored
sh scripts/plant-each.sh netlab-judge-ignores-counters netlab-dnat-after-new-drop
```

## Correction

2026-11-12: issue 178 of podvm is read now (the text of PodUtils/podvm#260
and of the issue). The crate needs no `pod-netstack` for a lab in a guest
that is root (call C148); the variant on a user-space stack is T-349.

Done in this step (the part that needs no guest and no root; calls C145
to C149; commits `3d5c771` and `8400a01`):

- `Knobs`: each knob of issue 178 with its range and its flag
  (`parse_flags`, `flags`); `check` refuses the three contradictions of
  sand-net and six more, each by type.
- The layout: the namespaces `PREFIX-NAME-g|r|w` and `-c` for cgnat, the
  veths and the addresses; NAT64's prefix 2001:db8:64::/96 and its pool.
- The 27 offline scenarios as knob sets; 25 come from their names (C145).
- The plan: `Lab::up` and `Lab::down` as argument lists, with the lessons
  L1, L3, L5, L6, F9, F18, F20; the counters through nft's JSON, as nft
  1.0.9 refuses a counter named `return` in its text (C148).
- The derivation (`Lab::expect`) of each probe's verdict, what it must
  see, its bound, and each counter that must move or stay (C149).
- The judge: the counters of `nft -j list counters` before and after, and
  the measurements, as PASS and FAIL rows; a flushed chain fails.
- `cargo test -p pod-netlab --all-features`: 47 passed (counters 3,
  expect 12, judge 8, knobs 9, plan 15), 2 ignored.
  `cargo test -p pod-netlab --test nft -- --ignored`: 2 passed (each
  ruleset of the 27 scenarios loads into nftables 1.0.9 under
  `unshare -rn`, and its nine counters read back).
- The plants `netlab-judge-ignores-counters` and
  `netlab-dnat-after-new-drop`: each caught
  (`sh scripts/plant-each.sh ID`). The fuzz target `netlab_counters`
  (its seeds: the output of nftables 1.0.9 for the nine counters, two
  tables, a counter two times, a counter with no packets); the stable
  test of `fuzz/` passes. The static probe calls the crate: the musl
  build passes `scripts/check-static.sh` and `scripts/check-probe.py`.

What remains: the probe program (T-345), the world's services (T-346),
each scenario on a real kernel with the planted flush (T-347), the VPN
and live scenarios (T-348), the user-space variant and a second guest
(T-349). `crates/pod-netlab/README.md` gives the parity with issue 178.

# T-208: `pod-extract`: a root directory entry with an empty name refuses its layer (PodUtils/podvm#261)

**Source:** PodUtils/podvm#261 (COORD-042 of podvm; PodUtils/podvm@T-1659, issue 172)
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`podvm run` refuses `public.ecr.aws/lambda/provided:al2023`: its layer
`sha256:ed646b77...` (gzip, 88128 bytes) has as its first entry the root
directory of the layer with an empty name (`name '' type 5 mode 0700`).
`apply_layer` takes `./`, `.` and `/` as the root and refuses each other
empty name, so the whole layer is refused.

## Premise

- Read: podvm's ask; `crates/pod-extract/src/safety.rs` (`is_archive_root`
  refuses `""` on purpose) and `crates/pod-extract/src/apply.rs` (the
  check of the root before `components`).
- `remove::collect` takes an empty name as `Kind::Normal` and makes no
  operation of it.

## Approach

`safety::is_root_entry(path, is_dir)`: the archive root, or an empty name
of the directory type. `apply_layer` skips it, as it skips `./` (the mode
of `.` is not applied there either). An empty name of each other type
stays refused. Tests: `is_root_entry` of each case; a layer whose first
entry is `""` of the directory type applies, and a file with an empty name
is still refused.

## Prove

```sh
cargo test -p pod-extract
```

## Done

2026-11-11. `safety::is_root_entry(path, is_dir)`: the root of the
archive (`./`, `.`, `/`), or an empty name of the directory type.
`apply_layer` skips it, as it skips `./`. An empty name of another type
stays refused. Tests: `is_root_entry` of each case (`safety/tests.rs`);
`a_root_directory_with_an_empty_name_is_the_root` (a layer whose first
entry is `""` as a directory, mode 0700, then
`THIRD-PARTY-LICENSES.txt`, applies; a file with an empty name is
refused). `cargo test -p pod-extract --all-features`: 55 passed. Plant
`extract-empty-root-refused` (the old check) gives podvm's error word for
word.

# T-209: `pod-confine`: the Landlock scopes (signals, abstract Unix sockets) and UDP stay open to a confined payload

**Source:** T-108 (a finding of its work)
**Category:** feature
**Milestone:** M7
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

A payload under a `Spec` of `pod-confine` can send a signal to each
process of its user outside its Landlock domain, connect to an abstract
Unix socket of the host (a session bus, an X server), and send UDP to
each address. The file and TCP rules of Landlock do not cover these, and
the default deny list has no rule for them. A profile with `net.ports`
reads as a closed network, and UDP stays open.

## Premise

- Read: Landlock ABI 6 (Linux 6.12) adds the scopes
  `LANDLOCK_SCOPE_SIGNAL` and `LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET`
  (`linux_raw_sys::landlock`) in `landlock_ruleset_attr.scoped`; T-108
  sets `scoped` to 0. Landlock ABI 7 has no UDP right.
- Not measured: the effect of the scopes on the payloads of podvm (a
  shell that signals its own children stays in its domain).

## Approach

`Spec::scope` (on in `Spec::new()`): the compile sets both scopes where
the ABI is 6 or newer, and the README names the ABI under which the field
gives nothing. `Spec::udp` (free in `Spec::new()`): where it is closed, the
deny list gets `socket` of `AF_INET` and `AF_INET6` with `SOCK_DGRAM`
(`EACCES`); `Deny` then needs a rule on the equal values of two arguments.
Tests in `tests/apply.rs`: a `kill` of the parent gets `EPERM`, and a UDP
socket gets `EACCES`. Ask podvm by a COORD issue whether its profiles need
a key for each.

## Decision

Calls C61 (the io_uring calls in the default list and with each closed
socket), C62 (a closed UDP is a refusal on a target with `socketcall(2)`)
and C63 (`ENOPROTOOPT` for an MPTCP socket under a TCP list), in
`docs/decisions.md`.

## Prove

```sh
cargo test -p pod-confine --all-features
```

## Done

2026-11-11, `dc36af5` (the code and the tests) and the commit after it
(the plants, the documents and the record).

- `Spec` has two new fields (breaking: a struct literal of `Spec` needs
  them). `scope` (on in `Spec::new()`; `no_scope()`) sets
  `LANDLOCK_SCOPE_SIGNAL` and `LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET` where
  the ABI is 6 or newer (`SCOPE_ABI`, `spec::SCOPES`,
  `Compiled::scoped()`); under ABI 6 it gives nothing. `udp` (true in
  `Spec::new()`; `close_udp()` sets false): where it is false, `socket(2)`
  of `AF_INET` and `AF_INET6` with `type & SOCK_TYPE_MASK == SOCK_DGRAM`
  gets `EACCES` (`Deny::udp()`), and the io_uring calls get `EPERM`.
  `Spec::deny_rules()` gives the rules of the filter: `deny`, then the
  rules of the switches.
- A closed UDP on a target with `socketcall(2)` (i686, s390x, powerpc64)
  is a refusal by name (call C62).
- A finding of the work, fixed in place: Landlock checks TCP sockets only
  (`sk_is_tcp` in `security/landlock/net.c`). On this host (Linux 6.18,
  ABI 7, `net.mptcp.enabled` 1) an MPTCP socket connected to a port outside
  the connect list of a ruleset, where a TCP connect got `EACCES`. A TCP list
  now adds `socket(IPPROTO_MPTCP)` with `ENOPROTOOPT` (`Deny::mptcp()`,
  call C63). The other sockets that Landlock does not see are T-213.
- `Deny` (breaking): `flags: Option<(u8, u32)>` is now `when: Vec<Arg>`:
  `Arg::AnyBit` (`when_flags(index, mask)`) and `Arg::Equal`
  (`when_equal(index, mask, value)`, with `BPF_ALU_AND_K`); each condition
  must hold. A rule whose conditions do not hold loads the number again and
  gives the call to the next rule of its number (before: `ALLOW`), so two
  rules share one number; a rule after a rule of its number with no
  condition is left out. The block of one flag condition keeps five
  instructions. The default list adds `io_uring_setup`, `io_uring_enter`
  and `io_uring_register` with `EPERM` (call C61): 79 instructions on
  x86_64, 73 before. The test `the_first_rule_of_a_number_wins` keeps its
  name and now proves the fall-through.
- `pod-hostprobe`: `BPF_ALU_AND_K`, the two scopes, `AF_UNIX`, `AF_INET6`,
  `SOCK_DGRAM`, `SOCK_TYPE_MASK`, `IPPROTO_MPTCP` (the feature `net` of
  `linux-raw-sys`), `SYS_SOCKETCALL` where the kernel has it, and the
  io_uring numbers.

Tests: `cargo test -p pod-confine --all-features`: 28 unit tests and 11 in
`tests/apply.rs`; each passes on this host (Landlock ABI 7, MPTCP on; no
check skipped). New in `tests/apply.rs`: with the scopes,
`kill(getppid(), 0)` and a connect to an abstract Unix socket of the
parent get `EPERM` and a signal to the child itself passes (with
`no_scope()` each passes); with UDP closed and an empty deny list,
`socket(AF_INET, SOCK_DGRAM)` and `socket(AF_INET6, SOCK_DGRAM |
SOCK_CLOEXEC)` get `EACCES`, `io_uring_setup` gets `EPERM`, and a TCP
socket and a Unix datagram socket pass; under a TCP list an MPTCP socket
gets `ENOPROTOOPT` and a TCP connect inside the list passes. A host with an
ABI under 6 skips the check of the scopes by name, and a host with no
MPTCP the check of MPTCP. `cargo test -p pod-enter --all-features --test
confine`: 7 pass. `cargo test -p pod-hostprobe --lib`: 110 pass. Clippy
of `pod-confine` is clean on i686 and aarch64 musl too. Plants (group
`confine`, each caught): `confine-scope-unset`, `confine-udp-open`,
`confine-udp-type-unmasked`, `confine-udp-io-uring-open`,
`confine-rule-no-fall-through`, `confine-rule-one-condition`,
`confine-mptcp-open`, `confine-socketcall-silent`; the patch of
`confine-absent-runs-unconfined` is made again on the new `apply`, and
the three plants of T-108 are caught. `sh scripts/agent/check.sh`: each
line 0.

Open: podvm's profiles have no key for `scope` and `udp`. The question of
the Approach (a COORD issue `(to podvm)`) goes with the hand-off of the
release that carries this work.

# T-213: `pod-confine`: under a TCP list or a closed UDP, a payload still makes sockets that Landlock and the filter do not see

**Source:** T-209 (a finding of its work)
**Category:** defect
**Milestone:** M7
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

Landlock checks TCP sockets only (`sk_is_tcp`), up to ABI 9. T-209 closes
MPTCP under a TCP list and UDP by `Spec::udp`. Other sockets still reach
the network past a TCP list or a closed UDP: SCTP (`IPPROTO_SCTP`, and
`SOCK_SEQPACKET` of `AF_INET` with protocol 0), SMC (`AF_SMC`, and
`IPPROTO_SMC` of Linux 6.11) with its TCP fallback, RDS over TCP, RxRPC
over UDP, VSOCK to the host, and raw and packet sockets where the payload
holds `CAP_NET_RAW`. The kernel can load the module of such a family at
the first `socket(2)`. On a target with `socketcall(2)` (i686, s390x,
powerpc64), the MPTCP rule of T-209 has a gap: a payload makes an MPTCP
socket through `socketcall`, whose arguments a filter cannot read. A
payload also connects to each Unix socket of a path that its user can
write (a system bus, a socket of a container engine): the ruleset handles
the file rights of ABI 5 and lower only.

## Premise

- Read: `security/landlock/net.c` of Linux master (7.4-rc2): the bind,
  connect and send hooks check `sk_is_tcp` and `sk_is_udp` only. Landlock
  ABI 10 adds `LANDLOCK_ACCESS_NET_BIND_UDP` and
  `LANDLOCK_ACCESS_NET_CONNECT_SEND_UDP`, and ABI 9 adds
  `LANDLOCK_ACCESS_FS_RESOLVE_UNIX` (a connect to a Unix socket of a path
  made outside the domain gets `EACCES`; `include/uapi/linux/landlock.h`).
  `linux-raw-sys` 0.12.1 has none of the three.
- Measured on 2026-11-11 (Linux 6.18, ABI 7): an MPTCP connect passed a
  port rule (fixed in T-209); `socket(AF_INET, SOCK_STREAM, IPPROTO_SCTP)`
  gave `EPROTONOSUPPORT` (no module here).
- Not measured: SMC, RDS, RxRPC and VSOCK on the hosts of podvm.

## Approach

Where the spec closes a socket (a TCP list, or `udp: false`), allow a list
of socket families: `socket(2)` and `socketpair(2)` pass for `AF_UNIX`,
`AF_INET`, `AF_INET6` and `AF_NETLINK`, and each other family gets
`EAFNOSUPPORT`. For `AF_INET` and `AF_INET6`, a type other than
`SOCK_STREAM` and `SOCK_DGRAM` fails, and a `SOCK_STREAM` with a protocol
other than 0 and `IPPROTO_TCP` fails. This needs a "not equal" condition in
`Deny` (a jump with its targets swapped). Where the ABI is 10 or newer,
close UDP with the Landlock UDP rights too: the LSM sees a socket of
io_uring and of `socketcall`. Where the ABI is 9 or newer, handle
`LANDLOCK_ACCESS_FS_RESOLVE_UNIX` (`handled_fs`), and give it to the paths
of a rule with `WRITE` or with a new access. On a target with
`socketcall(2)`, ask podvm
by a COORD issue: a refusal of a TCP list there, or a deny of
`socketcall` under a TCP list.

## Prove

```sh
cargo test -p pod-confine --all-features
```

## Done

2026-11-12, `4b5d13a` (the code and the tests), `6766940` (the plants).

- `Deny::when_not_equal` (`Arg::NotEqual`): a jump on a match to the
  next rule of the number. `Deny::families()`: `socket(2)` and
  `socketpair(2)` of a family other than `AF_UNIX`, `AF_INET`, `AF_INET6`
  and `AF_NETLINK` get `EAFNOSUPPORT`; `socket(2)` of IP of a type other
  than `SOCK_STREAM` and `SOCK_DGRAM` gets `ESOCKTNOSUPPORT`; a
  `SOCK_STREAM` of IP of a protocol other than 0 and `IPPROTO_TCP` gets
  `EPROTONOSUPPORT`. `Spec::deny_rules` adds them where the spec closes a
  socket, after the MPTCP rule, so MPTCP keeps `ENOPROTOOPT`.
- ABI 9: `handled_fs` handles `RESOLVE_UNIX`; `Access::WRITE` gives it,
  and it is a right of a file. ABI 10: a closed UDP handles
  `BIND_UDP` and `CONNECT_SEND_UDP` with no port rule. The values come
  from `include/uapi/linux/landlock.h` of Linux 7.4-rc2 (bits 16, 2 and 3;
  `pod_hostprobe::sys`), which has ABI 12. This host has ABI 7: only the
  compile of ABI 9 and 10 is tested.
- On a target with `socketcall(2)` the rules of a TCP list stay void for
  a socket made through it, as before; the question goes to podvm with
  the release.
- Tests: `a_not_equal_condition_holds_where_the_argument_differs`,
  `the_family_rules_close_each_socket_that_landlock_does_not_check`,
  `the_rights_of_abi_9_and_10_follow_write_and_a_closed_udp`, the switches
  test, the ABI test of `spec`, and `tests/sockets.rs`
  (`a_tcp_list_closes_the_sockets_that_landlock_does_not_check`: confined,
  `[EAFNOSUPPORT, EAFNOSUPPORT, ESOCKTNOSUPPORT, ESOCKTNOSUPPORT,
  EPROTONOSUPPORT, 0, 0, 0, 0]` for packet, VSOCK, raw IP, SCTP over
  `SOCK_SEQPACKET`, an SCTP stream, TCP, UDP, Unix and netlink; with no
  family rule the first two and the raw socket open). `cargo test -p
  pod-confine --all-features`: 32 unit tests, 12 of `apply`, 1 of
  `sockets` pass. Plants: `confine-families-open` and
  `confine-not-equal-as-equal` are caught; four plants of T-209 follow the
  code and are caught.

# T-216: `pod-enter`: `a_descriptor_passed_onto_its_own_number_reaches_the_payload` failed one time in a run of the workspace's tests

**Source:** the run of `sh scripts/agent/check.sh` before the push of T-025 and T-209
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

In one run of `cargo test --workspace --all-features` (2026-11-11, the
check of the merge of T-025 and T-209), the test
`a_descriptor_passed_onto_its_own_number_reaches_the_payload` of
`crates/pod-enter/src/tests.rs` failed: the payload `sh -c "echo passed
>&W"` exited 2 (the shell had no descriptor `W`). Ten runs of
`cargo test -p pod-enter --all-features --lib` after it passed (167
tests each). The code of the test and of `Fds.pass` did not change in the
merge.

## Premise

- Read: the test (a pipe with `O_CLOEXEC`, the write end passed onto its
  own number, the child clears the close-on-exec flag), and the payload
  `/bin/sh -c "echo passed >&W"`.
- Measured: `/bin/sh` here is dash, and dash takes `>&N` for one digit
  only: `echo passed >&12` exits 2 with `Syntax error: Bad fd number`.
  Under the load of the workspace's tests the pipe gets a number of two
  digits; alone it gets one digit. No race of descriptors: no other test
  of the process closes or `dup2`s onto a number of another one.

## Approach

Search the tests of `pod-enter` (and of `pod-hostprobe` that run in the
same process) for a `close` of a number after its owner closed it, and
for each close of a range in the test process. Make each test own its
descriptors (`OwnedFd`) so that a number is closed once. Prove it with a
loop of the test under load (`stress`, or the workspace's tests in a
loop) that fails before the fix.

## Prove

```sh
cargo test -p pod-enter --all-features --lib
```

## Done

2026-11-11. The test moves the write end to a number of 40 or more
(`F_DUPFD_CLOEXEC`) and the payload writes through `/proc/self/fd/W`, so
each run takes the case of two digits and the shell takes each number.
With `>&W` the test now fails at each run (exit 2); with the path it
passes. Plant `enter-same-fd-dash-digit` (the old `>&W`) is caught.

# T-221: `pod-confine`: a payload that runs as root keeps its capabilities, and reads the environment of its unconfined ancestors past the Landlock domain

**Source:** podrunner's T-042 (a direct message of 2026-11-11: as root, a confined payload reads `/proc/PID/environ` of a process outside its domain)
**Category:** defect
**Milestone:** M7
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`pod_confine::apply` set `no_new_privs`, the Landlock ruleset and the
seccomp filter, and kept the capabilities of the payload. A confined
payload that runs as root then reads `/proc/PID/environ` of its
unconfined ancestors of the same uid (podrunner's shim and podrunner
itself; for podvm, its launcher and its supervisor) past the Landlock
domain. podrunner found it (its T-042, a direct message); as `nobody` the
read is refused. podvm's units can run their payload as root.

## Premise

- Read: podrunner's case (Linux 6.18.44, ABI 7, uid 0 in the first user
  namespace, `CapEff` with each capability but `CAP_SYS_RESOURCE`): its
  confined payload reads the environment of the shim and of the test
  process, its unconfined ancestors, and not of PID 1.
- Measured here (the same kernel, as root, `/proc` readable by the
  rules), a child that reads the environment of its parent, the test
  process: with no confinement it reads; confined with no capability,
  `EACCES`; confined with each capability kept, it reads; with one
  capability kept, it reads with `CAP_SYS_ADMIN` (21) or `CAP_PERFMON`
  (38) alone, and not with `CAP_SYS_PTRACE` (19) or any other one. The
  parent with `PR_SET_DUMPABLE` 0 changes nothing.
- PID 1 here refuses `/proc/1/environ` to root with no confinement: a test
  on it shows nothing of the domain.

## Approach

A switch of `Spec` (on in `Spec::new()`): `apply` drops each capability of
the bounding set, then the effective, permitted and inheritable sets and
the ambient set, before the ruleset, with no allocation (`prctl
PR_CAPBSET_DROP` for each, `capset` with zero, `PR_CAP_AMBIENT_CLEAR_ALL`).
A spec that names a capability to keep keeps that one. Tests in
`tests/apply.rs`, as root only (a skip by name else): a confined child
gets `EACCES` for `/proc/1/environ`, and its `/proc/self/status` shows
`CapEff: 0`. A plant that keeps the capabilities. Tell podvm and podrunner
in the hand-off.

## Prove

```sh
cargo test -p pod-confine --all-features
```

## Correction

2026-11-11. The test of the drop skips as another user, and the GitHub
runners do not run as root: `plants.yml` missed the plant
`confine-capabilities-kept` (runs 43061056454 and 43066781733), and `ci`
never ran the test. `scripts/ci/root-confine-tests.sh` builds the test
binary of `tests/apply.rs` as the user and runs it as root (`sudo -n` on
a runner; as it is where the caller is root), and a skip line fails it.
The step "The tests of pod-confine as root" of the job `test` of `ci.yml`
and the plant run it. The plant is caught here (exit 1).

## Done

2026-11-11. `pod_hostprobe::sys` has `capget`, `capset`, `capbset_drop`,
`ambient_clear` and the numbers `CAP_SETPCAP`, `CAP_SYS_PTRACE`,
`CAP_SYS_ADMIN`, `CAP_PERFMON` (`sys/caps.rs`). `Spec::keep_caps` (0,
none, in `Spec::new()`), `keep_cap(n)`, `DOCKER_CAPS`; `apply` drops each
capability that the mask does not name after `no_new_privs` and before
the ruleset: the bounding set where the thread holds `CAP_SETPCAP`, then
`capset` of the three sets, then the ambient set (`EINVAL` of an old
kernel passes). `Call` has `CapGet`, `CapBoundingDrop` (its index is the
capability), `CapSet`, `CapAmbientClear`. The probe's child drops them
too. Test `a_confined_root_keeps_only_the_capabilities_of_the_spec` (root
only): the parent's environment reads with no confinement; confined with
no capability it is `EACCES` and the effective and permitted sets are
empty; with `DOCKER_CAPS` it is `EACCES` and no capability out of the set
stays; with `CAP_SYS_ADMIN` kept it reads, and that capability is the one
kept. `cargo test -p pod-confine --all-features`: 28 unit tests and 12 in
`tests/apply.rs` pass; `cargo test -p pod-enter --all-features --test
confine`: 7 pass. Plant `confine-capabilities-kept` (no drop) is caught;
the patches of `confine-absent-runs-unconfined` and `confine-scope-unset`
were made again after the change of `apply.rs` (`plant-each.sh --apply`
found them stale) and are caught. Breaking: `Spec` has the field
`keep_caps`, and a confined root payload keeps no capability unless the
spec names it (`DOCKER_CAPS` for docker's set; keep neither
`CAP_SYS_ADMIN` nor `CAP_PERFMON`).

# T-227: `pod-enter`: a rule of `Plan.confine` misses a path that the child mounts after the compile (PodUtils/podvm#267)

**Source:** PodUtils/podvm#267 (COORD-048 of podvm, found by PodUtils/podvm@T-1658 at `v0.25.0`)
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

On the namespace rung, a confined payload cannot write `/tmp`, although
the spec gives `/tmp` read and write. podvm's run at `v0.25.0`:

```text
$ podvm run --rm --podvm-confine='write=/tmp' busybox:musl sh -c 'echo x > /tmp/a; ls -ld /tmp'
sh: line 0: can't create /tmp/a: Permission denied
drwxrwxrwt    2 0        0               40 Nov 11 21:18 /tmp
```

The `/tmp` of the payload is the tmpfs that the namespace rung mounts in
the child (size 40, mode 1777); the `/tmp` of the rootfs is a directory
of mode 1775. The same run with no spec writes `/tmp`. podvm's reading:
`pod_enter::confine::compile` opens each path of the spec before the fork
(`Spec::compile_in`, `RESOLVE_IN_ROOT` under the root descriptor), so the
rule holds the directory of the rootfs; the child then mounts the tmpfs
on it, and no rule names the root of the tmpfs. Each path that the child
mounts after the compile has the same defect. podvm asks: open the paths
of the rules in the child, after its mounts and its root change and
before `apply` (an `open(O_PATH)` of a prepared path allocates nothing),
and keep the checks of the compile before the fork (the ABI, the TCP
list, the count, a missing path refused by name); or take the root of
each mount that the child makes. podvm recommends the first, as it also
covers a mount that the caller adds later. podvm's profile `errand` gives
`?/tmp` write, and podvm takes the tag that has the fix.

## Premise

- Read: `crates/pod-enter/src/launch.rs` (the child of the namespace
  rung: `unshare(CLONE_NEWNS)`, `/` private and recursive, the tmpfs on
  the `/tmp` of the image, then the `chroot` by path and the device and
  inode check), `crates/pod-enter/src/confine.rs` and
  `crates/pod-confine/src/compile.rs` (the rule keeps the descriptor of
  the compile). The namespace rung mounts one tmpfs, and no volume and no
  bind. The caller of a ladder rung makes its mounts before the call, so
  the compile already sees them.
- Measured on Linux 6.18.44 (Landlock ABI 7), as root, with a C program:
  a directory `D/tmp`, and a rule (read and write) on a descriptor of it
  that opens before a tmpfs mount on it, in a private mount namespace.
  - No mount: a create in `D/tmp` passes.
  - A tmpfs on `D/tmp`: with the rule on the directory under the mount,
    `EACCES`; with a rule on the root of the tmpfs (opened after the
    mount), it passes; with a rule on `D`, the parent of the mount point,
    it passes; in a subdirectory of the tmpfs, with the rule on the
    directory under the mount, `EACCES`.
  - A bind mount of `D/bsrc` on `D/bdst`: a rule on `bsrc` covers a create
    in `bdst`; a rule on `bdst` (the directory under the mount) does not
    (`EACCES`).
  - After a `chroot` by path into `D`: the same results.
- The measured cause: the walk of Landlock goes from the root of a mount
  to its mount point, and continues at the parent of the mount point. It
  does not read a rule on the mount point itself, which the mount hides
  (security/landlock/fs.c, `is_access_to_paths_allowed`: after
  `follow_up`, "Ignores hidden mount points"). Thus a rule on the `/tmp`
  of the image never covers the tmpfs on it. The mount namespace and the
  `chroot` by path are not the cause: a descriptor that opens in the
  namespace of the caller covers its directory in the new namespace (the
  case with no mount passes after the `unshare`), as the rule holds the
  inode and not the mount.
- Call C57 says that the tmpfs "stays under a rule on that directory, as
  Landlock walks up through the mount point". The measurement disproves
  it.
- Not measured: a ladder rung with a confinement.

## Approach

`pod-confine`: each rule keeps its path as a C string from the compile.
`Compiled::close_paths()` closes the descriptor of each rule after the
checks of the compile; `Compiled::open_paths()` opens the path of each
rule with no descriptor as `O_PATH | O_CLOEXEC`, in the view of the
caller (its mounts and its root), with no allocation. A failed open is a
`Refusal` with a new call `Call::OpenPath` and the index of the rule. The
compile keeps its checks. `pod-enter`: the entry closes the descriptors
after the compile, so that no rule crosses the fork; the child opens the
paths after the passed descriptors, its mounts, its root change and its
`chdir`, before the limits and `apply`, on each entry. A refusal goes
through the readiness pipe as step 15 with the path of its rule. Tests in
`tests/confine.rs`: the namespace rung with a spec that gives `/tmp`
write: a write into `/tmp` passes and lands on the tmpfs, and a write
into `/var` and into `/` of the image fails with `EACCES`; the same on the
chroot sequence and on the entry with no chroot; a rule on a path that
the tmpfs hides is a refusal by name. The rungs with a chroot need root:
a script runs the test binary as root in CI. A plant that keeps the
descriptors of the compile. A new call corrects call C57.

## Prove

```sh
cargo test -p pod-confine --all-features
cargo test -p pod-enter --all-features --test confine
sh scripts/ci/root-enter-confine-tests.sh
sh scripts/plant-each.sh enter-confine-rule-under-a-mount
```

## Done

2026-11-11, `ec31d6c` (the fix and the tests), `785ced7` (the plants),
and the commit of this section (the documents and call C68).

`pod-confine`: each `Rule` keeps its path as a C string from the compile.
`Compiled::close_paths()` closes the descriptor of each rule (its `fd` is
then -1); `Compiled::open_paths()` opens the path of each rule with no
descriptor as `O_PATH | O_CLOEXEC` in the view of the caller, with no
allocation, and a failed open is a `Refusal` of the new call
`Call::OpenPath` (code 11, `open(O_PATH)`) with the index of its rule.
`Refusal::describe` names the path. The compile keeps each check.
`pod-enter`: `confine::compile` calls `close_paths` after the compile, in
place of `keep_clear_of`; the child calls `open_paths` after the passed
descriptors, the mounts of the namespace rung, the root change and the
`chdir`, before the limits and `apply`, on each entry. A refusal is step
15 of the readiness pipe.

The test that failed on the code before the fix:
`a_rule_covers_the_tmpfs_of_the_namespace_rung` (the namespace rung gave
`/tmp/a=-13 /var/a=-13 /a=-13`; it now gives `/tmp/a=0 /var/a=-13
/a=-13`, the write lands on the tmpfs and not in the image, and the chroot
sequence gives the same), and
`a_rule_on_a_path_that_the_tmpfs_hides_is_refused` (the payload ran with
a rule on the directory under the tmpfs; now `the confinement failed at
open(O_PATH) for the path "/tmp/hidden": ENOENT (2)`). Also new:
`a_write_rule_holds_on_the_entry_with_no_chroot`, and the unit test
`the_paths_close_after_the_compile_and_open_again`.

`cargo test -p pod-confine --all-features`: 29 unit tests and 12 in
`tests/apply.rs` pass; `cargo test -p pod-enter --all-features`: 167 unit
tests, 5 in `tests/bounded_payload.rs`, 10 in `tests/confine.rs` (before
the fix 8 passed and 2 failed) and 2 in `tests/tty_refusal.rs` pass, as
root with no skip. `sh scripts/ci/root-enter-confine-tests.sh`: 10 pass
as root; the step "The confinement tests of pod-enter as root" of the
job `test` of `ci.yml` runs it, as the rungs with a chroot skip as
another user. `sh scripts/ci/root-confine-tests.sh`: 1 passes. `cargo
clippy -p pod-confine -p pod-enter --all-targets --all-features -- -D
warnings` and `cargo fmt --all --check` pass. Plants (run by hand with a
target of this work): `enter-confine-rule-under-a-mount` (the
descriptors of the compile cross the fork) is caught (`/tmp/a=-13`);
`enter-confine-rule-on-a-passed-number` of T-108 was stale, as no rule
descriptor crosses the fork now: its defect is now a child that opens the
paths before the passes, and it is caught (`EINVAL` for a rule on a
passed `/dev/null`); `enter-confine-not-applied` is still caught;
`sh scripts/plant-each.sh --apply`: 0 stale.

The namespace rung mounts one tmpfs and no volume or bind; the fix
covers each mount that a child makes on the path of a rule. Breaking:
`Call` has the variant `OpenPath`; `Rule` is no longer `derive(Debug)`
but has its own `Debug`; a `Compiled` of `pod-enter` holds no rule
descriptor before the fork.

# T-240: `pod-enter`: `release_tmpfs` leaves a busy tmpfs mounted and its directory in the store

**Source:** the run of `sh scripts/agent/check.sh` before the push of T-213 (`tmpfs_stages_onto_a_mount_or_refuses_naming_it` failed one time)
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`release_tmpfs` unmounted with `umount2(dir, 0)`. A process that holds a
descriptor or a directory in the tmpfs (a child of the payload that runs
on, or a fork of another thread of the caller) makes that unmount fail
with `EBUSY`; the removal then empties the tmpfs and cannot remove its
mount point. The pages stay, and the next run meets the directory. In a
run of the workspace's tests, the test
`tmpfs_stages_onto_a_mount_or_refuses_naming_it` failed at
`assert!(!dir.exists())`: another test of the binary forked while the
stage held a descriptor in the mount.

## Premise

- Read: `crates/pod-enter/src/stage.rs` (`stage_tmpfs`, `release_tmpfs`)
  and `pod_hostprobe::sys::umount`.
- Measured: a descriptor held open in the tmpfs makes the old release
  leave the directory (the new test fails on the old code).

## Approach

`pod_hostprobe::sys::umount_detach` (`MNT_DETACH`): the mount leaves the
tree at once, and the kernel frees it when the last holder goes. The
release and each cleanup path of `stage_tmpfs` use it. A test holds a
descriptor in the tmpfs across the release.

## Prove

```sh
cargo test -p pod-enter --lib -- stage::tests
```

## Done

2026-11-12. `umount_detach` and `MNT_DETACH` in `pod-hostprobe`;
`release_tmpfs` and the two cleanup paths of `stage_tmpfs` detach. The
test `a_release_frees_a_tmpfs_that_a_descriptor_holds` failed on the old
release and passes; `cargo test -p pod-enter -p pod-hostprobe --lib`:
168 and 111 pass.

# T-241: `pod-confine`: the permissions and the quiet flags of Landlock ABI 11 and 12

**Source:** T-213 (a reading of `include/uapi/linux/landlock.h` of Linux 7.4-rc2)
**Category:** feature
**Milestone:** M7
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

The seccomp list of `pod-confine` denies the namespace and mount calls by
number, and `apply` drops the capabilities that a spec does not keep. A
filter sees no call behind io_uring or `socketcall(2)`, and reads no
argument behind a pointer (`clone3`). Linux 7.4 (Landlock ABI 12) has
`LANDLOCK_PERMISSION_NAMESPACE_USE` and
`LANDLOCK_PERMISSION_CAPABILITY_USE` with `LANDLOCK_RULE_NAMESPACE` and
`LANDLOCK_RULE_CAPABILITY`: the LSM denies each namespace type and each
capability that no rule allows, on each kernel path. The ruleset of ABI
10 and later also has `quiet_access_fs`, `quiet_access_net` and
`quiet_scoped` (no audit record for a denial).

## Premise

- Read: `include/uapi/linux/landlock.h` and
  `security/landlock/syscalls.c` of Linux 7.4-rc2 (`landlock_abi_version
  = 12`; `struct landlock_ruleset_attr` has `handled_permissions`).
- Not measured: a kernel with ABI 11 or 12; this host has ABI 7.

## Approach

From ABI 12, `Spec::compile` handles both permissions: a namespace rule
for no type (the default deny list denies them all), and a capability
rule for each bit of `keep_caps`. The seccomp rules and the capability
drop stay for the older kernels. `pod_hostprobe::sys` gets the rule
types and the larger `LandlockRulesetAttr`. A test of the compile; a run
on a 7.4 kernel in CI where a runner has one.

## Prove

```sh
cargo test -p pod-confine --all-features
```

# T-250: `pod-supervise`: the test of a payload that kills its launcher raced the launcher's word of the start

**Source:** the run of `ci.yml` at `30a01f0` (job `test`)
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`a_payload_that_kills_its_launcher_leaves_a_dead_record_and_no_count` of
`crates/pod-supervise/tests/tree.rs` (T-202) failed in CI at its `start`:
`the launcher exited and did not say whether the payload started`. The
payload killed its launcher at once, and a launcher killed before it says
that the payload started makes `start` fail. The test is of the record
after the start, not of that case.

## Premise

- Read: `crates/pod-supervise/src/launcher.rs` (the word of the start;
  `Class::Unavailable` when the launcher exits before it), the test.
- Not measured: the race on this host (25 runs of the binary passed
  before and after the fix).

## Approach

The payload waits for a file that the test writes after `start`
returned, then kills its launcher.

## Prove

```sh
cargo test -p pod-supervise --test tree
```

## Done

2026-11-12. The payload waits for `orphan.go`, which the test writes
after `start`; 25 runs of the binary pass. The case of a launcher killed
before its word stays a refusal of `start` (`Class::Unavailable`).

# T-262: `pod-confine`: a TCP list is refused on a target with `socketcall(2)` (PodUtils/podvm#276)

**Source:** PodUtils/podvm#276 (COORD-055 of podvm, the answer to GitHub #85)
**Category:** defect
**Milestone:** M7
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

On a target whose kernel has `socketcall(2)` (i686, s390x, powerpc64),
the filter does not see a socket made through it: the MPTCP rule and the
rules of the families of a TCP list do not hold there (T-213). A closed
UDP is refused there by name; a TCP list was taken. podvm releases i686
and powerpc64le builds, and a profile with `net.ports` gives a TCP list
on each target. podvm asks for the same refusal of a TCP list, in
`pod-confine`, so that each consumer gets the same answer.

## Premise

- Read: PodUtils/podvm#276; `crates/pod-confine/src/compile.rs`
  (`deny_rules_on`, `SOCKETCALL`).
- Not measured: a payload on i686 that makes an MPTCP socket through
  `socketcall` under a TCP list (no i686 kernel here).

## Approach

`Spec::deny_rules_on` refuses a spec with a TCP list (`connect` or
`bind`) where the target has `socketcall(2)`, with `Class::Unsupported`
and a reason that names the gap; the test of the refusal for each list;
a plant that takes the list there.

## Prove

```sh
cargo test -p pod-confine --all-features --lib -- a_tcp_list_is_refused_where_the_kernel_has_socketcall
sh scripts/plant-each.sh confine-tcp-list-through-socketcall
```

## Done

2026-11-12. `crates/pod-confine/src/compile.rs`: a TCP list on a target
with `socketcall(2)` is `Class::Unsupported` ("the spec holds a TCP list,
and the kernel of this target has socketcall(2)"). The test
`a_tcp_list_is_refused_where_the_kernel_has_socketcall` (a `connect` list
and a `bind` list; the compile on the build target agrees with
`SOCKETCALL`); `the_switches_of_the_spec_add_their_rules` reads the rules
of a TCP list with no `socketcall`. `cargo test -p pod-confine
--all-features`: 33 + 12 + 1 passed. The plant
`confine-tcp-list-through-socketcall` is caught. Breaking: a consumer that
compiles a TCP list on i686, s390x or powerpc64 gets a refusal.

# T-266: `pod-guest`: the provision writes its image with `qemu-img convert`; `qemu-img commit` takes no output file (PodUtils/podvm#277)

**Source:** PodUtils/podvm#277 (COORD-056 of podvm, for PodUtils/podvm@T-1641)
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`podvm windows setup` booted and provisioned the ValidationOS guest under
KVM, then failed: `qemu-img commit -f qcow2 run.qcow2 base.podvm.qcow2`
answers "Expecting one image file name" (podvm's run 43148947380, QEMU
10.2.1). `commit_argv` of `crates/pod-guest/src/plan.rs` read the synopsis
as `commit [-b BASE] [-F FMT] FILENAME [OUTFILE]`. `qemu-img commit` has
no output file: it writes an overlay into its backing file, in place. The
unit test checked the argv alone, so it passed.

## Premise

- Read: PodUtils/podvm#277; `qemu-img --help` (8.2.2): `commit [...]
  filename`, one file; `convert [...] [-O output_fmt] filename
  output_filename`.
- Measured here (qemu-img 8.2.2): a qcow2 overlay over a VHDX base;
  `qemu-img commit -f qcow2 run.qcow2 out.qcow2` exits 1 with "Expecting
  one image file name"; `qemu-img convert -f qcow2 -O qcow2 run.qcow2
  out.qcow2` exits 0, and `qemu-img info` of `out.qcow2` names no backing
  file.
- Not measured: the provision of ValidationOS with the fix (podvm runs
  `kvm.yml` again with the tag).

## Approach

`convert_argv` in place of `commit_argv`: `qemu-img convert -f qcow2 -O
qcow2 OVERLAY OUT` reads the chain and writes a new image with no
backing file; the base is not written. A test that runs `qemu-img` and
`qemu-io` of the host: a raw base, an overlay, a write into the overlay,
the convert; the image stands alone and holds the base and the write, and
the base keeps each byte. `guest.yml` and the plant require the tools.

## Prove

```sh
POD_GUEST_TEST_QEMU=required cargo test -p pod-guest --all-features --test convert
sh scripts/plant-each.sh guest-commit-has-no-output-file
```

## Done

2026-11-12. `crates/pod-guest/src/plan.rs`: `convert_argv` (`qemu-img
convert -f qcow2 -O qcow2 OVERLAY OUT`); `provision` runs it. Breaking:
`commit_argv` is `convert_argv` (podvm re-exports it from
`podvm_windows`). `tests/convert.rs`
(`the_convert_writes_a_standalone_image_and_leaves_the_base`): passes
with qemu-img 8.2.2 here; with `POD_GUEST_TEST_QEMU=required` a host with
no `qemu-img` fails it. `guest.yml` sets the variable; `plants.yml`
installs `qemu-utils`. The plant `guest-commit-has-no-output-file` (the
argv of `commit` again) is caught: "Expecting one image file name".
`cargo test -p pod-guest --all-features --lib`: 89 passed. `guest.yml`
run 43154363052 (with `POD_GUEST_TEST_QEMU=required`): passed.

# T-320: `pod-extract`: a correct plain-tar layer is refused: the hash stops at the end marker (PodUtils/podvm#281)

**Source:** PodUtils/podvm#281 (COORD-060 of podvm, for PodUtils/podvm@T-1721).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`pod-extract` at `v0.30.0` refuses a correct uncompressed tar layer with
"digest mismatch". `layer::open_hashed` hashes the raw bytes that the
decompressor pulls through its `BufReader` of 64 KiB; both passes of
`extract_layers` stop at the end marker of tar, drop the stream and call
`verified`. GNU tar pads an archive to a record of 10 240 bytes after the
marker: when the marker is in one read of 64 KiB and the padding goes past
it, no read takes the padding, and the hash covers less than the blob.
The failure depends on where the marker falls, so `podvm import` of a GNU
tar fails at random (podvm's experiment 356 on arm64). podvm's
reproduction: one file of 60 000 bytes, `tar -cf` (71 680 bytes; the marker
ends at 61 952).

## Premise

- Read: `crates/pod-extract/src/layer.rs` (`open_hashed`, `HashedStream`)
  and `crates/pod-extract/src/lib.rs` (`extract_layers`, `verified`).

## Approach

Before each `verified`, read the rest of the raw blob into the hash: the
padding of tar, and the bytes after a compressed frame (a decoder can stop
at the end of its frame).

## Prove

```sh
cargo test -p pod-extract --all-features
```

## Done

2026-11-12. The commit that names T-320.

- `HashedStream` holds the raw reader (the file, hashed as each byte
  comes, behind its buffer of 64 KiB), which the decompressor reads too;
  `HashedStream::finish` reads it to its end into the hash. `verified`
  calls it before the digest counts, after each of the two passes. The
  bytes in the buffer were hashed as they came from the file, so the hash
  covers each byte once, for each compression.
- Test `a_tar_padded_to_its_record_after_the_end_marker_is_no_mismatch`
  (podvm's reproduction: one file of 60 000 bytes, the end marker at
  61 952, the padding to 71 680): it failed with the code before (the
  mismatch) and passes. `cargo test -p pod-extract --all-features`: 56
  passed; clippy is clean.
- The plant `extract-hash-stops-at-the-end-marker` (the call of `finish`
  dropped) is caught by that test.

podvm confirmed it on arm64: `arm.yml` run 43228497851 of podvm (`d4ed988e`, podcrates `v0.31.0`, `ubuntu-24.04-arm`); podvm closed PodUtils/podvm#281 and #282.

# T-321: `pod-enter`: `libc_beside` misses the aarch64 glibc of a Debian image: the search is depth-first from the end (PodUtils/podvm#282)

**Source:** PodUtils/podvm#282 (COORD-061 of podvm, for PodUtils/podvm@T-1721).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`pod_enter::abi::libc_beside` at `v0.30.0` gives `None` for a Debian
bookworm image on arm64: `PT_INTERP` is `/lib/ld-linux-aarch64.so.1`, `/lib`
is a link to `usr/lib`, and the library is in `/usr/lib/aarch64-linux-gnu/`.
`search_file` walked with a stack, the last name first, and stopped after
512 entries; `aarch64-linux-gnu` sorts before `apt`, `dpkg`, `gcc` and the
others, so the bound was spent first (on x86_64 `x86_64-linux-gnu` sorts
last and was found). The tree of `lib` and of `usr/lib` was walked two
times. podvm then declines the loader family for a dynamic glibc program
of a Debian or Ubuntu image on arm64.

## Premise

- Read: `crates/pod-enter/src/abi/search.rs` (`libc_beside`,
  `search_file`, `first_match`) and its tests.

## Approach

Each root resolved inside the root file system and walked once; first the
places of a multiarch layout (the name in a root and one directory under
it), with no bound; then the bounded walk, breadth-first. podvm's
reproduction as a test, for aarch64 and for x86_64.

## Prove

```sh
cargo test -p pod-enter --all-features
```

## Done

2026-11-12. The commit that names T-321.

- `search_file`: the roots `lib`, `lib64`, `usr/lib`, `usr/lib64`
  resolve inside the root (`resolve_dir_in`), and a real directory is
  walked once. First `NAME` in each root and in each directory one level
  under it (one look for each directory, no bound): the multiarch place.
  Then the walk of before, breadth-first (`VecDeque`), with its bounds of
  three levels and 512 entries. A breadth-first walk alone spends the
  bound in the directories that sort before `x86_64-linux-gnu`, so the
  multiarch look comes first.
- Test `the_multiarch_libc_is_found_beside_large_trees_of_a_merged_usr`
  (podvm's tree: `lib` a link to `usr/lib`, eight directories of 1 000
  files beside the triplet), for aarch64 (183) and x86_64 (62): the old
  search gave `None` on aarch64; now each gives its library. `cargo test -p
  pod-enter --all-features`: 169 passed; clippy is clean.
- The plant `enter-libc-search-misses-the-multiarch-dir` (no multiarch
  look, the walk from the end again) is caught by that test.

podvm confirmed it on arm64: `arm.yml` run 43228497851 of podvm (`d4ed988e`, podcrates `v0.31.0`, `ubuntu-24.04-arm`); podvm closed PodUtils/podvm#281 and #282.

# T-326: `pod-relay`: the trial of the blocking facade in podvm, for PodUtils/podvm@T-1678 (PodUtils/podvm#283)

**Source:** PodUtils/podvm#283 (COORD-062).
**Category:** feature
**Milestone:** M6
**Priority:** P1
**Effort:** L
**Status:** done

## Problem

PodUtils/podvm@T-1678 replaces `PodUtils/podvm:crates/podvm-ssh/src/mux.rs`,
`PodUtils/podvm:crates/podvm-ssh/src/ws.rs` and the curl pairing of
`PodUtils/podvm:crates/podvm/src/remote/` with `pod_relay::blocking`.
`adoption/podvm/` holds no `pod-relay.patch` and no `pod-relay.run`, so
podvm's entry is blocked.

## Premise

- Read: `PodUtils/podvm:crates/podvm-ssh/src/mux.rs` names its wire
  `reverse-v1`: the relay's `hello` with `version` 1. `pod-relay` speaks
  the dialect 1 (`reverse::control::PROTO`, `pod_ws::handshake::REVERSE_PROTO`)
  and reads a peer that states none as 1 (`UNSTATED`).
- Read: `crates/pod-relay/README.md`, section "podvm": `blocking::Client::new(Config::new(ctx))`.

## Approach

The procedure of a trial, as T-026 for podnet: a clone of podvm at the
commit of `adoption/podvm/commit`; podvm's relay glue over
`pod_relay::blocking` with podvm's `Context`; the files that go; each
behaviour that changes for podvm's users; the files pod-relay.patch
and pod-relay.run in `adoption/podvm/` (podvm's own tests of its relay, and
its live wire where a stand-in serves it); `adoption.yml` passes the trial.

## Prove

```sh
sh scripts/adoption.sh podvm pod-relay
```

## Done

2026-11-12, partial: the trial of podvm passed on this host. The commit of
this record holds adoption/podvm/pod-relay.patch and `.run`, the new
`adoption/podvm/commit`, and the changes of `pod-relay` (calls C130 to
C134). What remains: `adoption.yml` at the push, and podvm's take.

- The commit of the trial: podvm's `main` at `c5b61b45`
  (`c5b61b457aabaaade832198585a83fbfba45511f`, its take of `v0.31.0`). No
  other trial of podvm is open, so `adoption/podvm/commit` moves to it
  (call C134). Each podcrates tag of podvm moves to `v0.32.0`.
- The patch (36 files, 848 lines in, 2,562 out): `PodUtils/podvm:crates/podvm-ssh` takes
  `pod-relay` (features `blocking` and `plain-ws`) and `pod-ws`; the module
  `mux` is the glue (`mux.rs`, `mux/node.rs`, `mux/origin.rs`, 505 lines
  with no test): `context()` (the TLS of `pod-tls` with podvm's host bundles,
  the proxy of the environment over `pod_proxy::Dialer`, `Cache::none()`),
  `run_node` over `Client::run_node` with `Pair::for_node`, the server of
  each session on a socketpair (`server::spawn_socket`, `Local::new` and
  `on_finish`), `NodeOptions::say` for podvm's lines and `--once`, and
  `run_operator` over `Client::run_operator` with `--ready-wait` and the
  10 s goodbye; no end-to-end channel on either leg (call C133). `ws.rs`,
  `ws/handshake.rs`, `ws/tests.rs`, `tls.rs` and `mux/operator.rs` go, with
  the unused `base64` pin. The fake relay of `tests/mux_two_client` keeps
  its own WebSocket server over `pod_ws::frame` and refuses a wrong token
  (403) and a second node (409) at the upgrade. podvm's record (the
  citations of the removed files, named at `c5b61b45`), its `SECURITY.md`,
  `PodUtils/podvm:docs/architecture.md`, `PodUtils/podvm:docs/code-map.md`,
  `PodUtils/podvm:docs/limits.md` and `PodUtils/podvm:docs/runtime-state.md` (the rows of `pod-relay` and `pod-ws`; `base64`
  leaves; `sha2` 0.10.9 comes with the crate) follow.
- `sh scripts/adoption.sh podvm pod-relay`: each of the 17 steps exits 0:
  the glue's unit tests and the binaries' (83, 2, 3, 6, 2 passed), the fake
  relay with the real binaries (14 passed), the SSH test of three
  authenticated sessions through `sshd` (1 passed; OpenSSH was installed on
  this host for it, and the step says so where a host has no `sshd`),
  podvm's `remote::` tests (25 passed), clippy of the workspace with `-D
  warnings`, rustfmt, the features in the tree, the removed files, no copy
  of the moved items, the build of the helpers, podvm's gate (`check-todo:
  ok`) after the runtime-state rows, `check-consumer-toolchains.py` (5 pins,
  0 below 1.99), and `check-static.sh` of `node` and `operator`.
- The wire stays `reverse-v1` (the `hello` of version 1, the crate's
  dialect 1); a relay of another dialect ends the node with exit 125.
- The behaviours that change for podvm's users: (1) a token under 8
  characters, and a name that starts with `-` or `.`, are usage errors
  (125); (2) the `hello` is a hint: a frame cap or a session count that
  podvm refused, an `open` or data before the `hello` no longer end the
  node; (3) a wrong token (`403`) ends a node at once with 125, where podvm
  tried again for ever; a `409` at the first registration is tried again for
  30 s, then 125; (4) the node's lines are the crate's ("registered as NAME
  via RELAY (node speaks reverse-v1)" with no frame cap and session count;
  "podvm-ssh: node: ..." for each end of a socket and each failed try with
  the next one); (5) a session past the `hello`'s `maxSessions` is refused
  with "the node has its N sessions", not "node is full", and is not in
  the count of refused sessions; (6) the operator's words: "the session
  never started: REASON (relay close CODE)", and "the node did not answer
  ready within N s"; a `bye` before `ready` no longer ends it at once (it
  waits `--ready-wait`); (7) the redial is the crate's (at once after a
  socket that served, then the backoff of the opener), with pings on each
  socket; (8) `--once` with a first try that gives no socket exits 125, as
  before, through the line of call C132.
- `pod-relay`: `Pair::for_node` (call C130), `NodeOptions::say` and
  `blocking::Say` (call C131), the line of each failed try of a node (call
  C132), and the note that a facade's runtime runs only inside its calls
  (the drain of a session that the relay closed ends at the drop of the
  client: the glue drops it before it counts). Tests: `tests/blocking_given.rs`
  (3) and `pair::given` (2). `cargo test -p pod-relay --all-features`: 394
  passed, 19 ignored; clippy of the workspace with all features: clean.
- Plants: 2 new (group `relay`): scripts/plants/relay-t326-silent-failed-try
  (call C132) and scripts/plants/relay-t326-facade-says-nothing (call C131),
  each caught on this host by its test of `tests/blocking_given.rs`, which
  stops its node when an assertion fails, so that a planted defect fails
  the test in 10 s and does not hold it. relay-t643-426-reconnects was stale
  after the change of `reverse::run`: its patch is made again on the same
  line, and it is caught. `sh scripts/plant-each.sh --apply`: 0 stale.
- Findings: podvm has no pairing code in `PodUtils/podvm:crates/podvm/src/remote/`;
  its drives pair with curl and give the tokens in the argv of curl (T-330).
  The take adds the resumable layer and the end-to-end channel to podvm's
  build, which its plain legs do not use (T-290).
- Not measured: the trial in `adoption.yml`; the live relay through the
  glue; the size of podvm's release binaries after the take.

podvm took the trial with `v0.32.0` at `15485d1b` (PodUtils/podvm#285,
2026-11-12): the patch applied on podvm's `main` (`git apply --3way`), and
each step of `pod-relay.run` passed there; PodUtils/podvm@T-1678 is done. No
`taken` line: `crates/pod-relay/SOURCES` has no block of podvm (the crate
came from podssh). The trial files left `adoption/podvm/`; the take is in
the map of `adoption/README.md`.

# T-328: GNU `tar -xz -C DIR` fails with "Cannot open" right after `mkdir -p DIR` in a podvm unit (podrunner's note)

**Source:** podrunner's session (its take of `v0.31.0`, PodUtils/podrunner@T-094): podvm `v0.2.0-beta.5` on the GitHub x86_64 host, runner-image run 43205758271 of podrunner.
**Category:** research
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

In a podvm unit, `mkdir -p DIR` then GNU `tar -xz -C DIR` of the runner
image failed with "Cannot open: No such file or directory". podrunner
works around it with `cd DIR` and `gzip -dc | tar -xf -`. The case is
not reduced. `tar -C` opens the members relative to a directory fd
(`openat`, `mkdirat` with a dirfd), so the cause can be in the path
layer of `pod-enter` or of podvm, not in tar.

## Premise

- Read: podrunner's note (the message of its session).
- Measured by podrunner (runner-image run 43230901394, x86_64, podvm
  `v0.2.0-beta.5`): in `/tmp` of a plain `podvm run` unit each form passed
  (`-C`, `-C -z`, `cd`, `cd` with `gzip -dc`, `cd -z`). The failure does
  not come back there. podrunner's next run measures each form under the
  path of the failed step (the work directory of the runner).
- Not measured: the tier of the unit (rootfs or machine), the syscalls
  of tar, and which crate translates them.

## Approach

1. Reduce: in a unit of podvm's rootfs tier, `mkdir -p d && tar -xf
   small.tar -C d` against a tar of one directory and one file; then the
   same with `-z`.
2. If it fails: trace the `openat`, `mkdirat` and `fchdir` of tar with
   their dirfd, and compare with the path layer of `pod-enter`; a test of
   the crate with the reduced case.
3. If it does not fail in the crate's layer: a COORD issue `(to podvm)`
   with the reduced case.

## Prove

```sh
cargo test -p pod-enter --all-features
```

## Done

2026-11-12. Reduced by podrunner (PodUtils/podrunner#70, run 43242367198):
in a unit of the supervise rung, `tar -xzf x.tgz -C /tmp/d` (an absolute
`-C`) fails, and each relative form passes. No crate of podcrates holds
the runtime of the supervise rung (`pod-hostprobe` selects it; `pod-enter`
enters the namespace, chroot and userland rungs): the cause is in podvm.
Relayed to podvm, with the case and a guess, on #102 (COORD-063, also to
podrunner).

# T-353: `pod-oci`: a pull from the CNCF distribution registry on the loopback; the live pull from ECR Public each week

**Source:** `docs/live-calls.md` (row L4); `services.yml` run 42803032733
(ECR Public gave 429).
**Category:** feature
**Milestone:** M5
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

`crates/pod-oci/tests/live.rs` pulls the pinned guest userland from ECR
Public each day. ECR Public limits the shared addresses of the runners
(429, "Data limit exceeded"), so the test skips on a 429 and proves
nothing on that day. The offline fixture
(`crates/pod-oci/tests/common/registry.rs`) is a small server that this
crate wrote: one manifest, plain HTTP, no index, no token. The bearer
token flow, an index of two platforms and the answers of a real registry
are proven only by the live pull.

## Premise

- Measured: `services.yml` run 42803032733 failed `ecr-public-registry
  oci-digest` with HTTP 429 (the log read 2026-11-12); T-142 met "Data
  limit exceeded" once.
- Read (2026-11-12): https://github.com/distribution/distribution/releases
  (v3.1.2, 2026-09-24, Apache-2.0); the image `registry:3.1.2`, index
  `sha256:82f6ff4a7f982960c8aa51617fea3aabc485df6e55cce80d22deb1de36f1bc3c`
  (20 MB for amd64).
- Not measured: the token auth of distribution (`auth: token` needs a
  token server that signs a JWT) with the client of `pod-oci`; a push of
  an index of two platforms by `docker buildx` with no emulator (each
  image `FROM scratch`).

## Approach

1. scripts/ci/registry-tests.sh (call C151): `registry:3.1.2` through
   `scripts/pull-image.sh` on 127.0.0.1:5000, with htpasswd auth, then the
   token auth against a token stand-in of the test (an HTTP stand-in of
   `pod-testkit` that signs with a key of the test). The script pushes an
   image of the test: an index of linux/amd64 and linux/arm64, each
   `FROM scratch` with one file.
2. A test target `distribution` of `pod-oci`: `pull::resolve` of the
   index for each platform, the config and the layer by digest, the
   refusal of a digest that does not match, and a 401 with no login. With
   no registry it skips, unless `POD_OCI_TEST_REGISTRY=required`.
3. `live.rs` runs each week (T-352). The daily fact is the watch
   `oci-digest` of `ecr-public-registry`.

## Decision

Call C186: the release binary of distribution in place of the image, and
the token server in the test target in place of `pod-testkit`. Call C187:
no login and no token goes over plain HTTP. Call C188: the made-up login
gets a comment in `.github/secrets-allow.txt`, and no line.

## Prove

```sh
sh scripts/ci/registry-tests.sh
cargo test -p pod-oci --all-features
sh scripts/plant-each.sh registry-skip-fails
```

## Done

2026-11-12, commit `54bb911`. The test target `distribution` of `pod-oci`
runs against the release of distribution 3.1.2
(`registry_3.1.2_linux_amd64.tar.gz`, SHA-256
`40df2224d410f72ae425c3371873b078bbdbda3b8b612be9571f0e6751f3acc8`, the
digest that GitHub gives for the asset) on 127.0.0.1 over HTTPS. The test
pushes an index of linux/amd64 and linux/arm64; `pull::resolve` gives the
index and the manifest of each platform, and the config and the layer come
by digest with the pushed bytes, under htpasswd and under token auth (one
token ask, with the login, for `repository:pod-oci/test:pull`). A wrong
password, a request with no login and a token ask with no login get 401.
A layer and a manifest changed in the store are `Refusal::Digest`. A
registry over plain HTTP that asks for a login gets none (C187; a new unit
test proves it offline too). `sh scripts/ci/registry-tests.sh`: 5 passed,
each line said; with no registry, `POD_OCI_TEST_REGISTRY=required` fails
(the plant `registry-skip-fails`). `cargo test -p pod-oci --all-features`:
90 unit, client 3, distribution 5 (skipped with no registry), proxy_pull 2,
registry_fixture 1; live 1 ignored. Row L4 of `docs/live-calls.md` names
the stand-in. Not measured here: the step in a run of `ci.yml` on GitHub.

# T-330: `pod-relay`: podvm's members that make, ask about and stop a pair, in place of the curl of its drives

**Source:** T-326 (call C134).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

PodUtils/podvm@T-1678 names "a pairing through curl in
`PodUtils/podvm:crates/podvm/src/remote/`". podvm has no such code: its `remote ssh
serve` and `connect` take the parts of a pair as flags, and its drives
make, ask about and stop a pair with curl
(`PodUtils/podvm:experiments/389-remote-ssh.sh`,
`PodUtils/podvm:experiments/387-mux-two-client.sh`): `POST /v1/pair`,
then `GET /v1/status/NAME` and `POST /v1/stop/NAME` with the connect
token and the stop token in a `-H` argument of curl, which other users of
the host can read in the list of processes. The trial of T-326 does not
replace them.

## Premise

- Read: the clause 4 of `PodUtils/podvm:experiments/389-remote-ssh.sh` at
  podvm `c5b61b45`: the pair's answer goes to a file and `jq` reads each
  token into a variable of the shell.
- Read: the facade has `Client::pair`, `Client::status(&Pair)` and
  `Client::stop(&Pair)`; `pair::store` keeps a pair in a private file under
  a label, and `pair::load` reads it back with its three tokens.
- Not measured: a pair of the live relay through the facade (the egress of
  a session reaches no relay).

## Approach

A patch for podvm (a second trial file of `adoption/podvm/`, after
pod-relay.patch): `podvm remote ssh pair --relay wss://HOST [--label L]`
over `Client::pair` and `pair::store` in podvm's state directory, which
prints the name and the expiry and no token; `status` and `stop` over
`pair::load` and `Client::status` or `Client::stop`; `serve` and `connect`
take `--pair L` beside the token flags; each new flag gets its row in
podvm's parity table. The drives of podvm then call these members. If a
part of the facade is missing (a label in podvm's own directory, a pair
with less than 10 minutes left for `stop`), add it here with its test.

## Prove

```sh
cargo test -p pod-relay --all-features --test blocking_plain
sh scripts/adoption.sh podvm pod-relay
```

# T-345: `pod-netlab`: the probe program of the guest, which measures each probe of issue 178

**Source:** T-207 (the rest of its work; PodUtils/podvm#260, issue 178)
**Category:** feature
**Milestone:** M6
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

Issue 178: "`sand net check` runs `guest_check.py` in the guest. It only
measures." The probes: "tcp4/6 per port, udp, dns-udp, whoami, srcport
(bound to 40001), proxy, rtt (best of three), bulk, udp-burst (100
datagrams, bounds at 4 sigma), icmp over a ping socket, hairpin,
resolve, nat64-*, dns64, isp-whoami, inbound from the world". The
verdicts: "ALLOW, DROPPED (no answer), REFUSED (reset or
port-unreachable), DENY (local errno), UNRESOLVED, TIMEOUT (connected,
then stalled)". L2: "An unconnected UDP socket never sees
port-unreachable; probes connect first." F20: the ping socket needs the
probing gid in `ping_group_range`. T-207 gives `pod_netlab::probes`, the
target of each probe (`Probe::target`), its expectation and `Measured`;
nothing measures.

## Premise

- Read: issue 178; `crates/pod-netlab/src/probe.rs` (the names, the
  ports, `TRIES`, `SRCPORT`, `BURST`, `BULK_BYTES`) and
  `crates/pod-netlab/src/judge.rs` (`Measured`).
- Not measured: the errno of each verdict on the guest's kernel (Alpine
  3.22, `6.12.110-0-virt`); the time limits under TCG.

## Approach

A module `measure` of `pod-netlab` behind a feature `measure` (std
sockets and libc, no tokio), and a small static binary for the guest
that podvm ships: each probe of `Probe`, with a connected UDP socket
(L2), a ping socket (`SOCK_DGRAM`, `IPPROTO_ICMP` and `ICMPV6`), the bind
of 40001 for `srcport`, the best of three connects for `rtt`, the time to
the last byte of `GET /bulk`, 100 datagrams for `udp-burst`, the guest's
listener on 2200 for `hairpin` and the inbound probes, and the map of
each errno and ICMP error to a verdict. One line of output for each probe
(`NAME VERDICT [addr=A] [port=P] [n=N]`), its reader into `Measured`
with a fuzz target, and loopback tests of each verdict (ALLOW, REFUSED,
DROPPED with a silent listener, TIMEOUT with a stalled one, DENY with a
family that has no route).

## Prove

```sh
cargo test -p pod-netlab --all-features
```

# T-346: `pod-netlab`: the world's services (`world.py`) over `pod-dns` and `pod-proxy`

**Source:** T-207 (the rest of its work; issue 178, acceptance item 5)
**Category:** feature
**Milestone:** M6
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

Issue 178: "The world (`net/world.py`, stdlib) replies `you=ADDR
port=PORT` on TCP 22, 25, 443, 7844, 8443 and HTTP 80 (`GET /bulk` is
256 KiB), runs an open CONNECT proxy on 3128, echoes UDP 9999, and serves
DNS on 53 (`*.world.test` A+AAAA, `v4only.` A, `v6only.` AAAA, DNS64
synthesis with `--dns64`)." F21: "A wildcard-bound UDP server answers
from the route's preferred address; the world replies with
`IPV6_PKTINFO`." Acceptance item 5: "The world's DNS and proxy come from
podcrates crates, or the reason is recorded." With `--cgnat`, the probe
`isp-whoami` asks the carrier's 100.64.0.1 port 80 (call C147).

## Premise

- Read: issue 178; `crates/pod-netlab/src/probe.rs` (`WORLD_TCP`,
  `ECHO_PORT`, `NAME_BOTH`, `NAME_V4_ONLY`, `NAME_V6_ONLY`,
  `dns64_answer`).
- Not read: whether `pod-dns` has a DNS server or only the wire codec of
  its client; whether `pod-proxy` or `pod-broker` serves an open CONNECT
  proxy with no policy.

## Approach

A module `world` of `pod-netlab` behind a feature `world` (or a binary
of the guest image): the TCP replies, the HTTP server with `/bulk`, the
UDP echo that answers from the address of the request (`IPV6_PKTINFO`,
`IP_PKTINFO`), the DNS server over the wire codec of `pod-dns` (A, AAAA,
DNS64 in 2001:db8:64::/96), and the CONNECT proxy of `pod-broker` in its
open mode or of `pod-proxy`. A crate that lacks a server records why in
the README. Loopback tests of each service.

## Prove

```sh
cargo test -p pod-netlab --all-features
```

# T-347: `pod-netlab`: each offline scenario on a real kernel as root, with the planted flush

**Source:** T-207 (the rest of its work; issue 178, acceptance items 2 and 3)
**Category:** feature
**Milestone:** M6
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

T-207 derives each verdict and each counter from the knobs (call C149),
and loads each ruleset into nft 1.0.9 (test `each_ruleset_loads_into_nft`).
No lab ran on a kernel: the routes with an MTU, tayga, netem, the carrier,
the strict rule that each counter that no probe moves stays. Issue 178:
"`sand net matrix` brings every offline scenario up, checks it, and
tears it down", "Flushing the policy inside the guest makes the check
fail (planted)", F19 ("`apt-get install tayga` starts a host service and
a `nat64` tun; disable it"), and "A cage can be the guest ... bailey's
port list joins the expectation".

## Premise

- Read: issue 178; `crates/pod-netlab/src/plan.rs`.
- A GitHub runner is root and has network namespaces, nftables and
  `sch_netem` (`scripts/root-netns-tests.sh` runs the netns tests of
  `pod-hold` there). Not measured: tayga and `sch_netem` on
  `ubuntu-24.04`.

## Approach

A runner of `Step` (in the tests, or a module of the crate), the probes
of T-345 and the world of T-346; a new script in `scripts/ci/` that
`ci.yml` runs as root: each offline scenario up, checked, down, with
its time; the same run with `nft flush chain inet lab forward_policy`
before the probes must fail (the plant in a kernel). tayga from the
package with its service stopped (F19). A guest that is a cage: the
expectation of a lab takes the port list of the cage. The image and the
modules of podvm's guest stay with PodUtils/podvm@T-1662.

## Prove

```sh
cargo test -p pod-netlab --all-features
```

## Start condition

T-345 and T-346 are done.

# T-348: `pod-netlab`: the VPN and the live scenarios of issue 178 (12 of 39)

**Source:** T-207 (the rest of its work; issue 178; PodUtils/podvm@T-1663, issue 180)
**Category:** feature
**Milestone:** M6
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

Issue 178: "39 scenarios (`sand net scenarios`), each a knob set" and,
after the 27 offline ones, "plus the VPN and live ones". The issue gives
no name and no knob of those 12. "Tunnels and live uplinks are a
separate issue" (podvm's issue 180, PodUtils/podvm@T-1663: the tunnels
run in the same guest).

## Premise

- Read: issue 178 and podvm's ask PodUtils/podvm#260.
- Not read: issue 180 of podvm.

## Approach

Read issue 180 when podvm starts PodUtils/podvm@T-1663; add the knobs of
a tunnel (WireGuard in the lab) and of a live uplink, and the 12
scenarios by name, with their derivation; ask podvm through a COORD issue
for each fact that the issue does not give.

## Prove

```sh
cargo test -p pod-netlab --all-features
```

## Start condition

podvm starts PodUtils/podvm@T-1663, or gives the 12 names and knobs.

# T-349: `pod-netlab`: a lab on a user-space stack, and a lab with more than one guest

**Source:** T-207 (the rest of its work; issue 178, "podvm without root")
**Category:** feature
**Milestone:** M7
**Priority:** P3
**Effort:** L
**Status:** open

## Problem

Issue 178: "Lighter variant for policy-only labs: no kernel, a
user-space network stack. podnet already has one (`podnet net`: shim and
userspace stack) and `podnet pod` forces a program's traffic through a
proxy without root". And: "Not covered: more than one guest per lab, so
no client isolation test." The row of `pod-netlab` in `docs/design.md`
named `pod-netstack`; T-207 needs no user-space stack (call C148).

## Premise

- Read: issue 178; `crates/pod-netstack/README.md` (the stack and its
  `Admit` trait).
- Not measured: which knobs a user-space stack can hold (a policy, a
  broken family, DNS) and which need a kernel (NAT, PMTU, netem).

## Approach

A lab whose router is a `pod-netstack` stack with an `Admit` of the
policy, for the knobs of a policy alone, with the same expectations and
the same judge (the counters come from the stack); a refusal by name of
each knob that needs a kernel. A second guest namespace on the router's
inside link, and a probe of isolation between the guests.

## Prove

```sh
cargo test -p pod-netlab --all-features
```

# T-443: `pod-hostprobe`: a map from the findings to capabilities, and a cache across runs keyed by the kernel, the boot and the probe binary (GitHub #82)

**Source:** GitHub #82 (podswarm P-5).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

A podswarm worker runs the host probe at each start for its `caps` report
and for `podswarm doctor`. It needs one answer for each capability (for
example "FUSE mount in a user namespace: yes, by rows X and Y"), not raw
rows, and a second start on the same host must read a cache and start no
probe child, to keep the start of a worker in its time budget.

## Premise

- Read (2026-11-13): the comment of podswarm on GitHub #82 (asks P-5 and
  P-6); podswarm spec, file 21, sections 6.9 and 7 (ask P-5, milestone
  M1); file 22, section 4.
- In podcrates now: `run(&Settings, candidates)` gives `Findings`
  (`crates/pod-hostprobe/src/lib.rs`); `crates/pod-hostprobe/src/verdict.rs`
  has the line form; `assess(&Findings)` in
  `crates/pod-hostprobe/src/machine.rs` maps the findings for the machine
  rung only. No serde, no JSON, no schema, no cache.
- The JSON object of the findings with its schema
  (`schemas/hostprobe-report.v1.schema.json`, `report::schema()`) is
  T-107 (done): it is not written again here. This entry is the gap: the
  map, the cache, and the form `report(&Findings) -> serde_json::Value`
  of the ask (call C309).
- Not measured: the time of a full probe run on a worker.
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podswarm alone: podvm keeps its own cache of the
  report (T-485, call C241). It stays in M8 and waits until the podswarm
  session starts.

## Approach

After T-107: `capabilities(&Findings) -> Vec<Capability>` (a name, `yes`,
`no` or `unknown`, and the rows that decide it), in the JSON of T-107.
`CacheKey` (the `uname(2)` release, `/proc/sys/kernel/random/boot_id`, the
BLAKE3 of `/proc/self/exe`) and `run_cached(&Settings, candidates,
cache)`: the cache is a private file at a path of the caller, written
with `pod-core`'s private files. A file that does not parse is ignored
and written again.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
```

The tests: a second start with the same key reads the cache and starts
no probe child; a change of each part of the key runs the probes again; a
cache file that does not parse is written again; each capability names
its rows.

## Start condition

T-107 is done. The podswarm session starts, or a sibling asks for it (the
owner's ruling "podswarm", 2026-11-13).

# T-444: `pod-hostprobe`: the probe rows that `PROBES` lacks (user and PID namespace, cgroup2 delegation, FUSE in a user namespace, and more) (GitHub #82)

**Source:** GitHub #82 (podswarm P-6).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

A podswarm worker picks its containment rung, its view rung, its shield
and its spreading rung from the probe rows. Thirteen rows are absent.

## Premise

- Read (2026-11-13): the comment of podswarm on GitHub #82 (the table of
  13 rows and their order); podswarm spec, file 21, section 6.9 (ask P-6);
  files 06, 07, 12 and 13.
- In podcrates now: `PROBES` in `crates/pod-hostprobe/src/probes.rs` has
  67 rows (the ask counts 66), with `clone(CLONE_NEWUSER)`,
  `unshare(CLONE_NEWPID)`, `open(/dev/fuse, O_RDWR)`,
  `seccomp(NEW_LISTENER)`, `ptrace(PTRACE_TRACEME)` and
  `qemu-system-x86_64 --version`. The 13 rows of the ask are absent.
  `Outcome::skip` exists in `crates/pod-hostprobe/src/verdict.rs`.
- The order of podswarm: rows 1 to 4, 6 to 9, 12 and 13 for its M1; row 5
  (`PTRACE_SEIZE`) for its M3; rows 10 and 11 (qemu of each architecture,
  SNP and TDX) for its M8.
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podvm: the rows of the user and PID namespace,
  cgroup2 delegation and FUSE in a user namespace are the facts of the
  namespace rung and the FUSE rung of `pod-enter`, which podvm picks. It
  moved from M8 to M6 (item 4 of the work order). The rows that only
  podswarm reads (`PTRACE_SEIZE`, the qemu of each architecture, SNP and
  TDX) wait until the podswarm session starts.

## Approach

One row for each probe of the table, each in a probe child as the rows
of `PROBES` are, or a read of a file: `clone(CLONE_NEWUSER|CLONE_NEWPID)`;
the write open of `cgroup.subtree_control` of the own cgroup; the FUSE
mount in new user and mount namespaces; a notified `getppid` answered
with `SECCOMP_USER_NOTIF_FLAG_CONTINUE`; `PTRACE_SEIZE` of the probe's
own child; `memfd_secret(0)`; `ramfs` in a user namespace; the write open
of `memory.swap.max`; the swap state (`/proc/swaps`, zswap);
`qemu-system-ARCH --version` for each candidate; SNP and TDX from their
module parameters; `hidepid` from `/proc/self/mountinfo`; the count of
other root users from `/etc/passwd` and `/etc/group`. A row that cannot
run gives `Skip`, never "denied". The rows of the M1 of podswarm first.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
```

The tests: one test for each row with a fake result, and a real run of
each row on the CI kernel.

# T-445: `pod-hold`: the marker, the calls that fail with `ENOSYS`, and the reads of a path and an argv of a stopped task (GitHub #121)

**Source:** GitHub #121 (podswarm P-15).
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

podswarm uses one seccomp trap in three places: its view rung `trap`
(fill a file at its first open), the exec rung `seccomp` of spreading
(decide where a new program runs) and its network hold. The supervisor of
spreading must let its own stub `execve` pass with no notice, the `trap`
rung must make `io_uring_setup` fail, and the supervisor reads the path
and the argv of a stopped `execve`.

## Premise

- Read (2026-11-13): GitHub #121; podswarm spec, file 21, sections 6.5
  and 7 (ask P-15, milestones M2 and M3); file 06; file 07, sections 3.1
  and 3.2.
- In podcrates now: `crates/pod-hold/src/filter.rs` has `Calls::of`,
  `Calls::program` and `install` (it sets `no_new_privs` and installs on
  the calling thread); `crates/pod-hold/src/listen.rs` and
  `crates/pod-hold/src/reply.rs` take and answer the notices;
  `crates/pod-hold/src/mem.rs` has `Memory::{read, write}` and
  `read_word`. The single-thread child that installs and sends the
  descriptor needs no new call (the ask says so). The gap: the marker,
  the `ENOSYS` list, `read_cstr` and `read_argv`.
- The takeover itself is T-431 (`pod-proc`).
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podswarm alone: `podnet pod` holds `io_uring`
  already (`crates/pod-hold/src/filter.rs`) and needs no marker. It stays
  in M8 and waits until the podswarm session starts.

## Approach

`Calls::with_marker(nr, arg, value)`: no notice for `nr` when the low 32
bits of `args[arg]` (0 to 5) equal `value`. `Calls::enosys(&[nr])`:
`ENOSYS` and no notice. `Memory::read_cstr(addr, max)` (`max` counts the
NUL) and `Memory::read_argv(addr, max)` (a NULL pointer ends it; `max`
counts all bytes); a string over `max` and an address outside the task
are refused by name. A fuzz target runs `read_argv` over
`Memory::of_fd`.

## Prove

```sh
cargo test -p pod-hold --all-features
```

The tests: an `execve` with the marker value runs with no notice and with
another value notifies; a call of the `enosys` list fails with `ENOSYS`
and gives no notice; a single-thread child installs and sends the
descriptor, and a `Listener` of the parent reads its notices;
`read_cstr` and `read_argv` read the path and argv of a stopped `execve`.
The tests of `podnet pod` and podnet's adoption trial pass.

## Start condition

The podswarm session starts, or a sibling asks for it (the owner's ruling
"podswarm", 2026-11-13).

# T-446: `pod-hold`: a plan that the child applies before `execve` (`PreExec` and `Program::with_plan`) (GitHub #121)

**Source:** GitHub #121 (podswarm P-14).
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

The hold and the keeper of podswarm must share one fork: the held child
applies the keeper's plan (signals, session, umask, namespaces, limits,
cwd, descriptors, Landlock) after the fork and before `execve`. Without
it podswarm runs a shim process.

## Premise

- Read (2026-11-13): GitHub #121; podswarm spec, file 21, sections 6.5
  and 7 (ask P-14, milestone M7); file 14.
- In podcrates now: `crates/pod-hold/src/spawn.rs` has `Program::{new,
  with_env}` and `start(program, &calls)`: the child calls only `execve`.
  `pod-confine` compiles its rules before the fork and applies them in the
  child with no allocation (`docs/design.md`, section 4).
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podswarm alone (one fork for its hold and its
  keeper). It stays in M8 and waits until the podswarm session starts.

## Approach

`PreExec` (made before the fork, applied with no allocation): a reset of
the signal mask and of each handler, `setsid`, the umask, the namespaces
to enter in order, the resource limits, the cwd, the descriptors to move
(then each other descriptor from 3 up is closed), and
`pod_confine::apply` as the last step. `Program::with_plan(plan)`. A step
that fails writes the step and the errno to a close-on-exec pipe and the
child exits 127 with no exec; `start` returns a `Failure` that names the
step.

## Prove

```sh
cargo test -p pod-hold --all-features
```

The tests: a held child of `with_plan` sees each step before `execve` (a
limit, the cwd, the descriptor numbers, a namespace by its inode, a
Landlock rule); a failed step is named and no exec runs. The tests of
`podnet pod` and podnet's adoption trial pass.

## Start condition

The podswarm session starts, or a sibling asks for it (the owner's ruling
"podswarm", 2026-11-13).

# T-447: `pod-guest`: a Linux flavour of the plan (kernel and initramfs, one virtio-serial socket, one raw disk, mem-lock) and an SNP variant (GitHub #122)

**Source:** GitHub #122 (podswarm P-18).
**Category:** feature
**Milestone:** M7
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

On the shield `guest` of podswarm, no file name, file content, argv value
or environment value of a run appears in a host file or in the argv of a
host process. The guest boots a kernel and an initramfs directly, and
its only channel is one `chardev` Unix socket. The shield `sealed` boots
the same plan as an SEV-SNP guest.

## Premise

- Read (2026-11-13): GitHub #122; podswarm spec, file 21, sections 6.9
  and 7 (ask P-18, milestone M8); file 13, sections 7.2 and 8.3.
- In podcrates now: `plan::argv` in `crates/pod-guest/src/plan.rs` gives
  the UEFI flavour (OVMF in `pflash`, the root disk and a mailbox disk,
  `-nic none`, QMP, `Plan::emu_args`); `crates/pod-guest/src/dos.rs` has
  the DOS flavour. The mailbox disk is a host file. No direct kernel
  boot, no virtio-serial, no SNP.
- The boot of this plan by podvm is podswarm's ask V-7 (GitHub #124), for
  the podvm session. The check of the report is T-436.
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podvm: the direct boot of a Linux kernel and
  initramfs is the base of `pod-microvm` (podvm's M7, PodUtils/podvm#168).
  It moved from M8 to M7 (item 10 of the work order, with T-062). The SNP
  variant waits until the podswarm session starts.

## Approach

A module `linux` with `linux::Plan` and `linux::argv` (file 21, section
6.9): `-kernel`, `-initrd`, the fixed `-append` of the guest set, one
`virtserialport` on a `chardev` socket and one raw virtio-blk scratch
disk in the run directory (mode 0700), the QMP socket there,
`-overcommit mem-lock=on` on request, `-nic none` and `emu_args` as the
UEFI flavour has them. `Snp { cbitpos, ovmf }` adds
`confidential-guest-support` and `sev-snp-guest` with
`policy=0x30000`, `reduced-phys-bits=1` and `kernel-hashes=on`. No 9p,
virtiofs or `-netdev`.

## Prove

```sh
cargo test -p pod-guest --all-features
```

The tests: golden argv with and without `mem_lock` and of the SNP
variant; exactly one `virtserialport` and one raw drive; each path is in
the allowed set; a canary of the run is absent from the argv; no
`-fsdev`, `-virtfs`, `virtiofs` or `-netdev`; the UEFI and DOS golden
tests do not change.

## Start condition

It goes with T-062 (the studies of the widened scope of podvm). The SNP
variant waits until the podswarm session starts.

# T-448: `pod-enter`: the text of `fuse.rs` says that `mount(2)` of FUSE fails in a user namespace; it works since Linux 4.18 (GitHub #123)

**Source:** GitHub #123 (podswarm P-20).
**Category:** docs
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

A wrong fact in a crate document: a reader can skip a FUSE rung that
works. Since Linux 4.18 FUSE has `FS_USERNS_MOUNT`: a process with
`CAP_SYS_ADMIN` in a user namespace that owns its mount namespace can
mount it when `/dev/fuse` opens.

## Premise

- Read (2026-11-13): GitHub #123; podswarm spec, file 06, section 3 (the
  rung `fuse-userns`); file 21, section 7 (ask P-20).
- `crates/pod-enter/src/fuse.rs:24` says "namespace, and so is": the
  module text says that `mknod` of the node and `mount(2)` are both
  `EPERM` in a user namespace. The part about `mknod` is right; the part
  about `mount(2)` is wrong.
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podvm: a wrong fact in a document of `pod-enter`,
  a crate of podvm, can make a reader skip a rung that works. It moved
  from M8 to M6 (item 4 of the work order).

## Approach

Change the sentence to name the condition: `mknod` of the node is `EPERM`
in a user namespace; `mount(2)` of FUSE is allowed in a user namespace
that owns the mount namespace since Linux 4.18, when `/dev/fuse` opens.
Keep the rest of the paragraph. The row that shows the fact on a CI
kernel is T-444.

## Prove

```sh
cargo test -p pod-enter --all-features
cargo todo remap crates/pod-enter/src/fuse.rs
```

# T-485: `pod-hostprobe`: a reader of the report beside its writer, so that `podvm run` reads its probe cache (PodUtils/podvm#291)

**Source:** PodUtils/podvm#291 (COORD-069 of podvm; PodUtils/podvm@T-1746, the ask V-2 of podswarm, #124).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`podvm run` calls `pod_hostprobe::run` each time. podvm's cache holds the
JSON document of `report::document`, and only `podvm probe --cached`
reads it back as text: `Findings` has no reader (`rows` holds the names
of `probes::PROBES` as `&'static str`, and no type of the report derives
a deserializer). A second serializer in podvm would break podvm's rule of
one writer. PodUtils/podvm@T-1746 is blocked on this.

## Premise

- Read: PodUtils/podvm#291 (2026-11-13); `crates/pod-hostprobe/src/`
  (`Findings`, `Selection`, `probes::PROBES`, `verdict::encode` and
  `decode` of one row); podvm's writer `report::document` is podvm's own
  code (the file crates/podvm-probe/src/report/document.rs of podvm at `2edace9`,
  317 lines): it writes the findings and the selection with podvm's own
  fields (its version, its dispatch, the interpreter that answered).
  `pod-hostprobe` has no document of its own: that is T-107.
- Not measured: none here.

## Approach

The crate owns the document of its own types (call C241): `report::write(
&Findings, &Selection) -> String` (a versioned JSON object) and
`report::read(document: &str) -> Result<(Findings, Selection), ReadError>`,
each row name mapped to its `probes::PROBES` entry; `read` refuses a
document of an unknown version and a row that `PROBES` does not name. A
round-trip test: `write`, then `read`, then `write` gives the same text.
podvm's `report::document` embeds this object under one key beside its
own fields, and its cache reads that key back with `report::read`; so
podvm keeps one writer. This is the first part of T-107; its published
schema follows on the same object.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
```

## Done

2026-11-13, commit `5a2b5fd`. `pod-hostprobe` has the module `report`
(feature `report`, off by default; the serde and serde_json of the
workspace). `report::write(&Findings, &Selection) -> String` gives one
line of JSON, and `report::read(&str) -> Result<(Findings, Selection),
ReadError>` gives the two back. The object (calls C242 and C243; the
README, "The report", has each field):

```text
{"schema":1,
 "findings":{"rows":[{"name","verdict","errno","reason"}],
             "identity":{"uid","gid","groups","uid_map","gid_map","setgroups",
                         "cap_eff","cap_prm","cap_bnd","seccomp","seccomp_filters",
                         "no_new_privs","threads","unreadable"},
             "writable":[{"path","source","verdict","errno","reason",
                          "space":{"blocks_free","block_size","inodes_free"},
                          "space_errno"}],
             "self_exe"},
 "selection":{"rung","rejected":[{"rung","evidence"}],
              "controls_answered","control_notes"}}
```

Each row name of `read` is the `&'static str` of its entry of
`probes::PROBES`. `read` refuses with a `ReadError`: `Form` (not a JSON
object, a missing or an unknown field), `Version` (before the form),
`UnknownRow`, `DuplicateRow`, `Word` (a verdict or a rung),
`DenialWithoutErrno`. The new `Rung::requirement` gives the requirement
of a rejected rung, and `Selection::choose` uses it.

`cargo test -p pod-hostprobe --all-features`: 124 unit tests pass, 13 of
them in `report` (the round trip `write`, `read`, `write` on a fake set and
on the findings of this host under libtest, and each refusal);
`tests/children.rs` reads back the report of the whole set of this host
(66 rows, 7301 bytes) to the same text. The plant
`hostprobe-report-reads-an-unknown-row` (a `read` that keeps the name of
an unknown row) fails `a_row_that_probes_does_not_name_is_refused`. The
static probe turns on `report`. podvm embeds the object under one key of
its `report::document` and reads that key back (PodUtils/podvm@T-1746).
The answer on PodUtils/podvm#291 goes with the tag that holds `report`.

# T-486: `pod-enter`: an overlay root in the namespace rung, the image read-only under a tmpfs upper layer (PodUtils/podvm#294)

**Source:** PodUtils/podvm#294 (COORD-071 of podvm; PodUtils/podvm@T-1749, the ask V-5 of podswarm, #124).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

Each `podvm run` fills a new tree from the extracted image before the
namespace rung of `pod-enter` enters it. On a host with no shared blocks
the fill is a copy: podvm measured 681 ms of 2861 ms for `ubuntu:24.04`
and 8377 ms of 12611 ms for `golang:1.24.7-bookworm` (experiment 442,
the median of 5 runs). `run_namespace` takes a `RootDir` and has no form
of a root that leaves a shared lower directory unchanged.

## Premise

- Read: PodUtils/podvm#294 (2026-11-13); `crates/pod-enter/src/entry.rs`
  (`run_namespace`), `crates/pod-enter/src/plan.rs` (`Plan`).
- Not measured: the time of an overlay mount in a user namespace here;
  whether the hosts of CI allow it (Linux 5.11 and later mount overlayfs
  in a user namespace).

## Approach

A form of the root of the namespace rung, for example
`Root::Tree(RootDir) | Root::Overlay { lower, tmpfs_size }`: in the user
and mount namespace of the run, a private tmpfs holds the upper and work
directories, and overlayfs mounts the caller's lower directory under it
as the root; each write of the payload goes to the tmpfs and goes away
with the run. Where the mount is refused, the entry refuses with a typed
error that names `overlay`, before the payload starts; it never runs the
payload on the lower directory. The tmpfs `/tmp` of the rung and the
confine layer stay as they are. Tests: a payload that writes and removes
files under `/` leaves the lower directory with the same digest; a
refused mount gives the typed error; a plant that mounts the lower
directory writable must fail the first test.

## Prove

```sh
cargo test -p pod-enter --all-features
```

## Done

2026-11-14, commit `4ff6441`. `pod-enter` has the overlay root of the
namespace rung (calls C305, C306, C307): `Root::{Tree(&RootDir),
Overlay { lower, upper }}`, `Upper::{Tmpfs { size }, Dir { upper, work }}`,
`spawn_namespace_root`, `run_namespace_root`, `spawn_joined_root`
(`spawn_namespace` and `spawn_joined` stay for a tree). The child sets its
working directory to the held lower descriptor before the unshare,
mounts a tmpfs (`size=N,mode=0700`) on the root path, makes `upper` (with
the mode and owner of the lower root) and `work` in it, and mounts
overlayfs on the root path with `lowerdir=.` and the escaped upper and
work paths (`overlay_options`, `overlay_escape`). The rung then mounts its
tmpfs on `/tmp`, its proc and its bind of `/dev/pts` in the overlay, and
the landing check asks for `OVERLAYFS_SUPER_MAGIC` on a device that is
not the device of the lower directory. Each refusal is
`Error::Overlay { step, errno }` (`the overlay root was refused at STEP:
ERRNO; the payload did not run`, `Class::Unsupported`); there is no
fallback to chroot. `pod-hostprobe::sys` has `mount_data`,
`Statfs::f_type`, `OVERLAYFS_SUPER_MAGIC` and `PROC_SUPER_MAGIC`. A
finding fixed in place: a descriptor opened in the old mount namespace
does not mount as a lower layer in the new one (`overlayfs: failed to
clone lowerpath`), so the working directory moves before the unshare.
`userland_workdir` moved to `userland.rs` to keep `launch.rs` under 500
lines.

The proof on this host (Linux 6.18, ext4, as root; an overlay mount in a
user namespace, `unshare -Urm`, works here too): `tests/confine.rs` runs
16 tests with no skip. The new ones: a payload that writes a file,
changes and removes files of the image and makes a directory under `/`
(the path of the image holds `,` and `:`) sees each change, the image
keeps its digest (mode, owner, size, link target and bytes of each path),
and a second run sees the image as it was; a lower directory under the
most overlays that the kernel stacks gives `Error::Overlay` at `mounting
overlay on the root: EINVAL`, and the payload does not run; a joined
entry sees the writes of the running payload, and with the pidfd of a
process that ended enters an overlay root of its own. podvm's acceptance
test (PodUtils/podvm#323) is `tests/overlay_root.rs` (call C307: the
adapter calls `run_namespace_root` with `Upper::Dir`, an inner
`#![cfg]`, `vec!`). It passes with the static busybox of Ubuntu's
`busybox-static`, and skips with a line `SKIP:` with no static busybox or
where the rung refuses. 5 unit tests (the escape, the options, the step
names, the refusal, a relative upper directory) and a test of
`Statfs::f_type`. `scripts/ci/root-enter-confine-tests.sh` runs both
binaries as root and fails on a skip line; `ci.yml` and `plants.yml`
install `busybox-static`. The plants `enter-overlay-lower-as-root` (a
bind of the lower directory in place of the overlay),
`enter-overlay-falls-back` (chroot on the lower directory at a refusal)
and `enter-overlay-join-as-tree` (the join checks the landing of a tree)
each fail their test; `enter-ns-join-skipped` follows the new form of
`NsEntry::Join`.

# T-491: `pod-enter`: a fresh proc on the `/proc` of the image in the namespace rung (PodUtils/podvm#296)

**Source:** PodUtils/podvm#296 (COORD-073 of podvm; PodUtils/podvm@T-1758).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The namespace rung of `pod-enter` unshares the mount namespace (step 7),
makes `/` recursively private (step 8) and mounts a tmpfs on the `/tmp`
of the image (step 9). It mounts nothing on `/proc`. A payload on that
rung has no `/proc`: `go version` fails on `readlink /proc/self/exe`,
`head /proc/self/status` fails, and `mount` finds no `/proc/mounts`
(podvm's rows `imgs.go-version.ns`, `tools.go-mod.ns` and `run.proc.ns`).

## Premise

- Read: PodUtils/podvm#296 (2026-11-13); `crates/pod-enter/src/entry.rs`
  (`ns_tmp_target`, `ns_setup_step`), `crates/pod-enter/src/launch.rs`
  (the namespace block).
- podvm measured on its cloud host, as root: a new proc in a new mount
  namespace with no PID namespace mounts; in a user namespace with no
  PID namespace it fails with EPERM. Not measured here.

## Approach

After step 9, a step 11 mounts a fresh `proc` with
`MS_NOSUID|MS_NODEV|MS_NOEXEC` on the `/proc` of the image. The target
follows the rule of `ns_tmp_target`: a directory, not a symbolic link;
with no such directory the step skips. A failed mount does not stop the
rung: the entry writes one line that names the step and the errno, and
continues. `ns_setup_step(11)` names the step. No PID namespace (that
changes the signals and the reaping of the payload; a separate
decision). Optional, in the same block: a tmpfs of 64 MiB, mode 1777, on
the `/dev/shm` of the image, with the same rules. Tests: the target rule
(a directory, a file, a link, no directory); the name of the step; a
plant that mounts `proc` without `MS_NOEXEC` or stops the rung on a
refused mount must fail a test.

## Prove

```sh
cargo test -p pod-enter --all-features
```

## Done

2026-11-13, commit `dcf2fb4`. After the tmpfs on `/tmp` (step 9), the child
of the namespace rung mounts a fresh `proc` with
`MS_NOSUID|MS_NODEV|MS_NOEXEC` on the `/proc` of the image
(`ns_proc_target`: a directory, not a link; with none the step skips).
A refused mount does not stop the rung: the child writes
`PRODUCT: the namespace rung runs with no /proc: mounting proc on /proc
failed with errno N` to the payload's standard error (`ns_proc_refused`,
made before the fork; the errno in decimal with no allocation), and the
payload runs as before. `ns_setup_step(NS_PROC_STEP)` (15) names the
step. No PID namespace. `pod-hostprobe::sys` has `MS_NOSUID`,
`MS_NODEV` and `MS_NOEXEC`. Tests: the target rule; the line and the
decimal; the confine binary runs a reader payload as root: on the
namespace rung `/proc/self/status` opens and the `/proc` of its
`mountinfo` has `nosuid`, `nodev` and `noexec`, no mount reaches the
host, and on the chroot sequence the open fails with ENOENT. The plant
`enter-ns-proc-exec` (no `MS_NOEXEC`) fails the test; a check by hand
with no proc mount fails it too. The tmpfs on `/dev/shm` (the optional
part) is not done: podvm makes the directory itself (its PodUtils/podvm@T-1759) and
does not need it.

# T-492: `pod-enter`: an entry that joins the mount namespace of a running payload, for exec (PodUtils/podvm#297)

**Source:** PodUtils/podvm#297 (COORD-074 of podvm; PodUtils/podvm@T-1765).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

`podvm exec NAME CMD` on a container of the namespace rung enters the
root through `spawn_selected`, as `run` does. The child makes a new
mount namespace with a new tmpfs on `/tmp` (steps 7 to 9) and, with
T-491, a new proc. So `exec` does not see the files that the payload of
`run` wrote to `/tmp`; docker's `exec` sees them (podvm's row
`life.exec-sees-run-fs.ns`).

## Premise

- Read: PodUtils/podvm#297 (2026-11-13); `crates/pod-enter/src/launch.rs`
  (the namespace block), `crates/pod-enter/src/entry.rs`
  (`spawn_namespace`).
- `setns(2)` takes a pidfd since Linux 5.8. `pod-supervise` holds the
  pidfd of the payload, and podvm passes it from its record.
- Not measured: `setns` with a pidfd and `CLONE_NEWNS` on the hosts of
  CI.

## Approach

`spawn_joined(root, plan, pidfd, err)`: in the child, before the chroot
sequence, `setns(pidfd, CLONE_NEWNS)` in place of steps 7 to 9 and of
the proc mount; the entry mounts nothing. Then the chroot by path and
the device and inode check of the namespace rung (step 10). A refused
`setns` (an old kernel, EPERM, a payload that ended) is a setup step
with its own byte and name in `ns_setup_step`: the entry falls back to
`spawn_namespace` and writes one line that names the step. Tests, as
root in the confine binary: a payload of `run` on the namespace rung
writes a file in `/tmp` and waits; a payload of `spawn_joined` with the
pidfd of the first reads that file; a pidfd of a process that ended
gives the fallback line. A plant that skips the `setns` must fail the
read test.

## Prove

```sh
cargo test -p pod-enter --all-features
```

## Done

2026-11-13, commit `afef6c8`. `spawn_joined(root, plan, pidfd, err)`: the
child calls `setns(pidfd, CLONE_NEWNS)` before the passed descriptors
(a pass onto the number of the pidfd cannot replace it), mounts nothing,
then does the chroot by path and the device and inode check of the
namespace rung. A refused join (step byte `NS_JOIN_STEP`, 16, named
`setns(pidfd, CLONE_NEWNS)`) writes `ns_join_fallback_line` and enters
`spawn_namespace`. `pod-hostprobe::sys::setns`. Tests: the step name and
the line; the confine binary as root: a payload of `spawn_namespace`
writes `/tmp/shared` and waits, a reader of `spawn_joined` with its
pidfd opens the file; after the first ends, the same pidfd gives
`ESRCH (3)` on the fallback line, and the reader in its own namespace
gets ENOENT. The plant `enter-ns-join-skipped` fails the test.

# T-503: `pod-netlab`: three fixes of the knob flags: an empty `--udp`, the range of a number, the words of a flag (PodUtils/podvm#298)

**Source:** PodUtils/podvm#298 (COORD-075 of podvm; PodUtils/podvm@T-1770).
**Category:** defect
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podvm's QA pass found three faults in `crates/pod-netlab/src/knobs/flags.rs`:
(1) `Knobs::flags` writes `--udp` (or `--tcp`) with an empty list, so the
line does not parse back after a shell splits it; (2) a number above the
range of its type gives "the value is not a whole number"; (3) a bad word
gives "the value is not a word of this flag" and does not list the words.

## Premise

- Read: PodUtils/podvm#298 (2026-11-13); `crates/pod-netlab/src/knobs/flags.rs`,
  `knobs.rs`, `knobs/check.rs`, `scenario.rs`.

## Approach

(1) `flags` writes no `--tcp` or `--udp` for an empty list, and
`parse_flags` reads a missing list as empty; a test holds
`Knobs::parse_flags(&k.flags()) == k` for each scenario. (2) `number()`
names the range on an overflow (`the value is above 65535`), and `check`
names its own range where it is smaller. (3) `word()` lists the words of
the flag (`the value is not one of: none, src, dst, both`);
`KnobError::BadFlag` carries the text as a `String`. A plant for (1).

## Prove

```sh
cargo test -p pod-netlab --all-features
```

## Done

2026-11-13, commit `f3d362a`. (1) `Knobs::flags` writes no empty list:
`--tcp` or `--udp` only where it has ports, and `--tcp none` where both
are empty; `none` and a missing list read as empty. The test of the
scenarios also joins the flags with spaces, splits them as a shell does,
and reads them back, for each scenario and for the four shapes of the
two lists. (2) `number()` takes the range of the knob: digits past the
type give `the value is out of its range 1280 to 1499` (`--mtu 99999`),
a port past `u16` gives `1 to 65535`; a value with a sign or a letter
stays `not a whole number`. (3) `word()` lists the words of the flag
(`the value is not one of: none, nat44, nat66, both`); each enum of a
flag has `WORDS`, and `Policy::WORDS`. `KnobError::BadFlag` carries
`why: String` (an API change). `cargo test -p pod-netlab --all-features`:
each passes, with the new `a_refused_value_names_the_range_or_the_words`.
The plant `netlab-empty-port-list-written` fails the test of the
scenarios.

# T-504: `pod-enter`: a bind of the host's `/dev/pts` on the `/dev/pts` of the image in the namespace rung (PodUtils/podvm#296)

**Source:** PodUtils/podvm#296 (COORD-073 of podvm, its second half, 2026-11-13 06:05; PodUtils/podvm@T-1760).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm gives the payload of `run -t` a pty of the host's `/dev/ptmx`. On
the namespace rung `test -t 1` passes, but `tty` prints `not a tty`:
`ttyname` reads `/proc/self/fd/0` (`/dev/pts/N`) and stats that path in
the root, and the image has no `/dev/pts` (podvm's row
`run.tty-term.ns`).

## Premise

- Read: PodUtils/podvm#296 and PodUtils/podvm#299 (2026-11-13);
  `crates/pod-enter/src/launch.rs` (the namespace block, 499 lines).

## Approach

After the proc mount, a bind (`MS_BIND|MS_REC`) of the host's `/dev/pts`
on the `/dev/pts` of the image, where that target is a directory and not
a link; a refusal writes one line and the rung goes on, as T-491 does
for proc. The mounts that do not stop the rung move to a module of
their own, so `launch.rs` stays under 500 lines.

## Prove

```sh
cargo test -p pod-enter --all-features
```

## Done

2026-11-13, commit `42306a3`. The module `ns_optional` holds the mounts
that do not stop the rung: the fresh proc (T-491) and the bind of
`/dev/pts` (`ns_pts_target`, step `NS_PTS_STEP` 17, named
`binding /dev/pts on /dev/pts`). Each is built before the fork with its
line of refusal (`ns_refused_line`), and the child makes them in order
with `mount_each`, after the tmpfs on `/tmp`. `launch.rs` has 487 lines.
`pod-hostprobe::sys::MS_BIND`. Tests: the target rule and the line; the
confine binary as root: on the namespace rung the payload's `mountinfo`
has `/dev/pts`, `/dev/pts/ptmx` opens, and no bind reaches the host; on
the chroot sequence the open fails with ENOENT. The plant
`enter-ns-pts-unbound` fails the test; the plant `enter-ns-proc-exec`
moves to the new module and still fails its test.

# T-544: `pod-hold`: a seccomp rule that refuses a raw socket and an ICMP socket, with an opt-in (PodUtils/podnet#538)

**Source:** PodUtils/podnet#538 (COORD-073 of podnet; PodUtils/podnet@T-334)
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podnet's message: podnet added the `icmp` carrier of `pod-link`
(PodUtils/podnet@T-282): a raw `AF_INET`/`SOCK_RAW`/`IPPROTO_ICMP`
socket, behind `--allow-raw icmp`. The seccomp backend of `pod-hold`
(podnet's `pod --backend seccomp`) passes a
`socket(AF_INET, SOCK_RAW, IPPROTO_ICMP)` to the kernel unchanged: no rule
denies it and no rule emulates it. A program in a pod with `CAP_NET_RAW`
thus sends ICMP straight out of the host, past the upstream of the pod.

The ask: a rule for `AF_INET`/`AF_INET6` with `SOCK_RAW` (and
`IPPROTO_ICMP`/`IPPROTO_ICMPV6`): deny it with `EPERM` by default, with an
opt-in for a caller that gives it (podnet maps `--allow-raw icmp` of the
pod to it). A test: a raw ICMP socket in a held child gets `EPERM`, and
with the opt-in it opens. podnet's crates/podnet-netns/src/rules.rs is
frozen for the take, so podnet does not add the rule itself.

## Premise

- Read: `crates/pod-hold/src/filter.rs`. `Calls::sockets()` stops
  `socket` with `SECCOMP_RET_USER_NOTIF`, and the consumer's handler
  answers it. The crate has no rule on the arguments of `socket`.
- Read: the kernel reads the type of `socket` as an `int` and masks
  `SOCK_TYPE_MASK` (`0xf`) out of the flags `SOCK_CLOEXEC` and
  `SOCK_NONBLOCK`; a filter must mask the same.
- Read: a ping socket (`SOCK_DGRAM` with `IPPROTO_ICMP` or
  `IPPROTO_ICMPV6`) also sends ICMP out of the host, with no
  `CAP_NET_RAW`, where `net.ipv4.ping_group_range` lets the group in.
- Measured on this host: a held child under the seccomp listener runs (no
  `SKIP:` line), and as root the kernel opens a raw ICMP socket.

## Approach

1. A rule in the BPF program, before the list: `socket(AF_INET or
   AF_INET6, type & SOCK_TYPE_MASK == SOCK_RAW, any protocol)` and
   `socket(AF_INET or AF_INET6, SOCK_DGRAM, IPPROTO_ICMP or
   IPPROTO_ICMPV6)` return `SECCOMP_RET_ERRNO | EPERM` (C301). The kernel
   answers, so no notification reaches the supervisor.
2. `Calls::sockets()` has the rule; `Calls::of` has none (a list of the
   caller); `Calls::allow_raw()` is the opt-in: the call then goes on as
   each other `socket`, to the supervisor.
3. Tests: the BPF run of the rule (each family, type with flags, protocol,
   the high word of an argument, the other calls); a held child with a raw
   ICMP socket: `EPERM` and no notification by default; with the opt-in,
   the supervisor sees the call and proceeds with it, and the answer is the
   kernel's.

## Prove

```sh
cargo test -p pod-hold --all-features
sh scripts/plant-each.sh hold-t544-raw-not-denied hold-t544-type-flags-kept
sh scripts/agent/check-windows.sh
sh scripts/agent/check-macos.sh
```

## Done

2026-11-14, commit 456bcd8. `Calls::sockets()` refuses in the kernel,
with `EPERM`, `socket(AF_INET or AF_INET6, SOCK_RAW with any flag, any
protocol)` and a ping socket (`SOCK_DGRAM` with `IPPROTO_ICMP` or
`IPPROTO_ICMPV6`); `Calls::allow_raw()` is the opt-in (C301), and
`Calls::denies_raw()` says which. podnet maps `--allow-raw icmp` to
`Calls::sockets().allow_raw()`.

`cargo test -p pod-hold --all-features`: each test passed. New:
`filter::tests::a_raw_socket_and_an_icmp_socket_get_eperm_unless_allowed`
(a BPF run of each family, type with flags, protocol, a high word of
garbage, and the calls after the rule) and, in `tests/hold.rs`,
`a_raw_icmp_socket_gets_eperm_unless_the_hold_allows_it`: a held child
gets `EPERM` and the supervisor gets no notification; with the opt-in
the supervisor sees the `socket` call with `SOCK_RAW`, proceeds with it,
and the kernel of this host (root) opened the socket. The notification
tells the filter's answer from the kernel's. Plants, each caught by hand:
`hold-t544-raw-not-denied` (the held test) and
`hold-t544-type-flags-kept` (the BPF test). `check-windows.sh` and
`check-macos.sh`: 0 faults.

# T-568: `pod-enter`: a reason of `abi` carries its own verdict label, and a static object reads "names  in DT_NEEDED" (PodUtils/podvm#331)

**Source:** PodUtils/podvm#331 (COORD-091 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The reason of `Verdict::Refused(String)` is a sentence, and each printer
adds its own label: podvm's `system abi` prints `refused: {why}`, and its
interposer tier prints `podvm {verb}: interpose: declined for {argv0:?}:
{why}`. Six reasons of `crates/pod-enter/src/abi.rs` and
`crates/pod-enter/src/abi/closure.rs` hold a label of their own
(`. refused: ...`, `. note: Read from .dynsym: ...`), so the output says
"refused" two times, or "declined" and then "refused". podvm measured:

```
refused: /workspace/bin/podvm names  in DT_NEEDED, so it was built against unknown, and this rootfs carries glibc (/usr/lib/libc.so.6). refused: A preloaded object is loaded by the payload's OWN loader and resolves its imports against the payload's libc, so this one cannot serve it
```

A static object (no `DT_NEEDED`) gives `names  in DT_NEEDED` (the join of
an empty list) and "built against unknown", which sends the reader to look
for a libc and not at the static link.

## Premise

- Read: `crates/pod-enter/src/abi.rs` (`admits`, `pair_gates`),
  `crates/pod-enter/src/abi/closure.rs` (`admits_set`, `link_set`),
  `crates/pod-enter/src/abi/elf.rs` and `crates/pod-enter/src/abi/image.rs`
  (the reasons of `Unreadable`).
- Not measured here: podvm's printers. The report quotes them.

## Approach

Remove each label inside a reason and keep each sentence whole. A static
object gets its own refusal (`static_gate`), in `admits` after the
architecture and in `admits_set` before the C library of the set. Needs
that name no C library say so, not "built against unknown". Tests: no
reason of each path holds `refused:`, `note:` or `declined:`, or a gap of
two spaces; the static case names the static link. Two plants.

## Prove

```sh
cargo test -p pod-enter --all-features --lib abi::
sh scripts/plant-each.sh enter-abi-reason-carries-a-label enter-abi-static-object-has-no-reason
```

## Done

2026-11-14. The commit that names T-568.

- The six reasons of the report lose their label; each sentence stays
  whole (`. This is a build host ...`, `. The check reads .dynsym, because
  ...`, `. An object that cannot be checked ...`, `. A preloaded object
  ...`, `. The loader checks the version ...`). The two reasons of
  `Unreadable` (`elf.rs`, `image.rs`) lose their `refused:` too (C335).
- `static_gate`: an object with no `DT_NEEDED` is refused with "names no
  library in DT_NEEDED, so it is linked statically. No C library serves
  it, and a preloaded object cannot serve it either: a static program has
  no loader that loads one". `admits` runs it after the architecture,
  `admits_set` before the C library of the set. Needs that name no C
  library read "and no known C library is among them", not "built against
  unknown".
- Tests (`abi::tests::labels`): `a_static_object_is_refused_as_static`,
  `no_refusal_of_admits_carries_a_label` (the architecture, an unknown
  libc, the family, needs with no libc, the version, the symbol),
  `no_reason_of_an_unreadable_object_carries_a_label`; in
  `abi::closure::tests`: `a_static_object_is_refused_as_static_against_a_link_set`,
  and the version and definition refusals of `admits_set` check the labels.
  `cargo test -p pod-enter --all-features`: 181 passed in the library, and
  each other target passed.
- Plants: `enter-abi-reason-carries-a-label` (`refused:` back in the
  family refusal) fails `no_refusal_of_admits_carries_a_label`;
  `enter-abi-static-object-has-no-reason` (no static gate) fails
  `a_static_object_is_refused_as_static`. Each was applied and run by hand.

# T-569: `pod-hostprobe`: `probe_dir_exec` names the filesystem on each `EACCES`, also on a mount that is not noexec (PodUtils/podvm#335)

**Source:** PodUtils/podvm#335 (COORD-092 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`probe_dir_exec` of `crates/pod-hostprobe/src/exec.rs` gives the remedy
"move the directory off this filesystem, for example with $PODVM_STORE"
on each `ExecVerdict::Denied`. It reads no flag of the mount, and the
module doc says "`EACCES` from that run is the `noexec` answer". `EACCES`
from `execve` has more causes: a policy on the path or on the file (an
LSM, a seccomp filter, a sandbox). podvm measured a host with no `noexec`
mount (`grep -c noexec /proc/self/mounts` is 0), where one directory
refuses the run and three directories on the same device execute:

```
store_exec: no: /state/home/.local/share/podvm: a created file does not execute (/state/home/.local/share/podvm/.pod-hostprobe-exec.29370.0: execve refused with EACCES (Permission denied (os error 13))); move the directory off this filesystem, for example with $PODVM_STORE
```

The remedy cannot work there. podvm prints the detail as it is in
`podvm doctor store_exec` and in each verb that enters the store.

## Premise

- Read: `crates/pod-hostprobe/src/exec.rs`, `crates/pod-hostprobe/src/sys/proc.rs`
  (`statfs`, `Statfs::flag`, `ST_NOEXEC`, `ST_VALID`).
- Not measured here: a host with a policy that refuses one path. The
  report gives the measurement.

## Approach

On `Denied`, read the `noexec` flag of the mount of the directory with
`sys::statfs` and `Statfs::flag(ST_NOEXEC)`. `denial_cause(flag,
setting)` maps it to the sentence: `noexec` names the mount and the move
off the filesystem; not `noexec` names a policy on the path or on the
file and what to check, with no move; a flag that cannot be read (`None`:
an error, or no `ST_VALID`) names both causes. Fix the doc of the module
and of `try_exec`. Tests: each branch of `denial_cause`, the flag against
`/proc/self/mounts`, and a real `noexec` tmpfs as root. A plant that
always names the filesystem.

## Prove

```sh
cargo test -p pod-hostprobe --all-features --lib exec::
sh scripts/plant-each.sh hostprobe-exec-always-names-the-filesystem
```

## Done

2026-11-14. The commit that names T-569.

- `exec::mount_noexec(dir)` reads the flag (`None` where `statfs` fails or
  the kernel did not fill `f_flags`). `exec::denial_cause` gives: for
  `noexec`, "the mount of the directory is noexec; move the directory off
  this filesystem, for example with SETTING"; for a mount that is not
  `noexec`, "the mount of the directory is not noexec, so a policy on the
  path or on the file refuses the run (an LSM such as SELinux, AppArmor or
  Landlock, a seccomp filter, or a sandbox); check the policy of this path
  and the audit log of the kernel"; for no flag, both causes and the move
  only "if the mount is noexec" (C336). The doc of the module and of
  `try_exec` no longer says that `EACCES` is the `noexec` answer.
- Tests: `the_flag_of_the_mount_decides_the_remedy` (each branch; no
  "move" and no setting where the mount is not `noexec`),
  `the_noexec_flag_agrees_with_the_mount_table`, and
  `a_noexec_mount_is_named_by_the_probe` (root: a `noexec` tmpfs on the
  scratch directory; it ran here as root and did not skip).
  `cargo test -p pod-hostprobe --all-features`: 132 passed in the
  library, and each other target passed.
- Plant `hostprobe-exec-always-names-the-filesystem` (the flag ignored,
  each denial named as `noexec`) fails
  `the_flag_of_the_mount_decides_the_remedy`; applied and run by hand.

# T-570: `pod-hostprobe`: the refusal of the machine tier runs its legs together (PodUtils/podvm#337)

**Source:** PodUtils/podvm#337 (COORD-093 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`Assessment::refusal` of `crates/pod-hostprobe/src/machine.rs` joins its
legs with one space. The reason of a skipped leg is a sentence, so the
name of the next leg runs into it:

```
podvm windows: machine tier refused: qemu-system-x86_64 --version=skip: no qemu-system-x86_64 on PATH, so the emulator leg could not run open(/dev/kvm, O_RDWR)=ENOENT qemu-system-x86_64 -accel help=skip: no qemu-system-x86_64 on PATH, so the accelerator list could not be read
```

`podvm probe` also prints the verdict two times
(`refused: machine tier refused: ...`): podvm's report
(PodUtils/podvm:crates/podvm-probe/src/report.rs, `machine_block`) adds
`refused: ` before the string. The expected output separates the legs with
`; ` and has one verdict word.

## Premise

- Read: `crates/pod-hostprobe/src/machine.rs` (`refusal`,
  `refusal_detail`), `crates/pod-hostprobe/src/supervise.rs` (`refusal`,
  the same join), `crates/pod-hostprobe/src/report.rs` and
  `crates/pod-hostprobe/src/report/` (the JSON reader and writer of
  podcrates: it holds no refusal and prints no verdict word).
- Not measured here: podvm's printers. The report quotes them.

## Approach

Join the legs with `; ` after `machine tier refused: `. The supervise
tier has the same join: fix it in place. podcrates's report prints no
`refused:` before the string, so the double comes from podvm's printer
alone: podvm drops its own prefix (C337). Tests pin the separator and one
verdict word; a plant puts the space back.

## Prove

```sh
cargo test -p pod-hostprobe --all-features --lib -- machine:: supervise::
sh scripts/plant-each.sh hostprobe-machine-legs-run-together hostprobe-supervise-legs-run-together
```

## Done

2026-11-14. The commit that names T-570.

- `machine::Assessment::refusal` gives `machine tier refused: LEG; LEG;
  LEG`; `supervise::Assessment::refusal` gives `SCOPE refused: LEG; LEG`
  (the same join, fixed in place). podvm's case now reads
  `machine tier refused: qemu-system-x86_64 --version=skip: no
  qemu-system-x86_64 on PATH, so the emulator leg could not run;
  open(/dev/kvm, O_RDWR)=ENOENT; qemu-system-x86_64 -accel help=skip: ...`.
  The JSON field `tiers.machine.refusal` of podvm carries the same string.
- The double verdict: podcrates's `report` module prints no refusal and
  no `refused:`. The `refused: ` before the string is in podvm's printer
  (PodUtils/podvm:crates/podvm-probe/src/report.rs, `machine_block`), and
  podvm drops it (C337). podcrates keeps the verdict in the string.
- Tests: `machine::tests::the_legs_of_a_refusal_are_separated_where_a_reason_ends`
  (the three legs of podvm's case, the whole string, one `refused`) and
  `supervise::tests::the_refusal_names_every_missing_leg_in_run_order`
  (the whole string). `cargo test -p pod-hostprobe --all-features`: each
  test passed.
- Plants `hostprobe-machine-legs-run-together` and
  `hostprobe-supervise-legs-run-together` (a space between legs again)
  fail their tests; each was applied and run by hand.

# T-567: `pod-rootfs`: the completion of a root of two layers, the lower layer read-only (PodUtils/podvm#326)

**Source:** PodUtils/podvm#326 (COORD-090 of podvm; PodUtils/podvm@T-1749), PodUtils/podvm#327 (COORD-091, the acceptance test), #146.
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podvm completes the root of each run before the entry with
`pod_rootfs::complete`: `/etc/hosts`, `/etc/resolv.conf`, `/etc/passwd`
and `/etc/group`, `/etc/mtab`, the CA bundle and the directories it scans,
the sources of each package manager, the device shims and the links of
`/dev/fd`, and a read of `lib/` for the C library. Each step works on one
root path. With the overlay root of T-486
(`Root::Overlay { lower, upper: Upper::Dir { upper, work } }`) the lower
layer is the shared extracted image and must not change; each write must
land in `upper`. A copy-up of a fixed list of paths in podvm breaks at the
first new path, and cannot serve a scan of a directory that both layers
hold (`etc/ssl/certs`, `etc/apt/sources.list.d`).

The ask: a form of the completion for two layers. (1) A read (a file, a
link, a stat, a listing) sees `upper` over `lower`; an entry of `upper`
hides the same name of `lower`; a listing is the union. (2) A write goes
to `upper` only; a change of a file that only `lower` holds copies it up
first (mode, owner where the process may set it, link target); a new file,
link or directory is made in `upper`, with its parent directories copied
up (their modes). (3) A remove of an entry that only `lower` holds is
refused with a typed error; say whether a step removes. (4) `lower` is
never opened for write; a test holds it to a digest. podvm sent its
acceptance test (PodUtils/podvm#327) for `crates/pod-rootfs/tests/`.

## Premise

- Read: PodUtils/podvm#326 and #327 (2026-11-14); each module of
  `crates/pod-rootfs` (each path of the root goes through `write::Root`);
  `pod_extract::safety` (the `InRoot` walk); the digest of
  `crates/pod-enter/tests/confine/overlay.rs` (T-486).
- Measured on this host (Linux 6.18, ext4, as root, and as root with no
  `CAP_MKNOD`): a whiteout (`mknod` 0:0) and a `user.` attribute are
  permitted.
- Not measured: a mount of overlayfs over an upper layer that the crate
  completed (podvm's Prove of PodUtils/podvm@T-1749 does it).

## Approach

1. `Settings::upper: Option<PathBuf>`; with it, `complete` opens
   `write::Root::layers(lower, upper)`. Each operation of `Root` asks for
   the two layers first, so each step works unchanged.
2. `write::layers`: the resolution of a path in the merged view, one
   component at a time from a descriptor of each layer (`O_NOFOLLOW`), a
   link followed in the process, held in the root; whiteouts and opaque
   directories honored; a copy-up of a file (bytes, owner, mode) and of
   each parent directory (mode, owner); `LayerRefusal::{Remove, Opaque}`.
3. `pod-hostprobe::sys`: `fgetxattr` and `fsetxattr` for the opaque mark.
4. Tests: the whole completion of a fixture on two layers against the
   completion of one root that holds the same tree, the digest of `lower`
   with its times, the union, the hiding, the copy-up with its mode, the
   typed refusal; podvm's acceptance test; three plants.

## Prove

```sh
cargo test -p pod-rootfs --all-features
cargo test -p pod-hostprobe --all-features --lib sys::tests
sh scripts/plant-each.sh rootfs-layers-write-to-lower rootfs-layers-list-upper-only rootfs-layers-lower-over-upper
sh scripts/agent/check-windows.sh
sh scripts/agent/check-macos.sh
```

## Done

2026-11-14, commit f98067e. `pod-rootfs` completes a root of two layers
(calls C330 to C334): `Settings::upper`, `write::Root::{layers, upper,
written}`, `write::layers`, `write::{LayerRefusal, layer_refusal}`. A read
sees `upper` over `lower` (a whiteout and an opaque directory of `upper`
honored), a listing is the union, each write goes to `upper`, a change of
a file of `lower` copies it up first (bytes, owner, mode), each parent
directory is copied up with its mode and owner, and `lower` only opens for
read. A path resolves one component at a time with `O_NOFOLLOW` from a
descriptor of each layer, each link of the merged view followed in the
process, held in the root. `pod-hostprobe::sys` has `fgetxattr` and
`fsetxattr`.

The steps that remove: the device step removes a symlink at a device
path before the node (on two layers a link of `lower` stays, hidden under
the new node or shim, and the row is the row of one root), and
`ca-hash-dir` removes an old certificate file of the crate (on two
layers, one of `lower` is refused with `LayerRefusal::Remove`, and the
row is a failed row with the refusal). A replace (`Root::write`) of an
entry of `lower` is no remove: the new file of `upper` hides it.

`cargo test -p pod-rootfs --all-features`: 81 unit tests and the 3 tests
of `tests/two_layers.rs` passed, as root and as root with no `CAP_MKNOD`
(the shim path; the first test of podvm then stops with `SKIP:`, C334).
New: `write::layers::tests::{an_entry_of_upper_hides_the_same_name_of_lower,
a_listing_of_two_layers_is_the_union, a_change_of_a_lower_file_copies_it_up_with_its_mode,
a_remove_of_a_lower_entry_is_refused_with_its_type, a_link_resolves_in_the_merged_view,
layers_that_hold_each_other_are_refused}` and
`write::layers::tests::complete::two_layers_complete_as_one_root_and_the_lower_layer_keeps_its_digest`
(a fixture with a read-only passwd, links into `/proc` and out of the
root, a symlinked `lib` and trust store, a hash-indexed CA directory,
`http://` sources in a file and in a directory that both layers hold, and
a link at `dev/null`: the merged view is the tree of the completion of one
root, the rows agree, the lower layer keeps its digest with the times,
the upper layer holds no whiteout and no unchanged copy, and a second run
changes nothing). The test of the copy-up also makes a new directory
under a directory of `lower` alone, which the first test of podvm needs
(`etc/apt/apt.conf.d`). `cargo
test -p pod-hostprobe --all-features --lib sys::tests`:
`an_extended_attribute_comes_back_through_the_descriptor` passed. Plants,
each caught by hand: `rootfs-layers-write-to-lower` (a mode change of a
lower file goes to `lower`: "the completion changed the lower layer"),
`rootfs-layers-list-upper-only` ("a listing is the union of both
layers"), `rootfs-layers-lower-over-upper` ("the upper layer hides the
lower one"); `sh scripts/plant-each.sh --apply`: 0 stale.
`check-windows.sh` and `check-macos.sh`: 0 faults.

# T-573: `pod-supervise`: a foreground holder, so that a run that the caller waits for is Running (PodUtils/podvm#339)

**Source:** PodUtils/podvm#339 (COORD-094 of podvm; PodUtils/podvm@T-1786, PodUtils/podvm#330)
**Category:** feature
**Milestone:** M6
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

A beta tester's defect of podvm. A foreground `podvm run` is recorded
`Created` for the whole life of its payload. `pod_supervise::create`
writes `Created`, and only the detached launcher writes `Running`
(`crates/pod-supervise/src/launcher/supervise.rs`). So while the payload
runs: `ps` hides it (`list` filters `Running`), `system df` counts 0
running, `kill` refuses ("is created and not running"), `wait` refuses,
and `stop` returns `Ok((c, false))` at once: `podvm stop NAME` prints the
name and exits 0, and stops nothing.

In `pod-supervise`, `Running` means a launcher: it holds `lock_path(id)`
for its life (the whole test of `reconcile`), and `kill`, `stop` and
`wait` reach it through `launcher::signal` and `launcher::wait` on its
socket. A foreground run has no launcher. A `Running` that podvm writes
into the table reads as `Dead` at the next reconcile (no lock held), or
makes `stop` fail to connect.

The ask: a holder for a payload that the caller runs itself
(`foreground(&settings, &id, payload_pid)` takes the lock and writes
`Running`, the pid and `started_at`; `finish(exit_code)` writes `Exited`,
the code and `finished_at`; a drop with no `finish` writes `Dead`). It
holds `lock_path(id)` for its life; it serves the socket protocol of the
launcher on a thread (`SIG` to the payload, `WAIT` answered with the code
of `finish`); `kill`, `stop` and `wait` then work with no change, and
`ps` lists the run `Up`. podvm's Prove: `ps` lists the run while it
sleeps, `system df` counts it running, `kill` and `wait` work, and `stop`
ends the payload within its grace and exits 0 only then.

## Premise

- Read: `crates/pod-supervise/src/lib.rs` (the verbs: `kill` and `stop`
  ask `State::Running`, then talk to the socket),
  `crates/pod-supervise/src/table.rs` (`reconcile` takes the lock of
  each `Running` record; held means alive),
  `crates/pod-supervise/src/launcher.rs` (`handle`: `SIG N` is
  `kill(pid)`, safe only for the parent before the reap; `WAIT` is held
  until the end), `crates/pod-supervise/src/launcher/supervise.rs` (the
  order: the lock, the socket, the record `Running`; at the end the
  record, the removal of the socket, the answers, the release of the
  lock), `crates/pod-supervise/src/lock.rs`, the test
  `nothing_on_the_spawn_path_can_spawn_a_thread` (the crate starts no
  thread).
- Read: the launcher signals the payload only, not its process group
  (`crates/pod-supervise/src/lib.rs`: "`stop` and `kill` signal the
  payload only").
- Read: `flock` locks belong to the open file description, so a
  `reconcile` in the process of the hold also sees the lock held.
- Not measured: a foreground run of podvm with the hold (podvm's own
  Prove).

## Approach

1. `pod_supervise::foreground::<X>(&s, want, payload_pid) ->
   Result<Foreground<X>>`: a reconcile and a find, a refusal of a
   `Running` record, a pidfd of the payload, the lock of the container,
   the control socket (`launcher::listen`, shared with the launcher), the
   thread of the server, then the record `Running` with `pid`,
   `launcher_pid` (the process of the hold) and `started_at` (C345).
2. The thread answers each request with `launcher::handle` (shared with
   the launcher): `SIG N` is `pidfd_send_signal` on the pidfd (C346), a
   `WAIT` is held. A stop pipe ends the thread: `exit N` from `finish`, or
   nothing from a drop.
3. `Foreground::finish(code)`: the record `Exited` (the launcher's
   `ended`), the removal of the socket, the code to each waiter, the join
   of the thread, then the release of the lock (C347). A drop with no
   `finish` writes `Dead` (`noticed`, `finished_at`), then the same close
   with no answer: a waiter reads the record again.
4. The thread is the one thread of the crate. `launcher::start` refuses
   while a hold lives in the process (C348). The hold applies no
   `wall_ms`, kills no tree and writes no `end` (C349).
5. `pod_hostprobe::sys::pidfd_send_signal`, with its test.
6. A read bound (2 s) on each control request, in the launcher too: a
   client that connects and sends nothing stopped the watch of the
   payload, and would stop the `finish` of a hold.

## Prove

```sh
cargo test -p pod-supervise --all-features
cargo test -p pod-hostprobe --all-features --lib -- sys::tests::a_pidfd_signals_its_process_and_not_a_reaped_one
sh scripts/plant-each.sh supervise-t573-hold-takes-no-lock supervise-t573-drop-writes-nothing supervise-t573-wait-answered-at-once supervise-t573-start-not-refused
sh scripts/agent/check-windows.sh
sh scripts/agent/check-macos.sh
```

## Done

2026-11-14, commit c00d5fa. `pod_supervise::foreground::<X>(&s, want,
payload_pid) -> Result<Foreground<X>>` and `Foreground::finish(code) ->
Result<()>` (`crates/pod-supervise/src/foreground.rs`); a drop with no
`finish` writes `Dead`. The hold takes the lock of the container, serves
the control socket on one thread (`launcher::listen` and
`launcher::handle`, shared with the launcher; `SIG` through
`pod_hostprobe::sys::pidfd_send_signal`, new), and writes `Running`;
`start` refuses while a hold lives. Calls C345 to C349.

`cargo test -p pod-supervise --all-features`: 30 unit tests,
`tests/foreground.rs` 6 steps (a held payload is `Up` and `reconcile`
keeps it; a second hold and a `start` are refused; `kill` reaches it;
`stop` with a grace of 10 s ends it with no SIGKILL; a wait before
`finish` gets the bound and a wait at `finish` gets its code; a drop
writes `Dead`; a killed holder process reads `Dead`), `lifecycle` 5
steps, `tree` 7 steps, each passed. The foreground binary passed 30 runs
in a loop. `cargo test -p pod-hostprobe --all-features`: 130 unit tests
passed, with `a_pidfd_signals_its_process_and_not_a_reaped_one`. Plants,
each caught by hand with its EXPECT text:
`supervise-t573-hold-takes-no-lock`, `supervise-t573-drop-writes-nothing`,
`supervise-t573-wait-answered-at-once`,
`supervise-t573-start-not-refused`; `plant-each.sh --apply`: 0 stale.
`check-size.py`, `check-windows.sh` and `check-macos.sh`: 0 faults.

# T-580: `pod-rootfs`: an image with no CA bundle gets the host bundle when `host_cas` is false (PodUtils/podvm#341)

**Source:** PodUtils/podvm#341 (COORD-095 of podvm; PodUtils/podvm@T-1818; an external QA report of podvm, finding F-18).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`ca_bundle` in `crates/pod-rootfs/src/pkg/bundle.rs` writes `settings.host_bundle` to each usual path whose directory exists when the image has no bundle, and it does not read `settings.host_cas` on that path. Only the path of an image that has a bundle reads `host_cas`. So `podvm run --no-host-cas` of an image with no CA bundle still gets the host bundle, which can hold an intercepting CA that `SSL_CERT_FILE` names.

## Premise

- Read: PodUtils/podvm#341; `crates/pod-rootfs/src/pkg/bundle.rs` (`ca_bundle`, the branch `present.is_empty()`).

## Approach

When `host_cas` is false, the branch with no bundle gives the same `Skipped` row (degraded) as when the host gives no bundle, and writes no file. A test: an image with no bundle, `host_cas` false and a host bundle given, writes no file. A plant that the test catches.

## Prove

```sh
cargo test -p pod-rootfs --all-features
```

## Done

2026-11-14, commit 5502330. With `Settings::host_cas` false, `ca_bundle`
writes no file into an image with no CA bundle and gives one row
`ca-bundle`, `Skipped` and degraded, that names the bundle of the host it
did not write (call C369). `cargo test -p pod-rootfs --all-features`: 82
unit tests and 3 tests of `two_layers` passed, with
`no_host_cas_writes_no_host_bundle_into_an_image_with_none` (a bundle
named by `SSL_CERT_FILE`, and one that no variable named), which fails on
the code before the fix. Plant `rootfs-t580-no-bundle-ignores-host-cas`,
caught by hand with its EXPECT text; `plant-each.sh --apply`: 0 stale. `check-size.py`, `check-windows.sh` and `check-macos.sh`: 0 faults.

# T-581: `pod-extract`: a file of mode 4755, 2755 or 6755 loses its setuid and setgid bits (PodUtils/podvm#342)

**Source:** PodUtils/podvm#342 (COORD-096 of podvm; PodUtils/podvm@T-1813; an external QA report of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`crates/pod-extract/src/apply.rs` opens a file with `want = (mode | 0o600) & 0o7777` and calls `fchmod(fd, want_final)` after the content only when `want_final != want`. For a mode of 4755, 2755 or 6755 the two are equal, so no `fchmod` runs, and the `write` of the content as a user that is not root has already cleared `S_ISUID` and `S_ISGID`. As uid 1000, 11 programs of `debian:bookworm-slim` lose their setuid or setgid bit.

## Premise

- Read: PodUtils/podvm#342; `crates/pod-extract/src/apply.rs` (the open with `want`, the `fchmod` of `want_final`, the sidecar of a widened mode).

## Approach

After the last write, call `fchmod(fd, mode & 0o7777)` when the mode has a bit of `0o7000`, also when it is equal to `want`. A mode that the kernel refuses goes into the sidecar, as the widened mode does. Tests of 4755, 2755 and 6755 as the user of the test, and as root where the test runs as root. A plant that the test catches.

## Prove

```sh
cargo test -p pod-extract --all-features
```

## Done

2026-11-14, commit bc89311. A regular file whose mode has a bit of
`0o7000` gets `fchmod` on its descriptor after the last write; the mode
of the descriptor is read back, and a mode that the kernel did not keep
goes into the sidecar as `applied.mode` (calls C370, C371).
`cargo test -p pod-extract --all-features`: 58 unit tests passed, with
`drive::setid::set_id_bits_survive_the_write` (4755, 2755 and 6755 as
root, then the test binary again as uid 1000 and gid 1000:
`drive::setid::worker_extracts_as_a_user`; in a directory of group 0
with `S_ISGID` the sidecar records 0755 and 4755). On the code before
the fix the worker fails: `bin/su` is 0755, not 4755. Plants
`extract-t581-set-id-bits-not-set-again` and
`extract-t581-kept-mode-not-recorded`, each caught by hand with its
EXPECT text; `plant-each.sh --apply`: 0 stale. `check-size.py`, `check-windows.sh` and `check-macos.sh`: 0 faults.

# T-582: `pod-supervise`: `start` keeps the end fields of the run before, and a `Dead` record keeps its pid (PodUtils/podvm#343)

**Source:** PodUtils/podvm#343 (COORD-097 of podvm; PodUtils/podvm@T-1807; an external QA report of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`start` sets `state`, `launcher_pid` and `pid`, and keeps `exit_code`, `finished_at`, `end` and `noticed` of the run before. So `podvm inspect` of a restarted container shows `running` with the exit code 143 of the run before. `reconcile` writes `Dead` and keeps `pid`, so the record names a pid that is not its process.

## Premise

- Read: PodUtils/podvm#343; `crates/pod-supervise/src/lib.rs` (`start`, `reconcile`), `launcher/supervise.rs` and `foreground.rs` (T-573: the hold writes the same records).

## Approach

Clear `exit_code`, `finished_at`, `end` and `noticed` when the launcher (or the foreground hold of T-573) reports that the payload runs. Clear `pid` and `launcher_pid` when the record becomes `Dead`. A test of each, and a plant.

## Prove

```sh
cargo test -p pod-supervise --all-features
```

## Done

2026-11-14, commit d019392. `Container::mark_running` (the launcher, a
foreground hold, the value of `start`) clears `exit_code`,
`finished_at`, `end` and `noticed`; `Container::mark_dead` (the
reconcile with a free lock or no lock file, the drop of a hold) clears
`pid` and `launcher_pid` and sets `finished_at` when it has none (calls
C372, C373). `cargo test -p pod-supervise --all-features`: 31 unit tests
(with `table::tests::a_new_run_clears_the_end_and_a_dead_record_names_no_pid`),
`foreground` 6 steps (a hold again after an exit of 143; no pid on each
`Dead`), `lifecycle` 5 steps (a `start` again after an exit of 143; no
pid on `Dead`), `tree` 7 steps, each passed; `lifecycle` and
`foreground` fail on the code before the fix. Plants
`supervise-t582-run-keeps-the-exit-code`,
`supervise-t582-dead-keeps-the-pid`,
`supervise-t582-launcher-keeps-the-end`, each caught by hand with its
EXPECT text; the patch of `supervise-t573-drop-writes-nothing` follows
the new code of the drop. `plant-each.sh --apply`: 0 stale. `check-size.py`, `check-windows.sh` and `check-macos.sh`: 0 faults.

# T-583: `pod-confine` and `pod-enter`: an `exec=` rule does not deny a program that the image's loader starts (PodUtils/podvm#344)

**Source:** PodUtils/podvm#344 (COORD-098 of podvm; PodUtils/podvm@T-1819; an external QA report of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

On podvm's loader family (the userland rung) the kernel executes the loader of the image from a system directory. Landlock checks `EXECUTE` on the loader and only `READ` on the program, so a script and a dynamic ELF outside the `exec=` paths of `--podvm-confine` run. The QA report also says that a static ELF under a granted `exec=/tmp` is denied; the report contradicts itself there.

## Premise

- Read: PodUtils/podvm#344; the rules of `exec=` in `pod-confine` (the Landlock rules: the handled set and the grants) and in `pod-enter` (the userland rung and its loader).

## Approach

1. Check that `LANDLOCK_ACCESS_FS_EXECUTE` is in the handled set and is granted on each `exec=` path; a test settles the case of a static ELF under `exec=`.
2. Add an API that answers whether a resolved program may run under the exec table of the rules, so that podvm's interposer checks it before it maps the program (Landlock cannot see that exec).
3. Tests of a static ELF, a dynamic ELF and a script, inside and outside an `exec=` path, on the kernel rung and through the API.

## Prove

```sh
cargo test -p pod-confine -p pod-enter --all-features
```

## Done

2026-11-14, commit 9413cab. The kernel rung holds: `EXECUTE` is in the
handled set at each ABI, and a rule with `Access::EXECUTE` grants it; a
static ELF, a dynamic ELF and a script run under an execute path and
fail with `EACCES` outside it (Linux 6.18, ABI 7). The static ELF of the
report runs under its execute path. The entry with no chroot does not
hold: the loader, the interpreter and a memfd run each program outside
the execute paths (measured). New: `pod_confine::ExecTable`
(`Spec::exec_table`, `resolved`, `allows`, `encode`, `decode`),
`pod_confine::exec::allows_in` (no allocation, for a copy in podvm's
interposer), `pod_enter::Names::exec_table` (`POD_ENTER_EXEC_TABLE`,
set for a confined payload of the entry with no chroot, each value of
the caller removed), `pod_enter::check_exec(&plan, program)`
(`Error::CannotInvoke`, 126), `pod_testkit::elf`. Calls C386 and C387.

`cargo test -p pod-confine -p pod-enter --all-features`: pod-confine 37
unit tests (`exec::tests` 4) and `tests/exec.rs` 1; pod-enter 186 unit
tests (`confine::tests` 3) and the integration tests; no skip of the
new tests on this host. Plants `confine-exec-table-no-boundary`,
`confine-execute-not-handled`, `confine-execute-not-granted`,
`enter-check-exec-allows-each`, `enter-exec-table-not-set` and
`enter-exec-table-of-the-caller`: each fails its test with its EXPECT
text.

# T-584: `pod-enter`: a plan with two limits of one resource fails with `EPERM` (PodUtils/podvm#345)

**Source:** PodUtils/podvm#345 (COORD-099 of podvm; PodUtils/podvm@T-1827; an external QA report of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`podvm run --ulimit nofile=64 --ulimit nofile=128` fails in the child: `pod-enter` applies the two entries in order, the first lowers the hard limit to 64, and the second cannot raise it (`EPERM`). docker keeps the last value. podvm now coalesces the entries itself.

## Premise

- Read: PodUtils/podvm#345; `crates/pod-enter/src/limits.rs` and its tests.

## Approach

Refuse a plan with two entries for one resource before the child starts, with a typed error that names the resource, so that each caller coalesces first. A test and a plant.

## Prove

```sh
cargo test -p pod-enter --all-features
```

## Done

2026-11-14, commit f222da5. Each entry refuses a plan with two limits of
one resource before the fork with the new `Error::LimitTwice(Resource)`
(`Class::Usage` as a `Failure`); `limits::repeated` gives the resource to
a caller. The check of `MOST` moved before the banner. Call C383.

`cargo test -p pod-enter --all-features`: 183 unit tests (2 new in
`limits::tests`) and the integration tests. The new test fails on the
old code with `setrlimit(RLIMIT_NOFILE): EPERM`. Plant
`enter-limit-twice-reaches-the-child` fails it.

# T-585: `pod-oci`: the grammar of an insecure registry host, and a platform list with no repeat and no attestation (PodUtils/podvm#346)

**Source:** PodUtils/podvm#346 (COORD-100 of podvm; PodUtils/podvm@T-1842, PodUtils/podvm@T-1836; an external QA report of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`normalise_host` (`crates/pod-oci/src/transport.rs`) refuses only a URL and a `/`, so it takes a host with a space or a `!`. The `NoPlatform` refusal of an index lists `windows/amd64` twice, as it does not show `os.version`, and lists the attestation manifests as `unknown/unknown`.

## Premise

- Read: PodUtils/podvm#346; `crates/pod-oci/src/transport.rs` (`normalise_host`); `crates/pod-oci/src/spec.rs` (`NoPlatform`, the list `offered`).

## Approach

A host is a DNS name or an IP literal (IPv6 in brackets), with an optional port; anything else is refused with a typed error. The list `offered` has no duplicate, no entry with the annotation `vnd.docker.reference.type: attestation-manifest`, and `os.version` where the index gives it. Tests and a plant for each.

## Prove

```sh
cargo test -p pod-oci --all-features
```

## Done

2026-11-14, commit cf68521. `normalise_host` and the domain of a
reference take one grammar (`host`): a DNS name, an IPv4 address or an
IPv6 address in brackets, with an optional port from 1 to 65535; each
other text is `Refusal::Host { host, why }` (`Class::Usage`). An IPv6
registry in brackets now parses as a reference. The list `offered` of
`NoPlatform` has no repeat, no attestation manifest (no candidate
either), and `os.version` (`windows(10.0.17763.6532)/amd64`). New:
`spec::Platform::os_version`, `Platform::full_word`,
`Descriptor::is_attestation`, `REFERENCE_TYPE`, `ATTESTATION`. Calls
C384 and C385.

`cargo test -p pod-oci --all-features`: 94 unit tests (4 new) and the
integration tests. Plants `oci-insecure-host-any-text`,
`oci-offered-repeats-a-platform`, `oci-offered-names-an-attestation`
and `oci-offered-drops-the-os-version`: each fails its test.

# T-586: `pod-netlab`: the texts of a knob give the value as written, with its unit and range; the differences from sand-net (PodUtils/podvm#347)

**Source:** PodUtils/podvm#347 (COORD-101 of podvm; PodUtils/podvm@T-1837, PodUtils/podvm@T-1847; an external QA report of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P3
**Effort:** M
**Status:** partial

## Problem

The errors of the knobs give the scaled value with no unit (`--loss 101` reads "loss 10100 is out of its range 1 to 2000"), do not name the value (`--tcp 70000`), call a negative number "not a whole number", and say that `--cgnat` "needs nat44" where the scenario `cgnat` uses `both`. The surface differs from sand-net: 12 scenarios are missing (`corp-proxy-pod`, `vpn-*`, `warp-*`), five value sets differ, `--icmp allow` is refused, and `--loss` stops at 20 percent.

## Premise

- Read: PodUtils/podvm#347; `crates/pod-netlab/src/knobs/flags.rs`, `crates/pod-netlab/src/knobs/check.rs`; the list of scenarios; podvm's issue 260 (COORD-041, the lab guest of PodUtils/podvm@T-1662).

## Approach

1. Each error gives the value as the user wrote it, its unit and its range. A negative number gets its own text. The text of `--cgnat` agrees with the scenario.
2. For each difference from sand-net: add the scenario or the value, or write the reason in the README of the crate. Tests of the texts, and a plant.

## Decision

Calls C388 and C389. Done in commit 405ee36 (2026-11-14; `tests/knobs.rs` 15 tests, plant `netlab-range-text-no-unit`): the texts (the value as written with its
unit and range, a negative number, a bad word with its value, a stray
word, `--cgnat`), `--icmp allow`, and the reason of each other
difference in the README of the crate. Open: the five knob sets of the
QA report (`symmetric-nat`, `pmtu-*`, `cgnat*`, `nat64-*`, `locked-443`
and `corp-proxy`). Issue 178 gives no source for them, and sand-net's
source is gone: podvm gives the source of the values of the report, or
the sets stay as call C145 derived them.

## Prove

```sh
cargo test -p pod-netlab --all-features
```

# T-590: `pod-guest`: `fetch::store` resumes a download by Range, checks the size and the sha256, and has a ceiling for each entry (PodUtils/podvm#350, item 1)

**Source:** PodUtils/podvm#350 (COORD-103 of podvm; the Windows guest of podvm).
**Category:** feature
**Milestone:** M7
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

podvm runs Windows and DOS guests from media that it fetches itself (the owner of podvm accepts the terms of the vendors; podvm's ruling "Windows media"). A medium of some GB breaks off on a slow line, and `pod_guest::fetch::store` starts again from zero. podvm asks for a `.part` file, a resume after a break by Range, a check of the size against `Content-Length`, a sha256 check at the end, and a ceiling for each entry (PodUtils/podvm@T-1862). podvm adds the check of free disk, of `RLIMIT_FSIZE` and the progress line in its own wrapper.

## Premise

- Read: PodUtils/podvm#350; `crates/pod-guest/src/fetch.rs`.

## Approach

Write to `NAME.part`; on a new call, send `Range: bytes=N-` and take a `206` (a `200` starts again from zero); check `Content-Range` and the total size against `Content-Length`; check the sha256 over the whole file; rename to `NAME` only after the check. A ceiling of bytes for each entry refuses a larger body with a typed error. Tests against a local HTTP stand-in that breaks the body once, gives `200` to a Range, and gives a wrong size; plants.

## Prove

```sh
cargo test -p pod-guest --all-features
```

## Done

2026-11-14, commit 8e4b3ca. `pod_guest::fetch::store(&mut origin, dest,
want, ceiling, tries)` takes an `Origin` (`get(from) -> Reply { status,
length, range, body }`, the caller's HTTP client; `Stream::new(reader,
declared)` for one reader) and writes `NAME.part` under an exclusive lock
(`Refusal::Busy`). A break resumes with `Range: bytes=N-`; a `206` must
start at `N` and agree with its `Content-Length`; a `200` starts again
from zero; a `416` of the size of the file is a whole file. The size of
the object stays the same in each answer and a body does not pass it
(`Refusal::Size`); the ceiling of the entry holds for the declared size,
a `Content-Range` total and the bytes; the sha256 covers the whole file,
and the rename comes after the check. A partial file of an earlier call
resumes only with a pinned digest. New: `Reply`, `Stream`, `Span`,
`parse_content_range`, `Refusal::{Size, Range, Status, Broken, Busy}`,
`Refusal::keeps_part`. Calls C395 to C398.

`cargo test -p pod-guest --all-features`: 104 unit tests passed at the
commit (7 tests of the store keep their names; 9 new, 7 of them against
the HTTP stand-in of `pod-testkit`: a break and a resume, a later call
pinned and not pinned, a `200` to a Range, a body past its
`Content-Length`, a `206` from another offset, a `Content-Range` against
its `Content-Length`, a size that changes, a range over the ceiling, a
`416` of a whole file, `404` and `503`); the stand tests passed 30 runs
in a loop. Plants: `guest-fetch-range-offset-unchecked`,
`guest-fetch-200-to-a-range-appends`, `guest-fetch-body-past-its-size`,
each caught by hand.

# T-591: `pod-udf`: a new crate that reads UDF and gives the extent of a file in an ISO (PodUtils/podvm#350, item 2)

**Source:** PodUtils/podvm#350 (COORD-103 of podvm; the Windows guest of podvm).
**Category:** feature
**Milestone:** M7
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

A Windows ISO holds `ValidationOS.vhdx` (about 0.9 GB) in a 2.5 GB image. With the extent of the file in the image, a client fetches only that extent by Range requests. podvm asks for a UDF reader that finds a file by its path and gives its extents (PodUtils/podvm@T-1863). If podcrates declines, podvm vendors winquick's `udf.rs` (Apache-2.0).

## Premise

- Read: PodUtils/podvm#350; ECMA-167 and the UDF 2.60 specification (the anchor volume descriptor at sector 256, the partition and logical volume descriptors, the file set descriptor, file entries and extended file entries, the file identifier descriptors, short and long allocation descriptors).

## Approach

A new crate `pod-udf` with no I/O of its own: a trait that reads sectors at an offset, so that a caller reads from a file or by Range requests. `find(path) -> Extents` (the byte ranges in the image, in order, and the size). UDF 1.02 to 2.60 as Windows ISOs use them; metadata partitions where a Windows ISO has one. Typed errors, a limit on the depth and on the count of entries. Tests with a small UDF image built in the test or kept under `tests/data` (made by a tool, with its command written beside it); a fuzz target; the static probe.

## Prove

```sh
cargo test -p pod-udf --all-features
```

## Done

2026-11-14, commit 0ac0022. The new crate `pod-udf`: `Udf::open(reader)`, `Udf::open_with(reader, Limits)`, `Udf::find(path) -> Extents { size, parts: Vec<Extent> }` (`Extent::Data { offset, len }`, `Extent::Zero { len }`; `Extents::range()` for a file of one part), `Udf::list(path) -> Vec<Entry>`, `Udf::read(&extents, max)`, `find(reader, path)`; the trait `ReadAt::read_at(offset, buf)` with `Seeker(R: Read + Seek)`; `Error` and `TagFault`. ECMA-167 with UDF 1.02 to 2.60: the anchor at block 256 (2048, 512 or 4096 bytes), the main and the reserve volume descriptor sequence, partition maps of type 1 and the metadata partition (its mirror stands in), file entries and extended file entries with `short_ad`, `long_ad`, data in the entry and allocation extent descriptors, names in OSTA compressed unicode (8, 16, 254, 255), each tag checked. Calls C404 to C408. New code from the specifications; no dependency.

- `cargo test -p pod-udf --all-features`: 17 unit tests and 1 doc test, against images of the writer of the tests (`src/tests/writer.rs`; no `mkisofs`, `genisoimage` or `xorriso` on the host): a file in a subdirectory, each revision with `short_ad` and `long_ad`, a metadata partition whose file has two extents, a missing path, a file of two extents and a hole, an embedded file, names with no regard to case and in UTF-16, the limits, no anchor and a failed reader, a `Seeker`, the CRC; a corrupt descriptor (CRC, checksum, location), the reserve sequence, the metadata mirror, allocation extent descriptors (a continued list, a loop to the limit), the unsupported structures. 1 ignored test writes the fuzz seeds.
- Plants, each caught by hand: `udf-crc-unchecked`, `udf-second-extent-dropped`, `udf-metadata-first-extent-only`, `udf-reserve-sequence-unused`, `udf-allocation-extent-not-followed`, `udf-depth-unbounded`.
- The fuzz target `udf_image` (sparse records; 3 seeds of the test writer): 600,000 mutations with no panic, in a scratch build of the target body on stable Rust. The static probe and the lean probe call the reader.
- clippy, rustfmt and `cargo doc` are clean; `check-size.py`, `check-probe.py --no-tree`, `check-publish-order.py`, `check-design-paths.py` agree; `check-windows.sh` and `check-macos.sh`: 0 faults.
- Next: the owner reserves `pod-udf` on crates.io (TODO/PROGRESS.md, "Owner actions"); podvm takes the crate at the next tag (PodUtils/podvm@T-1863).

# T-592: `pod-guest`: the agent runs the command in a child `cmd /c`, writes a `WQREADY` marker, and has a keep-alive mode (PodUtils/podvm#350, item 3)

**Source:** PodUtils/podvm#350 (COORD-103 of podvm; the Windows guest of podvm).
**Category:** defect
**Milestone:** M7
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The agent of `pod-guest` runs the command with `call %WQ%\WQCMD.CMD`, so a payload that ends in `exit` stops the agent before it writes its results and shuts the guest down, and the run times out (PodUtils/podvm@T-1864). podvm also needs a marker that the agent is ready, and a mode that keeps the guest up after the command, for a warm start and `exec` later.

## Premise

- Read: PodUtils/podvm#350; `crates/pod-guest/src/agent.rs` (`AGENT_CMD` and its tests).

## Approach

Run the command as `cmd /c %WQ%\WQCMD.CMD < NUL > ... 2> ...` and record its exit code; write `WQREADY` to the mailbox when the agent starts; a keep-alive mode does not run `shutdown` and waits for the next command. stdin of the command stays NUL. Tests of the text of the agent, and a run in the DOS or the emulator test where the crate has one; plants.

## Prove

```sh
cargo test -p pod-guest --all-features
```

## Done

2026-11-14, commit fad33d8. The agent runs `cmd /c %WQ%\WQCMD.CMD < NUL >
... 2> ...` and keeps the exit code in `WQRC`; it writes `WQREADY.TXT`
(`agent::READY`, `agent::ready(&fat16)`) when it finds the mailbox. With
`WQKEEP.TXT` on the mailbox (`agent::KEEP`, `Request::mode =
Mode::KeepAlive`) it runs each next token and does not shut down; a last
command with no `WQKEEP.TXT` shuts the guest down. `Mode::Once` is the
default. `run` refuses `KeepAlive` with `Usage`. Calls C393 and C394.

`cargo test -p pod-guest --all-features`: 95 unit tests passed at the
commit (6 new; the tests of the agent move to `src/agent/tests.rs` with
their names, and a walk of `AGENT_CMD` line by line proves the order,
the keep-alive loop over three tokens, the reentry guard and the 600
tries). Plants: `guest-agent-call-ends-the-agent` (the old `call`; the new
test fails on it), `guest-agent-keep-alive-shuts-down`, each caught by
hand.

# T-593: `pod-guest`: the options of a plan: a NIC with `hostfwd`, virtio devices, CD-ROM and USB media, VNC, a serial channel, the Hyper-V flags (PodUtils/podvm#350, item 4)

**Source:** PodUtils/podvm#350 (COORD-103 of podvm; the Windows guest of podvm).
**Category:** feature
**Milestone:** M7
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

A plan of `pod-guest` gives a mailbox run with `-nic none`. A full Windows guest needs a NIC (`user` with `hostfwd`), virtio devices, CD-ROM and USB media, a VNC socket, a serial channel and the Hyper-V flags (PodUtils/podvm@T-1865).

## Premise

- Read: PodUtils/podvm#350; `crates/pod-guest/src/plan.rs` (the argv of QEMU).

## Approach

Typed options of the plan, each one to its QEMU arguments, with `-nic none` the default of the mailbox run. Each option is checked (a `hostfwd` port, a path of a socket). Tests of the argv for each option and for the default; plants.

## Prove

```sh
cargo test -p pod-guest --all-features
```

## Done

2026-11-14, commit 91f5fcd. `Plan::options: Options` (`plan::options`):
`nic` (`Nic::None` by default; `Nic::User(UserNet { model, forwards,
restrict })` with `hostfwd`), `virtio`, `media` (CD-ROM on AHCI, USB on
`qemu-xhci`, `boot`), `vnc`, `usb_tablet`, `serial_socket`, `channels`
(virtio-serial, `QGA_CHANNEL`), `hyperv` (`HYPERV_FLAGS` under KVM).
`Options::default()` gives the old line, argument for argument;
`Options::check()` refuses each bad value with `Usage`, and `spawn_uefi`
calls it. A comma in a path of an option list is doubled, also in the
drives of the plan (a finding). Calls C399 to C401.

`cargo test -p pod-guest --all-features`: 115 unit tests passed at the
commit (11 new: the default line, each option and each refusal, the check
at the start of the emulator). Plants: `guest-plan-default-has-network`,
`guest-plan-forward-port-unchecked`, each caught by hand.

# T-594: `pod-qga`: a new crate, a client of the QEMU Guest Agent over a Unix socket (PodUtils/podvm#350, item 5)

**Source:** PodUtils/podvm#350 (COORD-103 of podvm; the Windows guest of podvm).
**Category:** feature
**Milestone:** M7
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podvm needs `exec`, `cp` and a graceful `stop` for a full Windows guest. The QEMU Guest Agent gives them over a Unix socket (PodUtils/podvm@T-1868).

## Premise

- Read: PodUtils/podvm#350; the QEMU Guest Agent protocol (`guest-sync-delimited`, `guest-ping`, `guest-exec`, `guest-exec-status`, `guest-file-open`, `guest-file-read`, `guest-file-write`, `guest-file-close`, `guest-shutdown`); `crates/pod-guest/src/qmp.rs` (the client of QMP in the workspace).

## Approach

A new crate `pod-qga` with a typed client of these calls, the sync on connect, a bound on each call, and typed errors. Tests against a stand-in of the agent on a Unix socket; a fuzz target of the answers.

## Prove

```sh
cargo test -p pod-qga --all-features
```

## Done

2026-11-14, commit 3fb02ba. The new crate `crates/pod-qga`, new code from
the schema `qga/qapi-schema.json` of QEMU (calls C414 to C421):

- `Client::connect(Settings)` and `Client::sync` (the byte 0xFF, then
  `guest-sync-delimited` with a new id; old answers are dropped),
  `ping`, `exec`, `exec_status`, `wait_exec`, `file_open`, `file_read`,
  `file_write`, `file_close`, `read_file`, `write_file`, `shutdown`; a
  bound on each call and a size limit on each answer; `Error` with its
  call, `ErrorKind` and its class. `wire` (the requests, the answers and
  the typed returns) builds on each target; the client is on Unix only.
  No tokio; `Cargo.lock` takes no new crate.
- `cargo test -p pod-qga --all-features`: 9 unit tests, `tests/agent.rs`
  9 tests and `tests/faults.rs` 8 tests against a stand-in agent on a
  Unix socket, each passed; 30 runs in a loop passed. The fuzz target
  `qga_answer` with 8 seeds; `cargo test --manifest-path fuzz/Cargo.toml
  --lib`: 13 passed. The static probe calls the crate.
- Plants, each caught by hand with its EXPECT text:
  `qga-no-sync-after-a-timeout`, `qga-sync-takes-an-old-answer`,
  `qga-handle-left-open`, `qga-shutdown-with-no-answer-fails`;
  `plant-each.sh --apply`: 0 stale.
- clippy, rustfmt and `cargo doc` with `-D warnings` are clean;
  `check-windows.sh` and `check-macos.sh`: 0 faults; `check-size.py`,
  `check-probe.py --no-tree`, `check-publish-order.py`,
  `check-design-paths.py` agree.
- Next: the owner reserves `pod-qga` on crates.io (Owner actions); podvm
  takes the crate at the next tag.

# T-595: `pod-guest`: the provision waits for a ready guest in place of a fixed sleep (PodUtils/podvm#350, item 6)

**Source:** PodUtils/podvm#350 (COORD-103 of podvm; the Windows guest of podvm).
**Category:** feature
**Milestone:** M7
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

`setup` of the provision of `pod-guest` sleeps a fixed time. podvm asks for a readiness poll: a mailbox marker, serial text, or a change of the screen (PodUtils/podvm@T-1873). Later, an NTFS and hive writer for an offline setup, after podvm measures whether `AutoRun` fires on the x64 image.

## Premise

- Read: PodUtils/podvm#350; `crates/pod-guest/src/provision.rs`; T-592 (the `WQREADY` marker).

## Approach

A readiness of the provision: the `WQREADY` marker of T-592, a text on the serial channel of T-593, or a change of the screen through QMP `screendump`; a deadline with a typed error. The NTFS and hive writer waits for podvm's measure. Tests against stand-ins; plants.

## Prove

```sh
cargo test -p pod-guest --all-features
```

## Done

2026-11-14, commits 3ef196a and b938308 (a stale socket in the stand-in of a
test). `pod_guest::ready::wait(&plan, &mut child,
&Ready { signs, deadline, every })` with `Sign::{Marker, Serial(text),
Screen { settle }}` gives the index of the first sign that holds;
`Unready::{Deadline, Exited}` (`Timeout`, `Unavailable`) is the cause of
each other end. `provision(.., out, &ready, wait)` types only into a
ready guest (`Ready::screen(deadline)`) and stops the emulator after a
deadline. `qmp::Monitor::screendump` and `qmp::json_text` are new. The
NTFS and hive writer waits for the measure of podvm, as the Approach
says. Calls C451 to C454.

`cargo test -p pod-guest --all-features`: 130 unit tests passed at the
commit (7 new against stand-ins: a mailbox file, a log that grows, a QMP
server with a script of frames, `sleep` and `sh -c 'exit 3'` as the
emulator, a provision whose emulator exits at once); 20 runs of the tests
of `ready` in a loop. Plants: `guest-ready-a-still-screen-is-ready`,
`guest-ready-serial-drops-the-carry`, each caught by hand.

# T-596: `pod-guest`: a warm start from a saved state, with a fingerprint and a fallback to a cold boot (PodUtils/podvm#350, item 7)

**Source:** PodUtils/podvm#350 (COORD-103 of podvm; the Windows guest of podvm).
**Category:** feature
**Milestone:** M7
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

A Windows guest takes long to boot. podvm asks to freeze a booted guest with `migrate` to a file and to restore it with `-incoming`, with a fingerprint of QEMU and the base, and a cold boot when the fingerprint differs (PodUtils/podvm@T-1874). ValidationOS first; it needs the keep-alive mode of T-592.

## Premise

- Read: PodUtils/podvm#350; `crates/pod-guest/src/qmp.rs`, `plan.rs`; T-592.

## Approach

Save: QMP `migrate` to `exec:` or a file after the guest is ready in keep-alive mode; the fingerprint is the QEMU version, the machine options of the plan and the digest of the base. Restore: `-incoming` with the same options; a different fingerprint or a failed restore boots cold. Tests of the fingerprint and of the argv; a live test where QEMU is present.

## Prove

```sh
cargo test -p pod-guest --all-features
```

## Done

2026-11-14, commit 5d69e25. `pod_guest::warm`: `save(&plan,
&fingerprint, dir, wait)` sends QMP `stop`, `migrate` to
`file:DIR/state.bin`, copies the overlay, the mailbox and the variable
store, writes `fingerprint.txt` last (`DIR.part`, then the name `DIR`),
and sends `cont`; it refuses a guest that is not an idle agent in
keep-alive mode (`idle`) and a device that the emulator cannot migrate
(`Unavailable`). `restore(&s, qemu_img, &plan, &request, &warm)` makes a
thin overlay over the saved one, puts the next command on the saved
mailbox, and starts the emulator with `-incoming` (`incoming_argv`).
`start(..)` gives `(Child, Start::{Warm, Cold(Cold)})`: no state, another
fingerprint or a failed load (`Cold::{Missing, Stale, Failed}`) boots
cold with `stage`. `fingerprint(&plan, base_digest, agent)` holds the
QEMU version, the CPU of the host under KVM, the firmware, the base, the
agent and `machine_line(&plan)`. `stage_idle` stages a guest to save.
`qmp::Monitor::ask` and `qmp::json_member` are new. QEMU 8.2.2 refuses
the NVMe disks of the plan line, so the warm start of the plan needs QEMU
11 (PodUtils/podvm@T-1874). The command reaches the restored agent through the
mounted mailbox: T-602. Calls C500 to C507.

`cargo test -p pod-guest --all-features`: 147 unit tests passed at the
commit (17 new: the fingerprint, the restore line, the idle agent, the
save and the load against a QMP stand-in, the fallback of `start` for no
state, a stale state and a failed load, a warm start with a stand-in
emulator, and two live tests with QEMU 8.2.2 of the host and a firmware of
`hlt`); 20 runs of the tests of `warm` in a loop. Plants:
`guest-warm-stale-state-restores`, `guest-warm-save-keeps-a-command`,
each caught by hand.

# T-597: `pod-extract`: the umask of the process changes the modes of the tree, and the sidecar does not record it

**Source:** T-581 (a finding of its work).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`pod-extract` does not set the umask of the process and does not set a mode again to undo it. A file whose image mode equals its open mode and has no bit of `0o7000` gets no `fchmod`, so with a umask of 027 a file of mode 0755 lands as 0750. A directory has the same fault: `mkdirat(mode | 0o700)` applies the umask, and the pass of the permissions between layers only adds owner bits. The tree differs from the image, and the sidecar does not record it.

## Premise

- Read: the report of T-581 and calls C370, C371; `crates/pod-extract/src/apply.rs` (the open of a file, the `fchmod` after the write), the creation of a directory and the pass of the permissions.

## Approach

Set the mode of each file through its descriptor after the write in each case, and set each directory to its image mode with `fchmodat` in the pass of the permissions (or call `umask(0)` around the extraction, if the crate owns the process; record the choice as a call). A mode that the kernel does not keep goes into the sidecar, as T-581 does. A test extracts under umask 027 in a child process (the umask belongs to the whole process) and checks that 0755 and 0644 stay; a plant that the test catches.

## Prove

```sh
cargo test -p pod-extract --all-features
```

## Done

2026-11-14, commit 5b41ffb. Each file gets `fchmod` after the last write,
also when its mode is the mode of the open, and its mode is read back
(C374). The permission pass sets each directory that the layer wrote to
`mode | 0o700`, so a later layer that names a directory again sets its
mode too (C375). The crate does not call `umask(0)`. The pass changes a
directory through a descriptor opened with `O_DIRECTORY` and
`O_NOFOLLOW`: `fchmodat` of the name followed a symlink that a later
entry of the layer put there, and changed the mode of a file of the host
(C413). `cargo test -p pod-extract --all-features`: 62 unit tests passed,
with `drive::umask::the_modes_of_the_image_stay_under_a_umask_of_027`
(the test binary again under a umask of 027: 0755, 0644, 0775 and 0664
of files and directories stay, and the sidecar claims no other mode),
`a_later_layer_sets_the_mode_of_a_directory_again` and
`the_permission_pass_does_not_follow_a_symlink_at_the_name_of_a_directory`.
Each fails on the code before the fix. Plants
`extract-t597-file-mode-set-only-when-it-differs`,
`extract-t597-pass-only-widens` and `extract-t597-pass-follows-a-symlink`,
each caught by hand with its EXPECT text; the patch of
`extract-t581-set-id-bits-not-set-again` follows the new code and is
caught again. `plant-each.sh --apply`: 0 stale. `check-size.py`,
`check-windows.sh` and `check-macos.sh`: 0 faults.

# T-598: `pod-extract`: each directory has the mode of the image after the last layer, and a directory entry replaces a non-directory of an earlier layer (PodUtils/podvm#354)

**Source:** PodUtils/podvm#354 (COORD-107 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podvm's invariant: after the extraction, a directory in the tree is the directory that the image names, with the mode that the image names, and no write lands outside the tree. T-597 sets the mode of each file again, sets each directory that a layer wrote to `mode | 0o700`, sets a directory again when a later layer names it, and changes modes through an `O_NOFOLLOW` descriptor. Two parts remain:

1. The pass keeps the owner bits (`| 0o700`) so that the next layer can write; nothing runs after the last layer, so a directory of mode `0555` in the image stays `0755` in the tree, while the sidecar records `0555`.
2. A directory entry over a symlink (or another non-directory) that an earlier layer left at its name is not applied: `mkdirat` gives `EEXIST`, `make_dir` takes it as done, and the tree keeps the symlink while the sidecar records a directory.

podvm also measured that a second extraction over a tree whose directories are closed (`0555`) fails with `EACCES`, so a directory that exists must open for the write.

## Premise

- Read: PodUtils/podvm#354 (its patch and its measures, "The invariant" and items 2 to 4); the Done section of T-597 and calls C374, C375, C413; `crates/pod-extract/src/apply.rs` (`make_dir`, the pass of the permissions, `extract_layers`).

## Approach

After the last layer, set each directory that the extraction wrote to the mode of the image (the last layer that names it wins), through an `O_NOFOLLOW` descriptor, deepest first. A directory entry over a name that is not a directory removes that name first, as the crate replaces a file. On `EEXIST` of a directory, open it for the write (owner bits) until the last pass. Tests: a `0555` directory stays `0555` after the extraction; a directory entry over a symlink of an earlier layer gives a directory and no change outside the tree; a second extraction over a closed tree passes. A plant for each.

## Prove

```sh
cargo test -p pod-extract --all-features
```

## Done

2026-11-14, commit 195e11d. After the last layer, each directory entry of
the extraction gets the mode of the image, the deepest first, through a
descriptor opened with `O_DIRECTORY` and `O_NOFOLLOW`
(`modes::DirModes`; the last layer that names a directory wins; a
whiteout forgets the directories that it removed); `Extracted::dir_modes`
gives the count (C423). A directory entry over a symlink or a file of an
earlier layer removes that name and makes the directory; a directory
that exists gets the owner bits through the same descriptor, so an
extraction over a closed tree passes (C424). The removal of a name adds
the owner bits to each directory that it empties, and a whiteout to the
directory of its name, so `Place::remove`, `remove_tree` and a whiteout
work on a closed tree (C425). `cargo test -p pod-extract --all-features`:
69 unit tests passed, with `drive::dirmodes` (0555 and 1777 stay, a later
layer closes a directory again, through `extract`; a directory entry
over a symlink of an earlier layer gives a directory and no change
outside the tree; a whiteout forgets the mode; a second extraction over
a closed tree, a whiteout in a closed directory and the removal of the
closed tree pass as uid 1000, the worker run by hand with `setpriv`
too) and `modes::tests`. Six plants (`extract-t598-*`), each caught by
hand with its EXPECT text. `pod-rootfs` (the only caller in the
workspace): 82 unit tests and 3 integration tests passed. Its completion
of a tree whose `/etc` is 0555, measured as uid 1000, gives three
`Failed` rows (`resolv.conf`, `hosts`, `mtab`: `EACCES`); with `/etc`
0755 it gives none. Before this entry such an `/etc` was 0755 in the
tree.

# T-599: `pod-extract`: the `./` member of an archive is not counted as an entry that the crate could not make (PodUtils/podvm#352)

**Source:** PodUtils/podvm#352 (COORD-106 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`LayerStats::skipped` holds the entries of a type that the crate does not make. The `./` member of an archive names the destination, which exists, and its branch in `crates/pod-extract/src/apply.rs` is an entry, not a refusal; but it adds to `skipped` with the name `ArchiveRoot`. podvm then prints "1 entries were not materialised (ArchiveRoot)" for an image that lost nothing.

## Premise

- Read: PodUtils/podvm#352; `crates/pod-extract/src/apply.rs` (the branch of the root, `LayerStats`).

## Approach

The root member adds nothing to `skipped` and names nothing there; a fifo and a socket still count. Whether `entries` counts the members read or the entries written stays as it is (record the rule as a call). A test with a layer that holds `./` and a file, and with a layer that holds a fifo; a plant.

## Prove

```sh
cargo test -p pod-extract --all-features
```

## Done

2026-11-14, commit 1834657. The root member of an archive adds nothing
to `LayerStats::skipped` and names nothing in `skipped_kinds`; a fifo, a
socket and a device node still count; `entries` stays the count of the
members read (C426). `cargo test -p pod-extract --all-features`: 70 unit
tests passed, with `drive::the_root_member_is_not_a_skipped_entry` (`./`
and a file: 2 entries, 0 skipped; `./` and a fifo: 1 skipped, named
`Fifo`). Plant `extract-t599-root-member-skipped`, caught by hand with
its EXPECT text; the patch of `extract-empty-root-refused` follows the
new code and is caught again. `plant-each.sh --apply`: 0 stale.
`check-size.py`, `check-windows.sh` and `check-macos.sh`: 0 faults.

# T-601: `pod-udf`: a test against images that a real mastering tool made

**Source:** T-591 (a finding of its work).
**Category:** chore
**Milestone:** M7
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Each test image of `pod-udf` comes from a writer in its own tests, which follows the same reading of the specification as the reader. A misreading in both, or a quirk of a real mastering tool (oscdimg, mkisofs, xorriso, mkudffs), passes each test.

## Premise

- Read: the report of T-591 and calls C404 to C408; `crates/pod-udf/src/tests/writer.rs`.

## Approach

Keep a small image made by a real tool under `tests/data`, with its command beside it, or build one in CI with `xorriso -as mkisofs -udf` (UDF 1.02) and `mkudffs --udfrev=0x0250` (a metadata partition). Run `find` on a known path and compare the bytes. A live test against a Windows ISO by Range requests comes later.

## Prove

```sh
cargo test -p pod-udf --all-features
```

## Done

2026-11-14, commit 1efa073. `tests/tools.rs` reads three images of real
tools, kept with gzip (1.6 to 4.5 KB) under `crates/pod-udf/tests/data`
with the command of each: a UDF 1.02 bridge of genisoimage 1.1.11
(`-udf`; each file of a tree like a Windows ISO, its bytes at the offset
of each part, an empty file, a missing path, a directory as a file), an
empty UDF 2.01 volume of mkudffs 2.3, and a BD-R UDF 2.50 volume of
mkudffs 2.3 whose virtual partition map is refused by name (C512). The
tools came from `apt-get install`. xorriso writes no UDF; no tool here
writes a metadata partition (mkudffs 2.3 writes UDF 2.50 only with a
virtual partition, and the kernel has no UDF to fill a volume): the live
test against a Windows ISO stays the proof of that form. genisoimage
writes an empty file with no allocation descriptor, which the writer of
the tests does not.

`cargo test -p pod-udf --all-features`: 17 unit tests and 3 tests of
`tests/tools.rs` passed. Plants, each caught by hand:
`udf-t601-empty-file-needs-a-descriptor` (the tests of the writer pass
it), `udf-t601-virtual-map-read-as-metadata`.

# T-602: `pod-guest`: the next command of a guest in keep-alive mode reaches the agent through a mounted FAT mailbox

**Source:** T-592 (a finding of its work).
**Category:** research
**Milestone:** M7
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

In keep-alive mode (T-592) the agent finds its next command by a poll of the FAT mailbox while Windows has that volume mounted. Windows caches the volume, so a write of a new `WQGO.TXT` by the host may never reach the agent. No real guest has proved it.

## Premise

- Read: the report of T-592 and calls C393 to C401; `crates/pod-guest/src/agent.rs`; `.github/workflows/guest.yml`.

## Approach

Prove it in `guest.yml` on a real image. If the cache hides the write, send the next commands through `pod-qga` (T-594), or swap the mailbox medium through QMP between commands, or give a new mailbox at a warm restore (T-596). Record the result as a call.

## Prove

```sh
cargo test -p pod-guest --all-features
```

## Done

2026-11-14, commit f196202 (partial). The sources prove that the cache
hides the write: FastFAT reads the directory of a mounted volume from its
cache and goes to the disk only on `DO_VERIFY_VOLUME`, which no driver
sets for a fixed disk (call C543). The mechanism is the swap of the
mailbox medium through QMP (call C542): `swap::Swap` (`new`, `poll`,
`next`, `current`, `token`), `swap::settled` (the code of the token and
no `WQGO.TXT`), `change_request`, `other_file`;
`Options::mailbox: MailboxBus::{Nvme, Usb}` and `MAILBOX_DRIVE`;
`Fat16::{volume_id, set_volume_id}` and `VOLUME_ID`. The agent writes
`WQREADY.TXT` in each poll and looks for a lost mailbox again (call
C544). It needs no new component in the guest image.

`cargo test -p pod-guest --all-features`: 157 unit tests passed (9 new);
`a_real_emulator_changes_the_medium_of_a_usb_mailbox` swaps two times
with the QEMU 8.2.2 of the host; 20 runs of the new tests passed. Plants
`guest-swap-same-volume-id`, `guest-swap-before-settled`,
`guest-swap-writes-the-medium-in-the-guest`,
`guest-agent-idle-ends-without-the-medium`, each caught by hand;
`plant-each.sh --apply`: 0 stale. The answer ISO of the live test and its
install line ran with the QEMU 8.2.2 and xorriso 1.5.6 of the host (TCG,
a firmware of `hlt`): both CD-ROMs, `send-key` and `screendump`.

The proof on a real guest: `guest-windows.yml` (by hand) installs
Windows Server 2019 Server Core under KVM and runs
`tests/windows_keep_live.rs`: a first command, two next commands by a
swap, and the controls on a mounted USB and NVMe mailbox.

The first run (GitHub run 43634720450, 2026-11-14) failed before the
commands: Setup copied the files, and after its first reboot the firmware
ran the UEFI shell until the limit of 4800 s. The cause was T-629 (OVMF
deleted the boot entry of the root disk, which had no `bootindex`). The
next run follows the merge of T-629.

The second run (GitHub run 43647025370, job 132606998403, 2026-11-14, at
ae04e9e) installed Windows in 190 s, booted it with a USB mailbox, and
passed the first command (code 3, stdout `pod-guest-one`). The first
swap failed: Windows locks the medium that it has mounted, and QEMU
refuses `blockdev-change-medium` with no `force` ("Device 'mbox' is
locked and force was not specified"). The cause was T-630. The next run
follows the merge of T-630.

The third run (GitHub run 43650756623, job 132618465267, 2026-11-14, at
61d6f8a) passed in 12.5 min. Windows installed in 160 s; the first
command gave code 3 and `pod-guest-one`; the guest locked each medium
(`query-block`: locked); `Swap::next` with `force` gave two next commands
(`pod-guest-two`, `pod-guest-three`, code 0), each read from the new
medium within 2 s. The `RESULT:` lines: "the swap gave two next commands
to the agent, and both outcomes are right"; "a write into the mounted USB
mailbox reached the agent: no"; "a write into the mounted NVMe mailbox
reached the agent: no" (each control waited 150 s). The controls prove
call C543 on a real guest: the cache of FastFAT hides a write of the host
on a mounted medium of each bus.

# T-603: `pod-guest`: the host removes a file of the FAT mailbox, and a poll of the outcome reads an old token as no result yet

**Source:** T-592 (a finding of its work).
**Category:** defect
**Milestone:** M7
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`Fat16` has no way to remove a file, so the host cannot drop a stale `WQCODE.TXT` or the file `WQKEEP.TXT`. `outcome()` takes a code with an older token as a `Protocol` error, not as no result yet, so a poller of a guest in keep-alive mode gets an error for the command before.

## Premise

- Read: the report of T-592; `crates/pod-guest/src/fat16.rs` and `agent.rs` (`outcome`).

## Approach

Add `Fat16::remove(name)` and a poll form of `outcome` that gives `None` for an older token. Tests and a plant.

## Prove

```sh
cargo test -p pod-guest --all-features
```

## Done

2026-11-14, commit 5c942a2 (its plant again after T-610 in b938308).
`Fat16::remove(name) -> Result<bool>` (the
entry, its clusters and the long-name entries before it; `Ok(false)` for
no such file), `poll_outcome(&fat16, token) -> Option<Outcome>` (`None`
for no code, a code of another token, a code that does not parse yet),
and `next_command(&mut fat16, &request)` (the result before goes, then
the command, the mode file and the token). `outcome` after the power-off
still refuses another token. Call C402.

`cargo test -p pod-guest --all-features`: 119 unit tests passed at the
commit (4 new: the removal frees the clusters for a file of most of the
volume, the long-name entries go, the poll, the next command). Plants:
`guest-fat16-remove-leaks-clusters`, `guest-poll-takes-an-old-token`,
each caught by hand.

# T-604: `pod-rootfs`: the completion of a tree whose `/etc` is closed (`0555`) fails as a user that is not root, after T-598

**Source:** T-598 (a finding of its work).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

T-598 gives each directory the exact mode of the image after the last layer. `pod_rootfs::complete` as uid 1000 on a tree with `/etc` at `0555` then gives three `Failed` rows (`resolv.conf`, `hosts`, `mtab`: `EACCES`); before T-598 such an `/etc` was `0755` in the tree. The row of `identity` says "it is writable, so the useradd of the payload works", which is false for a closed directory. A payload that runs without `CAP_DAC_OVERRIDE` meets the same closed modes (a Fedora `/root` of `0550`). This must be fixed before a tag carries T-598.

## Premise

- Read: the report of T-598 and calls C423 to C426; `crates/pod-rootfs/src/write/` (the steps of the completion and `write::Root`), the row of `identity`.

## Approach

`pod-rootfs` opens a closed directory for its own write through a descriptor (owner bits) and sets the mode of the image back after the step, in each case and on two layers (T-567: the copy-up of the directory in the upper layer). The row of `identity` checks the mode of the directory too. Record the rule as a call. Tests as uid 1000 with `/etc` at `0555` (the worker of T-581 shows how); a plant.

## Prove

```sh
cargo test -p pod-rootfs --all-features
```

## Done

2026-11-14, commit 8d99721. Each write of the completion adds the owner
bits to its directory through the descriptor where one is off, and sets
the mode of the image back after the write
(`pod_extract::safety::OwnerBits`); on two layers the same for the
directory of the upper layer, a copy-up of a closed directory of the
lower layer with its mode, and for the copy-up of a directory into a
closed one. The walk of `Resolve::InRoot` with `make_dirs` opens a
closed parent for the one `mkdirat`. The `identity` row is degraded where
`/etc` has no owner write or search, and names its mode (C427).
`Root::dir_mode` is new. `cargo test -p pod-rootfs --all-features`: 84
unit tests and 3 integration tests passed, with
`closed::a_closed_etc_is_completed_and_keeps_its_mode` (a musl root with
`etc` and `etc/apt` at 0555, as uid 1000: no `Failed` row, `resolv.conf`,
`hosts`, `mtab` and the new `etc/apt/apt.conf.d` are written, 0555 comes
back, and the `identity` row names the closed `etc`; on one root and on
two layers, the lower layer unchanged). `cargo test -p pod-extract
--all-features`: 70 unit tests passed. Plants
`rootfs-t604-closed-dir-not-opened`, `rootfs-t604-mode-not-set-back`,
`rootfs-t604-upper-dir-not-opened`, `rootfs-t604-make-in-closed-dir` and
`rootfs-t604-identity-ignores-the-directory`, each caught by hand with
its EXPECT text.

# T-605: `pod-extract`: the sidecar does not record the mode that the kernel kept on a directory

**Source:** T-598 (a finding of its work).
**Category:** defect
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The kernel drops `S_ISGID` from a directory whose group is not a group of the process, with no error. The pass after the last layer (T-598) does not read the mode back, so the sidecar can record a mode that is not on disk. This is the side of the directories of T-581.

## Premise

- Read: the report of T-598; `crates/pod-extract/src/modes.rs` (`DirModes::set`) and the sidecar.

## Approach

Read back each mode in `DirModes::set` and record `applied.mode` in the row of the directory in the sidecar (write the row there, or update it). A test and a plant.

## Prove

```sh
cargo test -p pod-extract --all-features
```

## Done

2026-11-14, commit 06c2327. `DirModes::set` reads the mode of each
directory back; a mode that the kernel did not keep is one more row of
the directory in the sidecar with `applied.mode` (`Sidecar::push_dir`,
`Sidecar::kept_dir`); the last row of a path holds (C429). `cargo test -p
pod-extract --all-features`: 73 unit tests passed, with the worker of
`drive::setid` (as uid 1000, a directory of 2755 in a directory of group
0 keeps 0755, and its last row records `applied.mode` 0755; one of 0755
gets no second row) and
`sidecar::tests::a_kept_directory_mode_is_one_more_row_of_the_directory`.
Plant `extract-t605-kept-dir-mode-not-recorded`, caught by hand with its
EXPECT text; the patch of `extract-t598-no-pass-after-the-last-layer`
follows the new signature and is caught again.

# T-606: `pod-extract`: a second extraction over a closed tree fails on a directory with no owner read or under a closed parent that the layer does not name

**Source:** T-598 (a finding of its work).
**Category:** defect
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

After T-598 a tree keeps the closed modes of the image. A directory whose mode has no owner read (for example `0311`) makes the next extraction fail at the open of its descriptor, and a closed parent that a new layer does not name as an entry refuses the write of its children.

## Premise

- Read: the report of T-598 and call C424; `crates/pod-extract/src/apply.rs`.

## Approach

Before an extraction into a tree that is not empty, walk the tree once and add the owner bits to each directory through descriptors (`O_NOFOLLOW`); the pass after the last layer sets the modes back. A test of each case, and a plant.

## Prove

```sh
cargo test -p pod-extract --all-features
```

## Done

2026-11-14, commit ff1fd6f. Before the first layer the extraction adds
the owner bits to each directory of the tree at the destination
(`modes::open_tree`, `Extracted::dirs_opened`), each opened with
`O_NOFOLLOW`; a directory with no owner read gets its mode through the
`/proc/self/fd` link of an `O_PATH` descriptor. The removal opens a
directory the same way (C428). `cargo test -p pod-extract
--all-features`: 72 unit tests passed, with
`drive::dirmodes::an_extraction_over_a_tree_that_its_owner_cannot_read_passes`
(as uid 1000: a second extraction over a directory of 0311 passes and
0311 comes back; an entry under a closed parent that the layer does not
name lands; `dirs_opened` is 2; `Place::remove` removes the tree). Plants
`extract-t606-tree-not-opened` and `extract-t606-no-open-without-read`,
each caught by hand with its EXPECT text; the patch of
`extract-t598-removal-does-not-open-a-closed-directory` follows the new
code and is caught again.

# T-609: `pod-broker`: a 502 names the status of the upstream proxy or the dial error, and a 403 of `no-rule` names the half of the rule that failed (PodUtils/podvm#359)

**Source:** PodUtils/podvm#359 (COORD-108 of podvm; PodUtils/podvm@T-1820; an external QA report of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

A `CONNECT example.com:443` that `--allow example.com` permits answered `502 Bad Gateway: the far end failed` on the host of the QA report; most likely its upstream proxy refused, and nothing in the answer says so. A `GET http://example.com/` with `--allow example.com` gets `403 denied by the rule policy:no-rule`, because the default ports are `[443]`, and the text gives no clue that `--ports 80` is the fix.

## Premise

- Read: PodUtils/podvm#359; the answers of `pod-broker` (the 502 of a failed dial, the 403 of the rule policy).

## Approach

A 502 names the status line of the upstream proxy, or the dial error, and the proxy that it dialed through, with no body and no credential (for example `502 Bad Gateway: the upstream proxy answered "403 Forbidden"`). A 403 of `no-rule` names whether the host or the port failed, and the ports that the rule takes. Tests of both texts; a plant.

## Prove

```sh
cargo test -p pod-broker --all-features
```

## Done

2026-11-14, commit 7cbcfff. A refusal of the upstream proxy answers
`502 Bad Gateway: the upstream proxy http://HOST:PORT answered "403
Forbidden" to CONNECT ADDR:PORT`: the status with its RFC 9110 name, the
proxy label and the target, with no reason phrase, no body and no
credential (C469). Another 502 or 504 gives the failure and `(through the
upstream proxy LABEL)`; the words lose the user information of each URL
(`words::without_userinfo`, C470). A 403 of `policy:no-rule` names the
half that failed: `denied by the rule policy:no-rule: the rules of the
host example.com take the port 443, not 80`, or `no allow rule names the
host example.com` (`Compiled::no_rule`, `NoRule`, C471). `cargo test -p
pod-broker --all-features`: 80 tests passed, with 3 new unit tests of
`words`, `a_denied_target_names_the_half_of_the_rules_that_failed`
(`tests/policy.rs`) and `tests/words.rs` (2: a stand-in upstream on
loopback that answers 403 with its own phrase and a body, through a
proxy URL with a made-up user and password; a GET and a CONNECT that no
rule allows). Plants (group `proxy`), each caught by hand with its
EXPECT: `broker-502-names-no-upstream` and
`broker-502-shows-the-proxy-body` (`tests/words.rs`),
`broker-no-rule-names-no-half` (`tests/words.rs`),
`broker-502-keeps-the-userinfo` (`--lib`).

# T-615: `pod-extract`: `open_tree` and the removal hold one descriptor for each level of depth

**Source:** T-606 (a finding of its work).
**Category:** defect
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`open_tree` (T-606) and `remove_at` keep one open descriptor for each level of depth. A tree deeper than the limit of descriptors (about 1024) stops them early.

## Premise

- Read: the report of T-606 and call C428; `crates/pod-extract/src/modes.rs` (`open_tree`) and the removal (`remove_at`).

## Approach

Walk the tree with no ancestor held open (an explicit stack of paths reopened from the root with `O_NOFOLLOW` at each level, or `openat` from a held parent only), and refuse a tree past a depth limit with a typed error. A test with a tree deeper than a lowered `RLIMIT_NOFILE`; a plant.

## Prove

```sh
cargo test -p pod-extract --all-features
```

## Done

2026-11-14, commit e85ddea. `open_tree` and `remove_at` walk with one
directory open at a time (the new module `tree`): the walk keeps the
listing and the device and inode of each level, and goes up through `..`
only when its device and inode are those of the level above (`TempFail`
when a directory moved). A directory deeper than `MAX_DEPTH` (4096) below
the start is refused with the typed cause `TooDeep` (`Unsupported`,
`too_deep(&failure)`); an entry with more than `MAX_DEPTH` components is
refused (`Refusal::TooDeep`), so a layer cannot make a tree that the
removal refuses (C510). A finding fixed in place: the removal read a name
of a listing as lossy text, so a name that is not UTF-8 stayed and its
parent failed; it unlinks by the bytes now (`CBuf::from_bytes` of
`pod-hostprobe`).

`cargo test -p pod-extract --all-features`: 79 unit tests passed (new: a
tree of 200 levels under a soft `RLIMIT_NOFILE` of 64 in a child process,
the depth limit of both walks, a name that is not UTF-8, a directory that
moved, the depth of a path). Plants, each caught by hand:
`extract-t615-walk-holds-each-level` (the walk of the old code),
`extract-t615-no-depth-limit`, `extract-t615-path-depth-not-refused`,
`extract-t615-name-read-as-text`, `extract-t615-moved-parent-followed`.

# T-616: `pod-supervise`: the thread of the foreground hold blocks the shutdown signals

**Source:** PodUtils/podvm#362 (COORD-111 of podvm, a note on T-573; PodUtils/podvm@T-1708).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The thread that serves the socket of a foreground hold (T-573) leaves SIGINT and SIGTERM unblocked. A signal to the process can land on that thread with the default action, and the caller ends while its payload runs. podvm blocks the shutdown signals while `foreground` starts its thread, so that the thread takes the blocked mask.

## Premise

- Read: PodUtils/podvm#362; `crates/pod-supervise/src/foreground.rs` (the start of the thread); call C348.

## Approach

The hold blocks SIGINT, SIGTERM, SIGHUP and SIGQUIT in the mask of its own thread (`pthread_sigmask` at the start of the thread, or around its spawn), so that a signal to the process goes to a thread of the caller. A test: a process with a hold that gets SIGTERM from outside runs its own handler (or the main thread's default) and not an end on the thread of the hold; a plant.

## Prove

```sh
cargo test -p pod-supervise --all-features
```

## Done

2026-11-14, commit 8d23780. `foreground` blocks SIGHUP, SIGINT, SIGQUIT and
SIGTERM in the calling thread around the spawn of the thread of the hold,
then gives the caller its earlier mask: the thread takes the blocked mask
at its creation, with no open time (C485). `pod_hostprobe::sys::sigblock`,
`sigbit`, `SIGHUP` and `SIGQUIT` are new (C486). Other signals stay open
in the thread (C487).

`cargo test -p pod-supervise --all-features`: 31 unit tests,
`tests/foreground.rs` 7 steps (new:
`a_shutdown_signal_skips_the_thread_of_the_hold`: a holder process
blocks the four signals in its own thread after the start of the hold,
gets each one from outside, and keeps each one pending in `ShdPnd`; on
the old code it ends at SIGHUP), `lifecycle` 5 steps, `tree` 7 steps,
each passed; the foreground binary passed 30 runs.
`cargo test -p pod-hostprobe --all-features`: 136 unit tests passed,
with `sigblock_blocks_the_set_in_this_thread_and_sigrestore_gives_it_back`.
Plant `supervise-t616-hold-thread-takes-shutdown` (the block of an empty
set), caught by hand: "the holder ended at a shutdown signal";
`plant-each.sh --apply`: 0 stale. `cargo doc` of both crates with
`-D warnings`, `check-size.py`, `check-windows.sh` and `check-macos.sh`:
0 faults.

# T-610: `pod-guest`: `Fat16::put` writes a long name of up to 32 characters with VFAT entries and an 8.3 alias (PodUtils/podvm#360)

**Source:** PodUtils/podvm#360 (COORD-109 of podvm; PodUtils/podvm@T-1866).
**Category:** feature
**Milestone:** M7
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

Windows Setup reads `autounattend.xml` from the root of a USB medium. `Fat16::put` of `pod-guest` takes 8.3 names only, so podvm cannot put the answer file on a FAT medium that the crate builds.

## Premise

- Read: PodUtils/podvm#360; `crates/pod-guest/src/fat16.rs`; the VFAT long-name format (Microsoft FAT specification, "FAT Long Directory Entries").

## Approach

`Fat16::put` takes a long name of up to 32 characters in the root: the long-name entries (the last part first with the bit `0x40`, the sequence number, the checksum of the 8.3 alias, 13 UCS-2 units in each entry) before an 8.3 alias such as `AUTOUN~1.XML` with a number that no other file has. `get` and `list` read the long name back; an 8.3 name is written as before. Tests: the checksum against a known value, a read back, two long names with one alias stem, a name of 32 characters; a plant.

## Prove

```sh
cargo test -p pod-guest --all-features
```

## Done

2026-11-14, commit 69c596b. `Fat16::put` takes a long name of up to 32
characters (`MAX_LONG_NAME`): VFAT long-name entries before an 8.3 alias
`BASIS~N.EXT` with the first free `N`; `get`, `remove` and `list` read the
long name, the alias and the 8.3 name; `fat16::checksum` is public. The
aliases and the checksums are the ones of mtools 4.0.43, and mtools
(`mdir`, `mtype`) and `fsck.fat` read an image of the crate with its long
names. A finding fixed in place: a file that did not fit kept its
clusters. Calls C464 to C467.

`cargo test -p pod-guest --all-features`: 123 unit tests passed at the
commit (4 new: the checksum against mtools, a read back, two long names
of one stem with an 8.3 name in the way, a name of 32 characters and a
chain with another checksum). Plants:
`guest-fat16-long-name-checksum-rotates-left`,
`guest-fat16-alias-collides`, each caught by hand.

# T-617: `pod-guest`: an NTFS and hive writer for an offline setup of a Windows guest

**Source:** T-595 (its Approach; PodUtils/podvm#350, item 6).
**Category:** feature
**Milestone:** M7
**Priority:** P3
**Effort:** M
**Status:** blocked

## Problem

The provision of a Windows guest types the installer into a ready guest (T-595). An offline setup writes the answer and the first commands into the image before it boots: an NTFS writer and a writer of a registry hive. podvm measures first whether `AutoRun` fires on the x64 image; without it, an offline setup gains nothing.

## Premise

- Read: PodUtils/podvm#350, item 6; the Done section of T-595 and calls C451 to C454.

## Approach

When podvm's measure says that `AutoRun` fires: a writer of NTFS that adds a file to an image, and a writer of a registry hive that sets a value, each as a module of `pod-guest` or a crate of its own (a call). Tests against images that real tools made, and plants.

## Prove

```sh
cargo test -p pod-guest --all-features
```

## Blocker

podvm measures whether `AutoRun` fires on the x64 image (PodUtils/podvm#350, item 6).

# T-618: `pod-hostprobe`: the signal calls on mips and mips64

**Source:** T-616 (a finding of its work).
**Category:** defect
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`crates/pod-hostprobe/src/sys.rs` gives `SIG_BLOCK = 0`, `SIG_SETMASK = 2` and a mask of 8 bytes for `rt_sigprocmask` and `signalfd4`, the values of most Linux architectures. mips and mips64 use `SIG_BLOCK = 1`, `SIG_SETMASK = 3` and a mask of 16 bytes, so the kernel refuses these calls there with `EINVAL`. mips is not a target of a consumer (`docs/design.md`, section 7), so nothing fails today.

## Premise

- Read: the report of T-616 and call C486; `crates/pod-hostprobe/src/sys.rs` and `src/sys/sigmask.rs`.

## Approach

Give the values for each architecture (or take them from `linux_raw_sys`), and pass a mask of the size of the kernel's mask to `sigblock_shutdown`, `sigrestore`, `signalfd_shutdown` and `sigblock`. Add a compile check for mips to `scripts/agent/`, or record mips as a target with no signal calls.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
```

## Done

2026-11-14, commit 4f94f04. The signal calls keep signals 1 to 64 in a
`u64` and give the kernel a `kernel_sigset_t` of `linux_raw_sys`, word by
word (`to_kernel`, `from_kernel`), with its size (`SIGSET_BYTES`: 16 on
mips); `sigblock`, `sigrestore`, `sigblock_shutdown`, `signalfd_shutdown`
and the new `signalfd` use it. `SIG_BLOCK`, `SIG_SETMASK`, `SIGCHLD`
(18 on mips, 20 on sparc), `SIGINT`, `SIGTERM`, `SIGHUP`, `SIGQUIT` and
`SFD_CLOEXEC` come from `linux_raw_sys` (C511). A 32-bit big-endian
target also gets signal 1 in the first word now.

No compile check of mips goes into `scripts/agent/`: `pod-hostprobe`
refuses mips with `compile_error!` (no syscall trap, no ELF machine, no
`AUDIT_ARCH`), so mips is a target with no system calls. The module
`sys/sigmask.rs` alone passed `cargo clippy -Zbuild-std=core -- -D
warnings` (with `RUSTC_BOOTSTRAP=1` and `rust-src`) for
`mips64-unknown-linux-gnuabi64` and `mips-unknown-linux-gnu`, with a const
assertion of 1, 3 and 16 bytes; a wrong assertion failed there.
`pod-hostprobe` passed clippy `-D warnings` for `i686-unknown-linux-musl`,
`powerpc64-unknown-linux-gnu` and `s390x-unknown-linux-gnu`. Findings
fixed in place: the traps of powerpc64 and s390x had no Safety section
(clippy failed there), the doc comment on the `compile_error!` of mips
warned, and the test of `sigblock` read a mask of 128 signals as one
`u64`.

`cargo test -p pod-hostprobe --all-features`: 137 unit tests passed (new:
`the_kernel_mask_has_the_kernels_size_and_keeps_signals_1_to_64`); the
tests of `pod-supervise` and `pod-enter` passed. Plants, each caught by
hand: `hostprobe-t618-mask-not-the-kernels-size`,
`hostprobe-t618-mask-words-out-of-order`.

# T-619: `pod-hostprobe`: the open flags and the errno numbers of mips and sparc

**Source:** T-618 (a finding of its work).
**Category:** defect
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`crates/pod-hostprobe/src/sys.rs` fixes `O_CREAT`, `O_EXCL`, `O_NOCTTY`, `O_TRUNC` and the errno numbers above 34 (`ENAMETOOLONG` 36, `ENOSYS` 38, `ELOOP` 40) at the values of most architectures. mips uses other values (`ENOSYS` 89, `ELOOP` 90), and sparc other open flags. The crate refuses mips at compile time, so nothing fails today; the values are wrong the day that refusal goes.

## Premise

- Read: the report of T-618 and call C511; `crates/pod-hostprobe/src/sys.rs`.

## Approach

Take the open flags and the errno numbers from `linux_raw_sys::general` and `linux_raw_sys::errno`, as T-618 does for the signal values, and add a constant assertion for mips as T-618's Done section shows.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
```

## Done

2026-11-14, commit 8634ee5. The errno numbers, `Errno::name` and the open
flags (`O_RDONLY` to `O_NONBLOCK`) come from `linux_raw_sys::errno` and
`linux_raw_sys::general` in the new module `sys/numbers.rs`; the `cfg`
tables of `O_DIRECTORY`, `O_NOFOLLOW` and `O_NONBLOCK` go. Found and fixed
in place: `Errno::name` had no `ECHILD`. A `const` assertion checks the
values of mips and sparc. The module alone passed `cargo clippy
-Zbuild-std -- -D warnings` (with `RUSTC_BOOTSTRAP=1` and `rust-src`) for
`mips64-unknown-linux-gnuabi64`, `mips-unknown-linux-gnu`,
`sparc64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu`; a wrong
assertion failed for mips64 and sparc64. Call C521.

`cargo test -p pod-hostprobe --all-features`: 138 unit tests passed (new:
`the_creation_flags_and_the_errno_numbers_are_the_kernels`; the test of
the open flags now compares with `ENOTDIR` and `ELOOP`); the crates that
use `pod-hostprobe` passed `cargo check --all-targets`. Plants, each
caught by hand: `hostprobe-t619-o-excl-of-another-target`,
`hostprobe-t619-errno-of-another-target`.

# T-620: `pod-udf`: a live test against a Windows ISO by Range requests

**Source:** T-601 (a finding of its work).
**Category:** chore
**Milestone:** M7
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

No tool here makes a UDF 2.50 volume with a metadata partition, so `pod-udf` has no proof against one of a real mastering tool. A Windows ISO has one.

## Premise

- Read: the report of T-601 and call C512; `crates/pod-udf`; the service `microsoft-windows-eval` of `pod-services` (T-611); `.github/workflows/live.yml`.

## Approach

An ignored test, run in `live.yml`, that runs `Udf::find("/sources/boot.wim")` on the Windows evaluation ISO of `microsoft-windows-eval` through HTTP Range requests, and checks the size and a digest of the first and the last block.

## Prove

```sh
cargo test -p pod-udf --all-features -- --ignored
```

## Done

2026-11-14, commit `f437f85`. `tests/windows_live.rs` (ignored): a
`ReadAt` of the test sends one HTTP Range request for each read, with the
blocking client of `pod-proxy`; the URL and the size come from the service
`microsoft-windows-eval`. `Udf::find("/sources/boot.wim")` gives one part
of 547,572,640 bytes at bytes 20,668,416 of the ISO; the test checks the
size, the WIM magic, and the SHA-256 of the first and the last block
(checked against two Range reads of curl). 15 requests and 38 KB of the
5 GB image; the test passed here in 1.1 s. The step "pod-udf, boot.wim of
the Windows evaluation ISO by Range requests" of `live.yml`, the row L42
of `docs/live-calls.md`, call C541. The ISO is UDF 1.02 (revision 0x0102)
with no metadata partition: the test proves the reader on a Windows ISO of
a real mastering tool, not the metadata partition. The plant is by hand
(`plants.yml` reaches no host of another party): a shift of each data
extent by one block fails the test.

# T-622: `pod-guest`: the provision stops the emulator on each error after the spawn (PodUtils/podvm#370)

**Source:** PodUtils/podvm#370 (COORD-115 of podvm; PodUtils/podvm@T-1893).
**Category:** defect
**Milestone:** M7
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`provision` (`crates/pod-guest/src/provision.rs`) stops the emulator when `ready::wait` fails, but not when `type_into_console` fails: the `?` returns the error and drops the child with QEMU still running. podvm measured it: a second QMP client on the socket made the typing fail (`qmp read: Resource temporarily unavailable`), and `qemu-system-x86_64` still ran after podvm ended. A QMP error of any cause leaves the same emulator. podvm's rule is that no emulator stays after a run.

## Premise

- Read: PodUtils/podvm#370; `crates/pod-guest/src/provision.rs`, `emulator.rs` (`stop_emulator`), `ready.rs` and its tests (the QMP stand-in).

## Approach

A guard that owns the child after the spawn and stops the emulator on drop (`stop_emulator`), so that each error path stops it; the success path takes the child out of the guard. Check the other functions of the crate that spawn an emulator (`run`, the warm start of T-596) for the same fault. A test: a QMP stand-in that fails the `send-key`, and no emulator process is left; a plant.

## Prove

```sh
cargo test -p pod-guest --all-features
```

## Done

2026-11-14, commit 3a4e26c. `emulator::Running` holds a spawned emulator
and stops it on drop (`stop_emulator`), unless it exited and was reaped.
`provision`, `run` and the warm restore use it; the warm restore takes the
child out on success. Found and fixed in place: a failed `try_wait` in
the wait loops of `provision` and `run` also left the emulator running.
`dos::run` stopped the emulator on each failure already. Call C522.

`cargo test -p pod-guest --all-features`: 148 unit tests passed (new:
`provision::tests::a_provision_whose_keys_fail_leaves_no_emulator`, a
stand-in emulator and a QMP stand-in that refuses each `send-key`). Plant,
caught by hand: `guest-t622-guard-leaves-the-emulator`.

# T-625: `pod-hostprobe`: the test of the report runs the probes in a child that libtest refuses

**Source:** T-619 (a finding of its work).
**Category:** chore
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`the_findings_of_this_host_read_back_to_the_same_text` (`crates/pod-hostprobe/src/report/tests.rs`) runs the real probes inside the libtest binary. Each child of a probe runs that binary again with `--hostprobe-child`, which libtest refuses, so each run prints about 20 lines of "error: Unrecognized option: 'hostprobe-child'". The test passes, as it checks only that the report reads back to the same text, but the lines look like failures in a log, and the findings measure a refused child, not the host.

## Premise

- Read: the report of T-619; `crates/pod-hostprobe/src/report/tests.rs`; `crates/pod-hostprobe/tests/children.rs` (`harness = false`).

## Approach

Move the test to an integration test with `harness = false` that takes the child flag first, as `tests/children.rs` does, or give it a `Settings` whose children run a binary that takes the flag. The test then reads the findings of the host and prints no refusal.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
```

## Done

2026-11-14, commit a06f4ca. No unit test runs the probes (call C532). `the_findings_of_this_host_read_back_to_the_same_text` (`src/report/tests.rs`) is removed, and so is `every_child_probe_runs_and_answers_in_a_child` (`src/lib.rs`), which had the same fault and printed most of the refusals. `tests/children.rs` (`harness = false`, the child flag first) takes their checks: with the feature `report`, the report of this host reads back with the same count of rows, the same rung and the same text.

- `cargo test -p pod-hostprobe --all-features`: all passed, with no line "Unrecognized option" (62 before). `tests/children.rs`: 66 rows, 56 ok, 9 denied, 1 skipped; the report of 7215 bytes reads back to the same text.
- Plant `hostprobe-t625-rung-not-read-back` (the reader gives another rung), caught by hand: `tests/children.rs` fails with "the rung of this host does not read back".
- clippy, rustfmt and `cargo doc` of `pod-hostprobe` are clean; its README says that only `tests/children.rs` runs the probes and names the plant.

# T-626: `pod-udf`: the metadata partition of UDF 2.50 against an image of a real tool

**Source:** T-620 (a finding of its work).
**Category:** chore
**Milestone:** M7
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

The Windows evaluation ISO of the live test (T-620) is UDF 1.02 with no metadata partition. The tests of T-601 use images of genisoimage and mkudffs, and neither makes a metadata partition. So the read of the metadata partition of UDF 2.50 and later has proof against images that `pod-udf` writes for its own tests only.

## Premise

- Read: the Done section of T-620 and call C541; `crates/pod-udf/tests/windows_live.rs`; `crates/pod-udf/src`; `docs/live-calls.md` (row L42).

## Approach

Find a public image of a real mastering tool with a UDF 2.50 or later volume and a metadata partition (for example a Blu-ray or a newer Windows ISO). Measure its revision with a first Range read. Add it to `tests/windows_live.rs` with its own pins (the URL from a service of `pod-services`, the size, the digests of the first and the last block), a step of `live.yml` and a row of `docs/live-calls.md`. When no such image is public, record the search and the hosts in the Done section and keep the entry open.

## Prove

```sh
cargo test -p pod-udf --all-features -- --ignored
```

# T-628: `pod-guest`: the warm restore gives the next command on a new mailbox medium after the load

**Source:** T-602 (a finding of its work).
**Category:** defect
**Milestone:** M7
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The warm start (T-596) puts the next command on the saved mailbox before the restore. The restored RAM holds the cache of FastFAT for that volume, so the agent reads the cached directory and does not see the new `WQGO.TXT` (call C543). The next write of the guest can also put its cached sectors over the files of the host.

## Premise

- Read: the Done section of T-602 and calls C542 to C544; `crates/pod-guest/src/warm.rs`; `crates/pod-guest/src/swap.rs` (`Swap::next`).

## Approach

With a USB mailbox (`MailboxBus::Usb`), restore with the saved mailbox unchanged, then give the next command with `Swap::next` after the load: a new medium with a new volume id. A warm start with an NVMe mailbox refuses a next command, or boots cold, as the call says. A test with the QMP stand-in and a plant; the proof on a real guest goes into `guest-windows.yml`.

## Prove

```sh
cargo test -p pod-guest --all-features
```

## Done

2026-11-14, commit 0c7036d. `warm::restore` loads the state with the saved mailbox as it is and, after `cont`, gives the command on a new medium with the next volume id (`swap::Swap::restored`, then `Swap::next`, by `id`); a refusal of the swap is `Cold::Failed`. `restore(&s, qemu_img, &plan, agent, &request, &warm)` gives `(Child, Swap)`; `start` gives `Started { child, how, swap }` (a cold boot gives `Swap::new(&plan, token)`). The save reads the medium in the mailbox device from `query-block` (`swap::mailbox_file`) after `stop`: after a swap it is `MAILBOX.swap`. `save`, `restore` and `start` refuse a mailbox that is not `MailboxBus::Usb` with `Usage` and send no request: the restored guest reads the saved mailbox from its cache (call C584).

Measured on the QEMU 8.2.2 of this host (call C585): the root disk bus decides. With the root disk on NVMe (the plan line) `query-migrate` blocks the save ("non-migratable device: .../nvme"); with the root disk on AHCI or virtio-blk the line with the USB mailbox (`qemu-xhci`, `usb-bot`, `scsi-hd`) saves, loads, and takes `blockdev-change-medium` by `id` after the load. So the plan saves only from QEMU 11 (PodUtils/podvm@T-1874); the USB mailbox does not block a save.

`cargo test -p pod-guest --all-features`: 164 unit tests passed, 1 ignored. New: `warm::tests::start_tests::a_warm_start_needs_a_usb_mailbox`. Changed: `a_state_that_loads_starts_warm_with_the_next_command` (the saved mailbox as it is, then the swap after `cont`, the next volume id; it fails on the code before the fix), `a_save_writes_each_file_and_lets_the_guest_go_on` (the medium in the guest), `a_failed_save_keeps_the_state_before_and_the_guest_goes_on` (no medium), `warm::live::a_real_emulator_saves_and_loads_a_stream` (the plan line with its root disk on AHCI, the QEMU 8.2.2 of the host: save, load, swap). Plants, each caught by hand: `guest-warm-restore-writes-the-saved-mailbox`, `guest-warm-save-copies-the-mailbox-of-the-plan`, `guest-warm-nvme-mailbox-restores`; `plant-each.sh --apply`: 0 stale.

The live proof (call C586): `tests/windows_keep_live.rs` saves the idle guest after the two swaps and, after the USB control, restores it with a fourth command that must give `Start::Warm` and its outcome on the new medium. The runners of `guest-windows.yml` have QEMU 8.2.2, so the step prints the refusal there and runs where the QEMU of the runner saves the plan. The parent dispatches `guest-windows.yml` after the merge; the same run proves the `usb-bot` mailbox of T-632.

The live step (GitHub run 43658174746, 2026-11-15, at 94b5ce3): "warm
start: not run, the emulator cannot save the plan: ... non-migratable
device: 0000:00:02.0/nvme". The QEMU 8.2.2 of the runner refuses the
NVMe root disk of the plan, as call C585 measured; T-635 gives the bus
of the root disk.

# T-629: `pod-guest`: a plan with a boot medium gives the root disk no `bootindex`, so OVMF deletes the boot entry of the installed system

**Source:** the first run of `guest-windows.yml` (GitHub run 43634720450, job 132568898323), a finding of T-602.
**Category:** defect
**Milestone:** M7
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The first run of `.github/workflows/guest-windows.yml` failed. Windows Setup ran (the first screenshot shows "Getting files ready for installation (26%)"). After the first reboot of Setup, the firmware showed the UEFI Interactive Shell (EDK II, Ubuntu's OVMF 4M) until the limit of 4800 s; each screenshot from the third on is the shell. The map of the shell shows the installed NVMe disk with its GPT and its ESP (`FS2: NVMe(0x1,...)/HD(1,GPT,...)`), so the install wrote the disk.

The test `crates/pod-guest/tests/windows_keep_live.rs` (`install`) gives the first CD-ROM `bootindex=0` and the NVMe root disk no boot index. The crate does the same: `Medium::boot` gives `bootindex=0` (`crates/pod-guest/src/plan/options.rs`), and `argv` gives the root disk `nvme,drive=root,serial=wqroot` with no boot index (`crates/pod-guest/src/plan.rs`).

The cause to verify: with a `bootorder` from the fw_cfg of QEMU, OVMF (`SetBootOrderFromQemu` of `QemuBootOrderLib`) keeps the boot options that the list matches and removes the others. The entry of Windows Boot Manager on the NVMe disk matches no device of the list, so the firmware removes it; the "Press any key" prompt of the CD times out, and the shell runs.

## Premise

- Read: `crates/pod-guest/src/plan.rs`, `src/plan/options.rs` and its tests, `tests/windows_keep_live.rs`, the README of the crate (the uses of `Medium::boot`: none in the crate outside its tests; podvm sets it).
- Read: the source of OVMF on github.com/tianocore/edk2 (`OvmfPkg/Library/QemuBootOrderLib/QemuBootOrderLib.c`, `OvmfPkg/Library/PlatformBootManagerLib/BdsPlatform.c`) and of QEMU 8.2.2 (`hw/nvme/ctrl.c`, `hw/nvme/ns.c`).
- Not measured: a Windows install. This host has no KVM, and a Windows install under TCG is too slow for a session. The live proof stays with T-602.

## Approach

Verify the cause in the source of OVMF and on this host (call C574). When a plan has a medium that boots first, give the root disk `bootindex=1` (call C575: the medium first, the root disk next). A unit test of the argv, and a plant. The live test gives its root disk the same boot index.

## Prove

```sh
cargo test -p pod-guest --all-features --lib -- plan::
sh scripts/plant-each.sh guest-boot-medium-drops-the-root-disk
```

## Done

2026-11-14, commit 5950498. The cause holds (call C574): `SetBootOrderFromQemu` of OVMF keeps only the boot options that the boot order of QEMU names, and `PruneBootVariables` deletes the others, also the entry of an installed system on a root disk with no boot index. `argv` now gives the root disk `bootindex=1` when a medium boots first, and no boot index otherwise (call C575: the medium first, the root disk next). The live test of the Windows install (`install` in `tests/windows_keep_live.rs`) gives its root disk `bootindex=1`; its `boot` has no medium and uses `argv`.

Run on this host (QEMU 8.2.2, TCG, `OVMF_CODE_4M.fd` of Ubuntu's `ovmf` 2024.02-2ubuntu0.10): a GPT disk whose ESP holds the UEFI shell at `\EFI\pod\app.efi`, which only a `Boot####` entry names (`bcfg boot add` in a first boot, as setup writes its entry), and a CD with `bootindex=0` whose loader returns `EFI_TIMEOUT`, as the prompt of a Windows CD. With no boot index on the disk, the firmware starts the EFI Internal Shell after the CD; a later boot with the fixed line finds the entry deleted. With `bootindex=1` on the disk, the firmware starts the entry after the CD.

`cargo test -p pod-guest --all-features`: 158 unit tests passed, 1 ignored (new: `plan::options::tests::a_medium_that_boots_first_keeps_the_root_disk_in_the_boot_order`, which fails on the code before the fix). Plant, caught by hand: `guest-boot-medium-drops-the-root-disk`. The live proof stays with T-602: the parent dispatches `guest-windows.yml` again after the merge.

# T-630: `pod-guest`: the swap of a USB mailbox fails while Windows locks the medium

**Source:** the second run of `guest-windows.yml` (GitHub run 43647025370, job 132606998403, at ae04e9e), a finding of T-602.
**Category:** defect
**Milestone:** M7
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The second run of `.github/workflows/guest-windows.yml` installed Windows in 190 s and booted it with a USB mailbox. The first command passed (`swap: first command: code 3, stdout "pod-guest-one\r\n"`). Then `Swap::next` failed at `crates/pod-guest/tests/windows_keep_live.rs:294`:

```text
qmp refused {"execute":"blockdev-change-medium","arguments":{"device":"mbox","filename":".../swap/mailbox.img.swap","format":"raw"}}: {"error": {"class": "GenericError", "desc": "Device 'mbox' is locked and force was not specified, wait for tray to open and try again"}}
```

Windows sends PREVENT ALLOW MEDIUM REMOVAL to the mounted removable disk, so the tray of the `usb-storage,removable=on` device is locked. The QEMU test of T-602 (`a_real_emulator_changes_the_medium_of_a_usb_mailbox`) passed because no guest locked the medium.

## Premise

- Read: `crates/pod-guest/src/swap.rs` and its tests, `src/qmp.rs`, `tests/windows_keep_live.rs`, calls C542 to C544.
- Read: the source of QEMU 8.2.0 (`block/qapi-sysemu.c`: `do_open_tray`, `qmp_blockdev_open_tray`, `qmp_blockdev_close_tray`, `qmp_blockdev_change_medium`; `qapi/block.json`; `hw/scsi/scsi-disk.c`: `scsi_cd_eject_request_cb`, `scsi_cd_change_media_cb`, `scsi_disk_unit_attention_reported`, `ALLOW_MEDIUM_REMOVAL`; `hw/ide/atapi.c` and `hw/ide/core.c` for the ATAPI drive of the test).
- Measured on the QEMU 8.2.2 of this host: a guest that locks the medium of an ATAPI drive. The binary of Ubuntu has no `qtest` accelerator (`-accel qtest`: "invalid accelerator"), so the host cannot play the guest.
- Not measured: Windows. This host has no KVM; the live proof is the next run of `guest-windows.yml`.

## Approach

Swap a locked medium with `blockdev-change-medium` and `force: true` (call C581). Test it against a QMP stand-in that refuses a change with no `force` while the guest locks the tray, and against the QEMU of the host with a small guest that locks the tray (call C582). The live test prints the lock before each swap. Plants for the request and for the read of the lock.

## Prove

```sh
cargo test -p pod-guest --all-features --lib -- swap:: qmp::
sh scripts/plant-each.sh guest-swap-no-force guest-swap-lock-of-another-drive
```

## Done

2026-11-14, commit 159827f. `swap::change_request` sends `blockdev-change-medium` with `force: true` (call C581): QEMU 8.2 opens a locked tray only with `force`, clears the lock of the guest (`scsi_cd_eject_request_cb`), and reports the medium change as for an unlocked medium, so the path of C543 holds. New: `swap::mailbox_locked(query_block)`, `qmp::json_bool`, `qmp::list_entries` (QEMU writes the members of an entry in no fixed order). The live test prints the lock of the mailbox before each swap; its controls write no QMP command and need no change.

On this host (QEMU 8.2.2, TCG): a guest of 93 bytes at the reset vector of the `pc` machine locks an ATAPI drive `mbox` (call C582). No QMP command locks a tray, and the binary of Ubuntu has no `qtest` accelerator, so a guest is necessary. `query-block` gives `"locked": true`, a change with no `force` gets the error of the run on Windows, and two swaps with `Swap::next` put the new file in; the guest locks each new medium again after it took the reports of the change.

`cargo test -p pod-guest --all-features`: 163 unit tests passed, 1 ignored (new: `swap::tests::a_swap_changes_a_medium_that_the_guest_locked`, `swap::tests::the_lock_of_the_mailbox_is_read_from_its_entry_of_query_block`, `swap::tests::a_real_emulator_changes_a_medium_that_the_guest_locked`, `qmp::tests::a_bool_member_is_read_and_a_key_in_a_value_is_not_a_member`, `qmp::tests::each_object_of_a_list_response_is_one_entry`). Plants, each caught by hand: `guest-swap-no-force` (the stand-in test, the real-QEMU test and the request test fail), `guest-swap-lock-of-another-drive`. The live proof is the next run of `guest-windows.yml`: the parent dispatches it after the merge.

# T-632: `pod-guest`: the QMP commands of the mailbox name the drive by `id`, not by the deprecated `device`

**Source:** T-630 (a finding of its work).
**Category:** chore
**Milestone:** M7
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`swap::change_request` and the other QMP commands of the mailbox name the drive with the argument `device`. QEMU deprecated `device` for these commands in favour of `id` (the id of the device) in version 2.8. QEMU 8.2 and the current master still take it; a later version can remove it, and the swap then fails.

## Premise

- Read: the Done section of T-630 and calls C581, C582; `crates/pod-guest/src/swap.rs`; `crates/pod-guest/src/plan/options.rs` (the `usb-storage` device of the mailbox has no `id`); QEMU's `qapi/block.json` (`blockdev-change-medium`, `blockdev-open-tray`).

## Approach

Give the `usb-storage` device of the mailbox an id (`id=wqmbox`), and name it with `"id"` in each QMP command of the mailbox. The tests of the argv, the stand-in and the test on the QEMU of the host follow. A plant that sends `device` to a stand-in that refuses it.

## Prove

```sh
cargo test -p pod-guest --all-features
```

## Done

2026-11-14, commit ed53a06. `swap::change_request` names the device of the mailbox by `id` (`plan::MAILBOX_ID`, `wqmbox`), and `swap::mailbox_locked` reads the entry of `query-block` whose `qdev` is that id (call C583). An id on `usb-storage` is not enough: `usb-storage` gives its medium to a SCSI disk that it makes with no id, and QEMU 8.2.2 answers `id` `wqmbox` with "Device does not have a block device backend". The USB mailbox is now `usb-bot` (`plan::MAILBOX_USB`, `wqmboxusb`) with `scsi-hd,...,id=wqmbox`: the disk that `usb-storage` makes, in the same USB class. The NVMe mailbox keeps its line: no command names it.

`cargo test -p pod-guest --all-features --lib`: 163 passed, 1 ignored. The stand-in of a locked drive refuses `device` as QEMU with `-compat deprecated-input=reject` does; `a_real_emulator_changes_the_medium_of_a_usb_mailbox` and `a_real_emulator_changes_a_medium_that_the_guest_locked` run the QEMU 8.2.2 of the host with that option. Plant `guest-swap-names-the-drive-by-device`, caught by hand (five tests of `swap` fail on it); the patches of `guest-swap-no-force` and `guest-swap-lock-of-another-drive` follow the new code and are caught again. The device of the mailbox changed, so the live proof on Windows is the next run of `guest-windows.yml`: the parent dispatches it after the merge.

The live proof: `guest-windows.yml` (GitHub run 43658174746, 2026-11-15, at
94b5ce3) passed on Windows Server 2019 under KVM with the `usb-bot` and
`scsi-hd` mailbox: "the swap gave two next commands to the agent, and
both outcomes are right"; a write into a mounted USB or NVMe mailbox did
not reach the agent.

# T-633: `pod-guest`: the screen sign of the readiness fails on Validation OS, so the provision never types into a Windows guest

**Source:** PodUtils/podvm#372 (COORD-116 of podvm; PodUtils/podvm@T-1894).
**Category:** defect
**Milestone:** M7
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podvm reports a regression of `v0.45.0` (T-595): the provision of a Windows guest never types the installer. `kvm.yml` run 43171009630 (podcrates before `v0.45.0`, a fixed wait) provisions Validation OS 26100.9278 under KVM. Run 43619241341 (`v0.45.0`) fails on the same image, and on 26100.9818, with `Ready { signs: [Sign::Screen { settle: 15 s }], deadline: 600 s, every: 500 ms }`:

```text
the guest was not ready within 600s: no a screen that changed and then stayed the same for 15s
```

Under KVM the console of Validation OS has a cursor that blinks, so the sha256 of a dump never stays the same. Under TCG (settle 5 s, then 60 s) the wait ends early, on a black screen with only the pointer (dumps at about 3 and 4 minutes), the keys go nowhere, and the setup ends with `the installer wrote no SETUP.TXT`. The ask: a sign that holds after a fixed time from the spawn (`Sign::After`; this alone unblocks podvm), a screen sign that ignores a cursor and must differ from the first dump, tests with the QMP stand-in. The text of a failed wait reads badly ("no a screen").

## Premise

- Read: PodUtils/podvm#372; the Done section of T-595 and calls C451 to C454; `crates/pod-guest/src/ready.rs`, `crates/pod-guest/src/ready/tests.rs`, `crates/pod-guest/src/provision.rs`; the caller of podvm (`podvm-windows`: the provision before `v0.45.0` slept its `wait`, `--timeout` with 240 s by default).
- Not measured: a Windows guest. This host has no KVM; the live proof is the next Windows provision of podvm.

## Approach

`Sign::After(time)`: it holds when `time` passed since the start of `wait` (call C591). The screen sign reads each dump as a P6 PPM, counts a change whose pixels fit in a box of 32x32 as no change, and compares each dump to the dump of the last change (call C592). The settled screen must differ from the first dump and must not be blank (call C593). A new text of `Unready::Deadline` (call C594). Tests with the QMP stand-in and PPM frames; a plant for each test.

## Prove

```sh
cargo test -p pod-guest --all-features --lib -- ready::
sh scripts/plant-each.sh guest-ready-a-cursor-is-a-change guest-ready-a-blank-screen-is-ready guest-ready-the-first-screen-again-is-ready guest-ready-after-holds-at-once guest-ready-a-still-screen-is-ready
```

## Done

2026-11-14, commit 5508bfe; the plants and this record in the next commit. `ready::Sign::After(time)` holds when `time` passed since the start of `ready::wait`; the provision starts the wait right after the spawn (call C591). `Sign::Screen { settle }` keeps its shape and changes its meaning for each caller: each dump is a P6 PPM (`ready/screen.rs`), a change whose pixels fit in a box of `ready::CURSOR` (32x32) is no change, each dump is compared to the dump of the last change, and the settled screen must differ from the first dump and must not be blank (calls C592, C593). The text of a failed wait is "the guest was not ready within 600s; no sign held: SIGN; SIGN" (call C594).

What podvm passes, under KVM and under TCG: `Ready { signs: vec![Sign::After(t)], deadline, every: ready::EVERY }`, with `t` the fixed wait of the provision before `v0.45.0` (its `--timeout`, 240 s by default; the wait of run 43171009630) and a `deadline` larger than `t`: a time past the deadline never holds. Under KVM podvm can put the screen first, `vec![Sign::Screen { settle: 15 s }, Sign::After(t)]`: the console with its cursor now settles, and `After` bounds the wait. Under TCG `After` alone, until a run of podvm shows the screen sign after the black boot phase.

`cargo test -p pod-guest --all-features`: 167 unit tests passed, 1 ignored (new: `ready::tests::a_screen_with_a_cursor_that_blinks_settles`, `ready::tests::a_black_screen_or_the_first_screen_again_is_not_ready`, `ready::tests::after_holds_at_its_time_and_not_before`, `ready::tests::a_dump_is_read_as_a_ppm_and_compared_with_a_tolerance_for_a_cursor`); 20 runs of the tests of `ready` in a loop. The frames of the QMP stand-in are PPM images of 640x480. Plants, each caught by hand: `guest-ready-a-cursor-is-a-change` (the exact comparison of before: the cursor test meets the deadline), `guest-ready-a-blank-screen-is-ready` (the case of a pointer that moved on a black screen holds), `guest-ready-the-first-screen-again-is-ready`, `guest-ready-after-holds-at-once`; `guest-ready-a-still-screen-is-ready` of T-595 moves to `ready/screen.rs` and is caught. `plant-each.sh --apply`: 0 stale. The live proof is the next Windows provision of podvm.

# T-635: `pod-guest`: a setting of the bus of the root disk, so that a warm start runs on QEMU 8.2

**Source:** T-628 (a finding of its work).
**Category:** feature
**Milestone:** M7
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

The plan always puts the root disk on NVMe (`nvme,drive=root`). QEMU 8.2 refuses to save the state of an NVMe device ("non-migratable device"), so a warm start of the plan runs only from QEMU 11 (calls C500 to C507, C585). With the root disk on AHCI or virtio-blk, the same line with the USB mailbox saves, loads and takes the swap after the load on QEMU 8.2.2 (C585). podvm chose QEMU 11 for the warm start; the runners of GitHub have QEMU 8.2.2, so `guest-windows.yml` cannot prove a warm start today.

## Premise

- Read: the Done section of T-628 and calls C584 to C586; `crates/pod-guest/src/plan.rs`, `plan/options.rs`; `a_real_emulator_saves_and_loads_a_stream` (it runs the plan line with the root disk on AHCI).
- Windows needs the virtio driver for virtio-blk; AHCI needs none. A change of the bus needs a new install or image.

## Approach

`Options::root_bus` (`Nvme`, the default; `Ahci`; `VirtioBlk`) with its argv and the `bootindex` of T-629 on each bus. A test of the argv for each bus and a plant. The live step of the warm start in `guest-windows.yml` installs with `Ahci` and proves the warm restore of T-628 on QEMU 8.2. Tell podvm on the hand-off; podvm chooses its bus.

## Prove

```sh
cargo test -p pod-guest --all-features
```

# T-640: `pod-guest`: a cold boot of a Windows guest under KVM reaches no shutdown on one run of two, and a run that times out keeps no trace

**Source:** PodUtils/podvm#380 (COORD-120 of podvm; PodUtils/podvm@T-1864).
**Category:** defect
**Milestone:** M7
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

In `kvm.yml` run 43660534832 of podvm, at podcrates `v0.48.0`, three `podvm windows run` cold boots of a provisioned Validation OS 26100.9818 image, each with a limit of 600 s: run 1 (`exit 42`) timed out ("the guest did not power off within 600s", stdout empty); run 2 (`cmd /c exit 42`, the same command file after the drop of `cmd /c`) gave code 42 in 50 s; run 3 (`echo out & exit 7`) timed out. One boot of two reaches no shutdown, and the cause is not the command. `run` keeps no serial log, no screen and no mailbox of a run that times out, so the cause is not visible.

## Premise

- Read: `crates/pod-guest/src/agent.rs` (`AGENT_CMD`, `SETUP_CMD`), `src/lib.rs` (`run`), `src/emulator.rs`, `src/provision.rs`, `src/fat16.rs`, `src/qmp.rs`, `tests/windows_keep_live.rs`, `.github/workflows/guest-windows.yml`, the Done sections of T-592, T-595, T-602, T-622, T-630 and T-633.
- Each run boots a new overlay of the same provisioned image, with a new mailbox written before the spawn. So the disk state of each boot is the same, and only a race of the boot can give one result of two.
- Not measured: Windows. This host has no KVM; the live step is the next run of `guest-windows.yml`.

## Approach

Read the agent for each cause of a missed command or power-off, and fix each one that is possible (calls C612, C613). Make the agent log each step on the mailbox, and make `run` say what the mailbox tells after a timeout (call C614). Make `run` keep the evidence of a run that times out in a directory of the caller: a QMP `screendump` before the stop, then the serial log, the error stream of the emulator and the mailbox (call C615). Test the agent with the walk of `AGENT_CMD` and a clock, and `run` with a stand-in emulator and a QMP stand-in. Add six cold boots of `run` to `tests/windows_keep_live.rs`.

## Prove

```sh
cargo test -p pod-guest --all-features --lib -- agent:: evidence::
sh scripts/plant-each.sh guest-t640-one-shutdown guest-t640-nap-does-not-wait guest-t640-screen-after-the-stop
```

## Done

2026-11-14, commit 381b18c; the plants and this record in the next commit. The causes of the ask, read in the agent (call C612): possible are a shutdown that Windows refuses early in the boot (the agent ended after one refusal) and a `ping` that does not wait (the 600 tries of the probe then end in a few seconds, before a slow mailbox disk comes online); not possible are another drive letter (the scan covers D to Z), a task that does not start (`onstart` as `SYSTEM`, the same overlay of the provisioned image at each boot), a shutdown that an update blocks, and the cache of C543 (a cold boot writes the mailbox before the spawn). Fixed: the agent tries `shutdown` again each second for at most 300 tries (C612), waits for two changes of the second of `%TIME%` when `ping` fails (C613), and appends each step to `WQLOG.TXT` (`agent::LOG`, C614). `run` says what the mailbox tells after a timeout and after a power-off with no result (`evidence::seen`). New: `Settings::evidence` (`None` by default) and the module `evidence` (`dir`, `seen`, `SCREEN`, `SERIAL`, `EMULATOR`, `MAILBOX`); a run that times out takes a QMP `screendump` before the stop and copies the serial log, the error stream of the emulator and the mailbox into `DIR/timeout-TOKEN`, and its text names the directory (C615); `Timeout::failure_with`.

`cargo test -p pod-guest --all-features`: 175 unit tests passed, 1 ignored (new: `agent::tests::the_probes_wait_about_one_second_each_also_when_ping_fails`, `agent::tests::a_mailbox_that_comes_online_late_is_found_with_no_ping`, `agent::tests::the_agent_tries_the_shutdown_again_until_windows_takes_it`, `evidence::tests::a_run_that_times_out_keeps_a_screen_from_before_the_stop_and_its_logs`, `evidence::tests::a_run_with_no_evidence_directory_takes_no_screen`, `evidence::tests::the_mailbox_tells_what_the_agent_did`, `evidence::tests::the_directory_of_a_token_is_one_safe_name`); 20 runs of the tests of `agent` and `evidence` in a loop. The walk of the agent has a clock, `call`, and each new line. Plants, each caught by hand: `guest-t640-one-shutdown`, `guest-t640-nap-does-not-wait`, `guest-t640-screen-after-the-stop`; `guest-agent-keep-alive-shuts-down` and `guest-agent-idle-ends-without-the-medium` of T-592 and T-602 are written again for the new agent text and caught. `plant-each.sh --apply`: 0 stale.

The live step: `tests/windows_keep_live.rs` runs six cold boots of `run` on the installed Server 2019 disk, each with its own overlay, variable store, NVMe mailbox, command and limit of 600 s, prints `RESULT: cold boots: N of 6 reached their outcome`, and fails at its end when one did not; `guest-windows.yml` uploads the evidence of each job. The image of the test has the agent of this commit, so the step proves the fix and does not run the agent of `v0.48.0`. The parent dispatches it after the merge.

The live step ran: `guest-windows.yml` run 43683270673 at `1368e49` (by a dispatch, #165) passed, with "RESULT: cold boots: 6 of 6 reached their outcome". The warm start was not run (QEMU 8.2 cannot save the NVMe root disk, T-629), and the two controls of the mounted mailbox gave "no", as before.

podvm answered the question of its runs of experiment 450 (PodUtils/podvm#413): each Windows run copies the OVMF variable store into a fresh scratch directory (its `vars_run`), and the guest writes only that copy; no state crosses from run to run.

# T-641: `pod-guest`: the installer tries its shutdown again, and `dos::run` keeps the evidence of a run that times out

**Source:** T-640 (a finding of its work).
**Category:** defect
**Milestone:** M7
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

T-640 made the agent try `shutdown` again each second, as Windows can refuse a shutdown early in a boot, and made `run` keep the evidence of a run that times out. The command of the installer (`SETUP_CMD`) still runs `shutdown` once, and `dos::run` keeps no evidence on a timeout.

## Premise

- Read: the Done section of T-640 and calls C612 to C615; `crates/pod-guest/src/agent.rs` (`SETUP_CMD`), `crates/pod-guest/src/dos.rs`, the module `evidence`.

## Approach

The loop of T-640 around the `shutdown` of `SETUP_CMD`, with a test that walks the text and a plant. `dos::run` takes `Settings::evidence` and keeps the screen and the logs of a run that times out, as `run` does, with a test of the QMP stand-in.

## Prove

```sh
cargo test -p pod-guest --all-features
```

# T-644: `pod-enter`: a form of `run_ladder` that gives the caller the child before its wait (PodUtils/podvm#384)

**Source:** PodUtils/podvm#384 (COORD-122 of podvm; PodUtils/podvm@T-1786).
**Category:** feature
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

podvm holds the record of a foreground run with `pod_supervise::foreground` (T-573): it spawns the payload, gives the pid to the hold, then waits. A run that `PODVM_MODE` forces through the ladder calls `pod_enter::run_ladder(root, plan, mode, fd, err)`, which spawns and waits inside and closes the memfd of the parent. podvm gets no pid before the end, so its record reads `Created` while the payload runs, and `ps`, `kill`, `wait` and `stop` do not see it. Not urgent: `PODVM_MODE` is a switch for measurements.

## Premise

- Read: PodUtils/podvm#384; `crates/pod-enter` (`run_ladder`, `spawn_selected`, `spawn_userland`, `Child::wait_forwarding`).

## Approach

A `spawn_ladder` that returns a `Child` (its pid and `wait_forwarding`), as `spawn_selected` does; the memfd of the parent closes after the spawn, as now. `run_ladder` becomes `spawn_ladder` and the wait. A test that reads the pid before the end of the payload, and a plant.

## Prove

```sh
cargo test -p pod-enter --all-features
```

# T-647: `pod-services`: the host `api.ghcr.pkgforge.dev` of the static QEMU of podvm's store (PodUtils/podvm#389)

**Source:** PodUtils/podvm#389 (COORD-124 of podvm; PodUtils/podvm@T-1875).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

PodUtils/podvm@T-1875 fetches a pinned static QEMU 9.2.0 (`qemu-system-x86_64`, `qemu-img`) into its store on a host with no QEMU. The source is the pkgforge `bincache` (package `ppkg.qemu`), at `https://api.ghcr.pkgforge.dev/pkgforge/bincache/qemu/ppkg/<name>?tag=9.2.0-x86_64-linux&download=<name>`. podvm pins each file by size and sha256. podvm's test `hosts_tests::each_host_that_podvm_names_is_in_pod_services` needs each host that podvm names in `pod-services`; podvm keeps the host in its list `WAITING` until a tag of podcrates lists it. The ask: add `api.ghcr.pkgforge.dev` (purpose: the static QEMU of the store; HTTPS; a GET with redirects).

## Premise

- Read: PodUtils/podvm#389; the guest images of `crates/pod-services/services` (`freedos-image.toml`).
- Not measured: the redirect target of the endpoint and the sizes of the files.

## Approach

A service file `pkgforge-qemu.toml` (kind of a download of a tool): the endpoint of each file at the tag `9.2.0-x86_64-linux`, the size and the sha256 that podvm pins as facts with podvm's commit as the source, and an `http-sha256` watch. The tests of `pod-services` read it.

## Prove

```sh
cargo test -p pod-services --all-features
```

## Done

2026-11-15, the commit of the change. `crates/pod-services/services/pkgforge-qemu.toml` (kind `tool`): the endpoints `qemu-system-x86-64` and `qemu-img` (a name is a word, so `_` is `-`), the version and the sha256 and the length of each file, and an `http-sha256` watch of each. A GET of each file from this session gave 42899120 and 11009488 bytes and the sha256 values that podvm pins at `25024cb3`. The test `the_static_qemu_holds_the_pins_of_podvm` of `tests/podvm.rs`; `cargo test -p pod-services --all-features` passed.


# T-648: `pod-hostprobe`: a leg that reports a delegated cgroup2 subtree with no root (PodUtils/podvm#390)

**Source:** PodUtils/podvm#390 (COORD-125 of podvm; PodUtils/podvm@T-1747; the ask V-3 of podswarm in #124; G-12 of #36).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm's `run --memory`, `--cpus` and `--pids-limit` need cgroup2 where the host delegates a subtree to the user. The ask: a leg of `pod-hostprobe` that reports, with no root: (1) whether `/sys/fs/cgroup` is cgroup2 (`statfs` magic `CGROUP2_SUPER_MAGIC`); (2) the cgroup of the process (`/proc/self/cgroup`, the line `0::PATH`); (3) the nearest directory at or above it that the user can write, whose `cgroup.subtree_control` the user can write, and the controllers that `cgroup.controllers` offers there (`memory`, `cpu`, `pids`); (4) a verdict: `delegated` with the path and the controllers, or the reason that there is none (cgroup v1, no write access, a missing controller). podvm makes the child cgroup of each run and writes the limits itself. A hosted runner of GitHub and podvm's machine tier can have a delegation; podvm's cloud host has cgroup v1.

## Premise

- Read: PodUtils/podvm#390; `crates/pod-hostprobe/src/probes.rs` and `crates/pod-hostprobe/src/verdict.rs` (the form of a leg).
- Not measured: a host with a delegation (this sandbox).

## Approach

A module `cgroup` of `pod-hostprobe`: the reads behind a root path, so that a test gives a made-up tree (cgroup v1, no write access, a missing controller, a delegation); the leg in the report with its verdict and reason; a plant that reports a delegation for a tree with no write access.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
```

## Done

2026-11-15, the commit of the change. `crates/pod-hostprobe/src/cgroup.rs`: `probe()` (the `statfs` of `/sys/fs/cgroup`, the line `0::` of `/proc/self/cgroup`, the write test of `mkdir` and `rmdir` of a child and an open of `cgroup.subtree_control` for write) and `assess` over a made-up tree; `Delegation::Delegated { path, controllers, enabled }` or `None(Reason)` with `NoMount`, `NotCgroup2`, `NoMembership`, `NotWritable`, `MissingControllers`; `words` for one line. A membership with `..` is refused, and the walk does not leave the root. Six tests in `src/cgroup/tests.rs` (`cargo test -p pod-hostprobe --all-features --lib cgroup`: 6 passed). The plant `hostprobe-cgroup-missing-controller-delegated` (a directory with no `cpu` read as delegated) fails the first test. This session (cgroup v1 or no delegation) gives a verdict of none; a host with a delegation is podvm's run.


# T-649: `pod-guest`: a FAT image writer with directories, long names and a size from the content (PodUtils/podvm#386)

**Source:** PodUtils/podvm#386 (COORD-123 of podvm; PodUtils/podvm@T-1872).
**Category:** feature
**Milestone:** M6
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

PodUtils/podvm@T-1872: `podvm windows run -v HOST:C:\path[:ro]` as a one-way copy through a USB medium (`Options::media`, `MediumKind::Usb`) with a marker file. `pod_guest::fat16` writes the mailbox only: a fixed volume of 16 MiB, files in the root, 8.3 names and one long name. The ask: a writer of a FAT image (FAT32 above the FAT16 cluster limit) behind an MBR partition, from a directory tree of the host: directories at each depth and VFAT long names (up to 255 UTF-16 units, the case kept); a size from the content with a margin, up to a ceiling of the caller (for example `RLIMIT_FSIZE`); a refusal by name of an entry that the image cannot hold (a symbolic link, a special file, a name that FAT refuses, `<>:"/\|?*`, a trailing dot or space, a file over 4 GiB - 1); the mode 0600 of the image file; a reader for the tests, as `Fat16::from_image` is. The geometry of the mailbox (a partition at LBA 2048) is a good start. Not urgent: `podvm cp` of PodUtils/podvm@T-1868 is the other road.

## Premise

- Read: PodUtils/podvm#386; `crates/pod-guest/src/fat16.rs`.
- Not measured: the mount of such an image by Windows as a removable medium.

## Approach

A module `fat` of `pod-guest` beside `fat16`: the layout of FAT16 or FAT32 from the count of clusters, the directory entries with their long names, the size from the content, the refusals by name; a reader for the tests; a test of each refusal and a round trip of a tree. podvm's run under Windows proves the mount.

## Prove

```sh
cargo test -p pod-guest --all-features
```

# T-650: `pod-qga`: `guest-get-osinfo` (PodUtils/podvm#393)

**Source:** PodUtils/podvm#393 (COORD-127 of podvm; PodUtils/podvm@T-1868).
**Category:** feature
**Milestone:** M7
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm asks for a client of the QEMU Guest Agent for a running Windows guest: the sync and `guest-ping` with a bound; `guest-exec` with the output and `guest-exec-status` to the end; the file calls in chunks; `guest-shutdown` and `guest-get-osinfo`; the channel `org.qemu.guest_agent.0` on a socket of the run. `pod-qga` (T-594, in `v0.45.0`) has each call but `guest-get-osinfo`, and `pod_guest::plan` has the channel (`QGA_CHANNEL`).

## Premise

- Read: PodUtils/podvm#393; `crates/pod-qga/src/client.rs`, `crates/pod-qga/src/wire.rs`, `crates/pod-guest/src/plan/options.rs`; the schema of `guest-get-osinfo` (`GuestOSInfo` of `qga/qapi-schema.json` of QEMU: each member optional).
- Not measured: the agent of virtio-win on Windows.

## Approach

`wire::OsInfo` with `from_return` (each member an optional text; a member that is not a text is an error) and `Client::osinfo`. A test of the wire and a test against the stand-in agent. Tell podvm that the rest of the ask is in `v0.45.0`.

## Prove

```sh
cargo test -p pod-qga --all-features
```

## Done

2026-11-15, the commit of the change. `wire::OsInfo`, `Client::osinfo`, the answer of the stand-in agent; the tests `osinfo_takes_each_member_that_is_there_and_names_a_wrong_one` and `osinfo_names_the_system_of_the_guest`. `cargo test -p pod-qga --all-features` passed (28 tests); clippy of the crate is clean.

# T-651: `pod-services`: the aarch64 netboot of Alpine 3.22.6 and the static `qemu-system-aarch64` (PodUtils/podvm#394)

**Source:** PodUtils/podvm#394 (COORD-128 of podvm; PodUtils/podvm@T-1890, PodUtils/podrunner#102).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm boots a linux/arm64 guest of the machine tier on an x86_64 host (`qemu-system-aarch64 -M virt` under TCG). The kernel of the x86_64 guest comes from `alpine-netboot`; the arm64 guest has no pin, so podvm reads `PODVM_MACHINE_KERNEL_ARM64` until a tag has one. The ask: a service of the same shape for `alpine-netboot-3.22.6-aarch64.tar.gz` (341 396 208 bytes, sha256 `0a39889547d6fb1b0dc124f4baceadd08fef7a72708d5c92774a207f2bc41728`; `boot/vmlinuz-virt` `dd37c4460f933ec909ff022c34944ff7e1a5300aff99750e3bdc02cd4074c332`; `boot/modloop-virt` `da142869626ec9d4543ab46482af10b5ff2c8c1c2e01068ddf32360316a06218`). The NIC modules can follow. A smaller item: the static `qemu-system-aarch64` 9.2.0 of pkgforge (46 519 280 bytes, sha256 `1ed86579cf74ecbe4d118b098d96d3e127beebde048b3d8e5bc907781403531b`) for podvm's store.

## Premise

- Read: PodUtils/podvm#394; `crates/pod-services/services/alpine-netboot.toml`, `crates/pod-services/services/pkgforge-qemu.toml`.
- Measured: each size and sum of the ask, from this session.
- Not measured: the release of the kernel in `boot/initramfs-virt` (this session has no `cpio`); the NIC modules.

## Approach

A service `alpine-netboot-aarch64` (kind `guest-image`) with the archive, its sums and an `http-sha256` watch; the endpoint `qemu-system-aarch64` of `pkgforge-qemu` with its sums and a watch. A test of the pins.

## Prove

```sh
cargo test -p pod-services --all-features
```

## Done

2026-11-15, the commit of the change. `crates/pod-services/services/alpine-netboot-aarch64.toml` (the archive, `release`, `vmlinuz-virt-sha256`, `modloop-virt-sha256`, `archive-sha256`, `archive-bytes`, a watch) and the endpoint `qemu-system-aarch64` of `pkgforge-qemu` (its sha256, its length, a watch). Each size and sum of the ask, measured here, is the same. The test `the_aarch64_netboot_holds_the_pins_of_podvm` and the row of `qemu-system-aarch64` in `the_static_qemu_holds_the_pins_of_podvm`; `cargo test -p pod-services --all-features` and `cargo test -p pod-services-watch --all-features` passed. The release of the kernel and the NIC modules wait for podvm's ask.

# T-669: `pod-enter`: a UTS namespace and a host name on the namespace rung (PodUtils/podvm#403)

**Source:** PodUtils/podvm#403 (COORD-130 of podvm; PodUtils/podvm@T-1908, PodUtils/podvm#402, PodUtils/podrunner#115).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm adds `run --hostname NAME`: the official runner writes the host name into the job log of github.com, so podrunner needs a unit with its own name. The namespace rung of `pod-enter` unshares `CLONE_NEWNS` and no `CLONE_NEWUTS`, so podvm refuses `--hostname` there with exit 125. The ask: a field of the plan (`Plan.hostname: Option<String>`); where it is set, `unshare(CLONE_NEWUTS)` with the mount namespace, then `sethostname(NAME)`, before the exec; a named failure step (`ns_setup_step`) for each call. The release, the version and the machine of `uname` stay the host's.

## Premise

- Read: PodUtils/podvm#403; `crates/pod-enter/src/launch.rs` (the setup of the namespace rung in the child), `crates/pod-enter/src/entry.rs` (`ns_setup_step`, the fallback to chroot), `crates/pod-enter/src/plan.rs`.
- Measured: this session can unshare a UTS namespace as root.

## Approach

`Plan::hostname` and a module `uts`: the name checked before the fork (not empty, at most 64 bytes, no NUL); in the child, after the mounts of the namespace rung, `unshare(CLONE_NEWUTS)` (step `NS_UTS_STEP`) and `sethostname` (step `NS_HOSTNAME_STEP`). A plan with a name never falls back to chroot, and each entry with no UTS namespace (the chroot rung, a join, the entry with no chroot) refuses it by name: the payload never sees the host's name in place of its own. Unit tests of the checks and the steps; a live test with a static busybox (`uname -n`, `/proc/sys/kernel/hostname`, the host's name unchanged); a plant.

## Prove

```sh
cargo test -p pod-enter --all-features
```

## Correction

2026-11-15: the run of `ci` 43708818874 at `2f23632` failed on `a_plan_with_no_name_keeps_the_host_name` of `tests/hostname.rs`: as a user with no root, the rung fell back to chroot, and the chroot gave `EPERM`. The test unwrapped the result. It now skips there with the reason; `scripts/ci/root-enter-confine-tests.sh` runs it as root, where a skip fails the run. The session that wrote it ran each check as root, and missed the case.

## Done

2026-11-15, the commits of the change. `Plan::hostname` and the module `uts` (`NS_UTS_STEP` 21, `NS_HOSTNAME_STEP` 22, `MOST`); the child enters the UTS namespace and sets the name after the mounts of the namespace rung; `spawn_namespace` refuses in place of the fallback to chroot when the plan has a name, and `spawn_with` refuses a name on each entry with no namespace of its own. Unit tests (`uts::tests`: the checks, the refusal, the names of the steps); `tests/hostname.rs` with a static busybox, as root here: the payload sees `unit1` in `uname -n` and in `/proc/sys/kernel/hostname`, the host keeps its name, and a plan with no name sees the host's. With a wrong name in the plan the test failed. `scripts/ci/root-enter-confine-tests.sh` runs it as root in `ci.yml`. The plant `enter-hostname-not-set` (the child skips the two calls) is caught. `cargo test -p pod-enter --all-features` passed (189 unit tests); workspace clippy is clean.

# T-670: `pod-enter`: the namespace rung through a user namespace, and its leg in `pod-hostprobe` (PodUtils/podvm#406)

**Source:** PodUtils/podvm#406 (COORD-132 of podvm; PodUtils/podvm#397, PodUtils/podvm@T-1903).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

On a host where an unprivileged user can open user namespaces, podvm enters the userland rung and not the namespace rung. The namespace rung calls `unshare(CLONE_NEWNS)` in the user namespace of the caller, and the kernel needs `CAP_SYS_ADMIN` there, so an unprivileged caller gets `EPERM`; the probe reads the same (`clone(NEWNS)=EPERM`). podvm measured on its cloud host (kernel 6.18, uid 1000, no capabilities): `unshare -Urm sh -c 'mount -t tmpfs x /mnt'` works, and `podvm probe` selects `userland`. The ask: (1) `unshare(CLONE_NEWUSER | CLONE_NEWNS)` (and `CLONE_NEWUTS` of T-669) where `CLONE_NEWNS` alone gives `EPERM`; (2) `deny` into `/proc/self/setgroups`, then a one-line `uid_map` and `gid_map` (the caller's id to 0, as `unshare -Ur`); (3) the mounts and the chroot of the rung; (4) a named failure step for each new call; (5) a leg of `pod-hostprobe` that tries the same sequence, so that the selection takes the rung where it works. Files of the caller then show as uid 0 inside; podvm keeps its interposer view for each other id.

## Premise

- Read: PodUtils/podvm#406; `crates/pod-enter/src/launch.rs` (the child of the namespace rung), `crates/pod-enter/src/entry.rs` (`spawn_namespace`, `ns_setup_step`), `crates/pod-enter/src/uts.rs`; the probes of `pod-hostprobe` (`probes.rs`, `select.rs`).
- Not measured: this sequence as a user with no capabilities here (this session runs as root; a test needs a user of its own or the runner of GitHub).

## Approach

The maps and the paths built before the fork; in the child, the user namespace first, then the maps, then the steps of the rung as now; a step byte for each call (`unshare(CLONE_NEWUSER)`, `setgroups`, `uid_map`, `gid_map`). The entry tries the namespace rung as now and, at `EPERM` of `unshare(CLONE_NEWNS)`, tries the form with a user namespace before the fallback to chroot. A leg of `pod-hostprobe` (in a child: the same unshare, the maps, a tmpfs mount) and the rung in the selection where it passes. Tests as a user with no capabilities in `ci.yml` (the runner's user); a plant for each guard.

## Prove

```sh
cargo test -p pod-enter --all-features
cargo test -p pod-hostprobe --all-features
```

## Done

2026-11-15, commit 147239f9 and the next commit. `pod_hostprobe::userns` (`Maps`, `enter`, `Step`): `unshare(CLONE_NEWUSER)`, `deny` into `setgroups`, `0 EUID 1` and `0 EGID 1`, built before the fork and async-signal-safe after it (C641). `pod-enter`: the module `ns_user` (steps `NS_USER_STEP` 23, `NS_SETGROUPS_STEP` 24, `NS_UID_MAP_STEP` 25, `NS_GID_MAP_STEP` 26 in `ns_setup_step`); `spawn_namespace` and the overlay root try the rung as now and, at `EPERM` of `unshare(CLONE_NEWNS)` only, again with the user namespace first, then fall back to chroot with both refusals in the line (`both_refused`). The banner goes out one time for each entry (C644). `pod-hostprobe`: the census row `mount(tmpfs,/tmp) in unshare(NEWUSER+maps,NEWNS)` (`USERNS_LEG`), the namespace rung in the selection where it passes, `Provides::namespaces` `user+mount` (C642); the schema has the row. Tests: 3 unit tests of `ns_user`, 3 of `userns`, 2 of the selection, 1 of the row, the numbers of `geteuid` and `getegid`; `tests/userns.rs` and `tests/userns_leg.rs` as uid 65534 with no capabilities (kernel 6.18): the payload sees uid 0, the name `unit2`, a private `/tmp`, the word `namespace` and one banner, and the leg is `OK`. With no retry the test failed (`the namespace rung refused at unshare(CLONE_NEWNS) (EPERM (1))`). `scripts/ci/userns-tests.sh` in `ci.yml` (C643). The plants `enter-userns-not-tried`, `enter-userns-no-maps`, `enter-userns-banner-twice`, `hostprobe-userns-leg-not-selected` and `hostprobe-userns-leg-no-user` are caught. `cargo test -p pod-enter --all-features` (192 unit tests) and `cargo test -p pod-hostprobe --all-features` (148 unit tests) passed; workspace clippy is clean. Left: T-671 (the join of such a run), T-672 (a `/proc` there).

# T-671: `pod-enter`: the join of a run that entered the namespace rung through a user namespace

**Source:** T-670 (a finding of its work).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

A caller with no capabilities enters the namespace rung through a user namespace of its own (T-670). An exec into that run (`spawn_joined`) calls `setns(pidfd, CLONE_NEWNS)` from the user namespace of the caller. The kernel needs `CAP_SYS_ADMIN` over the user namespace that owns the mount namespace of the payload, and in the user namespace of the caller: the caller has neither, so the join gets `EPERM`. The entry then enters a mount namespace of its own (`ns_join_fallback_line`): the payload of the exec does not see the `/tmp` and the writes of the payload of the run.

## Premise

- Read: `crates/pod-enter/src/entry.rs` (`spawn_joined_root`), `crates/pod-enter/src/launch.rs` (the join in the child), `setns(2)` (a pidfd with several flags joins the namespaces in one call).
- Not measured: a join of the user and the mount namespace of a payload of uid 0 in its namespace, by the caller of uid 65534 that owns that namespace.

## Approach

Where the user namespace of the payload is not the one of the caller (compare `/proc/self/ns/user` with the one of the pid, before the fork), the child calls `setns(pidfd, CLONE_NEWUSER | CLONE_NEWNS)`: the owner of a user namespace holds each capability in it. A step byte and a name for the call. A live test as a caller with no capabilities in `tests/userns.rs` (a run, then a joined exec that reads a file of the run in `/tmp`); a plant.

## Prove

```sh
cargo test -p pod-enter --all-features
sh scripts/ci/userns-tests.sh
```

## Done

2026-11-15, commit 889c63d9; the plants in the next commits. `pod-enter`: the module `ns_join` (`Join`, built before the fork). Where the payload of the pidfd is in a user namespace that is not the one of the caller, the child calls `setns(pidfd, CLONE_NEWUSER | CLONE_NEWNS)` in one call (`NS_JOIN_USER_STEP` 27, "setns(pidfd, CLONE_NEWUSER | CLONE_NEWNS)"); else `setns(pidfd, CLONE_NEWNS)` as before (step 16). The entry reads the pid of the pidfd from `/proc/self/fdinfo` (`fdinfo_pid`), compares the device and inode of `/proc/self/ns/user` with the one of that pid, and reads the pid again (`other_user_namespace`); where `/proc` does not tell, the join is the mount namespace alone (C651). A refused join writes `ns_join_fallback_line` with the step, as before. Tests: 3 unit tests of `ns_join`; the live test `a_caller_with_no_capabilities_joins_its_run` in `tests/userns.rs` as uid 65534 with no capabilities (kernel 6.18): a run writes `/tmp/shared` and sleeps, and the joined exec sees the file and uid 0 with no fallback line. With the join of the mount namespace alone the test failed (`the entry does not share the mounts of the payload: ... (EPERM (1)); entered a mount namespace of its own`, exit 3). The root tests of the join (`tests/confine`, `scripts/ci/root-enter-confine-tests.sh`) pass. The plants `enter-userns-join-mount-only` and `enter-join-user-always` are caught; the plants `enter-ns-join-skipped`, `enter-overlay-join-as-tree`, `enter-confine-rule-on-a-passed-number` and `enter-ns-pts-unbound` follow the new code and are caught. A caller of another uid than the owner of the run's user namespace gets `EPERM` and the fallback, as before.

# T-672: `pod-enter`: a `/proc` for the namespace rung through a user namespace

**Source:** T-670 (a finding of its work).
**Category:** feature
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The namespace rung mounts a fresh `proc` on the `/proc` of the image (`ns_optional`). In a user namespace that the rung makes for a caller with no capabilities (T-670), the kernel refuses that mount with `EPERM`: a proc mount needs `CAP_SYS_ADMIN` over the user namespace that owns the PID namespace, and the rung makes no PID namespace. The child writes its one line and the payload runs with no `/proc`, as on the chroot rung. As root, the same rung gives a `/proc`.

## Premise

- Measured (kernel 6.18, uid 65534, no capabilities): in `unshare -Urm`, `mount -t proc p /mnt` fails ("permission denied"), and `mount --rbind /proc /mnt` works.
- Read: `crates/pod-enter/src/ns_optional.rs`.

## Approach

At `EPERM` of the proc mount in a user namespace, a recursive bind of the host's `/proc` on the `/proc` of the image (`MS_BIND | MS_REC`), then `nosuid`, `nodev` and `noexec` by a remount of the bind where the kernel takes it. The payload sees the host's processes, as on the rung as root (no PID namespace). A step of its own in the one line of a refusal. A live test as a caller with no capabilities (`/proc/self/status` reads); a plant.

## Prove

```sh
cargo test -p pod-enter --all-features
sh scripts/ci/userns-tests.sh
```

## Done

2026-11-15, commit 889c63d9; the plants in the next commits. `pod-enter`: the module `ns_proc_bind`. In a user namespace of the rung, at `EPERM` of the fresh proc, the child binds the host's `/proc` on the `/proc` of the image with `MS_BIND | MS_REC` (`NS_PROC_BIND_STEP` 28), then remounts the bind with `MS_REMOUNT | MS_BIND | MS_NOSUID | MS_NODEV | MS_NOEXEC`, and `MS_RDONLY` where the host's proc is read-only (`NS_PROC_REMOUNT_STEP` 29). A refused remount detaches the bind. Each refusal writes one line that names the refused fresh proc and the step after it. Before the fork the entry checks the host's `/proc`: a proc (`PROC_SUPER_MAGIC`) whose `/proc/self` is the pid of the entry (`proc_of_this_pid_namespace`); else it writes one line and keeps no `/proc`. `pod_hostprobe::sys`: `MS_RDONLY`, `MS_REMOUNT`. What the bind exposes (C652): the processes of the PID namespace of the entry, as the fresh proc of the rung as root shows them (the rung has no PID namespace); the options of the host's proc (`hidepid`, `subset`) and the mounts under it (`binfmt_misc`, the masks of a container), which the kernel locks in the user namespace; each file checks the credentials of the payload, which hold no capability outside its user namespace. The bind is in the mount namespace of the rung, under `/` made recursively private. Tests: 4 unit tests of `ns_proc_bind`; `tests/userns.rs` (`a_caller_with_no_capabilities_enters_the_namespace_rung`) as uid 65534 with no capabilities (kernel 6.18): the image has `/proc`, the payload reads `/proc/self/status`, `/proc/self/mountinfo` shows `/proc` with `nosuid,nodev,noexec`, and no line says "runs with no /proc". With no bind the test failed (exit 8); with no remount it failed (exit 9). The plants `enter-userns-proc-not-bound`, `enter-userns-proc-no-remount` and `enter-proc-bind-foreign-pid-ns` are caught. The refused remount has no live test: no host here refuses it (C653).

# T-676: `pod-hostprobe`: the KVM leg is a yes only for a device that runs a guest of the build's architecture (PodUtils/podvm#407)

**Source:** PodUtils/podvm#407 (COORD-133 of podvm; PodUtils/podvm#401, PodUtils/podvm@T-1907).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

In an arm64 container on an x86_64 host, `podvm-aarch64 probe` says `kvm: yes`: QEMU user mode passes the open of `/dev/kvm` and its ioctls to the x86_64 kernel, which runs no aarch64 guest. The machine tier then chooses KVM and fails at the start of the guest. The ask: after the open, `KVM_CREATE_VM` (type 0), then an ioctl that only the KVM of the build's architecture answers (aarch64: `KVM_ARM_PREFERRED_TARGET` on the VM descriptor; x86_64: `KVM_GET_SUPPORTED_CPUID` on the system descriptor, where `E2BIG` also means yes); a no, with the reason "the device runs no guest of ARCH (QEMU user mode?)", where it fails with `ENOTTY` or `EINVAL`; both descriptors closed.

## Premise

- Read: PodUtils/podvm#407; `crates/pod-hostprobe/src/probes/machine.rs` (the leg opened `/dev/kvm` and closed it); the ioctl numbers of `include/uapi/linux/kvm.h`.
- Not measured: a device of KVM (this session has no `/dev/kvm`); an arm64 container on an x86_64 host. podvm's probe there proves it.

## Approach

`SYS_IOCTL` in `sys`; `probes/kvm.rs`: `KVM_GET_API_VERSION` (12), `KVM_CREATE_VM`, the question of the architecture, each descriptor closed; a pure function from the three answers to the verdict, with unit tests of each answer and of the request numbers; on another architecture the open alone answers, as before. A plant.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
```

## Done

2026-11-15, the commit of the change. `sys::SYS_IOCTL`, `sys::ENOTTY`; `probes/kvm.rs`: `runs_native_guest` (`KVM_GET_API_VERSION` 12, `KVM_CREATE_VM`, then `KVM_GET_SUPPORTED_CPUID` on x86_64 or `KVM_ARM_PREFERRED_TARGET` on aarch64; each descriptor closed) and `verdict`; on another architecture the open alone answers. The unit tests `the_requests_are_the_kernel_numbers` (0xAE00, 0xAE01, 0xC008AE05, 0x8020AEAF) and `a_device_of_another_architecture_is_a_no_with_the_reason`; `cargo test -p pod-hostprobe --all-features` passed, clippy clean. The plant `hostprobe-kvm-other-arch-yes` is caught. No device of KVM here: podvm's probe in an arm64 container proves the live answer.

# T-688: `pod-enter`: the device nodes of the host on the namespace rung through a user namespace (PodUtils/podvm#414)

**Source:** PodUtils/podvm#414 (COORD-135 of podvm; PodUtils/podvm@T-1912, PodUtils/podvm@T-1909).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

In a user namespace `mknod` is refused, so the fixups of `pod-rootfs` leave regular files at `/dev/null`, `/dev/zero`, `/dev/random` and `/dev/urandom`; a `/dev/urandom` there ends after 1 MiB (`FILLED_BYTES`). podvm measured it in its own foreground entry through a user namespace (its QA row `run.urandom.ns`): `head -c 2000000 /dev/urandom | wc -c` gave 1048576. podvm's foreground entry binds the four nodes of the host onto the tree itself; for `run -d` and `create` it has no mount namespace of its own before pod-enter's. The ask: on the namespace rung through a user namespace (steps 23 to 26), after the private remount of `/` and before the chroot, bind the host's character devices `/dev/null`, `/dev/zero`, `/dev/full`, `/dev/random`, `/dev/urandom` and `/dev/tty` onto the image's, each where the image has a regular file or nothing at that path (an empty file made as the mount point), never over a link or a directory; a refused bind writes one line, as the optional mounts do (`ns_optional`), and the rung goes on.

## Premise

- Read: PodUtils/podvm#414; `crates/pod-enter/src/ns_optional.rs`, `ns_proc_bind.rs` (T-672: a bind of the host's proc in the same form), `launch.rs`, `ns_user.rs`.
- Not measured here: the rung as a user with no capabilities (the tests of T-670 run as uid 65534).

## Approach

The paths and the checks of the image (regular file, missing, link, directory) before the fork, through the descriptor of the root, with `O_NOFOLLOW`; in the child, in the user-namespace form only, after the private remount: an empty file as the mount point where the path is missing, then a bind of each host device (`MS_BIND`, no `MS_REC`), then a remount with `nosuid` and `noexec`; a refused step writes one line and the rung goes on. Unit tests of the choice of each path; a live test as uid 65534 (`head -c 2000000 /dev/urandom | wc -c` gives 2000000, `/dev/null` is a character device); a plant for each guard (no bind over a link).

## Prove

```sh
cargo test -p pod-enter --all-features
sh scripts/ci/userns-tests.sh
```

## Done

2026-11-15, commit 8b562b46; the plants in commit 022f3fe9. `pod-enter`: the module `ns_dev_bind`. With the user namespace of the rung (`ns_user::spawn_own`), the entry reads, before the fork, the kind of each of `DEVICES` (`/dev/null`, `/dev/zero`, `/dev/full`, `/dev/random`, `/dev/urandom`, `/dev/tty`) through the descriptor of the root with `O_NOFOLLOW` (`kind_of`, `choose`): a regular file, a missing path or a character device of the image takes the bind (C668); a link, a directory, another kind or an unreadable path takes one line; the host's node must be a character device. An image with no `/dev` directory, or a link there, takes one line (C670). In the child, after the optional mounts (`ns_optional::mount_each`) and before the chroot: an empty file as the mount point where the path is missing (`O_CREAT | O_EXCL | O_NOFOLLOW` through the descriptor of the image's `/dev`), the bind of the host's node (`MS_BIND`) onto `/proc/self/fd/N` of an `O_PATH | O_NOFOLLOW` descriptor that is a regular file or a character device (C669), then a remount with `MS_NOSUID | MS_NOEXEC` and no `MS_NODEV` (`MS_RDONLY` where the host's node is on a read-only mount); a refused remount removes the bind. Each refused step writes one line to the payload's standard error and the rung goes on (`NS_DEV_POINT_STEP` 30, `NS_DEV_BIND_STEP` 31, `NS_DEV_REMOUNT_STEP` 32 in `ns_setup_step`). The rung as root binds nothing. Tests: 8 unit tests of `ns_dev_bind` (also as uid 65534); `tests/userns.rs` (`a_caller_with_no_capabilities_gets_the_devices_of_the_host`) as uid 65534 with no capabilities (kernel 6.18), with an image `/dev` of regular files (`/dev/urandom` and `/dev/random` of 1 MiB), a missing `/dev/full` and `/dev/tty`, and a link at `/dev/zero`: `head -c 2000000 /dev/urandom | wc -c` and the same of `/dev/random` give 2000000, `/dev/null` and `/dev/tty` are character devices, a write to `/dev/full` fails with `ENOSPC`, `/proc/self/mountinfo` shows the bind of `/dev/urandom` with `nosuid` and `noexec` and no `nodev`, `/dev/zero` stays a link, and one line names it. With no bind the test failed (exit 3: the read of `/dev/urandom` ended at the file). The plants `enter-userns-dev-bind-over-link` (exit 7) and `enter-userns-dev-no-remount` (exit 8) are caught. The plants `enter-userns-not-tried`, `enter-userns-banner-twice`, `enter-userns-proc-not-bound` and `enter-overlay-falls-back` follow the new code and are caught. `cargo test -p pod-enter --all-features` (207 unit tests) and `sh scripts/ci/userns-tests.sh` passed. Not measured: a host with no `/proc` or with `/dev` mounted `nodev` (each device then gives one line).

# T-691: `pod-enter`: the root directory and the memfd move clear of the numbers of the passed descriptors (PodUtils/podvm#422)

**Source:** PodUtils/podvm#422 (COORD-138 of podvm; PodUtils/podvm@T-1914, PodUtils/podrunner#177).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`podvm run -d` exited 125 with "a passed descriptor goes onto 17, the number of the root file system or of the memfd" when its parent held descriptors 3 to 15. `RootDir::open` takes the lowest free number, and `launch.rs` refused before the fork when a number in `plan.fds.pass` was the number of the root or of the exec memfd. podvm passes its memo on 17, so a root that opens on 17 refused the run. Each caller that passes a fixed number can meet it. The ask: move the root and the memfd above each passed number with `fcntl(F_DUPFD_CLOEXEC)`, as `clear_of_passes` does for the readiness pipe, and do not refuse; where a collision cannot be cleared, name what holds the number (its kind from `fstat`).

## Premise

- Read: PodUtils/podvm#422; `crates/pod-enter/src/launch.rs` (the refusal, `clear_of_passes`, the uses of `root.fd()` and of the memfd in the child).

## Approach

A module `pass_clear`: `keep` gives the descriptor of the caller, or a close-on-exec copy above each passed number that the parent closes after the fork (`Moved`); the child uses the copy for `fchdir`, the check of the chroot and `execveat`. A failed copy names the number and the kind of its file. `clear_of_passes` of the readiness pipe moves there too (`launch.rs` is at the limit of 500 lines). Unit tests of the move, of a descriptor with no collision and of the kinds; a plant that keeps the root on the passed number.

## Prove

```sh
cargo test -p pod-enter --all-features --lib pass_clear
```

## Done

2026-11-15, the commits of the change. `crates/pod-enter/src/pass_clear.rs`: `keep` moves the root and the memfd of the caller above each passed number (`F_DUPFD_CLOEXEC`) and `Moved` closes the copies in the parent after the fork; the child uses the copies (`fchdir`, the check of the chroot, `execveat`); a failed move names the number and the kind of its file (`fstat`). `clear_of_passes` of the readiness pipe moved there; `launch.rs` is 481 lines. The tests of `pass_clear::tests` (3) pass; the plant `enter-root-on-a-passed-number` is caught.

# T-713: `pod-enter`: the tests of `tests/userns.rs` race on `ETXTBSY` as a user with no root

**Source:** `ci.yml` run 43730534119 at `7ef7aab` (PodUtils/podcrates#179), the job `test`, the step "The namespace rung through a user namespace".
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

As a user with no root, `scripts/ci/userns-tests.sh` runs the three tests of `crates/pod-enter/tests/userns.rs` in parallel threads. A test that forks while another test writes its copy of busybox gives the child that write descriptor until its exec, and the exec of that copy then fails with `ETXTBSY`. Two tests failed in CI, so the gates of `v0.53.0` stopped. As root, each test runs again as uid 65534 with `--test-threads=1`, so a session as root did not see it.

## Premise

- Measured: the test binary at `7ef7aab`, run as uid 65534 with a static busybox, 60 times on 4 cores: 5 runs failed with `ETXTBSY`.

## Approach

One lock for the tests of the file: `as_nobody` holds it for the whole test, so no test forks while another writes its copy.

## Prove

```sh
sh scripts/ci/userns-tests.sh
```

## Done

2026-11-15. With the lock, the same binary as uid 65534 failed 0 of 60 runs.

# T-738: `pod-enter`: bind the volumes of a plan on the namespace rung, also through a user namespace (PodUtils/podvm#440)

**Source:** PodUtils/podvm#440 (COORD-148 of podvm; PodUtils/podvm@T-1920, from PodUtils/podrunner#189).
**Category:** feature
**Milestone:** M6
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

A volume moves a podvm run off the namespace rung: the loader family serves it as a path rewrite, and a dynamic program that issues a raw `syscall` instruction (`uv`, `ruff`: `tempfile` through `rustix`) sees host paths and fails with `EACCES`. podvm binds each volume itself for a foreground `run` (its own user namespace has a mount namespace), but `run -d` and `create` enter through `pod-enter`'s user namespace (T-670 to T-672, T-688), where podvm has no mount namespace of its own.

## Premise

- Read: PodUtils/podvm#440. podvm's form is `PodUtils/podvm:crates/podvm/src/userns/host.rs` (`bind_volumes`, `locked_flags`) on its `main` at `1298b66a`.

## Approach

`Plan` takes the volumes: `(host, guest, read_only)` for each, in order. In the child of the namespace rung, after the private remount of `/`, the tmpfs and the device binds and before the `chroot`: open each guest path under the root with `openat2(RESOLVE_IN_ROOT | RESOLVE_NO_SYMLINKS)`, making each missing directory (or an empty file where the host side is a file); then `mount(host, /proc/self/fd/N, MS_BIND | MS_REC)`. A read-only volume is remounted `MS_REMOUNT | MS_BIND | MS_RDONLY` with the flags that a user namespace locks (read from the mount). A failed bind is a named refusal (a step of `ns_setup_step`); a guest path that crosses a symbolic link of the image is refused. Tests as root and as a user with no capabilities (`scripts/ci/userns-tests.sh`), with plants.

## Prove

```sh
cargo test -p pod-enter --all-features
sh scripts/ci/userns-tests.sh
```

## Done

2026-11-16, commit 039dd7f5 and the plants in commit a9647c73; calls C720 to C722. `pod-enter`: `Plan::volumes: Vec<VolumeBind { host, guest, read_only }>` and the module `ns_volume`. Before the banner and the fork, the entry refuses by name a guest path that is not absolute and normal or is `/` (`volume_guest_parts`), a guest path of two volumes, a host path that is not absolute or does not exist, and more than `VOLUMES_MOST` (255) volumes; the chroot rung, the ladder and the entry with no chroot refuse a plan with a volume, and the fallback of the namespace rung to chroot refuses it too. In the child, after the device binds and before the chroot, as root and through a user namespace: the root opens by its path, each component of the guest path opens with `openat2(RESOLVE_IN_ROOT | RESOLVE_NO_SYMLINKS)` and is made where it is missing (a directory, or an empty file for a host side that is not a directory), then `mount(host, /proc/self/fd/N, MS_BIND | MS_REC)`; a read-only volume opens again through the bind and is remounted `MS_REMOUNT | MS_BIND | MS_RDONLY` with the flags of `fstatfs` of that mount (`volume_remount_flags`: `nosuid`, `nodev`, `noexec`, the atime flags, `nosymfollow`). The steps are `NS_VOLUME_POINT_STEP` 33, `NS_VOLUME_LINK_STEP` 34 ("the guest path of a volume crosses a symbolic link of the image"), `NS_VOLUME_BIND_STEP` 35 and `NS_VOLUME_REMOUNT_STEP` 36; a refused one is `Error::Runtime` with `HOST:GUEST[:ro]`, the step and the errno, with no fallback. A join binds no volume; a working directory in a volume resolves in its host side before the fork (`Maps::of_binds`). `pod-hostprobe`: `sys::fstatfs` (the `statfs` code moves to `sys/statfs.rs`), `MS_NOATIME`, `MS_NODIRATIME`, `MS_RELATIME`, `MS_NOSYMFOLLOW`, `ST_NOSYMFOLLOW`. `pod-testkit`: `nobody::again_over_a_flagged_tmpfs`. Tests: `ns_volume::tests` (the checks before the fork, the refusal of the chroot rung before the banner, the words of each step, the flags of the remount, the walk with no mount, the working directory in a volume); `tests/userns.rs` `the_namespace_rung_binds_the_volumes_of_a_plan` as root and as uid 65534 over a tmpfs with `nosuid,nodev,noexec,noatime`: a volume read and written (the host sees the write), a read-only volume (`EROFS`; its mount holds `ro` and the four flags), a guest path that the image does not hold (made), a file volume, a guest path through a link of the image (refused with the step), a host path that does not exist (refused before the banner). Here (root, kernel 6.18, static busybox): `cargo test -p pod-enter --all-features` and `sh scripts/ci/userns-tests.sh` pass. The plants `enter-volume-skipped`, `enter-volume-ro-writable`, `enter-volume-through-a-link` and `enter-volume-flags-dropped` are caught. Breaking, for the release notes: `pod_enter::Plan` has the new field `volumes`, and a plan built with a struct literal names it (`volumes: Vec::new()`). podvm: for `run -d` and `create`, fill `Plan::volumes` from `-v HOST:GUEST[:ro]` and stop the userland rewrite of those volumes on the namespace rung; keep its own binds of the foreground `run` or move them here; keep `unmount_under` before each removal of a tree (the binds are in the mount namespace of the payload, and the image keeps each mount point that the child made).

# T-744: `pod-hostprobe`: the row `chroot(/tmp) in unshare(NEWUSER+maps)`, the chroot of a driver with no root (PodUtils/podrunner#191)

**Source:** PodUtils/podrunner#191 (COORD-067 of podrunner; PodUtils/podrunner@T-283).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`podrunner doctor` shows the rows `mount(tmpfs,/mnt) in clone(NEWNS)` and `chroot(/tmp)`. For a user with no root, both give `EPERM`, while a driver with namespaces (the namespace rung of `pod-enter`) can mount and chroot in its own user namespace. podrunner asks for a mount row and a chroot row in a user namespace with an ID map.

## Premise

- Read: PodUtils/podrunner#191. The mount row exists since `v0.52.0` (T-670): `mount(tmpfs,/tmp) in unshare(NEWUSER+maps,NEWNS)` (`pod_hostprobe::probes::USERNS_LEG`), with the maps written by the child itself in the sequence of `pod_hostprobe::userns::enter`.

## Approach

A census row `chroot(/tmp) in unshare(NEWUSER+maps)` (`USERNS_CHROOT`): the sequence of `userns::enter`, then `chroot(/tmp)`, in a child with no namespace flags, so a refusal of the user namespace is a denial with its errno. The schema of the report names it.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
sh scripts/plant-each.sh hostprobe-userns-chroot-no-userns
```

## Done

2026-11-16. `USERNS_CHROOT` and `p_userns_chroot` in `crates/pod-hostprobe`; `schemas/hostprobe-report.v1.schema.json` names the row; `tests/userns_leg.rs` runs both rows as uid 65534 and requires the chroot where the mount leg holds (both OK on this host). The plant `hostprobe-userns-chroot-no-userns` is caught.

2026-11-16, the fix of a missed plant: the plant ran `tests/userns_leg.rs` directly, and on the runner of `plants.yml` (Ubuntu 24.04, `kernel.apparmor_restrict_unprivileged_userns=1`) the test said `skipping` and passed (plants run 43797579278, shard 0: "MISSED hostprobe-userns-chroot-no-userns"). The plant now runs `scripts/ci/userns-tests.sh`, which turns the user namespaces on and fails on a skip, as the plant `hostprobe-userns-leg-no-user` of T-670 does. Here: `sh scripts/plant-each.sh hostprobe-userns-chroot-no-userns`: 1 caught.

# T-739: `pod-enter`: a rule of the confinement on a guest path of a volume is checked in the image, not in the volume

**Source:** T-738 (a finding of its work).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

On a rung with a chroot, `confine::compile` opens each path of the rules under the descriptor of the image before the fork (`Spec::compile_in`), and a path that does not exist is a refusal by name. The child opens each path again after its mounts and the chroot (`Compiled::open_paths`), so the rule holds the file that the payload sees. With the volumes of T-738, a rule on a guest path of a volume that the image does not hold (`-v /srv/data:/data` and a write rule on `/data`) is refused before the fork, though the child would bind it; and a rule on a path that the image holds is checked against the directory of the image under the mount point, not against the host side of the volume.

## Premise

- Read: `crates/pod-enter/src/confine.rs` (`compile`), `crates/pod-enter/src/ns_volume.rs` (`check_workdir`, which resolves a guest path in a volume through `Maps::of_binds`), the README of `pod-enter` ("a path that does not exist is a refusal by name before the fork").
- Not measured: a confined plan with a volume on the namespace rung.

## Approach

Where the plan has volumes, check each path of the rules through the table of the volumes (`Maps::of_binds`, the resolver of `abi::GuestRoot`): a path at or under the guest path of a volume resolves in its host side, each other path in the image; the child opens each path again after the binds, as now. A test on the namespace rung as root: a write rule on a volume that the image does not hold lets the payload write there, and a write outside the rules fails with `EACCES`.

## Prove

```sh
cargo test -p pod-enter --all-features
sh scripts/ci/userns-tests.sh
sh scripts/ci/root-enter-confine-tests.sh
```

## Done

2026-11-16, commit 2e9ec7e8; call C731. `pod-confine`: `Spec::compile_by(abi, open)`, a compile whose opener of the caller gives the descriptor of each path as the spec names it (or the errno of the refusal). `pod-enter`: where a plan has volumes, `confine::compile` opens each path of a rule through the walk of `Maps::of_binds` (`open_through`): a path at or under the guest path of a volume opens in its host side, each other path in the image, and a link of the image to a volume lands in the volume; a directory above a volume that the image does not hold (the child makes it) gets the root of the image for the check of the type; a path in neither the image nor a volume is `ENOENT`, refused by name before the banner. A plan with no volume keeps `compile_in`. The child is unchanged: it opens each path after the binds and the chroot (`Compiled::open_paths`), so a rule holds the root of the bind. Tests: `pod_confine` `the_opener_of_the_caller_resolves_each_path`; `confine::tests::a_rule_resolves_through_the_volumes` (a volume that the image does not hold, a path of the host side alone, a link of the image to a volume, a volume over a directory of the image, a made directory above a volume, `ENOENT` for a path in neither); `tests/userns.rs` `a_rule_of_the_confinement_holds_on_a_volume` as root and as uid 65534 over the flagged tmpfs: the payload reads a volume that the image does not hold under a read rule, writes a directory of the host side alone under a write rule, reads a volume under a rule on the made directory above it, and gets `EACCES` in the volume `/data` that no rule names and for a write outside the rules; a rule on `/nowhere` is refused before the banner. Here (root, kernel 6.18, Landlock, `PODVM_T486_BUSYBOX=/tmp/bbx/busybox`): `cargo test -p pod-enter -p pod-confine --all-features` (pod-enter 218 unit tests, pod-confine 38), `sh scripts/ci/userns-tests.sh` and `sh scripts/ci/root-enter-confine-tests.sh` pass. The plants `enter-confine-volume-in-the-image`, `enter-confine-volume-missing-accepted` and `enter-confine-volume-descriptors-kept` are caught (`sh scripts/plant-each.sh --group enter`: 23 caught, 0 missed or stale; `--apply`: 0 stale). Also fixed: `pass_clear::tests::the_kind_names_a_directory_and_a_pipe` read a closed low number that another test thread can take (a failure seen here); it reads a closed number over 900. Found: T-750 (the overlay root with an upper directory). For the release notes: podvm can now combine the rules of its confinement profile (`fs.read`, `fs.write`, `fs.execute`; `Plan::confine`) with `-v HOST:GUEST[:ro]` volumes (`Plan::volumes`) on the namespace rung, for `run -d` and `create`: a rule on a guest path of a volume (also one that the image does not hold, or a path under it that only the host side holds) is accepted and holds the volume; a rule that names no volume does not reach into it. No change of the API for podvm: `pod_confine::Spec::compile_by` is new.

# T-750: `pod-enter`: on an overlay root with an upper directory, the working directory and the paths of the rules are checked in the lower layer alone

**Source:** T-739 (a finding of its work).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The overlay root of the namespace rung (`Root::Overlay { lower, upper: Upper::Dir { upper, work } }`) enters through `launch` with `lower` as the root. Before the fork, the check of the working directory (`ns_volume::check_workdir`) walks `lower.path`, and the compile of the confinement opens each path of a rule under the descriptor of `lower` (`compile_in`, or the walk of T-739 with volumes). An upper directory of the caller holds files: podvm completes the root into it (`pod_rootfs::complete` with `Settings::upper`, T-567), and a run of a container keeps its writes there for the next run. A `-w` or a rule on a path that only the upper layer holds is refused before the fork, though the payload sees it after the mount of the overlay. A path that the upper layer removes (a whiteout) passes the check, and the child then refuses it.

## Premise

- Read: `crates/pod-enter/src/ns_overlay.rs` (`namespace_root_entry`: `spawn_own(lower, ..)`), `crates/pod-enter/src/launch.rs` (`check_workdir`, `confine::compile` with `root`), `crates/pod-enter/src/confine.rs` (`open_through`).
- Not measured: a confined run, or a run with `-w`, on an overlay root whose upper directory holds the path.

## Approach

A `GuestRoot` of two layers for the checks before the fork: each component reads the upper directory first (a whiteout, a character device 0:0, is a missing entry; a directory with `trusted.overlay.opaque` or `user.overlay.opaque` hides the lower one), then the lower one; the volumes stay above both. `check_workdir` and `confine::compile` (through `compile_by`) take it where the root is an overlay with an upper directory; `Upper::Tmpfs` keeps the lower layer, as its upper layer starts empty. Tests on the overlay root as root (`tests/confine/overlay.rs`): a `-w` and a write rule on a directory that only the upper layer holds pass, and a rule on a path that a whiteout removes is refused before the banner; a plant for each.

## Prove

```sh
cargo test -p pod-enter --all-features
sh scripts/ci/root-enter-confine-tests.sh
```

## Done

2026-11-16, commits d539fc08, ee3794c9, 1cba940a (local branch `fix750`);
call C738. Found: T-755.

- Cause: `launch::spawn_with` enters an overlay root with the lower
  directory as its root, and the two checks before the fork
  (`ns_volume::check_workdir` and `confine::compile`) read that root
  alone; the upper directory of `Upper::Dir` reached only the options of
  the mount.
- Fix: `ns_upper::View`, a `GuestRoot` of the volumes over the two layers:
  each component reads the upper directory first (a character device 0:0
  is a missing entry; `trusted.overlay.opaque` or `user.overlay.opaque`
  set to `y` hides the lower directory, as `pod-rootfs` reads a root of
  two layers), then the lower one; a volume hides both. `GuestRoot::hidden`
  (default `false`) tells `abi::walk_in` that a host path that exists is
  missing in the guest view. `check_workdir` and `confine::compile` (through
  `compile_by` and `open_through`, now generic) take the view where the plan
  has volumes or the root has an upper directory; a refused working
  directory names the upper layer. The upper directory comes from
  `NsEntry::upper`: `OverlayMount::upper_dir` for an own entry, and
  `Root::upper_dir` for a join (`NsEntry::Join` carries it). `Upper::Tmpfs`
  keeps the lower layer. `pod-extract` writes no whiteout into a tree (it
  applies the `.wh.` entries of a layer by removal); the upper directory
  holds the whiteouts of overlayfs (a character device 0:0), which the
  view reads.
- Tests: `ns_upper::tests` (an upper directory alone, a merged directory,
  a whiteout of a file and of a directory, an opaque directory, a file of
  the upper layer over a directory, a link of the upper layer, a volume
  over both layers, the working directory and its refusal, the opener of
  a rule); `tests/userns.rs`
  `the_checks_read_the_upper_layer_of_an_overlay_root` (`tests/userns/upper.rs`)
  as root and as uid 65534 over the flagged tmpfs, under the lock of T-713:
  `-w` on a directory of the upper layer alone runs there; a write rule on
  it is accepted and holds (the write lands in the upper directory, and a
  write outside the rules fails); `-w` and a rule on a path that a whiteout
  removes are refused by name before the banner; a volume over a directory
  of the upper layer wins for `-w` and a rule; a joined exec takes `-w` of
  the upper layer alone. Here (root, kernel 6.18, Landlock,
  `PODVM_T486_BUSYBOX=/tmp/bbx/busybox`): `cargo test -p pod-enter
  --all-features` (222 unit tests), `sh scripts/ci/userns-tests.sh` and
  `sh scripts/ci/root-enter-confine-tests.sh` pass.
- Plants (group `enter`): `enter-upper-workdir-in-the-lower`,
  `enter-upper-rule-in-the-lower`, `enter-upper-join-in-the-lower` (the
  live test), `enter-upper-hidden-read`, `enter-upper-volume-under-the-layers`,
  `enter-upper-whiteout-read`, `enter-upper-opaque-read` (the unit tests).
  The patches of `enter-ns-join-skipped`, `enter-overlay-join-as-tree`,
  `enter-confine-volume-in-the-image` and
  `enter-confine-volume-missing-accepted` follow the change of the code.
  `sh scripts/plant-each.sh --group enter`: 30 caught, 0 missed or stale; `--apply`: 0 stale.
- For the release notes: on the overlay root with an upper directory of
  the caller (`Root::Overlay { upper: Upper::Dir { .. } }`), podvm's `-w`
  and the paths of its confinement profile are checked before the fork in
  the view of the payload: a directory or a file that only the upper
  directory holds (a write of an earlier run, or `pod_rootfs::complete`
  with `Settings::upper`) is accepted, a path that a whiteout or an opaque
  directory of the upper directory removes is refused by name before the
  fork (it was refused by the child), and a volume still hides both
  layers. Also for `spawn_joined_root`. No change of the API for podvm:
  `abi::GuestRoot` has the new method `hidden` (a default; an
  implementation of a caller builds as before).

# T-755: `pod-enter`: the opaque attribute that overlayfs reads on an overlay root in a user namespace is not measured

**Source:** T-750 (a finding of its work).
**Category:** measurement
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The check before the fork (`ns_upper::View`, T-750) takes a directory of
the upper layer with `trusted.overlay.opaque` or `user.overlay.opaque` set
to `y` as opaque, as `pod-rootfs` reads a root of two layers. The overlay
mount of the namespace rung has no `userxattr` option, so overlayfs reads
`trusted.overlay.opaque` alone. A caller with no capabilities mounts the
overlay in a user namespace (`ns_user`), where `trusted.` attributes need
`CAP_SYS_ADMIN` in the first user namespace: there overlayfs can read no
opaque attribute. Where the check and the kernel disagree, a path that the
payload sees is refused before the fork (a false refusal, never a false
pass).

## Premise

- Read: `crates/pod-enter/src/ns_overlay.rs` (`overlay_options`: no
  `userxattr`), `crates/pod-rootfs/src/write/layers.rs` (`OPAQUE`).
- Not measured: which attribute overlayfs reads, and whether it makes an
  opaque directory, in a user namespace with no `userxattr` (kernel 6.18).

## Approach

Measure, as root and as uid 65534 through the rung: a directory of the
upper layer with each attribute over a lower directory with a file, and
what the payload sees. Then make the check read what the mount reads (the
mount as root: `trusted.`; in a user namespace: `user.` with a `userxattr`
mount, or none), or give the mount `userxattr` in a user namespace. A test
for each case.

## Prove

```sh
cargo test -p pod-enter --all-features
sh scripts/ci/userns-tests.sh
```

## Done

2026-11-16. Measured on kernel 6.18 (python, `setxattr`, `mount -t overlay`
as root and as uid 65534 in `unshare -Urm`): a mount of a host root with no
`userxattr` reads `trusted.overlay.opaque` alone; a mount with `userxattr`
reads `user.overlay.opaque` alone; a mount in a user namespace with no
`userxattr` reads neither (a `trusted.` mark that root set does not hide the
lower directory). Call C765. The overlay of the rung in a user namespace
has `userxattr` (`ns_overlay::USERXATTR`; `OverlayMount::userxattr`: the
namespace of the rung, or a caller that is uid 0 of a user namespace that
is not the initial one); a kernel that refuses the option with `EINVAL`
gets the mount with no such option. The check before the fork reads the one
attribute that the mount reads (`ns_upper::UpperDir`, `NsEntry::upper`; a
join reads the namespace of the run). Tests: `ns_upper::tests` (the mark of
`user.` makes no opaque directory on a mount of a host root), and
`tests/userns/upper.rs`, as root and as uid 65534: a payload on an overlay
root does not see a file of the image under an opaque directory of the
upper layer, and a rule on that path is refused before the fork. Plants
`enter-overlay-no-userxattr`, `enter-view-reads-both-opaque`.

2026-11-16: the plant `enter-overlay-no-userxattr` was missed in `plants.yml`
(runs 43894085749 and 43905557327, shard 5): the kernel of the Ubuntu
runner reads an opaque directory of an overlay in a user namespace, and
shows `userxattr` in `/proc/self/mountinfo`, with no such option, so the
planted defect has no effect there. The choice of the options is now one
function, `OverlayMount::attempts` (with `userxattr` first in a user
namespace, then the plain options; a host root: the plain options alone),
with the unit test `the_mount_in_a_user_namespace_tries_userxattr_first`;
the plant patches it and the unit test catches it on each kernel. The live
test also reads the option in the mountinfo of the payload, which holds on
a kernel with no patch of its own.

# T-758: `pod-hostprobe`: `chroot_usable` takes the row of the leg through a user namespace as the caller's chroot (PodUtils/podvm#443)

**Source:** PodUtils/podvm#443 (COORD-150 of podvm; PodUtils/podvm@T-1921).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podvm: "`probes::chroot_usable` takes each row whose name starts with
`chroot(`. On a host that refuses `chroot` to the caller (`chroot(/tmp)` =
EPERM) but opens a user namespace, the leg passes, so `chroot_usable` is
true." As uid 1000, `podvm exec` of a stopped run took the chroot sequence
and exited 125 with "chroot into the rootfs failed with EPERM"; each other
caller of `chroot_usable` in podvm got the same wrong answer. The row
`USERNS_CHROOT` of T-744 (`v0.59.0`) starts with `chroot(`.

## Premise

- Read: PodUtils/podvm#443; `crates/pod-hostprobe/src/probes/access.rs`
  (`chroot_usable`); the rows of `crates/pod-hostprobe/src/probes.rs`.
- Not measured: the host of podvm; the defect is in the match of the name.

## Approach

`chroot_usable` reads the row of the caller's own chroot alone, by the new
constant `probes::CHROOT`. A caller that wants the leg reads
`USERNS_CHROOT` by name. A test with the leg `Ok` and the caller's row
denied expects false.

## Prove

```sh
cargo test -p pod-hostprobe --all-features --lib -- probes::tests::only_an_ok_chroot_promises_entry --exact
sh scripts/plant-each.sh hostprobe-chroot-usable-takes-the-leg
```

## Done

2026-11-16. `probes::CHROOT` (`chroot(/tmp)`) and `chroot_usable` that
matches it alone. `only_an_ok_chroot_promises_entry` adds the leg: the leg
`Ok` with the caller's row denied, and the leg alone, give false; both `Ok`
give true. Here: `cargo test -p pod-hostprobe --all-features --lib` (174
unit tests). The plant `hostprobe-chroot-usable-takes-the-leg` is caught.
podvm removes its shadow of the function (PodUtils/podvm@T-1921) at the tag
that carries it, `v0.61.0`.

# T-786: `pod-enter`: the ids inside the user namespace of the namespace rung, so that a payload runs as the user of its image (PodUtils/podvm#447)

**Source:** PodUtils/podvm#447 (COORD-154 of podvm; PodUtils/podvm@T-1927; PodUtils/podrunner#196).
**Category:** feature
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podvm: "pod-enter's entry through a user namespace (`ns_user.rs`,
`pod_hostprobe::userns::Maps`) maps the effective ids of the caller to 0
(`map_line(id)` writes `0 <id> 1`). So the payload of `podvm run -d` of a
user with no root is root of a one-uid namespace, also where the image
names a `USER`, and docker runs the image's `USER` by default." podrunner's
runner image has `USER runner` (uid 1001); as root the official runner
refuses to run, GNU tar fails at each chown to an id that is not mapped,
and npm 6 runs its scripts as `nobody`. The ask: "A field of `Plan` (or of
the entry through a user namespace) that names the ids inside the
namespace, with 0 as the default. For example `Plan::userns_ids:
Option<(u32, u32)>`, which writes `<uid> <caller-uid> 1` and `<gid>
<caller-gid> 1`." The probe legs (`USERNS_LEG`, `USERNS_CHROOT`) keep the
map to 0.

## Premise

- Read: PodUtils/podvm#447.
- Not measured: the rung with an inside id that is not 0.

## Approach

`Plan` gets the inside ids (default 0, so each caller keeps the map of
now); the entry through a user namespace writes the one-line maps with
them. The process that makes the namespace keeps each capability in it
whatever its inside id, so the mounts of the rung stay as they are; the
`execve` of the payload with a uid that is not 0 drops the capabilities.
A test of the namespace rung as uid 65534: a payload with the inside ids
1001:1001 sees `id -u` 1001, the files of the tree as owned by 1001, and
no capability; the default stays 0.

## Prove

```sh
cargo test -p pod-enter --all-features
sh scripts/ci/userns-tests.sh
```

## Done

2026-11-16, commit 518ce97. `Plan::userns_ids: Option<(u32, u32)>`
(`None` is 0): on the namespace rung through a user namespace of its own
the maps are `<uid> <caller-uid> 1` and `<gid> <caller-gid> 1`
(`pod_hostprobe::userns::map_line(inside, id)`, `Maps::with_inside`,
`Maps::of_caller_inside`); the probe legs keep `Maps::of_caller`, the map
to 0. `u32::MAX` is refused before the fork. Checked for uid 0: the child
sets no ids and calls no `setuid`; the mounts, the device binds, the
volumes, the UTS namespace and the check of the working directory run
with the capabilities of the namespace, the same for each id inside; the
chown of the root of an overlay sets the caller's ids or keeps them
(C758); the `HOME=/root` of `Plan::env_for` is the caller's to replace
(C758); a join keeps the map of the run, and a root caller keeps its ids
(C757, T-787). Tests: `cargo test -p pod-enter --all-features` (223 unit
tests, 7 tests of `tests/userns.rs`), `cargo test -p pod-hostprobe
--all-features` (175 unit tests); `sh scripts/ci/userns-tests.sh` as root
(uid 65534 in the test) passed with no skip: with 1001:1001 the payload
sees `id -u` and `id -g` 1001, the maps, a file of the caller as
1001:1001, a file in `/tmp` as 1001, the host name, `CapEff` and `CapPrm`
0; with no ids it is 0 with its capabilities; an exec that joins the run
is 1001 with no capability. The plant `enter-userns-ids-ignored` is
caught (`left: Ok(21)`); the plants `enter-userns-banner-twice` and
`enter-userns-not-tried` follow the new lines.

# T-787: `pod-enter`: the ids of the plan for a caller that makes the namespace rung with no user namespace (root)

**Source:** T-786, a finding (`docs/decisions.md`, C757); PodUtils/podvm#452 (COORD-159 of podvm; PodUtils/podvm@T-1927).
**Category:** feature
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`Plan::userns_ids` names the ids of the payload inside the user namespace
of the namespace rung. A caller with `CAP_SYS_ADMIN` (root) makes the
mount namespace with no user namespace (`crates/pod-enter/src/ns_user.rs`,
`spawn_own`), so the ids of the plan do nothing there: the payload of an
image with `USER runner` runs as root, where docker runs it as the user
of the image. The chroot rung has the same gap.

## Premise

- Read: `crates/pod-enter/src/ns_user.rs`, `crates/pod-enter/src/launch.rs`
  (the child sets no ids before the `execve`).
- Not measured: a root caller of podvm with an image that names a `USER`.
- PodUtils/podvm#452 (2026-11-16): "A foreground `podvm run` of a user
  with no root enters a user, a PID and a UTS namespace of podvm's own at
  the start of `main` [...] There it maps the caller to 0, before it knows
  the image. Then it spawns the payload through pod-enter from inside that
  namespace, as uid 0. With `Plan::userns_ids = Some((1001, 1001))`,
  pod-enter sees a root caller and changes nothing." Measured by podvm as
  uid 1000: `podvm run --rm --user 1001 IMAGE cat /proc/self/uid_map`
  gives `0 1000 1`; `run -d` gives `1001 1000 1`. The ask: "Where
  `Plan::userns_ids` is `Some((uid, gid))` and the caller is uid 0 of a
  user namespace that is not the initial one, the child opens a nested user
  namespace before the exec: `unshare(CLONE_NEWUSER)`; `deny` into
  `setgroups`; `uid_map` `UID 0 1` and `gid_map` `GID 0 1` [...] A host root
  (the initial namespace) keeps T-787's behaviour. Alternatively, name a
  better way, for example a pre-exec hook of the plan."

## Approach

Where the rung (or the chroot rung) has no user namespace and the plan
names ids, the child calls `setgroups(0, NULL)`, `setresgid` and
`setresuid` with the ids of the plan after the mounts and the root change,
before the confinement and the exec; a refused call stops the payload with
its step. A tree that root extracted keeps the owners of the image, so the
ids of the plan are the ids of the image.

The first case to build is podvm's (PodUtils/podvm#452): a caller that is
uid 0 of a user namespace that is not the initial one (its
`/proc/self/uid_map` is not `0 0 4294967295`). There, after the mounts of
the rung and the root change, the child opens a nested user namespace
(`unshare(CLONE_NEWUSER)`, `deny` into `setgroups`, `uid_map` `UID 0 1`,
`gid_map` `GID 0 1`), then the confinement and the exec. A host root keeps
the behaviour of now until a consumer asks for `setresuid`.

## Prove

```sh
cargo test -p pod-enter --all-features
sh scripts/ci/userns-tests.sh
```

## Done

2026-11-16, commits ec12b6f and 3326c56. podvm's case (PodUtils/podvm#452):
where `Plan::userns_ids` is `Some`, the effective uid is 0 and
`/proc/self/uid_map` is not one line `0 0 4294967295`
(`pod_hostprobe::userns::is_initial_map`, read before the fork; a map that
cannot be read is a refusal), the child opens a nested user namespace
(`unshare(CLONE_NEWUSER)`, `deny` into `setgroups`, `<uid> 0 1`,
`<gid> 0 1`, the sequence of `pod_hostprobe::userns`). The step comes
after each mount of the rung, the volumes and the host name, and before
the root change, the confinement and the exec: the maps go through
`/proc/self` of the root of the caller, and the chroot needs only
`CAP_SYS_CHROOT` in the nested namespace. Each entry with a chroot takes
it (the namespace rung, the chroot rung, the ladder); a join, the entry
with no chroot and the rung through a user namespace of its own do not.
A refused step has its own byte (`NS_NESTED_USER_STEP` 37 to
`NS_NESTED_GID_MAP_STEP` 40) and is `Error::Runtime` with the ids, the
call and the errno: no fallback to chroot as 0 (C759, C760). A host root
keeps the ids of the caller: T-791 holds it. Tests:
`cargo test -p pod-enter --all-features` (227 unit tests, 4 new; 8 tests
of `tests/userns.rs`, 1 new), `cargo test -p pod-hostprobe --all-features`
(176 unit tests, 1 new); `sh scripts/ci/userns-tests.sh` as root passed
with no skip. The new test (`crates/pod-enter/tests/userns/nested.rs`)
starts again as uid 0 of a user namespace over uid 65534
(`pod_testkit::nobody::again_as_root_of_a_user_namespace`), and also
passed as uid 65534: with 1001:1001 the payload sees `id -u` and `id -g`
1001, the maps `1001 0 1`, a file of the caller as 1001:1001, a file in
`/tmp` as 1001, the host name, `CapEff` and `CapPrm` 0; the chroot rung
gives 1001; a joined exec is 1001 with no capability; with no ids it is
0; with `max_user_namespaces` 0 the entry names `unshare(CLONE_NEWUSER)`
and `ENOSPC` and does not fall back; as a host root the payload with the
ids stays 0. The plant `enter-userns-nested-ids-ignored` is caught
(`left: Ok(61)`); `sh scripts/plant-each.sh --apply` gives 0 stale.

# T-791: `pod-enter`: the ids of the plan for a host root (a change of ids with no user namespace)

**Source:** T-787, a finding (`docs/decisions.md`, C760).
**Category:** feature
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

`Plan::userns_ids` names the ids of the payload. A caller that is uid 0
of a user namespace that is not the initial one gets them through a nested
user namespace (T-787). A host root (uid 0 of the initial user namespace)
makes the mount namespace with no user namespace, and the child sets no
ids: the payload of an image with `USER runner` runs as root of the host,
where docker runs it as the user of the image. No consumer runs as a host
root with ids now: podvm's foreground run as root enters no user
namespace of its own, and asked only for the nested case
(PodUtils/podvm#452).

## Premise

- Read: `crates/pod-enter/src/ns_user.rs` (`nested_for`: the ids apply
  only where the map is not the initial one),
  `crates/pod-enter/src/launch.rs` (the child sets no ids before the
  `execve`).
- Not measured: a host root of a consumer with an image that names a
  `USER`.

## Approach

Where the caller is a host root and the plan names ids, the child calls
`setgroups(0, NULL)`, `setresgid` and `setresuid` with the ids of the plan.
The `chroot` needs `CAP_SYS_CHROOT`, which `setresuid` to a uid that is
not 0 removes: the change of ids comes after the `chroot`, the `chdir` and
the landing check, before the confinement and the exec. A refused call stops the
payload with a step of its own. A tree that root extracted keeps the
owners of the image, so the ids of the plan are the ids of the image. A
test as root: the payload with 1001:1001 sees `id -u` 1001 and `CapEff` 0.

## Prove

```sh
cargo test -p pod-enter --all-features
```

## Start condition

A consumer asks for the ids of the plan for a host root.

# T-800: `pod-services`: the host of the virtio-win guest tools, fedorapeople.org (PodUtils/podvm#456)

**Source:** PodUtils/podvm#456 (COORD-163 of podvm, PodUtils/podvm@T-1868).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm: "podvm PodUtils/podvm@T-1868 adds the virtio-win guest tools to the install of a
full Windows guest. The catalog pins `virtio-win-0.1.302.iso` from
`https://fedorapeople.org/groups/virt/virtio-win/direct-downloads/`
(877 373 440 bytes, with a pinned sha256). [...] The ask: list
`fedorapeople.org` in `pod-services`, with its owner and its use (a
download of pinned guest media)." podvm's test
`hosts_tests::each_host_that_podvm_names_is_in_pod_services` keeps the
host in its list `WAITING` until a tag of podcrates lists it.

## Premise

- Read: PodUtils/podvm#456; `crates/pod-services/services/microsoft-windows-eval.toml`
  (a guest ISO with a watch of its primary volume descriptor).
- Measured (2026-11-16, from this session): a HEAD of
  `archive-virtio/virtio-win-0.1.302-1/virtio-win-0.1.302.iso` answers 200
  with 877 373 440 bytes; the primary volume descriptor (a range read of
  sector 16) has the volume id `virtio-win-0.1.302`.
- Not measured: the path that podvm's catalog names under
  `direct-downloads/` (podvm's main does not hold PodUtils/podvm@T-1868 yet); the file of
  `archive-virtio/` is the one of that name and size.

## Approach

A service `fedora-virtio-win` (`guest-image`, the Fedora project) with
the endpoint of the ISO, its sha256 and its length, and the watch of
`microsoft-windows-eval`: the length and the SHA-256 of the primary volume
descriptor by a range, so that a new master of the ISO fails the watch
with no daily read of 837 MiB.

## Prove

```sh
cargo test -p pod-services --all-features --test podvm
```

## Done

2026-11-16, the commit of the change. `crates/pod-services/services/fedora-virtio-win.toml`:
the endpoint `iso` on `fedorapeople.org`, the facts `iso-sha256`
(`303f7ae4...`, measured from a GET of the whole file, 877 373 440 bytes), `iso-bytes` (877 373 440), and
the primary volume descriptor that the watch reads (`pvd-sha256`,
`volume-id` `virtio-win-0.1.302`). The test of the guest images lists the
service. podvm compares the sha256 with its catalog when it takes the tag;
a path under `direct-downloads/` other than `archive-virtio/` names the
same file only where its sha256 agrees.

# T-804: `pod-qga` has no `LICENSE` beside its manifest; a check of each crate that cargo packs (PodUtils/podvm#456)

**Source:** PodUtils/podvm#456 (COORD-163 of podvm, its second ask).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podvm: "`crates/pod-qga` at `v0.63.0` has no `LICENSE` file, where
`pod-guest` and the other crates have one. podvm's licence inventory of a
release (`podvm-release-licenses`) found no text for `pod-qga 0.63.0` and
stopped the nightly trial 43906889533." No check of podcrates held the
file: a new crate could ship with none.

## Premise

- Read: PodUtils/podvm#456; each directory of `crates/` and `examples/`:
  `pod-qga` and the new `pod-tor` have no `LICENSE`; the two probes of
  `examples/` have `publish = false`.
- Read: `scripts/check-publish-order.py` reads each crate of `cargo
  metadata` and knows which one cargo packs.

## Approach

`LICENSE` (the 0BSD text of the root) in `pod-qga` and `pod-tor`.
`check-publish-order.py`, which `ci.yml` runs on each push, fails for a
crate that cargo packs with no `LICENSE` beside its manifest; its
self-test has the cases.

## Prove

```sh
python3 -I scripts/check-publish-order.py --self-test
python3 -I scripts/check-publish-order.py
```

## Done

2026-11-16, the commit of the change. `crates/pod-qga/LICENSE` and
`crates/pod-tor/LICENSE`; `scripts/check-publish-order.py` checks each
packed crate (`licence_faults`), with three cases in its self-test. With
the file of `pod-qga` removed it fails with "pod-qga: no LICENSE beside
.../crates/pod-qga/Cargo.toml". podvm can drop its fall back to the text
of the root for this crate when it takes the tag after `v0.64.0`.

# T-806: `pod-enter`: a PID namespace with its own `/proc` on the namespace rung, which a join enters (PodUtils/podvm#457)

**Source:** PodUtils/podvm#457 (COORD-164 of podvm, PodUtils/podvm@T-1930; PodUtils/podrunner#200).
**Category:** feature
**Milestone:** M6
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

podrunner's QA (PodUtils/podrunner#200) ran `podvm run -d` of a user with
no root on the namespace rung: a step of a job can list each process of
the host with `ps`, end a `sleep` of the same user with `kill`, and end the
podrunner that started the unit with `pkill`. The entry of `pod-enter`
makes a user namespace and a mount namespace, and no PID namespace
(`crates/pod-enter/src/entry.rs`: "No PID or network namespace: the banner
says so"); in a user namespace of the rung the kernel refuses a fresh
`proc` with `EPERM`, as the PID namespace belongs to the user namespace of
the host, so the child binds the host's `/proc` (`ns_optional.rs`,
`ns_proc_bind.rs`), which shows the processes of the host.

## Premise

- Read: PodUtils/podvm#457; `entry.rs`, `ns_optional.rs`,
  `ns_proc_bind.rs`, `ns_join.rs` (the join of a run).
- Read: a `proc` mount needs a PID namespace that the user namespace of
  the mount owns: `CLONE_NEWPID` in the user namespace of the rung, then a
  fork, gives the first child pid 1 of a namespace whose `proc` mounts.
- Not measured: the rung with `CLONE_NEWPID` on the kernels of the labs.

## Approach

`Plan::pid_namespace: bool` for the namespace rung: the child makes
`CLONE_NEWPID` in its user namespace before it forks the payload; the
payload's parent in the namespace is a small init that reaps each child
and gives each signal to the payload (or the payload is pid 1, where the
plan asks for no init); the child mounts a fresh `proc` on the `/proc` of
the root in place of the bind of the host's `/proc`. The `Child` of the
caller keeps the host pid of the payload, so `pod-supervise` records host
pids as now. The banner names the PID namespace. The join of a run
(`ns_join`) enters it: `setns` of `/proc/PID/ns/pid_for_children`, then a
fork, so `exec` lands in it. Tests (the user namespace leg of
`tests/userns.rs`): the payload is pid 1 or the child of the init, `ps`
in it lists the processes of the run alone, a `kill` of a pid of the host
fails with `ESRCH`, the `/proc` is a `proc` of the new namespace, a join
lands in the namespace, and the end of the init ends each process of it.

## Prove

```sh
sh scripts/ci/userns-tests.sh
```

## Done

2026-11-16. `Plan::pid_namespace` (`ns_pid`): the namespace rung clones its
child with `CLONE_NEWPID`, and with `CLONE_NEWUSER` where the rung makes a
user namespace (`pod_hostprobe::userns::write_maps` maps it with no second
`unshare`); `EPERM` of the clone calls for the form with a user namespace.
The child mounts a fresh `proc` of the namespace and stays as its init:
the payload is pid 2, the init gives it each signal of `PID_FORWARDED`
that a process sent (not a signal of the terminal), reaps each orphan and
exits with the code of the payload. The `Child` is the host pid of the
init, not of the payload (C769): a SIGTERM of `pod-supervise` or of the
waiter reaches the payload, and a SIGKILL ends the namespace. A join of a
run in a PID namespace adds `CLONE_NEWPID` to its one `setns` call
(`other_namespace`, steps 43 and 44) and forks the payload of the exec into
it under a relay with the same loop. Each other entry refuses the plan,
and the rung does not fall back to chroot with it. Tests: three live
tests in `tests/userns/pid.rs` (pid 2, a fresh proc, no host pid, 6
processes at most; a SIGTERM to the init gives 143, a SIGKILL ends a
process in the background; a join sees `/proc/2` of the run and its own
pid there), as root and as uid 65534, each failing with
`pid_namespace: false`; unit tests in `src/ns_pid/tests.rs` and
`src/ns_join/tests.rs`; five plants `enter-pid-*`.

# T-811: `pod-enter`: the init of a PID namespace keeps the descriptors of the caller, so `run -d` never returns (PodUtils/podvm#460)

**Source:** PodUtils/podvm#460 (COORD-167 of podvm, PodUtils/podvm@T-1930); a defect of T-806.
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podvm took `v0.65.0` with `pid_namespace: true` for `run -d`: the
isolation holds, but `podvm run -d` never returns. The launcher of
`pod-supervise` reads its ready pipe to the end of file; the write end has
close-on-exec, which an exec drops. The init of the PID namespace (T-806)
is a fork with no exec, so it keeps each descriptor of the caller, the
ready pipe too, until the payload ends. The relay of a join is the same.

## Premise

- Read: PodUtils/podvm#460; `Pid::split` in `crates/pod-enter/src/ns_pid.rs`
  (the init closed the write end of the readiness pipe, the root and the
  memfd alone).
- Read: `close_range(2)` (Linux 5.9) closes a range in one call.

## Approach

The init and the relay close each descriptor, from 0 up, before their
loop: they read and write none (`sys::close_from` of `pod-hostprobe`:
`close_range`, else a close of each number). A live test: a caller that
waits for the end of file of a pipe with close-on-exec returns while the
payload runs, for the run and for a join; a plant.

## Prove

```sh
sh scripts/ci/userns-tests.sh
```

## Done

2026-11-17. `Pid::split` calls `sys::close_from(0)` in the init and in the
relay before the loop (`close_range(0, ~0, 0)`, else a close of each
number up to 65536, `crates/pod-hostprobe/src/sys/raw.rs`). Test:
`the_init_of_a_pid_namespace_keeps_no_descriptor_of_the_caller` of
`tests/userns.rs` (`pid::keeps_no_descriptor`): the end of file of a pipe
with close-on-exec comes within 5 s while the payload of the run and the
exec of a join run, as root and as uid 65534. Plant
`enter-pid-init-keeps-fds`.

# T-846: `pod-rootfs`: the source fixup reads the URLs of comment lines, so it probes `help.ubuntu.com` (PodUtils/podvm#463)

**Source:** PodUtils/podvm#463 (COORD-170 of podvm, PodUtils/podvm@T-1931, PodUtils/podrunner#202).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`pod_rootfs::sources::rewrite` finds each `http://` in the whole text of a
source file, comment lines too. Line 2 of the `ubuntu.sources` of
`ubuntu:24.04` is a comment with `http://help.ubuntu.com/...`, so the fixup
asks the probe about `help.ubuntu.com`: a request that the payload never
makes, through the caller's proxy. The rewrite also changes the URL of
the comment.

## Premise

- Read: PodUtils/podvm#463 (measured: three `CONNECT`s through
  `HTTPS_PROXY`, `help.ubuntu.com` the first); `crates/pod-rootfs/src/sources.rs`.
- Read: deb822 (`sources.list(5)`): a field that continues starts with a
  blank; a stanza ends at an empty line; `#` starts a comment line.

## Approach

The fixup reads a URL only on a line that the package manager reads: not
on a line whose first character that is not blank is `#`, and in a
`*.sources` file only in the field `URIs` (its first line or a line that
continues it). A test with the `ubuntu.sources` text of 24.04; a plant.

## Prove

```sh
cargo test -p pod-rootfs --all-features
```

## Done

2026-11-17, 1e42a7c: `sources::rewrite` takes `deb822` (the path ends with
`.sources`) and reads each line by `read`: a line whose first character
that is not blank is `#` is not read; in deb822, only the field `URIs`,
its first line or a line that continues it (a stanza ends at an empty
line). Test: `a_comment_and_a_field_that_is_not_uris_are_not_read`, with
the `ubuntu.sources` of 24.04 (the probe asks about `archive.ubuntu.com`
and `security.ubuntu.com` alone, and the comment stays as it was) and a
one-line file with a comment line after blanks. Plant
`rootfs-source-comment-read`, caught. The 85 unit tests of `pod-rootfs`
pass.

# T-857: `pod-enter`: a copy of a tree refuses the device nodes of the fixups, so a run of root on `rundir` or `tmpfs` never stages (PodUtils/podvm#467)

**Source:** PodUtils/podvm#467 (COORD-174 of podvm, PodUtils/podvm@T-1929).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`pod_enter::stage::copy_tree` refused each entry that is not a file, a directory or a link. Where `mknod(2)` passes (root), the fixups of the tree put real character devices into `dev/` (`dev/null`, `dev/zero`, `dev/random`, `dev/urandom`): nodes of podvm, not of the image. Each run of root with `PODVM_MODE=rundir` or `tmpfs` exited 125 before the payload.

## Premise

- Read: PodUtils/podvm#467 (measured with `busybox:musl` and `alpine:3.20`); `crates/pod-enter/src/stage.rs`; `pod_hostprobe::sys::mknodat` and `makedev`.

## Approach

A character or block device of the source is made again with `mknod(2)`, with the same mode and number, where it passes; else it is skipped and counted. A fifo or a socket stays a refusal. Tests with a real `dev/null` in the source, and with a maker that fails; a plant.

## Prove

```sh
cargo test -p pod-enter --all-features --lib stage
```

## Correction

2026-11-17, c324e5309: the plant run 44080440312 missed the plant: CI runs as a user with no root, so both tests of the copy skip at the fixture's `mknod(2)`. The step of one entry is `copy_entry` now, and `a_device_entry_goes_to_the_maker_with_its_mode_and_number` runs it on `/dev/null` (each user can read it) with a maker that records its call, and with one that fails: no root is needed. The plant expects that test; it is caught as root and as uid 65534.

## Done

2026-11-17, 6e2d50166: `copy_tree` returns `Staged { entries, nodes_skipped }` (was the count alone). A character or block device is made again by `mknodat(2)` with its mode and `rdev`; where that fails it is skipped and counted, and the copy goes on. A fifo or a socket is refused, with the words "not a file, directory, symlink or device node". Tests: `a_device_node_is_made_again_where_mknod_passes` (a real `dev/null` 1:3; its mode and number in the staged tree) and `a_device_node_is_skipped_and_counted_where_mknod_fails` (a maker that fails); each skips with a line on a host where the fixture `mknod(2)` fails; both ran here as root. 237 tests of the library of `pod-enter` pass. Plant `enter-stage-refuses-a-device-node`, caught.

# T-861: `pod-guest`: a mailbox run that times out before `WQREADY` tells nothing of the agent (PodUtils/podvm#468)

**Source:** PodUtils/podvm#468 (COORD-175 of podvm, PodUtils/podvm@T-1933, P1).
**Category:** defect
**Milestone:** M7
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

On a hosted runner with 2 CPUs, a foreground run of a Windows image with the mailbox agent timed out at 600 s in 3 of 4 runs with "the agent did not write WQREADY.TXT". The agent writes its first line to `WQLOG.TXT` only after it finds the mailbox, so a timeout cannot tell whether the agent did not start, did not find the volume in its 600 probes, or wrote and the write did not reach the disk.

## Premise

- Read: PodUtils/podvm#468 (podvm runs 44050312708, 44077136976, 44088895746; 44082202946 passed); `crates/pod-guest/src/agent.rs` (`AGENT_CMD`); `crates/pod-guest/src/evidence.rs` (with `Settings::evidence`, a run that timed out keeps a screen taken before the stop, the serial log, the error stream of the emulator and the mailbox).

## Approach

The agent writes a line to `COM1`, the serial log of the plan, when it starts, when it finds the mailbox, and when it gives up; `crate::evidence` copies that log. The ask's second part, a screen at the bound, is `Settings::evidence` already.

## Prove

```sh
cargo test -p pod-guest --all-features --lib agent
```

## Done

2026-11-17, f431c8306: `AGENT_CMD` appends `TIME wqagent start` to `COM1` before its first probe, `TIME wqagent found D: after N tries` when it finds the mailbox, and `TIME wqagent no mailbox after 600 tries` when it gives up (the label `:lost`). QEMU's UART keeps CTS and DSR up, so a write to `COM1` does not wait. Tests: `the_agent_gives_up_after_600_tries_with_no_mailbox` holds the two lines of a run with no mailbox, and `the_serial_log_shows_the_start_and_the_find`; the walk of the tests reads each new line. The tests of `pod-guest` pass. Plant `guest-agent-silent-before-the-mailbox`, caught. For podvm: an image takes the new agent at its next provision (the installer copies `AGENT_CMD`), the fingerprint of a warm state changes with it, and `Settings::evidence` keeps the screen and the serial log of each timeout.

# T-864: `pod-enter`: a new devpts instance and `/dev/ptmx` on the namespace rung, so that `openpty` works in a unit (PodUtils/podvm#469)

**Source:** PodUtils/podvm#469 (COORD-176 of podvm, PodUtils/podvm@T-1934; from PodUtils/podrunner#216).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

On the namespace rung, `/dev/pts` is the bind of the host's devpts (`ns_optional`), mounted `mode=600,ptmxmode=000`, and `ns_dev_bind::DEVICES` has no `ptmx`: `/dev/ptmx` does not exist, `os.openpty()` gives ENOENT, and an open of `/dev/pts/ptmx` gives EACCES. A job that needs a terminal (`script`, `pty.spawn`) fails. In a user namespace a new instance mounts and works (`mount -t devpts -o newinstance,ptmxmode=0666,mode=0620`).

## Premise

- Read: PodUtils/podvm#469; `crates/pod-enter/src/ns_dev_bind.rs` (`DEVICES`), `src/ns_optional.rs` (the bind of `/dev/pts`), and the rung's mount sequence.

## Approach

1. In a user namespace, mount a new devpts instance on the `/dev/pts` of the image (`newinstance,ptmxmode=0666,mode=0620`), and make `/dev/ptmx` a bind of `/dev/pts/ptmx`, with its mount point made where the image has none.
2. Where the mount is refused, keep the bind of the host's `/dev/pts`, also bind the host's `/dev/ptmx`, and write the refused line, so that the consumer names a unit with no pty.
3. A test in the userns tests: an `openpty` in the unit passes; a plant.

## Prove

```sh
sh scripts/ci/userns-tests.sh
```


## Correction

2026-11-17, 19479da04 (T-868): the fallback bound the host's `/dev/ptmx` on the image's `/dev/ptmx`, and an open of that bind never passes (`ENOENT`): the kernel looks for the devpts at `pts` beside the node, in the same mount, and a bind of one file holds no `pts`. The fallback now binds the `ptmx` of the devpts on the image's `/dev/pts` (the host's, after its bind), as with the new instance; the line of `devpts_refused` says `binds the host's /dev/pts and its ptmx`.
## Done

2026-11-17, 740ae46a8; the plant in d0a10811a. `pod-enter`: the module `ns_pts`. In a user namespace of the rung, where the image has a `/dev/pts` directory, the child mounts a new devpts instance there in place of the bind of the host's `/dev/pts` (`DEVPTS_OPTIONS` `newinstance,ptmxmode=0666,mode=0620`, no `gid=`: the group `tty` of the host has no id in the user namespace; `MS_NOSUID | MS_NOEXEC`; `NS_DEVPTS_STEP` 46), after the tmpfs on `/tmp` and the proc, and before the chroot. Then `ns_dev_bind::bind_each`, after the devices, binds `/dev/pts/ptmx` of the instance on the image's `/dev/ptmx` (`NS_PTMX_BIND_STEP` 47) through the descriptor of `/dev`, with an empty file as the mount point where the image has none, and remounts the bind `nosuid` and `noexec`. Where the kernel refuses the instance, the child writes `PRODUCT: the namespace rung binds the host's /dev/pts and /dev/ptmx: mounting a new devpts instance on /dev/pts failed with errno N` (`devpts_refused`), binds the host's `/dev/pts` as before, and binds the host's `/dev/ptmx` as a device (`NS_DEV_BIND_STEP` 31). A link at `/dev/ptmx` takes one line and no bind; an image with no `/dev/pts` directory takes neither mount. The rung as root keeps the bind of the host's `/dev/pts`. Tests: 3 unit tests of `ns_pts` (the options, the steps, the line) and `ns_dev_bind::the_binds_of_ptmx`; 242 unit tests of `pod-enter` pass. `tests/userns/pts.rs` (`a_caller_with_no_capabilities_gets_a_pty`), as uid 65534 with no capabilities (kernel 6.18, a host devpts with `ptmxmode=000`): `/dev/ptmx` is a character device, `/dev/pts` is not on the device of the host's, `/dev/pts/ptmx` has mode 0666, an open of `/dev/ptmx` gives `/dev/pts/0` as the one pty, the bind of `/dev/ptmx` is `nosuid` and `noexec`, and the payload gets no line. `sh scripts/ci/userns-tests.sh` passed (13 tests of `tests/userns.rs`); `cargo test -p pod-enter --all-features` passed. Plant `enter-userns-no-own-devpts` (exit 41: no `/dev/ptmx`), caught; the patches of `enter-ns-pts-unbound` and `enter-userns-proc-not-bound` follow the new code, and both are caught. Not measured: a host whose kernel refuses the new instance (the fallback and its line): T-868.

# T-868: `pod-enter`: a test of the fallback where the kernel refuses the devpts instance of the unit

**Source:** T-864 (PodUtils/podvm#469).
**Category:** chore
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

On the namespace rung in a user namespace, where the kernel refuses the new devpts instance (`ns_pts`), the child writes the line of `devpts_refused`, binds the host's `/dev/pts` and binds the host's `/dev/ptmx`. No test runs this path: the kernels of CI and of the cloud host mount the instance. A defect of the fallback (no line, no bind of `/dev/ptmx`, a bind in the wrong order) passes each check.

## Premise

- Read: `crates/pod-enter/src/ns_pts.rs`, `src/ns_optional.rs` (`mount_each`), `src/ns_dev_bind.rs` (`bind_each`, `Ptmx`), `tests/userns/pts.rs`.

## Approach

1. Find a way to make the kernel refuse the mount of devpts in the user namespace of a test with no change of the product: a lab of `pod-lab` (a seccomp filter that refuses `mount(2)` with the type `devpts` is not possible, as seccomp does not read strings; an LSM or an older kernel can), or a fault switch that only a test build has.
2. A test in `tests/userns/pts.rs`: the payload gets the line, `/dev/pts` is the host's, `/dev/ptmx` is the host's node, and an open of `/dev/ptmx` passes.
3. A plant of the fallback.

## Prove

```sh
sh scripts/ci/userns-tests.sh
```

## Done

2026-11-17, 19479da04, a780db8e3. The test `a_refused_devpts_instance_falls_back_to_the_host_s_pts` (`tests/userns.rs`, `tests/userns/pts.rs`) needs no change of the product: a seccomp filter on the thread of the test (`pod_confine::Deny` on `mount(2)` with the flags `DEVPTS_FLAGS`, `EPERM`), which the child takes, refuses the mount of the instance and no other mount. As uid 65534 with no capabilities, the payload gets the line of `devpts_refused` with errno 1, `/dev/pts` and `/dev/ptmx` are on the device of the host's devpts, and an open of `/dev/ptmx` passes exactly where the host's `/dev/pts/ptmx` lets each user read and write it. The test found a defect of T-864: the bind of the host's `/dev/ptmx` never opens (`ENOENT`, also with the host's devpts bound beside it; a test by hand with `unshare -Urm` as uid 65534 on kernel 6.18 gave the same). Fix: `ns_dev_bind` has one plan of `/dev/ptmx`, the `ptmx` of the devpts on the image's `/dev/pts` (`NS_PTMX_BIND_STEP`, its step now `binding the ptmx of the devpts on /dev/pts`), and `bind_each` takes no `own_pts`. On a host with `ptmxmode=000` (the hosts of CI and of the cloud), that `ptmx` does not open for the caller: the line of the refusal tells the consumer that the unit has the host's devpts. `sh scripts/ci/userns-tests.sh` passed (31 tests); 242 unit tests of `pod-enter` pass. Plant `enter-userns-devpts-fallback-unbound` (the fallback binds nothing on `/dev/pts`: exit 41), caught; `enter-userns-no-own-devpts` follows the fallback (exit 42: `/dev/pts` is the host's), caught.

# T-872: `pod-supervise`: the temp file of the table of containers keeps no lock, so a sweep of another process deletes it before its rename (PodUtils/podvm#568)

**Source:** PodUtils/podvm#568 (COORD-181 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The table is written to `staging/containers.json.PID.NANOS.partial` and renamed, with no `flock` on the temp file. podvm's `Store::open` sweeps each `.partial` that it can lock, so a second podvm process deleted the file before its rename: `run -d` of a fast payload ended `Dead` with no exit code in 6 of 10 runs, and `run` failed with `containers.json: No such file or directory`. The launcher also drops a failed write of the end of a run (`let _ = record(..)`).

## Premise

- Read: PodUtils/podvm#568; `crates/pod-supervise/src/table.rs` (the write of the table), `src/launcher.rs` (the record of the end).

## Approach

Hold an exclusive `flock` on the temp file from its create to its rename. Retry a failed write of the end of a run a few times, and keep the end in a side file that the reconcile reads when each try fails. Tests: a sweep that locks each `.partial` it can does not delete a file that a write holds; an end whose first write fails is in the table after the reconcile; a plant.

## Prove

```sh
cargo test -p pod-supervise --all-features
```

## Done

2026-11-17, 2b7acb6a3, 217c00611: the write of the table makes its temp file under a name that no sweep reads (`.partial.new`), takes its exclusive `flock`, and only then names it `.partial` (`create_locked`): a sweep of the staging directory never sees it unlocked, and the lock holds until the rename. A rename whose temp file went anyway starts the write again with a new file (8 tries). The launcher writes the end of a run with 5 tries, 200 ms apart (`record_end`); where each fails, it writes `end.json` beside the record (`end_note`), and `reconcile` gives a gone launcher's record that exit code and end in place of `Dead`. Tests: `the_temp_file_of_a_write_is_locked`; `a_sweep_never_takes_the_temp_file_of_a_write` (`tests/sweep.rs`, 00757e62c; a thread that deletes each `.partial` whose lock it gets, beside 200 writes: it takes none, and each write lands); `the_reconcile_reads_the_end_that_a_launcher_left_beside_the_record`. Not tested: the tries of `record_end` (no failure of the table can be made for a test as root). Plant `supervise-table-temp-unlocked` (no lock), caught.

# T-873: `pod-hostprobe`: the probe file of `probe_dir_exec` has a name that two processes of two PID namespaces share (PodUtils/podvm#568)

**Source:** PodUtils/podvm#568 (COORD-181 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`exec::probe_dir_exec` names its file `.pod-hostprobe-exec.PID.SEQ`. Each foreground run of podvm is pid 2 in its own PID namespace, so 6 of 12 parallel runs used one file, and one run removed it under another (`No such file or directory`).

## Premise

- Read: PodUtils/podvm#568; `crates/pod-hostprobe/src/exec.rs` (`probe_dir_exec`).

## Approach

Create the probe file with `O_CREAT | O_EXCL` and a random nonce in its name, and try again with a new nonce on `EEXIST`. Tests: two probes of one directory with the same pid and sequence take two files; a plant.

## Prove

```sh
cargo test -p pod-hostprobe --all-features exec
```

## Done

2026-11-17, 8450eb4a2, 431d793c2: `probe_dir_exec` creates its file with `create_new` (`O_CREAT | O_EXCL`) and mode 0600 (`create_probe`), under a name with the pid, the sequence and a nonce of 8 random bytes of the system (the clock where the system gives none); a name that exists takes the next one, 8 names at most. Test `a_probe_file_never_takes_the_file_of_another`: a name that another run holds is skipped and its file stays as it was; each name taken is an error; two names differ. The tests of `exec` pass. Plant `hostprobe-exec-probe-overwrites` (a create that truncates), caught.

# T-874: `pod-oci`: a credential helper gets `ServerURL` `https://HOST/v2/`, not the key of the configuration (PodUtils/podvm#568)

**Source:** PodUtils/podvm#568 (COORD-181 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

docker and podman give a credential helper the server key of the configuration (`HOST`, or `https://index.docker.io/v1/` for Docker Hub) for `get`, `store` and `erase`. `pod-oci` gives `https://HOST/v2/`, so a helper that keys on the configured name finds nothing.

## Premise

- Read: PodUtils/podvm#568; `crates/pod-oci/src/credentials.rs`.

## Approach

Give the helper the key of the configuration that matched (the `auths` key or the `credHelpers` key), and `https://index.docker.io/v1/` for Docker Hub. Tests: the input of the helper for a registry, for Docker Hub and for a key with a scheme; a plant.

## Prove

```sh
cargo test -p pod-oci --all-features credentials
```

## Done

2026-11-17, ae1c6384f, 4428352d3: `credentials::helper_server(host)` gives the `ServerURL` of docker and podman: the host as the configuration names it, or `HUB_SERVER` (`https://index.docker.io/v1/`) for each name of Docker Hub (`docker.io`, `index.docker.io`, `registry-1.docker.io`); `lookup_in`, `store_login_in` and `remove_login_in` give it to `get`, `store` and `erase`. `credHelpers` finds the helper of Docker Hub by `index.docker.io`, the name that docker writes, for each name of the hub. Tests: `a_helper_gets_the_server_key_of_docker`, `the_helper_of_docker_hub_is_found_by_each_of_its_names`; the tests of `credentials` pass. Not run: a real helper (no helper on the host of the tests). Plant `oci-helper-gets-the-api-url`, caught.

# T-875: `pod-supervise`: `run -d --rm` keeps the record and the tree after the end (PodUtils/podvm#568)

**Source:** PodUtils/podvm#568 (COORD-181 of podvm).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

A detached run has no field for `--rm`: after its end, its record and its tree stay. podvm refuses `-d --rm` by name until the launcher removes them.

## Premise

- Read: PodUtils/podvm#568; `crates/pod-supervise/src/launcher.rs`, `src/table.rs` (the detached record), `src/end.rs`.

## Approach

A field `rm` of the detached record. After the end is written and each waiter has its answer, the launcher removes the record and the tree. Tests: a detached run with `rm` leaves no record and no tree; a waiter gets the exit code first; a plant.

## Prove

```sh
cargo test -p pod-supervise --all-features
```

## Done

2026-11-17, 184f2dc62, 4536f72df: `Container::rm` (`serde(default)`, left out of the file when false, so a table of the form before reads) and `pod_supervise::auto_remove(s, want)`, which sets it on a record that is not running (a running one is a refusal: set it before the start). After the end of a detached run, the launcher writes the end, answers each waiter with `exit CODE`, reads `rm` from the record again, and removes the record and the directory of the container. Test: the lifecycle step `a_run_with_rm_leaves_no_record_after_its_waiter` (a waiter gets 4, then `get` is `NotFound` and the directory is gone); the tests of `pod-supervise` pass. Plant `supervise-rm-keeps-the-record`, caught. podvm can drop its refusal of `-d --rm` with the take.

# T-876: `pod-enter`: a volume whose source a link cannot redirect between the check of the caller and the bind (PodUtils/podvm#568)

**Source:** PodUtils/podvm#568 (COORD-181 of podvm).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The job of a unit can make a symbolic link in a `-v` source after podrunner checked the source and before `pod-enter` binds it (PodUtils/podrunner#277). `Plan::volumes` binds a path.

## Premise

- Read: PodUtils/podvm#568; `crates/pod-enter/src/ns_volume.rs`, `Plan::volumes` (`VolumeBind`).

## Approach

An option of `VolumeBind`: a base that the caller names. `pod-enter` opens the source with `openat2(RESOLVE_NO_SYMLINKS | RESOLVE_BENEATH)` under that base, before the fork, and binds the descriptor (`/proc/self/fd/N` or `move_mount`), not the path. A refused open is a refusal that names the volume. Tests: a source with a link under the base is refused by name; a plain source binds; a plant.

## Prove

```sh
cargo test -p pod-enter --all-features volume
```

## Done

2026-11-17, 657e5af28, 6f64fabf1: `VolumeBind::host_base: Option<String>` (and `VolumeBind` is `Default`). With a base, `host` must be the base or under it; before the fork `prepare` opens it with `openat2(O_PATH, RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)` from the base, and a link under the base (`ELOOP`, `EXDEV`) is a `Policy` refusal that names the volume: `its host path holds a symbolic link under its base`. In the child the source opens again the same way, and `mount_fd_on_fd` binds `/proc/self/fd/SRC` onto `/proc/self/fd/POINT`: a link made in the source after the open cannot redirect the bind. A refusal in the child is the step 48 (`NS_VOLUME_SOURCE_STEP`), named by the volume. Tests: `a_host_path_under_its_base_holds_no_link` (a link as the last component and on the way are refused, the base itself and a plain path open, a path out of the base is a usage error); the read-only volume of `the_namespace_rung_binds_the_volumes_of_a_plan` binds by its descriptor under its base, as root and as uid 65534 (`sh scripts/ci/userns-tests.sh`: 31 tests passed). Plant `enter-volume-base-not-checked`, caught; `enter-volume-ro-writable` follows the new code, caught.

# T-884: `pod-enter`: SIGHUP is not blocked and forwarded to the payload (PodUtils/podvm#569)

**Source:** PodUtils/podvm#569 (COORD-182 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`child.rs` blocks and forwards SIGINT and SIGTERM, not SIGHUP: SIGHUP takes the default action in pid 2, and the payload and `--podvm-ns-reaper` stay after the run ended (podvm #562).

## Premise

- Read: PodUtils/podvm#569.

## Approach

Add SIGHUP to the set that the entry blocks and forwards, and to `sigblock_shutdown` of `pod-hostprobe` where it has one. A test: a SIGHUP to the entry reaches the payload, and nothing stays.

## Prove

```sh
cargo test -p pod-enter --all-features
```

## Done

2026-11-17, 058c5c1d3, 1f2c23cc8: `sys::SHUTDOWN_SIGNALS` holds SIGHUP beside SIGINT and SIGTERM, so `sigblock_shutdown` and `signalfd_shutdown` block and read it: `Child::wait_forwarding` of `pod-enter` forwards it to the payload and names it (`signal_name`), and the launcher of `pod-supervise`, which reads the same set, forwards it too. The init of a PID namespace forwarded SIGHUP before. `sys::SYS_TGKILL` and `SYS_GETTID` are new. Tests: `a_sighup_of_the_waiter_reaches_the_payload` (a `tgkill` of SIGHUP to the waiting thread ends the payload with 129, and the line names SIGHUP); the test of the mask holds 16387. The tests of `pod-enter`, `pod-hostprobe` and `pod-supervise` pass. Plant `enter-sighup-not-forwarded`, caught.

# T-885: `pod-enter`: a fallback to chroot is not returned to the caller (PodUtils/podvm#569)

**Source:** PodUtils/podvm#569 (COORD-182 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

An image with no `/tmp` mount point makes the entry fall back to chroot, but the caller fixed the rung before the entry, so its record says `namespace`, and `--strict` passes (podvm #547).

## Premise

- Read: PodUtils/podvm#569.

## Approach

The entry returns the rung that it entered (and why it fell back), so the caller can record it and refuse it under `--strict`. A test with an image that has no `/tmp`.

## Prove

```sh
cargo test -p pod-enter --all-features
```

## Done

2026-11-17, 0c5f4d60c, 961f36409: `Child` has `entered: Option<Rung>`, the rung that the entry entered (`spawn_with` sets it from the word of the rung that it enters; the fallback to chroot enters with the word of `ENTERED_RUNG`, so a caller that selected the namespace rung sees `Some(Chroot)`). `Child::of(pid)` makes a child of no entry (`entered: None`). A strict caller compares `entered` with the rung that it selected. Tests: `the_child_names_the_rung_that_it_entered` (each word gives its rung back), and the run of a caller with no capabilities (`tests/userns.rs`) checks `entered == Some(Namespace)`; `sh scripts/ci/userns-tests.sh` passed (31 tests). Not run: a real fallback to chroot (it needs root, and the userns tests of CI run as a user). Plant `enter-entered-rung-unnamed`, caught. Breaking: a literal `Child { pid }` takes `Child::of(pid)`.

# T-886: `pod-enter`: a working directory under `/tmp` gives ENOENT (PodUtils/podvm#569)

**Source:** PodUtils/podvm#569 (COORD-182 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The caller makes `-w /tmp/NEW` in the tree; the child mounts the tmpfs on `/tmp` (step 9) before the `chdir` (step 11), which hides it (podvm #512).

## Premise

- Read: PodUtils/podvm#569.

## Approach

After the tmpfs and the volumes, the child makes the working directory (`mkdir -p` through the root, with no link) where it is under a mount of the rung. A test.

## Prove

```sh
cargo test -p pod-enter --all-features
```

## Done

2026-11-17, 584061289, 59cb4eb56: on the namespace rung, the components of a working directory under `/tmp` are built before the fork (`ns_volume::tmp_workdir_parts`); where the `chdir` after the chroot fails with `ENOENT`, the child makes each missing component from `/` (`mkdirat`, then `openat` with `O_NOFOLLOW`: no link on the way), as the ids of the payload, and `fchdir`s into it (`make_workdir`, no allocation); a failure there is the step 11 as before. Test: the run of a caller with no capabilities (`tests/userns.rs`) starts a payload in `/tmp/wd/deep`, a directory of the image that the tmpfs hides, and its write lands in the tmpfs, not in the image; `sh scripts/ci/userns-tests.sh` passed (31 tests). Plant `enter-tmp-workdir-hidden`, caught.

# T-887: `pod-supervise`: `logs -f` ends at a restart (PodUtils/podvm#569)

**Source:** PodUtils/podvm#569 (COORD-182 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The poll of 100 ms sees the state `Exited` between the stop and the start of a restart, drains once and returns (podvm #550).

## Premise

- Read: PodUtils/podvm#569.

## Approach

End the follow only when, after the drain, the start time and the pid of the record did not change. A test with a restart.

## Prove

```sh
cargo test -p pod-supervise --all-features
```

## Done

2026-11-17, 96c2f088c, 793b10a6e: `follow_once` gives the record where the container ended; after the final drain, `follow_from` reads the record again and ends only where `ended_for_good` holds (an end state, the same start and the same finish); else the run started again, and the follow goes on from its offset. Test `a_restart_does_not_end_the_follow` (the same ended run ends; a run that started again, and one that started and ended again, do not); the tests of `pod-supervise` pass. Plant `supervise-follow-ends-at-a-restart`, caught.

# T-888: `pod-supervise`: the log, the lock and the memo of a container follow a symbolic link (PodUtils/podvm#569)

**Source:** PodUtils/podvm#569 (COORD-182 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

A link at `containers/ID/log` sends the output of the payload to the target of the link (podvm #536).

## Premise

- Read: PodUtils/podvm#569.

## Approach

Open the log, the lock and the memo with `O_NOFOLLOW` through the descriptor of `table::contained()`. A test with a link at each.

## Prove

```sh
cargo test -p pod-supervise --all-features
```

## Done

2026-11-17, 75d5cdfeb, bd7822ccb: the launcher refuses a container whose directory is not inside the containers (`table::contained`), and opens the log with `O_NOFOLLOW`; `logs` reads it with `O_NOFOLLOW`; each `Lock` opens its file with `O_NOFOLLOW`, so a link at the lock of a container (or of the table) is refused. The "memo" of the ask is podvm's: pod-supervise has no such file. Test: the lifecycle step `a_link_at_the_log_or_the_lock_is_refused` (a link at the log and at the lock: the start fails, and the target of the link stays as it was); the tests of `pod-supervise` pass. Plant `supervise-log-follows-a-link`, caught.

# T-889: `pod-rootfs`: `run_mtab` says /proc is not mounted, where the entry mounts it (PodUtils/podvm#569)

**Source:** PodUtils/podvm#569 (COORD-182 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`net.rs` `run_mtab` reports "/proc is not mounted in the root" where the entry of the namespace rung mounts it, so `--strict=all` refuses each run (podvm #557).

## Premise

- Read: PodUtils/podvm#569.

## Approach

A setting "the entry mounts /proc"; with it, a link of `etc/mtab` into `/proc` stays and is no degradation. A test.

## Prove

```sh
cargo test -p pod-rootfs --all-features
```

## Done

2026-11-17, 1dc30c45a, 419594d81: `pod_rootfs::Settings::entry_mounts_proc` (false by default). With it, `etc/mtab` as a link to `/proc/...` or a relative `proc/...` (after `../`) stays: `Unchanged`, not degraded, "a symlink to T, which resolves in the /proc that the entry mounts". Without it the link is replaced, as before. Test `mtab_keeps_a_link_into_proc_where_the_entry_mounts_proc` (an absolute and a relative link stay with the setting, and are replaced without it); the tests of `net` pass. Plant `rootfs-mtab-proc-replaced`, caught. podvm sets it on the namespace rung.

# T-890: `pod-hostprobe`: the confinement key reads the namespaces of the reading process (PodUtils/podvm#569)

**Source:** PodUtils/podvm#569 (COORD-182 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`identity::confinement_key` reads `/proc/self/ns/mnt`, `uid_map` and the capabilities of the reader. podvm reads it in its own new user and mount namespace, so the key never repeats and `--cached` never serves (podvm #558).

## Premise

- Read: PodUtils/podvm#569.

## Approach

A key of a given process (`/proc/PID/...`); podvm passes the pid of its outer process. A test that two reads of one process give one key.

## Prove

```sh
cargo test -p pod-hostprobe --all-features identity
```

## Done

2026-11-17, da7a0f62a, 21567f2cf: `identity::read_of(pid)` reads the identity of a given process from `/proc/PID` (its maps, its status, and its uid, gid and groups from the lines `Uid`, `Gid` and `Groups`), and `confinement_key_of(pid, interpreter)` gives its key with its own mount namespace (`/proc/PID/ns/mnt`). `read()` and `confinement_key` are the case of `/proc/self` of the same reader. Test `the_key_of_a_pid_is_the_key_that_the_process_reads` (the key of this pid equals the own key; a pid with no process reads nothing; a child under `setpriv --no-new-privs` reads `NoNewPrivs` 1 where this process reads 0). Plant `hostprobe-key-of-the-reader`, caught. podvm passes the pid of its outer process.

# T-891: `pod-hostprobe`: the census probe of exec uses the fixed path `/tmp/execprobe` (PodUtils/podvm#569)

**Source:** PodUtils/podvm#569 (COORD-182 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

A file that stays at `/tmp/execprobe` (a killed probe, another tool) makes each later probe skip the row of exec (podvm #561).

## Premise

- Read: PodUtils/podvm#569.

## Approach

An `O_EXCL` name with a random nonce, as T-873. A test with a file at the old path.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
```

## Done

2026-11-17, be4af27ad, bc0e8b4a9: `probes::scratch::unique(base)` gives `BASE.PID.NONCE` (8 bytes of `getrandom`, the clock where it gives none). Each scratch path of a probe takes it, not only the one that podvm named: `exec(/tmp/execprobe)`, the two rows of `mknod`, the rows of `chown`, and `move_mount` (whose directory now goes after the probe). The row names stay. The create stays exclusive. Test `a_scratch_path_is_new_for_each_probe` (two names differ; a file at the old fixed path does not make the row of exec a skip); the tests of `pod-hostprobe` pass. Plant `hostprobe-scratch-path-fixed`, caught.

# T-892: `pod-hostprobe`: the never-claim text of the namespace rung is out of date (PodUtils/podvm#569)

**Source:** PodUtils/podvm#569 (COORD-182 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`select.rs` says the namespace rung has no "user, pid or network isolation, or a /proc filesystem"; since `v0.63.0` and `v0.66.0` a run on that rung has a user, a PID and a UTS namespace and its own `/proc` (podvm #546).

## Premise

- Read: PodUtils/podvm#569.

## Approach

A text built from the namespaces that the entry opened. A test of each form.

## Prove

```sh
cargo test -p pod-hostprobe --all-features select
```

## Done

2026-11-17, 881c3cb8b, b4b815473: `select::Opened { user, pid, proc }` and `Rung::must_never_claim_with(opened)`: for the namespace rung, "network isolation" and each of user isolation, pid isolation and a /proc filesystem that the entry did not open; the other rungs keep their text. `must_never_claim()` stays the conservative text for a caller that does not know what the entry opened (a never-claim must not claim more). Test `the_never_claim_follows_what_the_entry_opened`; the tests of `select` pass. Plant `hostprobe-never-claim-fixed`, caught. podvm builds `Opened` from its plan (`userns_ids`, `pid_namespace`) and the lines of the entry.

# T-893: `pod-guest`: QEMU starts in its own process group with no `PR_SET_PDEATHSIG` (PodUtils/podvm#569)

**Source:** PodUtils/podvm#569 (COORD-182 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

A SIGTERM to the run leaves QEMU running, and a Ctrl-C to the group misses it (podvm #566).

## Premise

- Read: PodUtils/podvm#569.

## Approach

`PR_SET_PDEATHSIG(SIGKILL)` in `pre_exec` of the emulator, checked against the parent's pid after the set. A test with a stand-in emulator.

## Prove

```sh
cargo test -p pod-guest --all-features
```

## Done

2026-11-17, b5ca5c03e, a62c7c3cf: `pod_hostprobe::sys::die_with_parent(&mut Command)` sets `PR_SET_PDEATHSIG(SIGKILL)` between the fork and the exec, and fails the spawn where the parent ended before the set (`getppid` against the pid before the fork); `pod-guest` (no unsafe code) calls it in `spawn_emulator`, which keeps its own process group. The kernel sends the signal when the THREAD that spawned the emulator ends: the doc of `spawn_emulator` says to call it from a thread that lives as long as the emulator. Test `the_emulator_ends_with_the_thread_that_started_it` (an emulator spawned from a thread that ends dies on SIGKILL); 177 tests of `pod-guest` pass. Plant `guest-emulator-outlives-its-caller`, caught. podvm: spawn the emulator from the main thread (or one that lives as long as the run).

# T-894: `pod-oci`: a wanted platform variant matches an entry with no variant (PodUtils/podvm#569)

**Source:** PodUtils/podvm#569 (COORD-182 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`spec.rs` matches `linux/amd64/v3` to an entry of plain `linux/amd64` (podvm #528, #544).

## Premise

- Read: PodUtils/podvm#569.

## Approach

A strict match of the variant, with the fallback of docker only where it holds (`arm/v7` and no variant for `arm64`), named. A table-driven test.

## Prove

```sh
cargo test -p pod-oci --all-features spec
```

## Correction

The Approach (a strict match) is not taken: docker and podman take a lower variant and no variant for a wanted one, as a CPU of a variant runs them (call C778).

## Done

2026-11-17, 7c4049e02, 066d4cb80: `select_platform` ranks each entry of the wanted OS and architecture by `ranked(arch, want)`, containerd's order (amd64 `v3`: `v3`, `v2`, `v1`, no variant; arm `v7`: `v7`, `v6`, `v5`, no variant; arm64: the wanted, `v8`, no variant), and takes the best, the first in the index where two rank the same; a variant above the wanted one is never taken, and an ask with no variant takes the first entry. Before, a wanted `v3` took no `v2` entry and took the first entry with no variant. `linux/amd64/v3` still resolves to `linux/amd64` where the index has no `v3` or `v2`: the platform of the manifest says so, and podvm records it. Test `a_wanted_variant_takes_the_best_compatible_entry` (a table of amd64, arm and arm64); the tests of `pod-oci` pass. Plant `oci-variant-loose-match`, caught.

# T-895: `pod-extract`: a hard link to a later member says "already extracted" (PodUtils/podvm#569)

**Source:** PodUtils/podvm#569 (COORD-182 of podvm).
**Category:** defect
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`apply.rs` gives "cannot link X to Y, which the layer says is already extracted: ENOENT" for a hard link whose target comes later in the layer (podvm #525).

## Premise

- Read: PodUtils/podvm#569.

## Approach

Defer a hard link whose target is not there yet to the end of the layer, and say "not yet in the layer" where it stays missing. A test.

## Prove

```sh
cargo test -p pod-extract --all-features
```

## Done

2026-11-17, 0cb1e7317, 674714959: a hard link whose target is not there yet (`ENOENT` at `linkat`) waits for the end of its layer (`apply_later.rs`: `Later`, `link_later`), and is made then with the same checks (no link on the way, no target outside the root); its sidecar row comes then. A target that is still missing gives `cannot link P to T: the target is not in the layer or before it`; another failure names the link with no claim about the layer. Test `a_hard_link_before_its_target_waits_for_the_end_of_the_layer` (the link shares the inode of a target that comes after it; a target that never comes is named as missing); the tests of `pod-extract` pass. Plant `extract-hardlink-not-deferred`, caught.

# T-944: `pod-enter`: the head of a payload cannot give a `PT_INTERP` path that lies past it, so Fedora 42's programs do not run on the userland rung (PodUtils/podvm#589)

**Source:** PodUtils/podvm#589 (COORD-193 of podvm; PodUtils/podvm@T-1981).
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

On a host that denies `chroot(2)` and user namespaces, podvm's userland rung classifies the payload with `pod_enter::abi` from the head that `ladder::payload_head` reads (64 KiB). Where `PT_INTERP` lies past it, the answer is "unknown": "the PT_INTERP path lies past the 65536 bytes read, so the reader cannot tell a static file from a dynamic one". Fedora 42's `/usr/bin/bash` has `PT_INTERP` at the file offset `0xf2000`, so no program of `quay.io/fedora/fedora:42` with that layout runs there: `podvm run quay.io/fedora/fedora:42 sh -c true` exits 125. The program header table is in the head; only the path is past it.

## Premise

- Read: PodUtils/podvm#589; `crates/pod-enter/src/abi.rs` (`interp_of`, `interp_of_file`), `src/abi/image.rs` (`Src::File` reads with `pread` in pieces), `src/ladder.rs` (`Payload` keeps its open file), `src/memfd.rs` (`eligible`).

## Approach

`abi::interp_of_open(path, &File)`: the classification of a file that the caller holds open, which reads a path past the head with a bounded `pread` (`Src::File`); `Payload::interp()` and `memfd::eligible_payload(&Payload)` use it. `interp_of` of a slice keeps its answer "unknown". podvm classifies with `Payload::interp` or `eligible_payload`.

## Prove

```sh
cargo test -p pod-enter --all-features --lib
```

## Done

2026-11-18: `abi::interp_of_open` (`interp_of_file` calls it), `ladder::Payload::interp`, `memfd::eligible_payload`. Tests: `a_header_past_the_head_is_unknown_and_never_static` (an open file reads the path at `0xf2000`; the head alone stays "unknown") and `a_payload_with_its_interp_past_the_head_is_dynamic` (`payload_head`, then `Payload::interp` gives the loader and `eligible_payload` refuses with `HasInterp`). `cargo test -p pod-enter --all-features --lib`: 246 passed. Plant `enter-interp-from-the-head-alone`, caught.

# T-956: `pod-extract`: a layer of more than one gzip member or zstd frame is cut short after the first, with no error (PodUtils/podrunner#317, ask 2)

**Source:** PodUtils/podrunner#317 (COORD-107; PodUtils/podrunner@T-560)
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`layer::open_hashed` read a gzip layer with `flate2::read::GzDecoder` and a zstd layer with `ruzstd`'s `StreamingDecoder`. Each stops at the end of its first member or frame. A valid layer of many members (eStargz, bgzip, a concatenated stream) or of many zstd frames gave the tar reader the first part alone, and the extraction ended with no error. The digest stayed true, as `HashedStream::finish` hashed the rest of the blob.

## Premise

- Read: PodUtils/podrunner#317; `crates/pod-extract/src/layer.rs`; flate2's `bufread::GzDecoder` (it consumes only the bytes that it uses) and ruzstd 0.9.1's `StreamingDecoder` (it reads only the bytes of its frame; `into_inner`).

## Approach

`members::Members` reads each member in turn: after a member, the next one starts only when the bytes that follow begin with its magic (`1f 8b`; `28 b5 2f fd`), so bytes after the last member (zeros of a padded blob) stay for `finish`. A zstd skippable frame is skipped. A read-ahead (`Ahead`) looks at the magic across the end of its buffer.

## Prove

```sh
cargo test -p pod-extract --all-features --lib members
cargo test -p pod-extract --all-features --lib layer::tests::a_layer_of_two_gzip_members_reads_whole_and_hashes_whole
```

## Done

2026-11-19, commit "pod-extract: each gzip member and each zstd frame of a layer (T-956, PodUtils/podrunner#317)". `crates/pod-extract/src/members.rs` with four tests (each member for gzip and zstd, with zeros after the last; a zstd skippable frame; a magic across a read of one byte; a blob of another kind and a member cut short fail), and a test of `open_hashed` with a tar in two gzip members: the stream is whole and the digest is that of the whole blob. The plant `extract-first-member-alone`. The size of a gzip layer from its trailer is the size of its last member alone in a layer of many members; the doc of `uncompressed_size` says so, and the space check stays an estimate there. The test: `layer::tests::a_layer_of_two_gzip_members_reads_whole_and_hashes_whole` of `crates/pod-extract/src/layer.rs`.

# T-957: `pod-extract`: no extended attribute of a layer is applied, so `security.capability` of `ping` is lost (PodUtils/podrunner#317, ask 1)

**Source:** PodUtils/podrunner#317 (COORD-107; PodUtils/podrunner@T-558); podvm's same ask for its sidecar and unified tarball (COORD-106 of podvm)
**Category:** feature
**Milestone:** M6
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

podrunner publishes a copy of the root file system of a GitHub-hosted runner as an OCI image whose PAX tar layers carry `SCHILY.xattr.*` records, among them `security.capability` of `ping` and `mtr-packet`. `crates/pod-extract/src/apply.rs` applies no extended attribute: each is dropped with no word, and `ping` then fails in the unit.

## Premise

- Read: PodUtils/podrunner#317; `crates/pod-extract/src/apply.rs` (no path for an extended attribute).
- Not read: how the `tar` crate gives the PAX records of an entry (`Entry::pax_extensions`); podvm's COORD-106.

## Approach

Read the `SCHILY.xattr.NAME` records of each entry, and set each with `lsetxattr` after the file is written and before its mode and times are set (a write after a set of `security.capability` clears it). Set the attributes of the `user.` space in each case, and those of `security.` and `trusted.` when the process has the privilege; report each attribute that the extraction cannot set, with its name and its path, in the result of the extraction and in its log line, never a silent drop. A test: a layer with a file with `security.capability` and one with `user.test` round-trips where the privilege holds, and the report names `security.capability` where it does not.

## Prove

```sh
cargo test -p pod-extract --all-features xattr
```

## Done

2026-11-19, this commit. `crates/pod-extract/src/xattr.rs`: `read` takes the `SCHILY.xattr.NAME` records of each entry before its content, inside the bounds of Linux (a name of 255 bytes, a value of 64 KiB, 64 KiB of names) and of the crate (1 MiB of values and 1024 records for each entry); a record past a bound, a mark of overlayfs (`trusted.overlay.`, `user.overlay.`) and a record that the `tar` reader cannot parse are named, with no value kept. A file gets each attribute through its descriptor after its last write and before its `fchmod`; a directory, a hard link (also one that waits for the end of its layer) and a symlink get them after they are made, through an `O_NOFOLLOW` descriptor or `lsetxattr` of `/proc/self/fd/N/NAME`. The kernel decides: each refusal (`EPERM` for `security.capability` with no `CAP_SETFCAP`) is named with its path and its errno in `LayerStats::xattrs` and `Extracted::xattrs` (`xattr::Count`: set, not set, the first 8), at the end of the line of the layer, and in the sidecar row of its path with its value (`"xattrs":[{"name","value":"0x..","reason"}]`, call C791). A refused set of a value that the file has already is a set (a hard link to a target of mode 0444). `pod-hostprobe` has `lsetxattr`. Tests: `drive::xattrs::extended_attributes_are_set_or_named` (a file with `security.capability`, files, a directory of mode 0555, a symlink and a hard link with `user.test`; each is set when a probe of the same name on the same kind of object sets, else named; as root `security.capability` sets and the worker as uid 1000 names it; in `scripts/ci/root-uid1000-tests.sh`), `drive::xattrs::a_record_past_a_bound_is_named_and_not_kept`, `sidecar::tests::an_attribute_not_set_is_in_the_row_with_its_value`. `cargo test -p pod-extract --all-features`: 89 passed. The plant `extract-xattrs-dropped`. T-961 holds what the `tar` reader does with the records before this crate reads them.

# T-961: `pod-extract`: the `tar` crate reads a PAX header or a GNU long name whole with no bound, and cuts the PAX records at each newline byte

**Source:** T-957 (a finding of its work)
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

`tar` 0.4.46 reads the data of a PAX extended header (`x`) and of a GNU long name or long link (`L`, `K`) into one buffer of the size that its header gives, with no bound: a layer with a header that gives 8 GiB makes the extraction allocate 8 GiB before `pod-extract` sees the entry. Its `PaxExtensions` cuts the records at each newline byte and not by the length of each record: a `SCHILY.xattr.` value with a byte 0x0a (a capability set with bits 1 and 3 in one byte, a `security.capability` of revision 3 with root id 10, a binary `user.` value) does not parse, and the lines after it can parse as records that the writer did not write. T-957 names such a record as not set and reads no record of the entry after it; the attribute is lost.

## Premise

- Read: `tar` 0.4.46 `src/archive.rs` (`next_entry`: `EntryFields::from(entry).read_all()` for `x`, `L` and `K`), `src/entry.rs` (`read_all`: `read_to_end`, the capacity capped at 128 KiB, the size not capped), `src/pax.rs` (`PaxExtensions::new`: `a.split(is_newline)`; `Builder::append_pax_extensions` writes the length of each record).
- Not measured: the memory of a crafted layer; how many real images have a value with a newline byte.

## Approach

Read each layer with `Entries::raw(true)` and handle the `x`, `L` and `K` members in the crate: read their data with a bound (1 MiB for a PAX header, 4096 bytes for a long name, as `PATH_MAX`), refuse a layer past it with a `Refused` cause, and parse each PAX record by its length (`LEN key=value\n`), so that a value can hold any byte. The crate then gives `path`, `linkpath`, `size`, `uid` and `gid` of the PAX records to the entry, as the `tar` reader does now, in both passes (`remove::collect` and `apply_layer`). Tests: a header that gives 2 MiB is refused with no buffer of that size; a value with a newline byte round-trips; the records of the current tests parse the same.

## Prove

```sh
cargo test -p pod-extract --all-features --lib pax
```

## Done

2026-11-19, this commit. `crates/pod-extract/src/pax.rs`: `Framed` reads the stream of a layer before the `tar` reader, in both passes (`remove::collect`, `apply_layer`), and follows its members. An `x` past `PAX_MAX` (1 MiB) and an `L` or a `K` past `LONG_MAX` (4096 bytes) are refused before their data is read, as `Refused` with the new cause `Refusal::Member` that names the member and its byte offset. The records of an `x` parse by their length (`LEN KEY=VALUE\n`, as POSIX and Go's `archive/tar`); a record that does not parse refuses the layer. The `SCHILY.xattr.` records and the `path` and `linkpath` records go to `Side`, one `Member` for each entry of the `tar` reader; the others go to the `tar` reader in a new `x` with its checksum, a `path` or a `linkpath` with no newline byte too, and one `size` record, the last, so that both readers frame the member the same. A record other than these with a newline byte refuses the layer. `xattr::read` takes the records of the side: a value with a byte 0x0a is set. The extension blocks of a GNU sparse header are followed. Tests: `pax::tests` (8: the parse by length, the count of the digits of a length, a binary value with a newline byte and a false `path` in it round-trips, a `path` with a newline byte, two `size` records, the bounds of `x`, `L` and `K` in both passes with a reader that fails on a read past the header, a record that does not parse, a global header). `cargo test -p pod-extract --all-features`: 97 passed. The plant `extract-t961-pax-header-unbounded`. The test of the bound: `pax::tests::a_member_past_its_bound_is_refused_before_its_data`.

# T-975: `pod-microvm`: a User-Mode Linux kernel in seccomp mode and its launcher, for a guest with no KVM, no ptrace and no namespace

**Source:** PodUtils/podvm#602 (COORD-206 of podvm, under the ruling "Shared code"; PodUtils/podvm@T-2005), for the parity table of PodUtils/podrunner#316
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

podvm's rung with no namespace, no root and no ptrace must run an image with systemd as PID 1 and its services as their own users (the parity table of PodUtils/podrunner#316). Its only route is the QEMU guest of `pod-guest` under TCG: 24 to 46 s for each run. User-Mode Linux in its seccomp mode (Linux 6.16 or later, `seccomp=on`) needs no ptrace, no namespace, no KVM and no QEMU, and gives a real kernel, PID 1, a writable cgroup2 and `setuid`; from 6.19 it has SMP (`ncpus=N`). podvm measured it (its experiment 467 at `38b766f`): Debian 12 with MariaDB and PostgreSQL, `seccomp=on ncpus=4`, a whole run in 14.7 s against 45.6 s under TCG. No crate of podcrates builds or starts such a kernel; `pod-microvm` is planned (`docs/design.md`) and does not exist.

## Premise

- Read: PodUtils/podvm#602; the row `pod-microvm` of `docs/design.md`; `crates/pod-guest/README.md`.
- Not read: podvm's experiment 467 (`experiments/467-uml-seccomp.sh`, `experiments/results/uml-seccomp.txt`); the 8 patches of pocket_vm (Apache-2.0 and MIT, https://github.com/flyinghorse0510/pocket_vm) on Linux 7.2: an SMP fault in `chan_kern.c` (`free_irq` from the SIGIO handler), a deadlock of the vector network driver, and the vector driver in a static kernel.
- podvm's answers (PodUtils/podvm#602, 2026-11-20): x86_64 alone (upstream `ARCH=um` runs on x86 hosts; one architecture is enough for podvm's rung); no patch of pocket_vm was needed (experiment 467 and the Debian 12 test ran on a stock Linux 7.3.5 with no network; pocket_vm's fixes are for a console under SMP and the vector network driver); the channel gives the exit code and the stdout and stderr of the guest's command as two streams, as the mailbox of `pod-guest` does; volumes come after the first release of the launcher.
- Not measured: a static `ARCH=um` kernel built in the CI of podcrates.

## Approach

1. A reproducible build of a static `ARCH=um` kernel for x86_64 (a stock mainline release, 7.3.5 or later, `CONFIG_STATIC_LINK`, seccomp mode, SMP, no network), with no patch unless a test of podcrates meets a fault of pocket_vm's list (then the patch with its source and licence), built in CI and published as an asset of each release with its SHA-256 in `SHA256SUMS` and `release-index.json` (the one rule: a static binary).
2. A launcher in `pod-microvm`: start the kernel with `seccomp=on ncpus=N`, the copy-on-write block device (`ubd0=COW,BASE`), and a channel that gives back the exit code and the stdout and stderr of the guest's command as two streams, in the form of the mailbox of `pod-guest`. Volumes wait for a later release.
3. A test that starts the kernel under a launcher that gives EPERM to `ptrace`, `unshare`, `setns`, `chroot` and `mount`, boots a small image to its PID 1, and reads its exit code.

Answer podvm on a COORD issue: podcrates takes it.

## Prove

```sh
cargo test -p pod-microvm --all-features
```

# T-976: `pod-enter`: the library path of the loader family has no `DT_RUNPATH` or `DT_RPATH` directory of a program or its libraries

**Source:** PodUtils/podvm#603 (COORD-207 of podvm; PodUtils/podvm@T-2003)
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

On podvm's userland rung the loader family runs a program through `ld.so --library-path DIRS` (with `--inhibit-cache` on glibc 2.16 or later). `userland::lib_dirs` builds DIRS from the directory of the loader, the standard directories of the image and the image's loader configuration (`ldconf`). It reads no `DT_RUNPATH` or `DT_RPATH` of the program or of its libraries, and `--library-path` takes the place of the search that ld.so does with them. So a program whose library is only in a directory of its own `RUNPATH` does not load: `systemctl` of systemd 261, with `libsystemd-shared-261.so` in `/usr/lib/systemd` (`RUNPATH=/usr/lib/systemd`), fails on that rung.

## Premise

- Read: PodUtils/podvm#603; `crates/pod-enter/src/userland.rs` (`lib_dirs`, `LoaderPlan`); `crates/pod-enter/src/abi/closure.rs` (`guest_lib_dirs`, `link_set`).
- Not read: whether `abi::Elf` reads the dynamic section's `DT_RUNPATH`, `DT_RPATH` and `DT_STRTAB` now.

## Approach

`abi::Elf` reads `DT_RUNPATH` and `DT_RPATH`. For a program, `lib_dirs` puts, before the standard directories, the `RUNPATH` (else `RPATH`) directories of the program and then of each library that it loads, in the order of ld.so, with `$ORIGIN` (and `${ORIGIN}`) resolved against the guest directory of the file that names it, `$LIB` and `$PLATFORM` as ld.so takes them on that machine, each resolved inside the image through the table as the others. A table test: a program with `RUNPATH=$ORIGIN/../lib/x` and a library only there loads; a program with `RUNPATH=/usr/lib/systemd`; a `RUNPATH` that leaves the image is not added.

## Prove

```sh
cargo test -p pod-enter --all-features runpath
```

## Done

2026-11-20, this commit. `abi::Search` (`crates/pod-enter/src/abi/dynamic.rs`)
reads `DT_NEEDED`, `DT_RUNPATH`, `DT_RPATH` and the machine of a file with
no read of its symbols; `Elf` reads `.dynamic` through the same function,
and its fields stay as they are (podvm builds `Elf` with each field).
`userland::runpath::runpath_dirs` walks the program and each library that
it loads, breadth first, and gives the `RUNPATH` (else `RPATH`) directories
as guest paths, each one that resolves inside the image; `Tokens` expands
`$ORIGIN` (the resolved directory of the program, the opened directory of a
library), and for glibc `$LIB` and `$PLATFORM` (call C832).
`userland::lib_dirs_for` puts them before `lib_dirs`, and `loader_plan` gives
them for its payload, through `GuestRoot::guest_of` (new, a provided method;
`Maps` maps a volume back). `abi::libc_beside` with the root `/` searched
paths relative to the working directory; it keeps the slash now (the test
`the_tokens_agree_with_the_loader_of_the_host` failed on it). Measured: the
loader of Ubuntu 24.04 (glibc 2.39) expands `$LIB` to
`lib/x86_64-linux-gnu` and `$PLATFORM` to `haswell` on this host, and the
walk gives the same values. Before the fix, `cargo test -p pod-enter
--all-features --test runpath` failed `an_absolute_runpath_of_the_image_loads`
("libpodrp.so: cannot open shared object file"); `a_runpath_of_origin_loads`
passed, as the loader of the host expands `$ORIGIN` against the host path
of the program. After the fix: `cargo test -p pod-enter --all-features
runpath` passes 10 unit tests and 2 tests with a real loader; `cargo test
-p pod-enter --all-features` passes 260 unit tests and each integration
target; clippy and fmt pass. The plants `enter-runpath-not-in-the-plan` and
`enter-runpath-leaves-the-image` prove the checks. Two limits stay, named in
`crates/pod-enter/README.md`: the loader reads an absolute `DT_RPATH` on the
host before the library path, and a dynamic child that podvm's interposer
starts takes the library path of the caller (`names.lib_path`) with no
`RUNPATH` directory of the child: that part of PodUtils/podvm@T-2003 is
podvm's interposer.

# T-977: `pod-release`: the pinned fetch of a static tool that a project runs (gVisor's `runsc` for podvm)

**Source:** PodUtils/podvm#604 (COORD-208 of podvm, under the ruling "Shared code"; PodUtils/podvm@T-2009)
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

podvm's `runsc` tier boots an image's systemd as PID 1 in a gVisor sandbox in 7 to 12 s on a host that delegates no cgroup: the fastest route to systemd for a user with no root (Debian 12 with MariaDB and PostgreSQL: a run in 10.2 s, against 45.6 s for the QEMU guest under TCG). podvm fetches no `runsc`, so the route is there only where the host has one. No crate of podcrates fetches a pinned tool for run time: `scripts/ci/fetch-tool.sh` serves CI alone, and `pod-release` fetches and checks the releases of PodUtils.

## Premise

- Read: PodUtils/podvm#604; `crates/pod-release/README.md` (`fetch::Fetcher`, `update`, `verify`); `scripts/ci/fetch-tool.sh`.
- podvm's answer (PodUtils/podvm#604, 2026-11-20): it pins the SHA-256 that it measures itself, as in each `SHA256SUMS` of PodUtils; the module takes no SHA-512 (gVisor's `.sha512` stays a check of the person who moves the pin).
- Not read: the release files of gVisor (`release-20261102.0`: the URL of `runsc` for each architecture and its `.sha512`).

## Approach

A module of `pod-release` (feature `fetch`): a `Tool` names a URL for each architecture and its SHA-256, pinned by the caller; `fetch` downloads it through the caller's `pod_proxy::Client` into the caller's store under a lock, checks the hash before it makes the file executable and before each first exec of a run, and gives one true line on each failure (no network, a proxy, a refusal, a hash mismatch, a full disk). A cached file whose hash holds is not fetched again. Tests against a stand-in server: a fetch, a cached second call, a mismatch that leaves no executable file, two callers at once. podrunner may take it for its units.

## Prove

```sh
cargo test -p pod-release --all-features tool
```

# T-995: `pod-enter`: `Child::wait_forwarding` forwards SIGHUP, SIGINT and SIGTERM only, so a SIGUSR1 or a SIGQUIT kills the waiter and the payload stays

**Source:** PodUtils/podvm#608 (COORD-211 of podvm; PodUtils/podvm@T-2001)
**Category:** defect
**Milestone:** M6
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`Child::wait_forwarding` (`crates/pod-enter/src/child.rs`) blocks SIGHUP,
SIGINT and SIGTERM, reads them from a signalfd and forwards them to the
payload. Each other signal keeps its default action in the waiter. A
SIGUSR1, SIGUSR2, SIGQUIT or SIGALRM to the waiter kills the waiter, and the
payload continues with no waiter. podvm measured it (as uid 1000, a
foreground `podvm run --rm alpine:3.20 sh -c 'echo START; sleep 20'`): a
SIGHUP gives exit 129 and "forwarded SIGHUP to the payload", and the `--rm`
record goes; a SIGUSR1 gives exit 138 with no line, `sleep 20` stays, and the
record is a false `Dead (exited while the launcher was not watching)`. The
waiter writes the record and removes the `--rm` tree, so its death leaves the
false record.

The ask: forward the set that the init of `Plan::pid_namespace` gives the
payload (call C769), docker's `--sig-proxy` with no tty: SIGHUP, SIGINT,
SIGQUIT, SIGUSR1, SIGUSR2, SIGTERM, SIGALRM and SIGPWR (each signal that a
process can catch, except SIGCHLD, SIGPIPE, SIGURG, SIGWINCH and the signals
of job control). Name each forward on `err`, as now. Forward SIGINT and
SIGQUIT only where a process sent them (`ssi_code` 0 or less: `SI_USER`,
`SI_QUEUE`, `SI_TKILL`): the signal of a terminal reached the process group of
the payload already.

## Premise

- Read: PodUtils/podvm#608; `crates/pod-enter/src/child.rs`
  (`wait_forwarding`, `forward_loop`); `crates/pod-enter/src/ns_pid.rs`
  (`FORWARDED`, `forwards`); the test
  `a_sighup_of_the_waiter_reaches_the_payload` in
  `crates/pod-enter/src/tests.rs`.
- The init has one list, `ns_pid::FORWARDED` (SIGHUP, SIGINT, SIGQUIT,
  SIGUSR1, SIGUSR2, SIGTERM and SIGWINCH). It has no SIGALRM and no SIGPWR.
- Not measured here: podvm's run. The test below measures the waiter.

## Approach

One list in `ns_pid`: `PROXIED` (the eight signals of the ask), and
`FORWARDED` is `PROXIED` and SIGWINCH, so the init gets SIGALRM and SIGPWR
too. `wait_forwarding` blocks `PROXIED` and reads it from its signalfd; it
reads `ssi_code` and forwards SIGINT and SIGQUIT only where `ssi_code` is 0
or less. `status::signal_name` names each signal of the list. Tests: a
SIGUSR1 to the waiter reaches the payload, and the waiter gives the exit of
the payload; a SIGINT with `SI_KERNEL` is not forwarded, and a SIGQUIT with
`SI_QUEUE` is.

## Prove

```sh
cargo test -p pod-enter --all-features waiter
```

## Done

2026-11-20, this commit.
`ns_pid::PROXIED` (`pod_enter::WAIT_FORWARDED`) is the one list: SIGHUP,
SIGINT, SIGQUIT, SIGUSR1, SIGUSR2, SIGTERM, SIGALRM and SIGPWR;
`FORWARDED` (`PID_FORWARDED`) is `PROXIED` and SIGWINCH, so the init gets
SIGALRM and SIGPWR too (call C831). `wait_forwarding` blocks the list,
reads `ssi_signo` and `ssi_code` from its signalfd, and forwards SIGINT and
SIGQUIT only where `ssi_code` is 0 or less (`child::waiter_forwards`).
`status::signal_name` names each signal of the list. `sys::SIGALRM`,
`sys::SIGPWR` and `sys::SYS_RT_TGSIGQUEUEINFO` are new in `pod-hostprobe`.
The tests of the waiter moved to `crates/pod-enter/src/child/tests.rs`.
Before the fix, `cargo test -p pod-enter --all-features --lib waiter`
failed two tests: `a_sigusr1_of_the_waiter_reaches_the_payload` gave
`Ok(0)` for `Ok(7)` (the payload ran its 20 s, the SIGUSR1 stayed pending),
and `a_sigint_of_the_terminal_is_not_forwarded_by_the_waiter` gave `Ok(3)`
for `Ok(5)` with "forwarded SIGINT to the payload". After it: `cargo test
-p pod-enter --all-features` passes 249 unit tests and each integration
target; `cargo clippy -p pod-enter -p pod-hostprobe --all-targets
--all-features -- -D warnings` and `cargo fmt --all --check` pass. The
plants `enter-waiter-signal-list-short` and
`enter-waiter-terminal-signal-forwarded` prove the two tests;
`enter-sighup-not-forwarded` moved onto `waiter_forwards`.

# T-996: `pod-supervise`: a SIGQUIT, SIGUSR1, SIGUSR2, SIGALRM or SIGPWR ends the detached launcher, and the launcher reads SIGHUP and gives it to nobody

**Source:** the work of T-995 (2026-11-20)
**Category:** defect
**Milestone:** M6
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

The detached launcher of `pod-supervise` (`serve` in
`crates/pod-supervise/src/launcher.rs`) blocks SIGHUP, SIGINT and SIGTERM
(`sys::sigblock_shutdown`), reads them from a signalfd, and gives SIGINT and
SIGTERM to the payload. It reads a SIGHUP and gives it to nobody. Each other
catchable signal keeps its default action: a SIGQUIT, SIGUSR1, SIGUSR2,
SIGALRM or SIGPWR to the launcher ends it. The payload then ends by its
`PR_SET_PDEATHSIG(SIGKILL)`, and no launcher writes `exited` into the table:
the record reads as `Dead` (the reconcile of a killed launcher), not as the
end of the payload by a signal. The waiter of `pod-enter` had the same
defect (T-995) and forwards the list `pod_enter::WAIT_FORWARDED` now.

## Premise

- Read: `serve` and the comment on its mask in
  `crates/pod-supervise/src/launcher.rs`; the `wait` of the control socket
  (an end of the stream with no answer is `Gone`); call C485 (the hold of
  `foreground` blocks SIGHUP, SIGINT, SIGQUIT and SIGTERM).
- Not measured: a signal to a real launcher. Not read: whether a consumer
  sends a signal other than SIGINT and SIGTERM to a launcher by its pid.

## Approach

The launcher blocks the list of the waiter (`pod_enter::WAIT_FORWARDED`, or
the same list moved to `pod-hostprobe` where `pod-supervise` does not take
`pod-enter`), reads it from its signalfd, and gives each signal of the list
to the payload, SIGHUP too; SIGINT and SIGQUIT only where `ssi_code` is 0 or
less, as `pod_enter::child::waiter_forwards` does (share the rule, do not
copy it). A test: a SIGUSR1 to a launcher reaches a payload that traps it,
and the record gets the code of the payload.

## Prove

```sh
cargo test -p pod-supervise --all-features launcher_forwards
```

# T-997: `pod-beneath`: one open of a path under a base with no link and no allocation, for the volume base of podvm and pod-enter

**Source:** PodUtils/podvm#611 (COORD-213 of podvm)
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`--podvm-volume-base BASE` (PodUtils/podrunner#300 ask 3,
PodUtils/podvm@T-1972) opens each volume under BASE with
`openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)`. The same walk exists in
four places: pod-enter `crates/pod-enter/src/ns_volume.rs`
(`VolumeBind::host_base`),
`PodUtils/podvm:crates/podvm/src/userns/host.rs` (`beneath`,
`check_volume_under_base`), `PodUtils/podvm:crates/podvm-dispatch/src/vols.rs`
(`beneath`) and `PodUtils/podvm:crates/podvm-interpose/src/map/base.rs`
(the `LD_PRELOAD` interposer: no allocation, no std, its own raw
`openat2`). Each splits the host path at
BASE, walks the rest with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS`, and gives
a descriptor or a refusal: a link, `..`, not a directory, not under BASE,
the kernel has no `openat2` (ENOSYS). The texts of a refusal differ.

The ask: a primitive in podcrates that each copy can take. It is `no_std`
and has no allocation, so that the interposer can call it in a path call.
It takes byte slices and a buffer of the caller, and it gives a descriptor
or a refusal enum that names each case. Its tests are the table of the
interposer: a link made between the walk and the call, a volume replaced by
a link, `..`, an absent name with a planted link, a kernel with no
`openat2` (ENOSYS). The guest side of pod-enter (`RESOLVE_IN_ROOT`) is
podcrates' call.

## Premise

- Read: PodUtils/podvm#611; podvm at its `main` head: the three copies and
  their tests `a_volume_under_the_base_reaches_no_file_out_of_the_base`
  (12 cases of `pin`), `a_volume_of_the_loader_family_opens_under_its_base_as_a_directory`
  (8 cases) and `a_volume_opens_under_its_base_with_no_link`;
  `crates/pod-enter/src/ns_volume.rs` (`beneath`, `open_beneath`, the
  guest walk `point`, `component`).
- `pod-core` and `pod-hostprobe` use std, and the interposer takes no
  crate that uses std. No crate of podcrates is `no_std`.
- The copies differ in the open of BASE (`open(2)`, `openat2` with no
  `resolve`), in the retry of EAGAIN (podvm-dispatch alone tries 256
  times), and in the flags: copy 2 writes the `O_DIRECTORY` of x86_64,
  which is `O_DIRECT` on aarch64.
- Not measured here: a kernel with no `openat2` (the tests give ENOSYS
  through a fake of the calls).

## Approach

A new crate `pod-beneath` (call C834): `#![no_std]`, Linux only, its one
dependency `linux-raw-sys` (the flags and the errno numbers of each
architecture). The caller gives the system calls by the trait `Calls`
(`openat2`, `close`): the interposer gives its raw call, podvm-dispatch
its own, pod-enter the calls of `pod-hostprobe`; the feature `libc`
(default) gives `Libc`, through `syscall(2)` of the C library. The
functions: `split` (the path under BASE, lexical), `open_beneath` (one
`openat2` under a directory, EAGAIN tried again), `open_under_base` (BASE,
then the rest under it), `open_parent_beneath` (the directory and the last
name, for a call that makes its file or does not follow its last link).
The enum `Refusal` names each case, with its errno and one line of text.
pod-enter's host side (`VolumeBind::host_base`) takes the crate; its guest
walk stays (call C835). The README gives the take of each copy of podvm.

## Prove

```sh
cargo test -p pod-beneath --all-features
cargo test -p pod-enter --all-features --lib ns_volume
cargo clippy -p pod-beneath -p pod-enter --all-targets --all-features -- -D warnings
```

## Done

2026-11-20, commit 2a6a0565 and the next one (the plants).
A new crate `pod-beneath` (call C834): `#![no_std]`, no allocation, Linux
only, `linux-raw-sys` its one dependency. `split`, `open_beneath`,
`open_under_base`, `open_parent_beneath`, `last_name`, the trait `Calls`,
`Libc` (feature `libc`, default), `OpenHow`, `EAGAIN_TRIES` and the enum
`Refusal` (`NotUnderBase`, `BaseNotAbsolute`, `Base`, `Link`, `DotDot`,
`NotDirectory`, `NoOpenat2`, `TooLong`, `Nul`, `Open`, each with `errno()`
and `text()`). `open_under_base` checks that both paths fit the buffer
before the base opens, so a refusal opens nothing. `pod-hostprobe` takes
its `OpenHow` and gives `sys::Beneath`; the host side of a volume of
`pod-enter` (`VolumeBind::host_base`) takes the crate, and a `..` above the
base is now "goes above its base by `..`" (call C835). The README gives the
take of each copy of podvm. Tests: `tests/table.rs`
(`a_volume_opens_under_its_base_as_a_directory_with_no_link`,
`a_path_of_a_volume_under_the_base_reaches_no_file_out_of_the_base`,
`a_host_path_splits_at_its_base`,
`a_path_that_does_not_fit_the_buffer_opens_nothing`,
`each_refusal_has_its_errno_and_a_line`) and `tests/race.rs`
(`a_link_made_between_the_walk_and_the_call_is_refused`,
`a_volume_replaced_by_a_link_is_refused`,
`a_dotdot_above_the_directory_is_refused`,
`an_absent_name_with_a_planted_link_makes_no_file_out_of_the_base`,
`a_kernel_with_no_openat2_gives_its_own_refusal`,
`eagain_is_tried_again_and_then_given`). `cargo test -p pod-beneath
--all-features` passes 11 tests (5 and 6); `cargo test -p pod-enter
--all-features --lib ns_volume` passes 8; `cargo test -p pod-hostprobe
--all-features --lib` passes 180; `cargo clippy -p pod-beneath -p
pod-hostprobe -p pod-enter -p static-probe-lean --all-targets
--all-features -- -D warnings` and `cargo fmt --all --check` pass; `cargo
check` of the three crates with their tests for x86_64-pc-windows-gnu and
x86_64-apple-darwin passes. The plant `beneath-symlinks-followed` (no
`RESOLVE_NO_SYMLINKS`) is caught; the plant
`enter-volume-base-not-checked` moved onto the new code and is caught.

# T-998: `pod-confine` and `pod-enter`: the program that a dynamic loader runs takes the rule of `exec=` (PodUtils/podvm#613)

**Source:** PodUtils/podvm#613 (COORD-215 of podvm), ask 1
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Landlock checks `EXECUTE` on the file that `execve` names. The dynamic
loader of an image (`ld-linux-x86-64.so.2`, `ld-musl-x86_64.so.1`) must be
executable, and it runs the program of its first argument: it only reads
and maps that program. So `ld.so /tmp/prog` runs a program that `exec=`
denies. `check_exec(&plan, program)` of `pod-enter` takes one host path,
and gives `Ok` for the loader when the loader is under an execute path.

podvm holds the rule in three copies (PodUtils/podvm@T-1984):
`PodUtils/podvm:crates/podvm-interpose/src/exec/ldso.rs` (`is_loader`,
`each_program`: pure, no allocation, no crate item, about 100 lines), which
the handler of the syscall tier and `podvm_enter::confine::check_first`
include as it is. Its table test is
`the_program_of_the_loader_is_each_argument_that_a_loader_can_run`. The ask:
`is_loader(path)` and `each_program(args, check)` in podcrates, with each
loader's reading of its options, taken as it is where it fits; then the
check of `pod-enter` applies the rule when the payload is the loader. podvm
then drops its copy.

## Premise

- Read: PodUtils/podvm#613; podvm at its `main` head: `ldso.rs`, its test
  file `tests_t1984.rs`, `podvm_enter::confine::check_first` and its test
  `the_first_program_takes_the_rule_of_exec`;
  `crates/pod-enter/src/confine.rs` (`check_exec`),
  `crates/pod-confine/src/exec.rs`.
- The options of glibc's loader (`elf/rtld.c`) and of musl's
  (`ldso/dynlink.c`), read from the sources: glibc takes a valued option
  and its next argument, an older glibc takes each other argument as the
  program, a newer one refuses an unknown `--name`; musl takes
  `--name=value` and `--` and refuses an unknown `--name`.
- musl's `make install` installs `ld-musl-ARCH.so.1` as a link to
  `libc.so`, and musl's `libc.so` is the loader: a check of the resolved
  name alone (as podvm's copy does) does not see the loader there.
- Not measured: a run of musl's `libc.so` as a loader on this host (no musl
  image here).

## Approach

A module `pod_confine::loader`, one file with no allocation, no item of
the crate and no item of std (core alone), so that an object that takes no
dependency keeps a copy of the file: podvm's `ldso.rs` as it is, and the
name of musl's loader file (`libc.so`, `libc.musl-ARCH.so.1`) in
`is_loader`. `pod-enter`: `check_exec_argv(&plan, argv)` gives the answer
of `check_exec` for each file that the kernel executes for `argv` (host
paths): the program, the interpreter of each `#!` line, and where one is a
loader (by the name as given or as resolved), each argument that the
loader can take as its program. The table test of podvm comes as it is,
with rows for musl's names.

## Prove

```sh
cargo test -p pod-confine --all-features --test loader
cargo test -p pod-enter --all-features --lib confine
cargo clippy -p pod-confine -p pod-enter --all-targets --all-features -- -D warnings
```

## Done

2026-11-20, commit 98a6f209 (calls C836, C841). `pod_confine::loader`
(`is_loader`, `each_program`, `VALUED`, `FLAGS`): podvm's `ldso.rs` as it
is, one file with no item of std and no other item of the crate
(`crates/pod-confine/SOURCES` names podvm's file), and musl's `libc.so` and
`libc.musl-ARCH.so.1` in `is_loader`. `pod-enter`: `check_exec_argv(&plan,
argv)` checks the program, each `#!` interpreter and each program of a
loader, by the name of the loader as given and as resolved; a loader as the
program of a loader is a refusal. Tests: `tests/loader.rs`
(`the_program_of_the_loader_is_each_argument_that_a_loader_can_run`, the
table of podvm with 13 names and 14 rows;
`the_loader_file_takes_no_item_of_std_or_of_the_crate`) and
`confine::program::tests::check_exec_argv_takes_the_rule_for_the_program_of_the_loader`.
`cargo test -p pod-confine --all-features --test loader` passes 2; `cargo
test -p pod-enter --all-features --lib confine` passes 7; `cargo clippy -p
pod-confine -p pod-enter -p pod-lab --all-targets --all-features -- -D
warnings` and `cargo fmt --all --check` pass. The plants
`enter-loader-program-unchecked` and `confine-loader-musl-libc-so` are
caught. For podvm: its copy reads the resolved name alone, so a musl image
with `ld-musl-ARCH.so.1` as a link to `libc.so` runs `ld-musl /tmp/prog`
out of `exec=`; the take of this file closes it.

# T-999: `pod-confine`: a table of any access in the shape of the exec table, and `pod-enter` gives the payload a write table (PodUtils/podvm#613)

**Source:** PodUtils/podvm#613 (COORD-215 of podvm), ask 2
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Landlock governs no change of mode, owner or times (`chmod`, `chown`,
`utimensat`). On the entry with no chroot podvm's interposer and the
handler of its syscall tier refuse each one on a file outside each path
with write (PodUtils/podvm@T-1985), from a table `PODVM_WRITE_HOST` of the
host path of each rule with write. podvm builds that table by a misuse:
it passes each rule with write through `Spec::exec_table` as a rule with
execute (`PodUtils/podvm:crates/podvm-supervise/src/table.rs`,
`set_write_table`). The ask: `Spec::table(Access)` in the shape of
`ExecTable`, and `pod-enter` writes a write table at the spawn as it writes
the exec table (for example `Names::write_table`).

## Premise

- Read: PodUtils/podvm#613; podvm at its `main` head: `set_write_table`,
  its callers in `PodUtils/podvm:crates/podvm/src/lifecycle/confine.rs`,
  the readers `PodUtils/podvm:crates/podvm-interpose/src/map/meta.rs` and
  `PodUtils/podvm:crates/podvm-dispatch/src/meta.rs`, and podvm's `NAMES`
  (`PodUtils/podvm:crates/podvm-enter/src/lib.rs`: a literal with each
  field of `Names`, no `..`).
- `crates/pod-confine/src/exec.rs`, `crates/pod-enter/src/confine.rs`
  (`exec_var`), `crates/pod-enter/src/launch.rs` (the filter of the
  environment), `crates/pod-enter/src/names.rs`.

## Approach

`pod_confine::PathTable`: the struct of the exec table, with
`ExecTable` a name of the same type, so each use of `ExecTable` stays.
`Spec::table(access)` gives the paths of each rule that holds each bit of
`access`; `Spec::exec_table()` is `table(Access::EXECUTE)`. `Names` gets
the field `write_table` (`POD_ENTER_WRITE_TABLE` in `Names::DEFAULT`): the
entry with no chroot writes `table(Access::WRITE).resolved()` there beside
the exec table, and removes each value of the caller on each entry. A new
field of `Names` breaks podvm's literal at its take: podvm adds one line
(`write_table: "PODVM_WRITE_HOST"`).

## Prove

```sh
cargo test -p pod-confine --all-features --lib exec
cargo test -p pod-enter --all-features --lib confine
cargo clippy -p pod-confine -p pod-enter --all-targets --all-features -- -D warnings
```

## Done

2026-11-20, commit 98a6f209 (call C840). `pod_confine::PathTable`, with
`ExecTable` a name of the same type; `Spec::table(access)`, and
`exec_table()` is `table(Access::EXECUTE)`. `Names::write_table`
(`POD_ENTER_WRITE_TABLE`): the entry with no chroot gives the payload
`table(Access::WRITE).resolved()` beside the exec table, and each entry
removes the value of the caller. Tests:
`exec::tests::a_table_of_an_access_holds_each_rule_with_that_access_once`,
`confine::tests::the_entry_with_no_chroot_writes_the_exec_and_the_write_table`
and `confine::tests::the_payload_gets_the_exec_table_and_no_value_of_the_caller`
(both tables in a real payload, and a forged value of each). `cargo test -p
pod-confine --all-features --lib exec` passes 5; `cargo test -p pod-enter
--all-features --lib confine` passes 7; clippy as in T-998 passes. The
plants `enter-write-table-not-set` and `enter-write-table-of-the-caller` are
caught; `enter-exec-table-not-set` and `enter-exec-table-of-the-caller` moved
onto the new code and are caught.

# T-988: `pod-enter`: the check of `exec=` for the first program of a caller with guest paths: `PATH`, a relative name and each `#!` line (PodUtils/podvm#613)

**Source:** PodUtils/podvm#613 (COORD-215 of podvm), ask 3
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`check_exec(&plan, program)` takes a host path. A caller of the entry
with no chroot holds the first program as a guest name: it must resolve
`PATH`, a relative name and the interpreter of each `#!` script itself,
through its table of paths, before it can ask. podvm does that in
`podvm_enter::confine::check_first` (PodUtils/podvm@T-2013), with its own
walk of the links (`host_resolved`). The ask: `check_exec_guest(plan,
maps, argv)` in `pod-enter`, where its userland module already resolves
the program.

## Premise

- Read: PodUtils/podvm#613; podvm at its `main` head: `check_first`,
  `host_resolved`, its callers (`PodUtils/podvm:crates/podvm/src/lifecycle/userland.rs`,
  `PodUtils/podvm:crates/podvm-supervise/src/launcher.rs`) and its test
  `the_first_program_takes_the_rule_of_exec`.
- `pod-enter` resolves a guest path through a table with
  `abi::resolve_in` (`crates/pod-enter/src/abi/search.rs`), finds a bare
  name on `PATH` with `ladder::guest_invoked`, and parses a `#!` line with
  `memfd::shebang_parts`.
- `check_first` takes the argv of the `#!` script from its caller (podvm's
  `guest_argv`); `pod-enter` can read the line itself.

## Approach

`check_exec_guest(&plan, &maps, argv)`: the first program by `PATH` (a
bare name) or by the working directory of the plan (a relative name),
resolved through `maps`; then the rule of T-998 on each file that the
kernel executes, with each `#!` line read from the file (the kernel's
argv: the interpreter, its one argument, the script as invoked, the
arguments). A first program or an interpreter that does not resolve is
left to the spawn, which names it (127), as in podvm. The test of podvm
comes with its rows.

## Prove

```sh
cargo test -p pod-enter --all-features --lib confine
cargo clippy -p pod-enter --all-targets --all-features -- -D warnings
```

## Done

2026-11-20, commit 98a6f209 (call C841). `check_exec_guest(&plan, &maps,
argv)`: a bare name on `plan.path_dirs` (`ladder::guest_invoked`), a
relative name under `plan.working_dir`, each path through `maps` with
`abi::resolve_in`, each `#!` line read from the file (the kernel's argv),
and the rule of T-998 for a loader. A program or an interpreter that does
not resolve is `Ok`, as in podvm's `check_first`. Test:
`confine::program::tests::the_first_program_by_guest_paths_takes_the_rule_of_exec`
(the 12 rows of podvm's test and 16 more: a volume, `..`, the interpreter of
a `#!` line and the loader as one, more than 4 levels of `#!`, musl's loader
as a link to `libc.so`, a link to the loader, a loader as the program of a
loader). `cargo test -p pod-enter --all-features --lib confine` passes 7;
the plant `enter-loader-program-unchecked` is caught by it too.


# T-1012: `pod-guest`: a saved state of the caller's machine line with no UEFI `Plan`, for podvm's Linux guest (PodUtils/podvm#618)

**Source:** PodUtils/podvm#618 (COORD-217 of podvm), ask 1; PodUtils/podvm@T-2008
**Category:** feature
**Milestone:** M7
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

podvm's machine tier saves the state of a guest that booted the init of an
image; each later run starts from that state (under TCG with four vCPUs,
the first output in 1.3 s to 1.9 s against 19 s for a boot). podvm uses
`pod_guest::qmp::Monitor`, `pod_guest::warm::qemu_version` and `host_cpu`.
`pod_guest::warm::save` and `restore` take a `Plan` of the UEFI flavour,
and podvm's Linux guest has its own machine line (direct kernel boot, 9p
devices). So podvm wrote a second copy in
`PodUtils/podvm:crates/podvm/src/machine/guest/state/save.rs` and
`PodUtils/podvm:crates/podvm/src/machine/guest/state/run.rs`: `stop`,
`migrate` to a file of a unique name, `cont`, a sync, then the rename;
`-incoming` and a wait for `running` on QMP; a fingerprint of the image,
the binary, QEMU, the CPU, the kernel, the memory and the machine line.
The ask: a form of `save` and `restore` that takes the machine line of the
caller (or a trait for it) in place of a `Plan`, so that both guests take
one copy; podvm then drops its own.

## Premise

- Read: PodUtils/podvm#618; podvm at `f258ec1`:
  `PodUtils/podvm:crates/podvm/src/machine/guest/state.rs` (the
  fingerprint, `Transport`: `file:`, or `exec:cat` for a QEMU before 8.2),
  `state/save.rs` (`save`, `migrate`, `running`), `state/store.rs` (the
  files `.vm` and `.json`, partial names `.part-UNIQUE`, the record with
  the memory size, the length and the transport), `state/run.rs` and the
  fake monitor of `state/tests_save.rs`.
- Read: `crates/pod-guest/src/warm.rs`, `crates/pod-guest/src/warm/state.rs`
  and their tests, and the five plants `guest-warm-*` that patch
  `crates/pod-guest/src/warm/state.rs`.
- Not measured: podvm's machine line under the new form; podvm measures it
  at its take.

## Approach

A module `warm::line` that knows no `Plan`: `freeze` (connect, the check of
`blocked-reasons`, `stop`, a hook while the guest is stopped before and
after the stream, `migrate` with a bound and a cancel, a `file:` save that
falls back to the pipe where the caller asks, `cont`); `save` (the stream
to `STREAM.part-UNIQUE`, a sync, a record with the fingerprint, the
length, the transport and the caller's notes, then the renames);
`check` (no record or stream: `Cold::Missing`; another fingerprint:
`Cold::Stale` with the first line that differs; another length:
`Cold::Missing`); `incoming` (the line, then `-incoming` and the URI);
`running` (the wait for the end of the load and `cont`); `Inputs` (the
fingerprint of a line: the emulator, the CPU, a file by its length and
time or by its digest, any value, the machine line). The `Plan` API keeps
its names and its on-disk form and calls the new items.

## Prove

```sh
cargo test -p pod-guest --all-features --lib warm
cargo clippy -p pod-guest --all-targets --all-features -- -D warnings
sh scripts/plant-each.sh --apply
```

# T-1013: `pod-services`: the pinned static QEMU at 11.0.0, with the fix of the use-after-free of `address_space_translate_for_iotlb` (PodUtils/podvm#618)

**Source:** PodUtils/podvm#618 (COORD-217 of podvm), ask 2; PodUtils/podvm@T-2016
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

QEMU ended on SIGSEGV 2 s to 5 s into the boot of a four-vCPU guest under
TCG in 6 of about 245 starts on a busy host (QEMU 8.2.2 of Ubuntu 24.04;
podvm's experiment 469: 2 of 30). The kernel log names
`address_space_translate_for_iotlb`. That matches the upstream fix
"system/physmem: fix use-after-free with dispatch" (QEMU 10.1, stable
10.0.4). The pinned static QEMU of podcrates is 9.2.0, before the fix
(`crates/pod-services/services/pkgforge-qemu.toml`). The ask: the pinned
static QEMU at 10.0.4 or later.

## Premise

- Read: the pin is the service `pkgforge-qemu` (pkgforge's bincache,
  package `ppkg.qemu`) and its test
  `crates/pod-services/tests/podvm.rs`; no workflow of podcrates builds
  QEMU. podvm pins the same files in
  `PodUtils/podvm:crates/podvm-windows/src/tools.rs`.
- Read: the metadata of pkgforge's bincache and pkgcache for x86_64 Linux
  (`meta.pkgforge.dev`) and the tags of the GHCR packages: each static
  `qemu-system-*` and `qemu-img` is at 9.2.0 only.
- Read: the fix is QEMU commit `2865bf1c5795371ef441e9029bc22566ccff8299`.
  The compare of GitHub gives `v10.1.0` and `v11.0.0` ahead of it and 0
  behind; stable 10.0.4 holds it as patch 09 of 59 of the stable series
  (a cherry-pick with another hash).
- Read: `hermeticbuild/qemu-prebuilt` (MIT; the Alpine recipe of
  `ziglang/qemu-static`, `--static`, no patch of QEMU) publishes QEMU
  11.0.0 as the release `11.0.0.1`, with a SLSA provenance attestation of
  each archive by its release workflow. Measured through the mirror: the
  SHA-256 and the length of each archive equal the digest of the release
  API and the subject of the attestation; each binary is a static-pie ELF
  with no `PT_INTERP` and no `DT_NEEDED`, linked with musl, and holds
  "QEMU emulator version 11.0.0" and the devices `virtio-9p-pci`, `nvme`,
  `qemu-xhci` and `usb-storage`.
- Not measured: a run of these binaries (no third-party binary runs in
  this session), and a boot of podvm's guests with them.
- The newest stable QEMU is 11.1.3; no static build of it is published
  (T-1014).

## Approach

A new service `hermeticbuild-qemu` beside `pkgforge-qemu`, in the shape of
`wasmtime-release`: the archive of `qemu-system-x86_64`,
`qemu-system-aarch64` and `qemu-img` for an x86_64 host and the archive of
the data (`share/qemu`), each with its SHA-256 and length, the member and
the SHA-256 and length of each binary, and an `http-sha256` watch of each
archive. `pkgforge-qemu` stays until podvm takes the new service: podvm's
test of its hosts names `api.ghcr.pkgforge.dev`. podvm then removes the
old one from its store and podcrates removes `pkgforge-qemu`.

## Prove

```sh
cargo test -p pod-services --all-features --test podvm
cargo test -p pod-services --all-features
```

The live watch of each archive: the dispatch of `services.yml` with
`service=hermeticbuild-qemu` (`gh workflow run services.yml -f
service=hermeticbuild-qemu`) passes on `main`.

# T-1014: `pod-services`: a static QEMU of the newest stable point release; the pin of T-1013 is 11.0.0, and 11.0.5 and 11.1.3 are out

**Source:** PodUtils/podvm#618 (COORD-217 of podvm), ask 2; a finding of T-1013
**Category:** feature
**Milestone:** M6
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

The pin of T-1013 is QEMU 11.0.0, the only static build of a QEMU after
10.0.4 that a project publishes. QEMU has the stable releases 11.0.1 to
11.0.5 and 11.1.0 to 11.1.3 after it; each holds fixes that 11.0.0 does
not have. No static build of them is published: pkgforge's bincache has
9.2.0 only, and `hermeticbuild/qemu-prebuilt` has 11.0.0 only.

## Premise

- Read: the tags of `qemu/qemu` (`v11.1.3` is the newest stable, `v11.2.0-rc1`
  is the newest tag), the releases of `hermeticbuild/qemu-prebuilt`
  (`11.0.0.0`, `11.0.0.1`) and the bincache metadata of pkgforge.
- Read: the recipe of `hermeticbuild/qemu-prebuilt` (an Alpine container,
  the static libraries of Alpine, a static libslirp, `configure --static`)
  takes any upstream tag.

## Approach

Two ways; take the first that holds: a rebuild of pkgforge's `ppkg.qemu`
at the newest stable (the party of `pkgforge-qemu`; a request to its
maintainers), or a workflow of podcrates that builds the newest stable in
an Alpine container with the recipe of T-1013 and publishes the archives
with their SHA-256 and an attestation. Then move the facts of
`hermeticbuild-qemu` (or a new service) to that release.

## Prove

```sh
cargo test -p pod-services --all-features --test podvm
```
