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
- podrunner is a consumer (PodUtils/podvm#227 section 2.7). The host qualification that the design review (DR-15) gives to `pod doctor` comes from this report, and `podrunner doctor` reads it. The podvm driver reads `podvm probe --json`.

## Approach

Serde types behind a feature. A `schema` field. A JSON Schema file in `schemas/`. The value `unknown` where
a filter answers `EPERM`.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
```

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

- Read: `crates/pod-supervise/src/launcher.rs:389` is `let _ = sys::kill(pid, sig);`, in the control handler of the launcher. It signals one pid. This agrees with the issue.
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

- Read (this tree): `crates/pod-oci/src/registry.rs:284-331`.
  `Client::blob<W: Write + Restart>` streams into a sink with a
  `Verifier`. `Restart` sets the sink back for a new try (`:40-42`). The
  retry is around the whole transfer (`ATTEMPTS`, `:75`). The doc comment
  names the proxy that cuts at a round size (`:289-291`) and refuses a
  resume: `crates/pod-oci/src/registry.rs:293` says "a resume trusts". The
  review cites the method from `:288`; its doc comment starts at `:284`.
- Read: `crates/pod-oci/src/registry/http.rs:12`: `get` takes an `Accept`
  value only, so a `Range` header needs a new parameter.
  `crates/pod-oci/src/registry/http.rs:114-116` and
  `crates/pod-oci/src/net.rs:9-11`: `pod-proxy` follows the redirect of a
  blob and sends no `Authorization` header to another origin. They agree
  with the review.
- Read: `crates/pod-oci/src/pull.rs:22-26` (`FETCH_WORKERS`: the limit is
  what the registry serves) and `:186-191` (`fetch_parallel` gives the jobs
  in strides); `crates/pod-oci/README.md:112` (podvm keeps
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
**Status:** open

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
```

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

# T-326: `pod-relay`: the trial of the blocking facade in podvm, for PodUtils/podvm@T-1678 (PodUtils/podvm#283)

**Source:** PodUtils/podvm#283 (COORD-062).
**Category:** feature
**Milestone:** M6
**Priority:** P1
**Effort:** L
**Status:** open

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

# T-328: GNU `tar -xz -C DIR` fails with "Cannot open" right after `mkdir -p DIR` in a podvm unit (podrunner's note)

**Source:** podrunner's session (its take of `v0.31.0`, PodUtils/podrunner@T-094): podvm `v0.2.0-beta.5` on the GitHub x86_64 host, runner-image run 43205758271 of podrunner.
**Category:** research
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

In a podvm unit, `mkdir -p DIR` then GNU `tar -xz -C DIR` of the runner
image failed with "Cannot open: No such file or directory". podrunner
works around it with `cd DIR` and `gzip -dc | tar -xf -`. The case is
not reduced. `tar -C` opens the members relative to a directory fd
(`openat`, `mkdirat` with a dirfd), so the cause can be in the path
layer of `pod-enter` or of podvm, not in tar.

## Premise

- Read: podrunner's note (the message of its session).
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

# T-353: `pod-oci`: a pull from the CNCF distribution registry on the loopback; the live pull from ECR Public each week

**Source:** `docs/live-calls.md` (row L4); `services.yml` run 42803032733
(ECR Public gave 429).
**Category:** feature
**Milestone:** M5
**Priority:** P2
**Effort:** M
**Status:** open

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

## Prove

```sh
sh scripts/ci/registry-tests.sh
cargo test -p pod-oci --all-features
```
