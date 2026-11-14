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
  and recursive, a private tmpfs on the `/tmp` of the image, a fresh
  `proc` (`nosuid`, `nodev`, `noexec`) on the `/proc` of the image where
  that directory exists (`ns_proc_target`), a bind of the host's
  `/dev/pts` on the `/dev/pts` of the image where that directory exists
  (`ns_pts_target`, so `ttyname` of a pty finds its path), a chroot by path with a device
  and inode check against the descriptor. A failure of the setup enters
  chroot and names the fallback on the error writer. A refused proc mount
  or bind of `/dev/pts` does not stop the rung: the child writes one line
  to the payload's standard error (`ns_refused_line`), and the payload
  runs with no `/proc` or `/dev/pts`, as on the chroot rung. The rung has no PID namespace: the
  payload's `/proc` shows the host's processes.
* The overlay root of the namespace rung (`spawn_namespace_root`,
  `run_namespace_root` with `Root::Overlay { lower, upper }`): the
  caller's tree is the read-only lower layer of an overlay, so a shared
  extracted image needs no copy for each run. The upper layer is a
  private tmpfs of the run (`Upper::Tmpfs { size }`, the writes go away
  with the run) or a directory of the caller with an empty work directory
  on its file system (`Upper::Dir { upper, work }`). The child changes its
  working directory to the held lower descriptor before the unshare,
  mounts the tmpfs of the layers on the root path, makes the upper and
  work directories in it (the upper one with the mode of the lower root),
  and mounts overlayfs on the root path with `lowerdir=.`: the lower layer
  is the descriptor, not a path (`overlay_options`; `overlay_escape`
  escapes `,`, `:` and `\` of a path). Then the rung goes on as for a
  tree. The landing check asks for an overlay (`OVERLAYFS_SUPER_MAGIC`)
  whose device is not the device of the lower directory. A refusal at any
  step (an older kernel, a host policy, a lower layer that overlayfs does
  not take) is `Error::Overlay { step, errno }`, which names `overlay`,
  before the payload starts: there is no fallback to chroot, which would
  run the payload on the lower directory. `Root::Tree` is
  `spawn_namespace`.
* The joined entry (`spawn_joined`, for an exec): `setns` on the pidfd of
  a running payload of the namespace rung joins its mount namespace, then
  the chroot by path and the check of the rung. The payload of the exec
  sees the `/tmp` and the `/proc` of the payload of the run; the entry
  mounts nothing. A refused join (a kernel before 5.8, `EPERM`, a process
  that ended) writes `ns_join_fallback_line` and enters
  `spawn_namespace`. `spawn_joined_root` takes a `Root`: for an overlay
  root the chroot must land on an overlay that is not the lower
  directory, and a refused join enters an overlay root of its own.
* The entry with no chroot (`spawn_userland`, `userland`): the loader of
  the image by host path, or a staged memfd for a static payload; the
  table of path maps for the interposer (`userland::Maps`: the root pair
  and one pair for each volume).
* The launch ladder (`ladder`, `spawn_ladder`): memfd, FUSE, tmpfs, run
  directory, cache, in that order. A forced rung that is down refuses
  with its reason. `memfd::probe` measures that a memfd executes (a
  trampoline through the same flags and `execveat` as the launch).
* The reader of ELF files (`abi`): the C library of an image from the
  `PT_INTERP` of the payload, the `DT_NEEDED` closure of an object, and
  the check of a preload against the symbols and versions of `.dynsym`.

Each entry can confine its payload (`Plan::confine`, a `pod_confine::Spec`:
Landlock rules for the paths and the TCP ports, a seccomp deny filter and
`no_new_privs`). The parent probes the host (`pod_confine::check`) and
compiles the spec before the banner and the fork: on each rung with a
chroot (the chroot sequence, the namespace rung, the ladder) the paths are
guest paths, and each opens under the root descriptor with
`RESOLVE_IN_ROOT`, as the payload sees it after the chroot; on the entry
with no chroot they are host paths. A host with no Landlock, or a path
that does not exist, is a refusal by name before the fork. Then the
descriptors of the rules close (`Compiled::close_paths`), and the child
opens each path again (`Compiled::open_paths`) after the passed
descriptors, its mounts, its root change and its `chdir`, before the
limits. Why: Landlock does not read a rule on a directory under a mount
point, so a rule that the compile opened on the `/tmp` of the image does
not cover the tmpfs that the namespace rung mounts on it in the child
(PodUtils/podvm#267). The child opens the path that the payload sees: the
root of that tmpfs. A path that the tmpfs hides (a directory under the
`/tmp` of the image) is a refusal by name in the child (`the confinement
failed at open(O_PATH) for the path "/tmp/hidden": ENOENT (2)`). The child
applies the spec after the limits and `no_new_privs`, as the last step
before the exec; a refused call stops the payload and is named
(`the confinement failed at landlock_add_rule(LANDLOCK_RULE_PATH_BENEATH)
for the path "/usr": EINVAL (22)`).

The table of path maps (`userland::Maps`) is also the resolver of the
entry with no chroot. Each guest path goes through `abi::GuestRoot`: a
plain path is the table of the root pair alone, and a `Maps` adds the
volume pairs, where the pair with the longest guest prefix serves a path.
The walk keeps the guest path: a link target is a guest path again, and
`..` removes a guest component. So a link in a volume that points out of
it goes to the side of the table that holds its target, never to the host
directory beside the volume, and `..` past the guest root is an escape.
The working directory (`check_workdir`, the child of `spawn_userland`) and
the first program (`ladder::payload_head`, `resolve_payload`,
`guest_exe`, `guest_invoked`, `userland::loader_plan`,
`userland::invocation_path`, `abi::resolve_in`) take the table, so `-w`
or a program in a volume is the directory or the file of the volume.

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
| `Names` | `product`, `mode_request`, `mode_active`, `cache`, `maps`, `lib_path`, `device_spec`, `device_fds`, `cache_marker`, `memfd_label`, `reserved` (the guest paths that the interposer of the caller never rewrites; `Maps::new` refuses a volume under one). `Names::DEFAULT` gives `pod-enter`, `POD_ENTER_MODE`, `POD_ENTER_ACTIVE_MODE`, `POD_ENTER_CACHE`, `POD_ENTER_MAPS`, `POD_ENTER_LIB_PATH`, `POD_ENTER_DEVICE_SPEC`, `POD_ENTER_DEVICE_FDS`, `.pod-enter-cache-complete`, `pod-enter-payload`, no reserved path |
| `Plan { argv, env, working_dir, fds, banner, path_dirs, names, limits, no_new_privs, die_with_parent, confine }`, `Program` | what the payload will be; `Plan::{argv_for, env_for(&names, ..), set_active_mode(&names, ..), path_from, program, program_candidates, DEFAULT_PATH}`; `confine: Option<pod_confine::Spec>` (guest paths on a rung with a chroot, host paths on the entry with no chroot) |
| `limits` | `Limit { resource, soft, hard }`, `Resource` (`from_word` of docker's `--ulimit` word, `word`, `number` of the kernel of this architecture, `name`), `INFINITY`, `MOST` (64): the child of each entry sets each limit (`prlimit64` on itself) and then `PR_SET_NO_NEW_PRIVS` where `Plan::no_new_privs` asks, after the root change and the `chdir`, before the exec; a refused call stops the payload and is named (`setrlimit(RLIMIT_NOFILE): EPERM`) |
| `RootDir::open`, `Fds`, `Child`, `Bounded` | the held root; the descriptors of the payload (a passed descriptor onto the number of the root or of the memfd is refused before the fork; the readiness pipe moves clear of each passed number, and the child opens the paths of the rules of the confinement after the passes; a descriptor passed onto its own number loses its close-on-exec flag, so the payload holds it); a started payload (`wait`, `wait_forwarding`, `wait_bounded`) |
| `spawn`, `run`, `spawn_ladder`, `run_ladder`, `spawn_userland`, `run_userland`, `spawn_namespace`, `run_namespace`, `spawn_namespace_root`, `run_namespace_root`, `spawn_joined`, `spawn_joined_root`, `spawn_selected`, `run_selected` | the entries; a `run_*` waits and forwards SIGINT and SIGTERM. `spawn_userland` and `run_userland` take `&Maps` after the plan: the working directory resolves through it before the fork, and a table whose root is not `root.path` is refused |
| `Root` (`Tree(&RootDir)`, `Overlay { lower, upper }`, `dir`), `Upper` (`Tmpfs { size }`, 0 is the default of the kernel; `Dir { upper, work }`, both absolute), `overlay_escape`, `overlay_options`, `OVERLAY_OPTIONS_MOST` (4095: longer options are a refusal before the fork), `NS_OVERLAY_TMPFS_STEP`, `NS_OVERLAY_DIRS_STEP`, `NS_OVERLAY_STEP` | the overlay root of the namespace rung |
| `ENTERED_RUNG`, `entered_rung`, `entered_word` | the rung that a selection of `pod_hostprobe::select` enters |
| `check_workdir(&root, ..)` (`root` is a `GuestRoot`: a path or a `Maps`; a missing directory inside a volume names the volume `HOST:GUEST`, and the image otherwise), `ns_tmp_target`, `ns_fallback_line(&names, ..)`, `ns_setup_step`, `chroot_landed`, `NsMount` | the checks of the entries, pure where they can be |
| `Error`, `Result`, `status` | `Runtime(Failure)`, `CannotInvoke`, `NotFound`, `NsSetup`, `Overlay { step, errno }` (`Class::Unsupported` as a `Failure`); `Error::{runtime, payload_code, code_or}`; `status::{CANNOT_INVOKE, NOT_FOUND, exit_status}` |
| `ladder` | `Mode` (`parse(&names, ..)`, `name`, `ORDER`), `Availability` (`refusal_reason(&names, ..)`), `choose(&names, ..)`, `resolve_payload`, `guest_exe` (the resolved guest path), `guest_invoked` (the `$0` that the kernel gives: the unresolved join of the `PATH` directory), `payload_head` (a `Payload { path, len, head }` with `file()`: the head for the decision, the open file for the stage). Each of the four takes a `GuestRoot` (a path or a `Maps`) |
| `memfd` | `memfd_create`, `stage(&names, bytes)` (bytes in hand), `stage_file(&names, file)` and `stage_file_with(&names, file, each)` (a copy in chunks of `STAGE_CHUNK`, 1 MiB; `each` sees each chunk, for a digest), `seal`, `write_full`, `exec_fd`, `eligible`, `MemfdRefusal`, `ScriptRefusal`, `script_refusal`, `first_shebang_line`, `has_shebang`, `shebang_parts` (the interpreter and its one argument, as the kernel parses a `#!` line in `SHEBANG_HEAD`, 256 bytes), `is_executable`, the `MFD_*` flags; `probe::{memfd_exec, answer(&names, ..), MemfdExec, Standing, Answer, read_sysctl, TRAMPOLINE}` |
| `stage` | `copy_tree`, `stage_tmpfs`, `release_tmpfs`, `cache_dir`, `stage_cache(&names, ..)`, `Refusal` |
| `fuse` | `check_node`, `check_node_at`, `serve_tree`, `release_mount`, `ActiveMount`, `FUSE_NODE` |
| `device` | `parse`, `Mapping`, `canon`, `spec_entry`, `serve_entry`, `banner_line(&names, ..)`, `open_all`, `Opened`, `specs_of(&names, ..)`, `push_spec(&names, ..)`, `push_serve(&names, ..)`, `CHILD_FD_BASE`, `HOST_FD_FLOOR`, `SEP_ENTRY` |
| `userland` | `Family`, `LoaderPlan` (`inhibit_cache`: the loader is glibc 2.16 or later, so a caller can give `--inhibit-cache`), `lib_dirs` (the loader's directory, the standard ones with `/usr/local/lib`, then the image's `/etc/ld.so.conf` or `/etc/ld-musl-ARCH.path`; each resolved inside the image), `takes_inhibit_cache`, `ldconf` (`configured_dirs`, the bounds `MOST_FILES`, `MOST_DEPTH`, `MOST_DIRS`, `MOST_BYTES`), `invocation_path`, `loader_argv_for`, `loader_plan` (the three take a `GuestRoot`), `host_env(&names, ..)`; `Volume { guest, host }`, `Maps` (`of_root(rootfs)`, `new(&names, rootfs, &volumes)` with the refusals of more than `MAX_MAP_PAIRS - 1` volumes (by the name of `names.maps`), of a guest side that is not absolute, is `/`, is not a normal path, repeats one, or is a reserved path of `names` or is under one (by name), and of a host side that is not absolute; `rootfs`, `volumes`, `volume_at`), `userland_maps_table(&names, &maps)` (the root pair, then each volume pair), `userland_volume_table(&names, &maps)` (the volume pairs only), `MAX_MAP_SIDE` (4096 bytes), `MAX_MAP_PAIRS` (32): each side that the variable cannot spell is refused with its own name (the rootfs path, the guest side or the host side of the volume) |
| `binfmt` | `support_for`, `Support`, `copy_in_target`, `copy_interpreter_in`, and the reader of `pod_hostprobe::binfmt` |
| `abi` | `Elf` (`read`, `parse`, `flavour`), `Flavour`, `Versioned`, `Unreadable`, `Verdict`, `admits`, `admits_set`, `interp_of` (head-safe), `interp_of_file`, `HEAD_BYTES`, `READ_CEILING`, `PHDR_TABLE_MAX`, `libc_beside`, `resolve_in`, `resolve_dir_in` (each takes a `GuestRoot`), `GuestRoot` (`root`, `host_of`, `above_mount`; for `Path`, `PathBuf`, `str`, `String` and `userland::Maps`), `ResolveKind`, `link_set`, `LinkSet`, `Member`, `guest_lib_dirs` |

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

No step holds a whole payload, and no step bounds the size of a file:

| Bound | Value | What it bounds |
| --- | --- | --- |
| `abi::HEAD_BYTES` | 64 KiB | the head that `ladder::payload_head` reads for the decision; `interp_of` on a head says "unknown" (an `Unreadable`), never "static", where a program header or the `PT_INTERP` path lies past it |
| `abi::PHDR_TABLE_MAX` | 64 KiB | the program header table (the bound of the kernel) |
| `abi::READ_CEILING` | 64 MiB | the bytes that `Elf::read` holds for one file: the header, the two tables and the sections of the checks, each read with `pread`. A file whose tables need more is an `Unreadable` that names the bound |
| `memfd::STAGE_CHUNK` | 1 MiB | the buffer of `stage_file`. The memfd holds the payload as a file of the kernel |

## Parity

Against podvm `crates/podvm-enter` at `284f429`:

| Capability of podvm | Here |
| --- | --- |
| [x] The chroot sequence, `RootDir`, `Fds`, `Child`, `Bounded`, the forwarding wait, `exit_status` | the same |
| [x] The namespace rung, its fallback, `ns_tmp_target`, `ns_setup_step`, `chroot_landed` | the same; `ns_fallback_line` takes `&Names` (the word `podvm:` is `Names::product`) |
| [x] The entry with no chroot (`spawn_userland`, `run_userland`, `userland`) | the same; `userland_maps_table` and `host_env` take `&Names` (`host_env` sets `Names::lib_path`) |
| [x] `userland::LIB_PATH_VAR` (`PODVM_LIB_PATH`) | `Names::lib_path`; `host_env` sets it |
| [x] `PODVM_MODE`, `PODVM_ACTIVE_MODE` in `Plan` (`MODE_REQUEST_VAR`, `MODE_ACTIVE_VAR`) | `Names::{mode_request, mode_active}`; `Plan::names`; `env_for` and `set_active_mode` take `&Names` |
| [x] The ladder: `Mode`, `Availability`, `choose`, `resolve_payload`, `guest_exe`, `payload_bytes` | the same; `Mode::parse`, `choose`, `refusal_reason` take `&Names` (`PODVM_CACHE` is `Names::cache`). `payload_bytes` and its 128 MiB ceiling (`PAYLOAD_CEILING`) are gone: `payload_head` reads the head, and a payload of any size classifies (PodUtils/podvm#222) |
| [x] `ladder::GUEST_EXE_VAR` (`PODVM_GUEST_EXE`) | stays in podvm: the crate does not read or write it |
| [x] memfd: the flags, `memfd_create`, `stage`, `seal`, `eligible`, the script refusal, the probe that executes a trampoline | the same; `stage` and `probe::answer` take `&Names` (the label `podvm-payload`, the words of the fix line). `stage_file` copies a payload of any size in chunks of 1 MiB |
| [x] `stage`: `copy_tree`, tmpfs, cache | the same; `stage_cache` takes `&Names` (the marker `.podvm-cache-complete`) |
| [x] The FUSE rung (`serve_tree`, the read-only server, `release_mount`) | the same |
| [x] `--device` by descriptor (`SPEC_VAR`, `SERVE_VAR`) | `Names::{device_spec, device_fds}`; `specs_of`, `push_spec`, `push_serve`, `banner_line` take `&Names` |
| [x] `binfmt` (re-export of the reader, `support_for`, the copy) | the same, from `pod_hostprobe::binfmt` |
| [x] `abi` (the reader, the closure, `admits`, `admits_set`, `resolve_in`) | the same; each offset is checked (podvm truncated a 64-bit offset on a 32-bit target and allocated by a count from the file). `Elf::read` has no 128 MiB ceiling: it reads the tables with `pread` (`READ_CEILING`). `interp_of` is head-safe, so it serves podvm's `complete/capability.rs` |
| [x] `Error` with `exit_code()` (125, 126, 127), `EXIT_*` of `podvm-probe` | `Error::Runtime(Failure)`; `payload_code`, `code_or`; 126 and 127 are `status::*`; 125 stays in podvm |
| [x] The tests: 137 unit and integration tests | each with its name; `resolve_past_a_denied_listing` needs root (`need_root!`), the cover of ptmx needs a user namespace (`need_userns!`), the fifo test `mkfifo`, the exec probe `/bin/sh`; 1 new test |

## Adoption map (podvm)

| Old path | New path |
| --- | --- |
| `podvm_enter::{run, spawn, spawn_selected, spawn_userland, run_ladder, run_namespace, run_selected, run_userland, check_workdir, Plan, Program, RootDir, Fds, Child, Bounded, Error, Result, ENTERED_RUNG, entered_rung, entered_word, exit_status}` | `pod_enter::*`, re-exported by `podvm_enter` |
| `podvm_enter::{abi, binfmt, fuse, plan}` | `pod_enter::*`, re-exported |
| `podvm_enter::{ladder, memfd, device, userland, stage}` | `pod_enter::*` with the functions that take `&Names` wrapped by `podvm_enter` with podvm's signature |
| `PODVM_MODE`, `PODVM_ACTIVE_MODE`, `PODVM_CACHE`, `PODVM_MAPS`, `PODVM_DEVICE_SPEC`, `PODVM_DEVICE_FDS`, `.podvm-cache-complete`, `podvm-payload`, `podvm:` | `podvm_enter::NAMES` |
| `Plan { .. }` | `Plan { .., names: podvm_enter::NAMES, limits: Vec::new(), no_new_privs: false, die_with_parent: false, confine: None }` (`--ulimit` and `--security-opt no-new-privileges` fill the limits; the launcher of a detached run sets `die_with_parent`; podvm's record of a detached run keeps them in its own fields) |
| the confine layer of PodUtils/podvm@T-1658 (the profile keys `fs.read`, `fs.write`, `fs.execute`, `net.ports`) | `Plan::confine: Some(pod_confine::Spec)`: `Spec::new().allow(path, Access::READ)` for each key (`Access::WRITE`, `Access::EXECUTE`), `connect_to(port)` (and `bind_to(port)` where a profile asks); `podvm probe` reports `pod_confine::probe()` |
| `Plan::env_for(image, caller)`, `Plan::MODE_REQUEST_VAR` | `Plan::env_for(&NAMES, image, caller)`, `NAMES.mode_request` |
| `ladder::Mode::parse(s)` | `podvm_enter::ladder::parse_mode(s)` |
| `Error::Runtime(text)` | `podvm_enter::runtime(text)` |
| `e.exit_code()` | `use podvm_enter::ExitCode as _;` then `e.exit_code()` (125, 126, 127) |
| `podvm_enter::{EXIT_RUNTIME_ERROR, EXIT_CANNOT_INVOKE, EXIT_NOT_FOUND}` | stay, from `podvm_probe::exit` |
| `crates/podvm-enter/src/*` but `lib.rs`, `tests/tty_refusal.rs` | leave podvm |
| podvm-gate: the pin `33e tty_refusal` | goes; the test is in this crate |
| document-state: `Mode` in `crates/podvm-enter/src/ladder.rs` | `pod_enter::ladder::Mode`, re-exported |
| `ladder::payload_bytes(..)` then `memfd::eligible(&bytes)`, `has_shebang(&bytes)`, `script_refusal(path, &bytes)` | `ladder::payload_head(..)` then the same calls on `&p.head` (the head holds the `#!` line and the ELF headers) |
| `memfd::stage(&bytes)` of a payload | `pod_enter::memfd::stage_file(&NAMES, p.file())` (wrap it in `podvm_enter::memfd`) |
| `ladder::PAYLOAD_CEILING`, the 128 MiB refusal | gone; `doctor` and `probe` can report `abi::HEAD_BYTES`, `abi::READ_CEILING` and `memfd::STAGE_CHUNK` as bounds of memory |
| `lifecycle/script.rs`: `shebang`, `head::shebang` of the interposer (its tests pin the same cases) | `pod_enter::memfd::shebang_parts` |
| `lifecycle/script.rs`: `invoked_path` (calls `guest_exe`: its `$0` has the links followed) | `pod_enter::ladder::guest_invoked` |
| `complete/capability.rs`: `static_head` | `pod_enter::abi::interp_of(path, &head)` (`Ok(None)` is static, an error is unknown), or `interp_of_file(path)` |
| `podvm_enter::userland::Volume`, `volume_at(&volumes, p)` | `pod_enter::userland::Volume` (the same fields), `maps.volume_at(p)` |
| `podvm_enter::userland::userland_maps_table(root, &volumes)`: `side`, `normal`, the absolute, `/` and second-volume checks of the guest side, `MAX_VOLUMES` | `let maps = Maps::new(&NAMES, rootfs, &volumes)?` (`NAMES.reserved` is `["/proc", "/.podvm"]`), then `pod_enter::userland::userland_maps_table(&NAMES, &maps)`, or `userland_volume_table(&NAMES, &maps)` under `--unsafe-host-paths` (no root pair); the bound is `MAX_MAP_PAIRS`, so `MAX_VOLUMES` is `MAX_MAP_PAIRS - 1`, and `Maps::new` checks it. `NEVER_MAPPED` is `NAMES.reserved`, and `Maps::new` checks it. `volume_maps` goes; `volumes_of` stays in podvm |
| `lifecycle::volume::refuse_paths`: `-w` or a first program in a volume refused by name | goes; give `&maps` to `check_workdir`, `ladder::payload_head`, `resolve_payload`, `guest_exe`, `guest_invoked`, `userland::invocation_path`, `userland::loader_plan`, `abi::resolve_in` (`interpose.rs`) and to `lifecycle::script::follow` |
| `run_userland(root, plan, argv, ..)`, `spawn_userland(root, plan, argv, ..)` (`lifecycle/userland.rs`, `podvm-supervise`'s `launcher.rs`) | `run_userland(root, plan, &maps, argv, ..)`; `Maps::of_root(rootfs)` where the run has no volume. The child enters the working directory that the walk resolved: a link of the image in `-w` is followed inside the image, not on the host |
| `userland::invocation_path` | the invoked spelling stays only where the host opens the resolved file through it (the same device and inode); else the resolved file |

## Tests

`cargo test -p pod-enter`. Each test makes its own temporary directory. A
test that needs root, a user namespace or a tool skips with a line
`skipping, REASON` where the host does not give it (`pod-testkit`). As root
the stage test mounts a tmpfs under the temporary directory and removes it.
`tests/bounded_payload.rs` makes a sparse file of 200 MiB (almost no disk),
and a counting allocator asserts that each step holds less than 4 MiB of
heap; its stage test skips under an emulator. `tests/confine.rs` (no
libtest: the binary is the payload too) runs the checks of podvm's
experiment 414 through the entry with no chroot (host paths), the chroot
sequence and the namespace rung (guest paths, in an image of the test
binary; these need root): a read outside the read list and a TCP connect
outside the port list fail with `EACCES`, a connect inside passes, the
payload has `NoNewPrivs: 1` and `Seccomp: 2`, and `unshare` and `setns`
fail with `EPERM`. It also checks that a missing path and a host with no
Landlock (a seccomp filter stands in for it) stop the entry before the
fork, and that a passed descriptor takes the place of no rule and of no
readiness pipe. On the namespace rung, a write rule on `/tmp` covers its
tmpfs (the write lands on the tmpfs, not in the image), and a write into
`/var` or `/` of the image fails with `EACCES`; the chroot sequence gives
the same; a rule on a path that the tmpfs hides is a refusal by name; a
write rule holds on the entry with no chroot. The namespace rung gives a
payload a `/proc` with `noexec`, and the chroot sequence gives none. A
joined entry opens a file that the running payload wrote in its `/tmp`;
with the pidfd of a process that ended, it names the step and opens
nothing. On an overlay root (the path of the image holds `,` and `:`),
a payload that writes a new file, changes and removes files of the image
and makes a directory under `/` sees each change, the image keeps its
digest (mode, owner, size, link target and bytes of each path), and a
second run sees the image as it was; a lower directory under the most
overlays that the kernel stacks is a refusal (`Error::Overlay` at
`mounting overlay on the root: EINVAL`), and the payload does not run; a
joined entry sees the writes of the running payload under `/`, and with
the pidfd of a process that ended enters an overlay root of its own.
`tests/overlay_root.rs` is podvm's acceptance test of the overlay root
(PodUtils/podvm#323) with an upper directory of the caller: the writes
are in the upper layer, the image is the same byte for byte, and a file
that was only read did not go up. It runs a static busybox
(`PODVM_T486_BUSYBOX`, else `/bin/busybox`) and skips with a line
`SKIP:` with none, or where the rung refuses the overlay root. A
host with no Landlock
skips the runs of a confined payload. The tests of the rungs with a
chroot skip as another user, so CI runs the two binaries as root, with
the package `busybox-static` (`scripts/ci/root-enter-confine-tests.sh`;
a skip line fails it). The
plants `enter-confine-not-applied`,
`enter-confine-rule-on-a-passed-number`, `enter-confine-rule-under-a-mount`,
`enter-pipe-on-a-passed-number`, `enter-pass-onto-the-root`,
`enter-own-number-stays-cloexec`, `enter-ns-proc-exec`,
`enter-ns-join-skipped`, `enter-ns-pts-unbound`,
`enter-overlay-lower-as-root`, `enter-overlay-falls-back` and
`enter-overlay-join-as-tree`
(`scripts/plants/`) prove the checks.
