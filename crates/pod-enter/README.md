# pod-enter

The entry of a payload into an extracted root file system on Linux, with
no daemon and no root. One fork shape for each entry: the parent builds
each thing that the child touches before the fork, the child does only
async-signal-safe work, and an `O_CLOEXEC` readiness pipe tells a
successful `execve` (the end of the file) from a failed step (the step and
its errno). There is no sleep in the start.

The entries:

* The chroot sequence (`spawn`, `run`): `RootDir::open` refuses a symbolic
  link and holds the root as a descriptor; each descriptor of the payload
  opens before the root changes (`Fds`); `fchdir`, `chroot(".")`,
  `chdir("/")`, the `WorkingDir`; then the child resolves the program
  inside the new root (`Plan::program_candidates`).
* The namespace rung (`spawn_namespace`): a mount namespace, `/` private
  and recursive, a private tmpfs on the `/tmp` of the image, a chroot by
  path with a device and inode check against the descriptor. A failure of
  the setup enters chroot and names the fallback on the error writer.
* The entry with no chroot (`spawn_userland`, `userland`): the loader of
  the image by host path, or a staged memfd for a static payload; the
  table of path maps for the interposer.
* The launch ladder (`ladder`, `spawn_ladder`): memfd, FUSE, tmpfs, run
  directory, cache, in that order. A forced rung that is down refuses
  with its reason. `memfd::probe` measures that a memfd executes (a
  trampoline through the same flags and `execveat` as the launch).
* The reader of ELF files (`abi`): the C library of an image from the
  `PT_INTERP` of the payload, the `DT_NEEDED` closure of an object, and
  the check of a preload against the symbols and versions of `.dynsym`.

docker's rules for `Entrypoint` and `Cmd`, the default `PATH` and `HOME`,
and the codes 126, 127 and 128 + signal are in `plan` and `status`.

The crate is Linux only (`#![cfg(target_os = "linux")]`): on another
target it is empty. It reads no variable of the environment. The caller
gives the names of its variables and files in `Names`; the crate writes
them into the environment of a payload, removes them from it, or names
them in a refusal. The banner of the caller goes to the error writer that
each entry takes; the payload owns the standard output.

## API

| Item | What |
| --- | --- |
| `Names` | `product`, `mode_request`, `mode_active`, `cache`, `maps`, `device_spec`, `device_fds`, `cache_marker`, `memfd_label`. `Names::DEFAULT` gives `pod-enter`, `POD_ENTER_MODE`, `POD_ENTER_ACTIVE_MODE`, `POD_ENTER_CACHE`, `POD_ENTER_MAPS`, `POD_ENTER_DEVICE_SPEC`, `POD_ENTER_DEVICE_FDS`, `.pod-enter-cache-complete`, `pod-enter-payload` |
| `Plan { argv, env, working_dir, fds, banner, path_dirs, names }`, `Program` | what the payload will be; `Plan::{argv_for, env_for(&names, ..), set_active_mode(&names, ..), path_from, program, program_candidates, DEFAULT_PATH}` |
| `RootDir::open`, `Fds`, `Child`, `Bounded` | the held root; the descriptors of the payload; a started payload (`wait`, `wait_forwarding`, `wait_bounded`) |
| `spawn`, `run`, `spawn_ladder`, `run_ladder`, `spawn_userland`, `run_userland`, `spawn_namespace`, `run_namespace`, `spawn_selected`, `run_selected` | the entries; a `run_*` waits and forwards SIGINT and SIGTERM |
| `ENTERED_RUNG`, `entered_rung`, `entered_word` | the rung that a selection of `pod_hostprobe::select` enters |
| `check_workdir`, `ns_tmp_target`, `ns_fallback_line(&names, ..)`, `ns_setup_step`, `chroot_landed`, `NsMount` | the checks of the entries, pure where they can be |
| `Error`, `Result`, `status` | `Runtime(Failure)`, `CannotInvoke`, `NotFound`, `NsSetup`; `Error::{runtime, payload_code, code_or}`; `status::{CANNOT_INVOKE, NOT_FOUND, exit_status}` |
| `ladder` | `Mode` (`parse(&names, ..)`, `name`, `ORDER`), `Availability` (`refusal_reason(&names, ..)`), `choose(&names, ..)`, `resolve_payload`, `guest_exe`, `payload_bytes`, `PAYLOAD_CEILING` |
| `memfd` | `memfd_create`, `stage(&names, bytes)`, `seal`, `write_full`, `exec_fd`, `eligible`, `MemfdRefusal`, `ScriptRefusal`, `script_refusal`, `first_shebang_line`, `has_shebang`, `is_executable`, the `MFD_*` flags; `probe::{memfd_exec, answer(&names, ..), MemfdExec, Standing, Answer, read_sysctl, TRAMPOLINE}` |
| `stage` | `copy_tree`, `stage_tmpfs`, `release_tmpfs`, `cache_dir`, `stage_cache(&names, ..)`, `Refusal` |
| `fuse` | `check_node`, `check_node_at`, `serve_tree`, `release_mount`, `ActiveMount`, `FUSE_NODE` |
| `device` | `parse`, `Mapping`, `canon`, `spec_entry`, `serve_entry`, `banner_line(&names, ..)`, `open_all`, `Opened`, `specs_of(&names, ..)`, `push_spec(&names, ..)`, `push_serve(&names, ..)`, `CHILD_FD_BASE`, `HOST_FD_FLOOR`, `SEP_ENTRY` |
| `userland` | `Family`, `LoaderPlan`, `lib_dirs`, `invocation_path`, `loader_argv_for`, `loader_plan`, `host_env`, `userland_maps_table(&names, ..)` |
| `binfmt` | `support_for`, `Support`, `copy_in_target`, `copy_interpreter_in`, and the reader of `pod_hostprobe::binfmt` |
| `abi` | `Elf` (`read`, `parse`, `flavour`), `Flavour`, `Versioned`, `Unreadable`, `Verdict`, `admits`, `admits_set`, `interp_of`, `libc_beside`, `resolve_in`, `resolve_dir_in`, `ResolveKind`, `link_set`, `LinkSet`, `Member`, `guest_lib_dirs` |

A failure of the entry is `Error::Runtime` with a `pod_core::Failure` at
`Stage::Host`. Its class follows the errno where a system call refused
(`ENOENT` `NotFound`, `EPERM`/`EACCES`/`EROFS` `Refused`, `ENOSYS`/
`EOPNOTSUPP` `Unsupported`, `EAGAIN`/`EINTR` `TempFail`, else
`Unavailable`); a bad argument is `Usage`, a forced rung that is down or no
rung `Unsupported`, a ceiling or an escape `Policy`, a symbolic link as the
root `Policy`. A failure of I/O keeps its cause. The exit code of a runtime
failure belongs to the caller (`code_or(125)` for docker's).

The reader of ELF files reads files of an image that the caller does not
control: each offset goes through checked arithmetic and `usize::try_from`,
and the count of the section headers is bounded by the file, so a hostile
file gives a refusal and no panic, also on a 32-bit target.

## Parity

Against podvm `crates/podvm-enter` at `6cd3442`:

| Capability of podvm | Here |
| --- | --- |
| [x] The chroot sequence, `RootDir`, `Fds`, `Child`, `Bounded`, the forwarding wait, `exit_status` | the same |
| [x] The namespace rung, its fallback, `ns_tmp_target`, `ns_setup_step`, `chroot_landed` | the same; `ns_fallback_line` takes `&Names` (the word `podvm:` is `Names::product`) |
| [x] The entry with no chroot (`spawn_userland`, `run_userland`, `userland`) | the same; `userland_maps_table` takes `&Names` |
| [x] `PODVM_MODE`, `PODVM_ACTIVE_MODE` in `Plan` (`MODE_REQUEST_VAR`, `MODE_ACTIVE_VAR`) | `Names::{mode_request, mode_active}`; `Plan::names`; `env_for` and `set_active_mode` take `&Names` |
| [x] The ladder: `Mode`, `Availability`, `choose`, `resolve_payload`, `guest_exe`, `payload_bytes` | the same; `Mode::parse`, `choose`, `refusal_reason` take `&Names` (`PODVM_CACHE` is `Names::cache`) |
| [x] `ladder::GUEST_EXE_VAR` (`PODVM_GUEST_EXE`) | stays in podvm: the crate does not read or write it |
| [x] memfd: the flags, `memfd_create`, `stage`, `seal`, `eligible`, the script refusal, the probe that executes a trampoline | the same; `stage` and `probe::answer` take `&Names` (the label `podvm-payload`, the words of the fix line) |
| [x] `stage`: `copy_tree`, tmpfs, cache | the same; `stage_cache` takes `&Names` (the marker `.podvm-cache-complete`) |
| [x] The FUSE rung (`serve_tree`, the read-only server, `release_mount`) | the same |
| [x] `--device` by descriptor (`SPEC_VAR`, `SERVE_VAR`) | `Names::{device_spec, device_fds}`; `specs_of`, `push_spec`, `push_serve`, `banner_line` take `&Names` |
| [x] `binfmt` (re-export of the reader, `support_for`, the copy) | the same, from `pod_hostprobe::binfmt` |
| [x] `abi` (the reader, the closure, `admits`, `admits_set`, `resolve_in`) | the same; each offset is checked (podvm truncated a 64-bit offset on a 32-bit target and allocated by a count from the file) |
| [x] `Error` with `exit_code()` (125, 126, 127), `EXIT_*` of `podvm-probe` | `Error::Runtime(Failure)`; `payload_code`, `code_or`; 126 and 127 are `status::*`; 125 stays in podvm |
| [x] The tests: 137 unit and integration tests | each with its name; `resolve_past_a_denied_listing` needs root (`need_root!`), the cover of ptmx needs a user namespace (`need_userns!`), the fifo test `mkfifo`, the exec probe `/bin/sh`; 1 new test |

## Adoption map (podvm)

| Old path | New path |
| --- | --- |
| `podvm_enter::{run, spawn, spawn_selected, spawn_userland, run_ladder, run_namespace, run_selected, run_userland, check_workdir, Plan, Program, RootDir, Fds, Child, Bounded, Error, Result, ENTERED_RUNG, entered_rung, entered_word, exit_status}` | `pod_enter::*`, re-exported by `podvm_enter` |
| `podvm_enter::{abi, binfmt, fuse, plan}` | `pod_enter::*`, re-exported |
| `podvm_enter::{ladder, memfd, device, userland, stage}` | `pod_enter::*` with the functions that take `&Names` wrapped by `podvm_enter` with podvm's signature |
| `PODVM_MODE`, `PODVM_ACTIVE_MODE`, `PODVM_CACHE`, `PODVM_MAPS`, `PODVM_DEVICE_SPEC`, `PODVM_DEVICE_FDS`, `.podvm-cache-complete`, `podvm-payload`, `podvm:` | `podvm_enter::NAMES` |
| `Plan { .. }` | `Plan { .., names: podvm_enter::NAMES }` |
| `Plan::env_for(image, caller)`, `Plan::MODE_REQUEST_VAR` | `Plan::env_for(&NAMES, image, caller)`, `NAMES.mode_request` |
| `ladder::Mode::parse(s)` | `podvm_enter::ladder::parse_mode(s)` |
| `Error::Runtime(text)` | `podvm_enter::runtime(text)` |
| `e.exit_code()` | `use podvm_enter::ExitCode as _;` then `e.exit_code()` (125, 126, 127) |
| `podvm_enter::{EXIT_RUNTIME_ERROR, EXIT_CANNOT_INVOKE, EXIT_NOT_FOUND}` | stay, from `podvm_probe::exit` |
| `crates/podvm-enter/src/*` but `lib.rs`, `tests/tty_refusal.rs` | leave podvm |
| podvm-gate: the pin `33e tty_refusal` | goes; the test is in this crate |
| document-state: `Mode` in `crates/podvm-enter/src/ladder.rs` | `pod_enter::ladder::Mode`, re-exported |

## Tests

`cargo test -p pod-enter`. Each test makes its own temporary directory. A
test that needs root, a user namespace or a tool skips with a line
`skipping, REASON` where the host does not give it (`pod-testkit`). As root
the stage test mounts a tmpfs under the temporary directory and removes it.
