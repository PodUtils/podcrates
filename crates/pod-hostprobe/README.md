# pod-hostprobe

The probes of a Linux host, and the raw system calls under them. A probe
runs the operation that a caller needs, in a disposable child, and
records the errno of the kernel. Four rules:

1. Each probe runs in a disposable child: `clone(2)` with no `CLONE_VM`,
   and a new start of `/proc/self/exe` with a private argument.
2. A probe records the errno, never a boolean.
3. "Could not run" is a third state (`Skip`), and it never reads as
   "denied".
4. The controls run with the bogus-argument discriminator in the same run,
   so that a probe that stops to discriminate says so.

The crate is Linux only (`#![cfg(target_os = "linux")]`): on another
target it is empty, and a binary of the other targets still links it. It
uses no libc wrapper: the numbers of the calls and the layouts of the
kernel structs come from `syscalls` and `linux-raw-sys`, and on mips,
mips64, s390x, powerpc and powerpc64 (where `syscalls` does not build on
stable Rust) from `linux-raw-sys` with a trap of its own (powerpc64 and
s390x; mips and 32-bit powerpc refuse to compile).

## API

| Item | What |
| --- | --- |
| `Settings` | `child_flag`, `child_name` (the private argument and the `argv[0]` of a probe child), `directory_setting` (the setting that the messages of `space` and `exec` name, or none) |
| `run(&Settings, candidates) -> Findings` | the whole set: one child for each row of `probes::PROBES`, the identity, the write probes |
| `run_child(name, &mut dyn Write) -> i32` | the work of a child: the binary calls it when its first argument is `Settings::child_flag` |
| `Findings` | the rows, `identity`, `writable`, `self_exe` |
| `verdict` | `Verdict` (`Ok`, `Denied`, `Skip`), `Outcome`, the wire line `pod-hostprobe/1` (`encode`, `decode`), `Outcome::row` (the row format of the Go reference instrument) |
| `child` | `Spawner`: `clone_accepted`, `run_probe` |
| `probes` | `PROBES` (66 rows: census, attribution, machine, supervise), `Kind`, `Group`, `find`, `MACHINE_LEGS`, `SUPERVISE_LEGS`, `SUPERVISION_LEGS`, `chroot_usable`, `ptmx_usable`, `fuse_usable`, `PTMX`, `FUSE` |
| `select` | `Rung`, `Strictness`, `Rejection`, `Selection` (`choose`, `meets`, `exit_code`, `STRICT_FLOOR`), `Provides`, `control_rows` |
| `mounts` | `Mounts`: create, configure, attach, use; `summary`, `capability_note` |
| `machine` | `assess`, `Assessment`, `Profile` (`Full`, `Tcg`), `Leg`, the leg names, `accelerators_in` |
| `supervise` | `assess` (the mediation legs), `assess_supervision` (the lifecycle legs), `Assessment`, `Leg` |
| `identity` | `Identity`, `read`, `is_single_id_map`, `ConfinementKey`, `confinement_key(&Identity, &Interpreter)` |
| `interp` | `detect(is_set) -> Interpreter`, `QEMU_VARS`: is an emulator in the way |
| `binfmt` | `registrations`, `Registration`, `machine_for`, `SELF_MACHINE`, `BINFMT_DIR` |
| `writable` | `candidates(named)`, `probe(candidates)`, `WriteProbe`, `FIXED`, `VARIABLES` |
| `space` | `read`, `require(&Settings, ...)`, `choose`, `file_ceiling`, `dir_bytes`, `mib`: blocks and inodes before a write |
| `exec` | `probe_dir_exec(&Settings, dir)`, `try_exec`: can a file created under a directory execute (`noexec`) |
| `sys` | the raw system calls: `Errno`, the `SYS_*` numbers, the constants, `CBuf`, the wrappers, `clone_fork`, `shed_after_fork`, `close_in_children`; `statfs` gives `Statfs { f_bsize, f_bavail, f_files, f_ffree, f_flags }` from the `struct statfs64` of the kernel on each target (`statfs64(2)` on a 32-bit target), with `Statfs::flag` and the mount flags `ST_RDONLY`, `ST_NOSUID`, `ST_NODEV`, `ST_NOEXEC`, `ST_SYNCHRONOUS`, `ST_VALID`, `ST_MANDLOCK`, `ST_NOATIME`, `ST_NODIRATIME`, `ST_RELATIME` |

A failure of `space` and `exec` is a `pod_core::Failure` at
`Stage::Host`: `Class::Usage` for a path with a NUL, `Class::Unavailable`
for no space or an unknown free space, the class of the I/O error
otherwise.

The crate reads no variable of the environment. The caller gives the
named directories of the write probe (`writable::candidates`), and a
function that tells which variables are set (`interp::detect`). A probe
child gets the environment of its parent as it is.

## Parity

Against podvm `crates/podvm-probe`, `crates/podvm-image/src/space.rs` and
`crates/podvm-image/src/exec_probe.rs` at `2c03085`:

| Capability of podvm | Here |
| --- | --- |
| [x] One disposable child for each probe, the line and the exit status cross-checked | `child` |
| [x] The three states, the wire line | `verdict` (magic `pod-hostprobe/1`) |
| [x] The probe table and the bodies (66 rows) | `probes` |
| [x] The selection of the rung, `Strictness`, the controls, `Provides` | `select` |
| [x] `BANNER_ROWS`, `banner_cell` | stay in podvm, with its report |
| [x] Create, configure, attach and use of a mount | `mounts` |
| [x] The machine legs and profiles | `machine` |
| [x] The supervise and supervision legs | `supervise` |
| [x] The ID maps and the cache key | `identity` (the interpreter is a parameter) |
| [x] The emulator check | `interp` (the environment is a parameter) |
| [x] `binfmt_misc` | `binfmt` |
| [x] The write probe | `writable` (the candidates are a parameter) |
| [x] The raw system calls, the trap of powerpc64 and s390x, the shed of fds after a fork | `sys` |
| [x] Blocks, inodes, the file-size ceiling, the choice of a destination | `space` |
| [x] The exec probe of a directory | `exec` (also tries `ETXTBSY` again, within a bound) |
| [x] `exit`, `json`, `report`, `nongoals` | stay in podvm: its CLI and its report |
| [x] The tests: 90 unit tests (all of the moved files but `the_banner_cell_names_the_errno_rather_than_the_word_denied`), 10 integration tests | yes, with their names; 2 new unit tests; `tests/children.rs` runs the rows |

## Adoption map (podvm)

| Old path | New path |
| --- | --- |
| `podvm_probe::sys::*` | `pod_hostprobe::sys::*` |
| `podvm_probe::{Findings, run, run_child}` | `pod_hostprobe::{Findings, run, run_child}`: `run(&settings, &writable::candidates(&named))`; `run_child(name, &mut stdout)` writes the line (a name that is not a probe gives a skip line and 2; podvm printed `podvm: no probe named` on stderr) |
| `podvm_probe::CHILD_FLAG` | `pod_hostprobe::Settings::child_flag`, podvm's value `--probe-child` |
| the `argv[0]` `podvm` of a probe child | `Settings::child_name`, podvm's value `podvm` |
| `$PODVM_STORE` in the messages of `space` and `exec_probe` | `Settings::directory_setting`, podvm's value `Some("$PODVM_STORE")` |
| `podvm_probe::verdict::*` | `pod_hostprobe::verdict::*` (`WIRE_MAGIC` is `pod-hostprobe/1`) |
| `podvm_probe::select::{Rung, Selection, Strictness, Provides, Rejection, control_rows}` | `pod_hostprobe::select::*` |
| `podvm_probe::select::{BANNER_ROWS, banner_cell}` | podvm, with `report` |
| `podvm_probe::probes::*` | `pod_hostprobe::probes::*` |
| `podvm_probe::identity::confinement_key(&id)` | `pod_hostprobe::identity::confinement_key(&id, &interp::detect(&is_set))` |
| `podvm_probe::interp::detect()` | `pod_hostprobe::interp::detect(&\|v\| std::env::var_os(v).is_some())` |
| `podvm_probe::writable::probe()` | `pod_hostprobe::writable::probe(&writable::candidates(&named))`; podvm reads `PODVM_STORE` and then `writable::VARIABLES`, and gives each set value as `(value, "$NAME")` |
| `podvm_probe::{binfmt, machine, mounts, supervise, child, identity}` | `pod_hostprobe::{binfmt, machine, mounts, supervise, child, identity}` (`child::Spawner::new(&settings)`) |
| `podvm_probe::{exit, report, nongoals, json}` | stay in podvm |
| `podvm_image::space::{read, require, choose, mib, dir_bytes, file_ceiling, Have, Free, Candidate}` | `pod_hostprobe::space::*`; `require(&settings, ...)`; the error is a `pod_core::Failure`, which podvm maps to its `Error::NoSpace` (`Class::Unavailable`) and `Error::Store` (`Class::Usage`) |
| `podvm_image::exec_probe::{probe_store_exec, try_exec, ExecProbe, ExecVerdict}` | `pod_hostprobe::exec::{probe_dir_exec, try_exec, ExecProbe, ExecVerdict}`; `probe_dir_exec(&settings, dir)` |
| the gate paths of `crates/podvm-gate` (`podvm-probe` tests, `namespace`, `namespace_base`) | the adoption patch changes them |

The scratch names of the probes start with `pod-hostprobe` (podvm:
`podvm`); each is made and removed by one probe.

## Tests

`cargo test -p pod-hostprobe`. A test that needs a user and a mount
namespace starts with `pod_testkit::need_userns!()`, and a test that runs
`/bin/sh` with `pod_testkit::need_tool!("/bin/sh")`: on a host that lacks
it, the test skips with a line `skipping, REASON`. `tests/children.rs`
has no libtest: it reads the child flag first and runs the whole set in a
private mount namespace where it can get one.

## Sources

`SOURCES` lists each imported file of podvm with its blob, and the
`check-sources.py` check compares them with podvm's `main`. The files of
podvm that stayed (`exit.rs`, `json.rs`, `report.rs`, `report/`,
`nongoals.rs`) are `skip` lines.
