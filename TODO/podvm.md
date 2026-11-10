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
- podrunner is a consumer (PodUtils/podvm#227 section 2.7). It needs a proof string for a capability record with `boundary: confined`. Without `confined`, `trusted` jobs run only if the owner answers yes to DR-4.

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
**Status:** open

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
**Status:** open

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

# T-162: `pod-supervise`: kill the tree, a wall timeout, rusage and the count of residual processes (PodUtils/podvm#227)

**Source:** PodUtils/podvm#227 section 2.3 (COORD-021 of podvm, 2026-11-10)
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

A signal of the launcher reaches one pid. A grandchild of a payload survives `podvm stop` and `rm -f` (a measurement of the design review, M2, `05-lifecycle.log`). The ask A-V-07 of `docs/research/fleet/asks.md` (a run timeout that kills the tree, peak RSS and CPU in the report) routes to T-060, which is done, and no open entry owns the tree kill. The consumers: podrunner (it retires its per-unit shim when this lands), podvm (`PodUtils/podvm@T-1655`, the launcher of its WP-V4) and G-12 of GitHub #36 (end in 60 s). The tree kill serves podvm's `PodUtils/podvm@T-1655` and G-12 of #36 as well as podrunner. This entry waits like the other M8 entries. The owner can lift the wait.

## Premise

- Read: `crates/pod-supervise/src/launcher.rs:351` is `let _ = sys::kill(pid, sig);`, in the control handler of the launcher. It signals one pid. This agrees with the issue.
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

The owner names the plan of GitHub #47 (DR-1) and the place of the code (DR-2). The ruling "CI tracker" (2026-11-10) leaves #47 as it is, and the ruling "Priority" (2026-11-09) keeps M8 waiting. The owner can lift the wait for this entry.

Depends on no other crate.

# T-166: `pod-hold`: the hold of each connect of a payload, from podnet's `podnet-netns` (PodUtils/podnet#422)

**Source:** PodUtils/podnet#422 (COORD-014 of podnet; PodUtils/podnet@T-232); T-109; PodUtils/podnet#414.
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** L
**Status:** partial

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

2026-11-10, partial: the clean run of the trial of podnet on this tree,
the cross tests, the release `v0.15.0` and the take of podnet remain.
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
- Next step: `sh scripts/adoption.sh podnet pod-hold` in one clean run
  (about 40 minutes; it needs about 10 GB of disk). Then cross-build the
  tests of `pod-hold` for i686, aarch64, riscv64gc and loongarch64 musl
  (`cargo zigbuild --target T -p pod-hold --tests`) and run each binary,
  with `tests/hold` as `hold`, under `QEMU_LD_PREFIX=/ qemu-ARCH`; record
  where qemu-user refuses the seccomp listener (the test prints `SKIP:`).
  Then the release `v0.15.0` and a COORD issue `(to podnet)` with the
  patch.

# T-169: `pod-hold`: the dial state of a held `connect` of the seccomp hold, from podnet's `connect.rs`

**Source:** T-166 (the cut of `pod-hold`).
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** open

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
  not this state: podnet's view is needed before the import.

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

# T-170: `pod-hold`: the netns hold of a SYN (dial first, then accept or RST), from podnet's netns router

**Source:** T-166 (the cut of `pod-hold`).
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** M
**Status:** open

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
  T-166 read it as the namespace and the TUN. podnet's view is needed.

## Approach

Ask podnet with the COORD issue of the entry above. If the hold of a SYN
is in the cut: import `packet.rs` (`parse_tcp_syn`, `rst_reply_for_syn`,
`ipv4_checksum`, with the tests `parses_a_tcp_syn` and
`the_rst_reply_answers_the_syn` of podnet's `lib.rs`) and the hold of a
SYN of `backend.rs` behind a trait of the consumer for the dial, after
the smoltcp version of `pod-netstack` is settled.

## Prove

```sh
cargo test -p pod-hold
sh scripts/adoption.sh podnet pod-hold
```
