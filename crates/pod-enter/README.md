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
  runs with no `/proc` or `/dev/pts`, as on the chroot rung. With no PID
  namespace of the plan, the payload's `/proc` shows the host's processes.
* The namespace rung for a caller with no capabilities (`ns_user`,
  PodUtils/podvm#406): at `EPERM` of `unshare(CLONE_NEWNS)`, the entry
  tries the rung again with a user namespace first, as `unshare -Ur` makes
  it: `unshare(CLONE_NEWUSER)`, `deny` into `/proc/self/setgroups`, then
  one line in `uid_map` and in `gid_map` (the effective ids of the caller
  to the ids of `Plan::userns_ids`, 0 by default; the sequence of
  `pod_hostprobe::userns`, built before the fork),
  then each step of the rung as for root. Each call has its step
  (`NS_USER_STEP` 23, `NS_SETGROUPS_STEP` 24, `NS_UID_MAP_STEP` 25,
  `NS_GID_MAP_STEP` 26). Only then does the entry fall back to chroot, and
  the line names both refusals (`both_refused`). The overlay root takes
  the same retry. The files of the caller show as owned by the ids inside
  (0 by default) in the payload, each other owner as 65534. The kernel refuses a fresh proc there (the
  PID namespace belongs to the user namespace of the host): the child
  binds the host's `/proc` recursively (`NS_PROC_BIND_STEP` 28) and
  remounts the bind `nosuid`, `nodev` and `noexec`
  (`NS_PROC_REMOUNT_STEP` 29; a refused remount removes the bind). It
  binds only a proc whose `/proc/self` is the pid of the entry
  (`proc_of_this_pid_namespace`): a proc of another PID namespace gives
  one line and no `/proc`. The banner goes out one time for each entry,
  also over a retry or a fallback.
* The ids of the payload inside the user namespace (`Plan::userns_ids`,
  PodUtils/podvm#447): `Some((uid, gid))` writes `<uid> <caller-uid> 1`
  and `<gid> <caller-gid> 1`; `None` is 0. The child holds each capability
  in the namespace that it makes, whatever its ids there, so each mount of
  the rung is the same; only the caller's ids are mapped, so each file
  that the child makes gets them. The `execve` of a uid that is not 0
  clears the permitted and the effective capabilities: the payload of a
  user of its image holds none. A join of a run keeps the map of the run
  (an exec of a run with 1001 is 1001, whatever its plan). `u32::MAX` is
  refused before the fork.
* The ids of the plan for a caller that is uid 0 of a user namespace that
  is not the initial one (`ns_user`, PodUtils/podvm#452): its
  `/proc/self/uid_map` is not `0 0 4294967295`
  (`pod_hostprobe::userns::is_initial_map`, read before the fork; a map
  that cannot be read is a refusal). Such a caller makes the mount
  namespace with no user namespace of the rung. Where `Plan::userns_ids`
  is `Some((uid, gid))`, the child opens a nested user namespace after
  each mount of the rung (the volumes and the host name too) and before
  the root change: `unshare(CLONE_NEWUSER)`, `deny` into `setgroups`,
  `<uid> 0 1` and `<gid> 0 1` (the sequence of `pod_hostprobe::userns`).
  There `/proc/self` of the root of the caller holds the maps; after it the
  kernel refuses a mount of the mount namespace of the caller, and the
  chroot needs only `CAP_SYS_CHROOT` in the nested namespace. Then the
  chroot, the confinement and the exec: the `execve` of the uid of the plan
  clears each capability. The chroot rung and the ladder take the same
  step. A refused call stops the payload with its step
  (`NS_NESTED_USER_STEP` 37, `NS_NESTED_SETGROUPS_STEP` 38,
  `NS_NESTED_UID_MAP_STEP` 39, `NS_NESTED_GID_MAP_STEP` 40): the error is
  `Error::Runtime` with the ids, the call and the errno, never `NsSetup`
  and never a fallback to chroot as 0. A host root (the initial
  namespace), a join of a run (the map of the run holds) and the entry
  with no chroot keep the ids of the caller (T-791).
* The device nodes of the host on the namespace rung through a user
  namespace (`ns_dev_bind`, PodUtils/podvm#414). The kernel refuses
  `mknod` there, so the image holds regular files at the paths of
  `DEVICES` (a `/dev/urandom` that ends after 1 MiB). Before the fork,
  the entry reads the kind of `/dev/null`, `/dev/zero`, `/dev/full`,
  `/dev/random`, `/dev/urandom` and `/dev/tty` through the descriptor of
  the root with `O_NOFOLLOW` (`kind_of`, `choose`): a regular file, a
  missing path or a character device takes the bind; a link, a directory
  or another kind takes one line and no bind. The host's node must be a
  character device. An image with no `/dev` directory (or a link there)
  takes one line. In the child, after the optional mounts: an empty file
  as the mount point where the path is missing (`O_CREAT | O_EXCL |
  O_NOFOLLOW` through the descriptor of the image's `/dev`), a bind of
  the host's node (`MS_BIND`, `NS_DEV_BIND_STEP` 31) onto
  `/proc/self/fd/N` of an `O_PATH | O_NOFOLLOW` descriptor that is not a
  link (`NS_DEV_POINT_STEP` 30), then a remount with `nosuid` and
  `noexec` and not `nodev` (`NS_DEV_REMOUNT_STEP` 32; `MS_RDONLY` where
  the host's node is on a read-only mount). A refused step writes one
  line to the payload's standard error and the rung goes on; a refused
  remount removes the bind. The bind needs the host's `/proc` before the
  chroot. The rung as root binds nothing: there `mknod` works.
* A pty on the namespace rung through a user namespace (`ns_pts`,
  PodUtils/podvm#469). The host's devpts usually has `ptmxmode=000`, and
  the image holds no `/dev/ptmx`, so `openpty` fails with the bind of the
  host's `/dev/pts`. In a user namespace, where the image has a
  `/dev/pts` directory, the child mounts a new devpts instance there in
  place of that bind (`DEVPTS_OPTIONS`: `newinstance,ptmxmode=0666,mode=0620`,
  no `gid=`: the group `tty` of the host has no id there; `nosuid` and
  `noexec`; `NS_DEVPTS_STEP` 46). Then, with the devices, it binds the
  `ptmx` of the instance on `/dev/ptmx` (`NS_PTMX_BIND_STEP` 47), with an
  empty file as the mount point where the image has none, and remounts
  the bind `nosuid` and `noexec`. Where the kernel refuses the instance,
  the child writes one line (`devpts_refused`: `the namespace rung binds
  the host's /dev/pts and its ptmx: mounting a new devpts instance on
  /dev/pts failed with errno N`), binds the host's `/dev/pts` as before,
  and binds its `ptmx` on `/dev/ptmx`, which opens only where the host's
  `ptmxmode` lets the caller open it. A bind of the host's `/dev/ptmx`
  never opens: the kernel looks for the devpts at `pts` beside that node
  in the same mount. A link at `/dev/ptmx`
  takes one line and no bind. The rung as root keeps the bind of the
  host's `/dev/pts`.
* The volumes of a plan on the namespace rung (`Plan::volumes`, a
  `Vec<VolumeBind { host, guest, read_only }>`, PodUtils/podvm#440), as
  root and through a user namespace, in their order. Before the banner
  and the fork, the entry refuses by name a guest path that is not an
  absolute normal path (`volume_guest_parts`: no `.`, `..` or empty
  component, not `/`), a guest path that two volumes give, a host path
  that is not absolute or does not exist, and more than `VOLUMES_MOST`
  (255) volumes. In the child, after the private remount of `/`, the
  tmpfs, the optional mounts and the device binds, before the chroot: the
  root opens by its path, and each component of the guest path opens under
  it with `openat2(RESOLVE_IN_ROOT | RESOLVE_NO_SYMLINKS)`. A missing
  directory is made (mode 0755), and a missing last component is an empty
  file where the host side is not a directory. A symbolic link of the
  image on the path gives `ELOOP` (`NS_VOLUME_LINK_STEP` 34: "the guest
  path of a volume crosses a symbolic link of the image", `Class::Policy`);
  another refused open or make is `NS_VOLUME_POINT_STEP` 33. Then
  `mount(host, /proc/self/fd/N, MS_BIND | MS_REC)` (`NS_VOLUME_BIND_STEP`
  35). A volume with `host_base` (a directory that the caller checked,
  which holds `host`) opens its host path under the base with
  `pod-beneath` (`openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)`), before
  the fork for a refusal by name and again in the child, and the bind
  takes that descriptor: a link that the host makes under the base after
  the check moves no bind. A link under the base or a `..` above it is
  `Class::Policy`; a host path that is not under its base is a usage
  error; a refused open in the child is step 48. A read-only volume opens again through the bind, `fstatfs` reads
  the flags of that mount, and `MS_REMOUNT | MS_BIND | MS_RDONLY` gives
  `nosuid`, `nodev`, `noexec`, the atime flags and `nosymfollow` again
  where the mount has them (`volume_remount_flags`; `NS_VOLUME_REMOUNT_STEP`
  36): a user namespace locks them. The mounts under the host path keep
  their flags. A refused step stops the payload: the error is
  `Error::Runtime` with the volume (`volume_label`, `HOST:GUEST[:ro]`), the
  step and the errno, never a run with a volume missing, and never a
  fallback to chroot. The chroot rung, the ladder and the entry with no
  chroot refuse a plan with a volume (the entry with no chroot takes its
  volumes in `userland::Maps`). A join (`spawn_joined`) binds none: the
  mount namespace of the run holds them. A working directory in a volume
  resolves in the host side of the volume before the fork
  (`userland::Maps::of_binds`, which a caller gives to `check_workdir`),
  and so does the path of each rule of the confinement.
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
  escapes `,`, `:` and `\` of a path). In a user namespace (the one
  that the rung makes, or the one of a caller that is uid 0 of a user
  namespace that is not the initial one) the mount has `userxattr`
  (`USERXATTR`): overlayfs then reads and writes `user.overlay.*`, as it
  cannot read `trusted.` there; a kernel before 5.11 refuses the option
  (`EINVAL`), and the mount goes on without it. Then the rung goes on as
  for a tree. The landing check asks for an overlay (`OVERLAYFS_SUPER_MAGIC`)
  whose device is not the device of the lower directory. A refusal at any
  step (an older kernel, a host policy, a lower layer that overlayfs does
  not take) is `Error::Overlay { step, errno }`, which names `overlay`,
  before the payload starts: there is no fallback to chroot, which would
  run the payload on the lower directory. `Root::Tree` is
  `spawn_namespace`. With `Upper::Dir`, the checks before the fork (the
  working directory, and the path of each rule of the confinement) read
  the upper directory over the lower one, as overlayfs shows them
  (`ns_upper`, the rules of `pod-rootfs`): an entry of the upper layer
  hides the same name of the lower one, a whiteout (a character device
  0:0) removes it, and a directory whose opaque attribute is `y` hides
  the lower one (`trusted.overlay.opaque` for a mount of a host root,
  `user.overlay.opaque` for a mount with `userxattr`: the attribute that
  the mount reads); a volume hides
  both layers. So `-w` or a rule on a path that only the upper directory
  holds passes, and a path that it removes is a refusal by name before
  the fork. A join of an overlay run reads the same view.
  `Upper::Tmpfs` starts empty: the checks read the lower layer.
* The PID namespace of the namespace rung (`Plan::pid_namespace`,
  `ns_pid`): the entry clones its child with `CLONE_NEWPID`, and with
  `CLONE_NEWUSER` where the rung makes a user namespace, so that the user
  namespace of the rung owns the PID namespace. The child mounts a fresh
  `proc` of the namespace on `/proc`; the bind of the host's `/proc` is not
  a fallback. The child is pid 1 and stays as the init: just before the
  exec it forks the payload (pid 2), gives each signal of `PID_FORWARDED`
  (`WAIT_FORWARDED` and SIGWINCH) that a process sent to it,
  reaps each orphan, and exits with the code of the payload (128 + the
  signal for a payload that a signal ended). The init holds the mounts,
  the root, the limits and the confinement of the payload. The `Child` is
  the host pid of the init; a SIGKILL to it ends each process of the
  namespace. `EPERM` of `clone(CLONE_NEWPID)` (`NS_PID_STEP` 41) calls for
  the form with a user namespace, as `EPERM` of `unshare(CLONE_NEWNS)`
  does. Each other entry refuses the plan, and the namespace rung does not
  fall back to chroot with it.
* The joined entry (`spawn_joined`, for an exec): `setns` on the pidfd of
  a running payload of the namespace rung joins its mount namespace, then
  the chroot by path and the check of the rung. Where the payload is in a
  user namespace that is not the one of the caller (`ns_user`), one call
  joins both (`setns(pidfd, CLONE_NEWUSER | CLONE_NEWNS)`,
  `NS_JOIN_USER_STEP` 27): the owner of that user namespace holds each
  capability in it. The entry compares `/proc/self/ns/user` with the one
  of the pid of the pidfd (`other_user_namespace`) before the fork. Where
  the payload is in a PID namespace that is not the one of the caller
  (`other_namespace`), the same call adds `CLONE_NEWPID`
  (`NS_JOIN_PID_STEP` 43, `NS_JOIN_USER_PID_STEP` 44): the child forks the
  payload of the exec into the namespace and stays as its relay, with the
  loop of the init. The payload of the exec
  sees the `/tmp` and the `/proc` of the payload of the run; the entry
  mounts nothing. A refused join (a kernel before 5.8, `EPERM`, a process
  that ended) writes `ns_join_fallback_line` and enters
  `spawn_namespace`. `spawn_joined_root` takes a `Root`: for an overlay
  root the chroot must land on an overlay that is not the lower
  directory, and a refused join enters an overlay root of its own.
* The entry with no chroot (`spawn_userland`, `userland`): the loader of
  the image by host path, or a staged memfd for a static payload; the
  table of path maps for the interposer (`userland::Maps`: the root pair
  and one pair for each volume). The library path of the loader names the
  `RUNPATH` directories of the payload and of its libraries, resolved in
  the image (`userland::lib_dirs_for`). Two limits stay: the loader reads
  a `DT_RPATH` before the library path, so an absolute `RPATH` directory
  that the host also holds is read on the host first; and a dynamic child
  that the interposer starts takes the library path of the caller
  (`names.lib_path`), with no `RUNPATH` directory of the child.
* The launch ladder (`ladder`, `spawn_ladder`): memfd, FUSE, tmpfs, run
  directory, cache, in that order. A forced rung that is down refuses
  with its reason. `memfd::probe` measures that a memfd executes (a
  trampoline through the same flags and `execveat` as the launch).
* The reader of ELF files (`abi`): the C library of an image from the
  `PT_INTERP` of the payload, the `DT_NEEDED` closure of an object, and
  the check of a preload against the symbols and versions of `.dynsym`.
  The reason of a `Verdict::Refused` and of an `Unreadable` carries no
  verdict word (`refused:`, `note:`): the caller adds its label. A static
  object (no `DT_NEEDED`) gets a refusal that names the static link.

Each entry can confine its payload (`Plan::confine`, a `pod_confine::Spec`:
Landlock rules for the paths and the TCP ports, a seccomp deny filter and
`no_new_privs`). The parent probes the host (`pod_confine::check`) and
compiles the spec before the banner and the fork: on each rung with a
chroot (the chroot sequence, the namespace rung, the ladder) the paths are
guest paths, and each opens under the root descriptor with
`RESOLVE_IN_ROOT`, as the payload sees it after the chroot; on the entry
with no chroot they are host paths. On an overlay root with an upper
directory, each path resolves in the upper layer over the image (see the
overlay root above). Where the plan has volumes, each path
resolves through them (`pod_confine::Spec::compile_by` with the walk of
`userland::Maps::of_binds`, as for the working directory): a path at or
under the guest path of a volume opens in its host side, and each other
path in the image. A directory above the guest path of a volume that the
image does not hold is made by the child, and its rule is accepted. A
host with no Landlock, or a path that neither the image nor a volume
holds, is a refusal by name before the fork. Then the
descriptors of the rules close (`Compiled::close_paths`), and the child
opens each path again (`Compiled::open_paths`) after the passed
descriptors, its mounts, its root change and its `chdir`, before the
limits. Why: Landlock does not read a rule on a directory under a mount
point, so a rule that the compile opened on the `/tmp` of the image does
not cover the tmpfs that the namespace rung mounts on it in the child
(PodUtils/podvm#267). The child opens the path that the payload sees: the
root of that tmpfs, and the root of the bind of a volume (the child opens
the paths after the binds of the volumes). A path that the tmpfs hides (a directory under the
`/tmp` of the image) is a refusal by name in the child (`the confinement
failed at open(O_PATH) for the path "/tmp/hidden": ENOENT (2)`). The child
applies the spec after the limits and `no_new_privs`, as the last step
before the exec; a refused call stops the payload and is named
(`the confinement failed at landlock_add_rule(LANDLOCK_RULE_PATH_BENEATH)
for the path "/usr": EINVAL (22)`).

On the entry with no chroot the kernel executes the loader of the image,
or the interpreter of a script, and only maps or reads the payload, and a
static payload runs from a memfd: Landlock checks `EXECUTE` on none of
these payloads (measured on Linux 6.18, ABI 7: a static ELF, a dynamic
ELF and a script outside each execute path run there). Thus the entry
gives the payload the exec table of the spec (`pod_confine::ExecTable`,
each host path resolved) in the variable `names.exec_table`, and removes
each value of the caller; the interposer checks each program that it
routes through the loader against it (`pod_confine::exec::allows_in`).
`check_exec(&plan, program)` gives the same answer to the caller for the
payload (and for a script, the script and its interpreter) before
`spawn_userland`, or before it stages a memfd: a program outside each
execute path is `Error::CannotInvoke` (126), as `EACCES` of the kernel.
`check_exec_argv(&plan, argv)` (host paths) and
`check_exec_guest(&plan, &maps, argv)` (guest paths: `PATH`, the working
directory and the links of the image through the table) check each file
that the kernel would execute for an argv: the program, the interpreter of
each `#!` line, and where one is a dynamic loader
(`pod_confine::loader`), each argument that the loader can take as its
program. Landlock governs no change of mode, owner or times, so the entry
gives the payload the write table of the spec too
(`Spec::table(Access::WRITE)`, resolved) in `names.write_table`, and
removes each value of the caller.
A rung with a chroot gets no variable: the kernel executes each program
there.

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
| `Names` | `product`, `mode_request`, `mode_active`, `cache`, `maps`, `lib_path`, `exec_table`, `write_table`, `device_spec`, `device_fds`, `cache_marker`, `memfd_label`, `reserved` (the guest paths that the interposer of the caller never rewrites; `Maps::new` refuses a volume under one). `Names::DEFAULT` gives `pod-enter`, `POD_ENTER_MODE`, `POD_ENTER_ACTIVE_MODE`, `POD_ENTER_CACHE`, `POD_ENTER_MAPS`, `POD_ENTER_LIB_PATH`, `POD_ENTER_EXEC_TABLE`, `POD_ENTER_WRITE_TABLE`, `POD_ENTER_DEVICE_SPEC`, `POD_ENTER_DEVICE_FDS`, `.pod-enter-cache-complete`, `pod-enter-payload`, no reserved path |
| `Plan { argv, env, working_dir, fds, banner, path_dirs, names, limits, no_new_privs, die_with_parent, confine, hostname, volumes, userns_ids }`, `Program`, `VolumeBind { host, guest, read_only, host_base }` | what the payload will be; `Plan::{argv_for, env_for(&names, ..), set_active_mode(&names, ..), path_from, program, program_candidates, DEFAULT_PATH}`; `confine: Option<pod_confine::Spec>` (guest paths on a rung with a chroot, host paths on the entry with no chroot); `hostname: Option<String>` (the namespace rung enters a UTS namespace and sets it; each other entry refuses a plan with a name, and the rung does not fall back to chroot with one); `volumes: Vec<VolumeBind>` (the namespace rung binds each one, in order; each entry with no mount namespace refuses a plan with one, a join binds none, and the rung does not fall back to chroot with one); `userns_ids: Option<(u32, u32)>` (the uid and the gid of the payload in the user namespace of the rung; `None` is 0) |
| `limits` | `Limit { resource, soft, hard }`, `Resource` (`from_word` of docker's `--ulimit` word, `word`, `number` of the kernel of this architecture, `name`), `INFINITY`, `MOST` (64): the child of each entry sets each limit (`prlimit64` on itself) and then `PR_SET_NO_NEW_PRIVS` where `Plan::no_new_privs` asks, after the root change and the `chdir`, before the exec; a refused call stops the payload and is named (`setrlimit(RLIMIT_NOFILE): EPERM`); `repeated` names the first resource that a list gives two times, and each entry refuses such a plan before the fork with `Error::LimitTwice`: the caller coalesces first (docker keeps the last value) |
| `RootDir::open`, `Fds`, `Child`, `Bounded` | the held root; the descriptors of the payload (a passed descriptor onto the number of the root or of the memfd is refused before the fork; the readiness pipe moves clear of each passed number, and the child opens the paths of the rules of the confinement after the passes; a descriptor passed onto its own number loses its close-on-exec flag, so the payload holds it); a started payload (`wait`, `wait_forwarding`, `wait_bounded`); `WAIT_FORWARDED`, the signals that `wait_forwarding` and the init of `Plan::pid_namespace` give to the payload: SIGHUP, SIGINT, SIGQUIT, SIGUSR1, SIGUSR2, SIGTERM, SIGALRM and SIGPWR, as docker's `--sig-proxy` with no tty (SIGINT and SIGQUIT only from a process, as a terminal sends them to the process group of the payload too); each forward is a line on `err` |
| `spawn`, `run`, `spawn_ladder`, `run_ladder`, `spawn_userland`, `run_userland`, `spawn_namespace`, `run_namespace`, `spawn_namespace_root`, `run_namespace_root`, `spawn_joined`, `spawn_joined_root`, `spawn_selected`, `run_selected` | the entries; a `run_*` waits and forwards SIGINT and SIGTERM. `spawn_userland` and `run_userland` take `&Maps` after the plan: the working directory resolves through it before the fork, and a table whose root is not `root.path` is refused |
| `Root` (`Tree(&RootDir)`, `Overlay { lower, upper }`, `dir`), `Upper` (`Tmpfs { size }`, 0 is the default of the kernel; `Dir { upper, work }`, both absolute), `overlay_escape`, `overlay_options`, `USERXATTR`, `OVERLAY_OPTIONS_MOST` (4095: longer options, with `USERXATTR`, are a refusal before the fork), `NS_OVERLAY_TMPFS_STEP`, `NS_OVERLAY_DIRS_STEP`, `NS_OVERLAY_STEP` | the overlay root of the namespace rung |
| `check_exec(&plan, program)` | whether the confinement of the plan lets the program at a host path run on the entry with no chroot: `Ok` with no confinement or under an execute path, `Error::CannotInvoke` outside each one, `Error::NotFound` where the path does not resolve |
| `check_exec_argv(&plan, argv)`, `check_exec_guest(&plan, &maps, argv)` | `check_exec` for each file that the kernel executes for `argv`: the program, the interpreter of each `#!` line (at most 4, as the kernel), and each program of a dynamic loader (`pod_confine::loader::each_program`; a program with no `/`, one that does not resolve, and a loader as the program of a loader are `Error::CannotInvoke`). `check_exec_argv` takes host paths; `check_exec_guest` takes guest paths: a bare name on `plan.path_dirs`, a relative one under `plan.working_dir`, each one through `maps`, and a program or an interpreter that does not resolve is `Ok` (the spawn names it, 127) |
| `ENTERED_RUNG`, `entered_rung`, `entered_word` | the rung that a selection of `pod_hostprobe::select` enters |
| `check_workdir(&root, ..)` (`root` is a `GuestRoot`: a path or a `Maps`; a missing directory inside a volume names the volume `HOST:GUEST`, and the image otherwise), `ns_tmp_target`, `ns_fallback_line(&names, ..)`, `ns_setup_step`, `chroot_landed`, `NsMount` | the checks of the entries, pure where they can be |
| `Error`, `Result`, `status` | `Runtime(Failure)`, `CannotInvoke`, `NotFound`, `NsSetup`, `Overlay { step, errno }` (`Class::Unsupported` as a `Failure`), `LimitTwice(Resource)` (`Class::Usage` as a `Failure`); `Error::{runtime, payload_code, code_or}`; `status::{CANNOT_INVOKE, NOT_FOUND, exit_status}` |
| `ladder` | `Mode` (`parse(&names, ..)`, `name`, `ORDER`), `Availability` (`refusal_reason(&names, ..)`), `choose(&names, ..)`, `resolve_payload`, `guest_exe` (the resolved guest path), `guest_invoked` (the `$0` that the kernel gives: the unresolved join of the `PATH` directory), `payload_head` (a `Payload { path, len, head }` with `file()`: the head for the decision, the open file for the stage). Each of the four takes a `GuestRoot` (a path or a `Maps`) |
| `memfd` | `memfd_create`, `stage(&names, bytes)` (bytes in hand), `stage_file(&names, file)` and `stage_file_with(&names, file, each)` (a copy in chunks of `STAGE_CHUNK`, 1 MiB; `each` sees each chunk, for a digest), `seal`, `write_full`, `exec_fd`, `eligible`, `MemfdRefusal`, `ScriptRefusal`, `script_refusal`, `first_shebang_line`, `has_shebang`, `shebang_parts` (the interpreter and its one argument, as the kernel parses a `#!` line in `SHEBANG_HEAD`, 256 bytes), `is_executable`, the `MFD_*` flags; `probe::{memfd_exec, answer(&names, ..), MemfdExec, Standing, Answer, read_sysctl, TRAMPOLINE}` |
| `stage` | `copy_tree`, `stage_tmpfs`, `release_tmpfs`, `cache_dir`, `stage_cache(&names, ..)`, `Refusal` |
| `fuse` | `check_node`, `check_node_at`, `serve_tree`, `release_mount`, `ActiveMount`, `FUSE_NODE` |
| `device` | `parse`, `Mapping`, `canon`, `spec_entry`, `serve_entry`, `banner_line(&names, ..)`, `open_all`, `Opened`, `specs_of(&names, ..)`, `push_spec(&names, ..)`, `push_serve(&names, ..)`, `CHILD_FD_BASE`, `HOST_FD_FLOOR`, `SEP_ENTRY` |
| `NS_USER_STEP`, `NS_SETGROUPS_STEP`, `NS_UID_MAP_STEP`, `NS_GID_MAP_STEP`, `calls_for_user_namespace`, `both_refused` | the namespace rung through a user namespace (PodUtils/podvm#406): the steps in `ns_setup_step`, the refusal that calls for the retry (`EPERM` of `unshare(CLONE_NEWNS)`), the cause of the fallback line |
| `NS_NESTED_USER_STEP`, `NS_NESTED_SETGROUPS_STEP`, `NS_NESTED_UID_MAP_STEP`, `NS_NESTED_GID_MAP_STEP` | the steps 37 to 40 of the nested user namespace of a caller that is uid 0 of a user namespace (PodUtils/podvm#452); a refused one is `Error::Runtime` with the ids of the plan, not `NsSetup` |
| `DEVICES`, `Kind`, `kind_of`, `Choice`, `choose`, `NS_DEV_POINT_STEP`, `NS_DEV_BIND_STEP`, `NS_DEV_REMOUNT_STEP`, `ns_dev_refused_start(&names, ..)`, `ns_dev_skipped_line(&names, ..)`, `no_dev_line(&names)` | the device nodes of the host on the namespace rung through a user namespace (PodUtils/podvm#414): the choice of each path, the steps in `ns_setup_step`, the lines |
| `DEVPTS_OPTIONS`, `DEVPTS_FLAGS`, `NS_DEVPTS_STEP`, `NS_PTMX_BIND_STEP`, `devpts_refused(&names)`, `ptmx_of` | a devpts instance of the unit and its `/dev/ptmx` on the namespace rung through a user namespace (PodUtils/podvm#469): the options, the steps in `ns_setup_step`, the line of a refused instance |
| `NS_VOLUME_POINT_STEP`, `NS_VOLUME_LINK_STEP`, `NS_VOLUME_BIND_STEP`, `NS_VOLUME_REMOUNT_STEP`, `VOLUMES_MOST`, `volume_label`, `volume_guest_parts`, `volume_remount_flags` | the volumes of the namespace rung (PodUtils/podvm#440): the steps 33 to 36 in `ns_setup_step` (a refused one is `Error::Runtime` with the volume, not `NsSetup`), at most 255 volumes, `HOST:GUEST[:ro]`, the components of a guest path or why it is not one, the flags of the read-only remount for the `f_flags` of `statfs` |
| `uts` | `NS_UTS_STEP`, `NS_HOSTNAME_STEP` (the steps of `unshare(CLONE_NEWUTS)` and `sethostname` in `ns_setup_step`), `MOST` (64 bytes): the host name of the payload on the namespace rung (PodUtils/podvm#403) |
| `userland` | `Family`, `LoaderPlan` (`inhibit_cache`: the loader is glibc 2.16 or later, so a caller can give `--inhibit-cache`), `lib_dirs` (the loader's directory, the standard ones with `/usr/local/lib`, then the image's `/etc/ld.so.conf` or `/etc/ld-musl-ARCH.path`; each resolved inside the image), `lib_dirs_for(root, interp, program)` (the `DT_RUNPATH` (else `DT_RPATH`) directories of the program and of each library that it loads, in the order of the loader, then `lib_dirs`: the loader runs by host path, so it reads an absolute directory of a `RUNPATH` on the host, and the library path names the one of the image), `runpath` (`runpath_dirs`, the guest directories, each resolved inside the image, one that leaves it not added; `Tokens`: `$ORIGIN` for both C libraries, and for glibc `$LIB` from the directory of its C library (`lib/TRIPLET` for a multiarch directory, else its last component) and `$PLATFORM` (`glibc_platform`: `haswell` on an Intel x86-64 CPU of that class with glibc 2.26 or later, else `at_platform`, the `AT_PLATFORM` of the kernel); the bounds `MOST_OBJECTS`, `MOST_DIRS`), `takes_inhibit_cache`, `ldconf` (`configured_dirs`, the bounds `MOST_FILES`, `MOST_DEPTH`, `MOST_DIRS`, `MOST_BYTES`), `invocation_path`, `loader_argv_for`, `loader_plan` (the three take a `GuestRoot`; the library directories of `loader_plan` are those of `lib_dirs_for` for the payload), `host_env(&names, ..)`; `Volume { guest, host }`, `Maps` (`of_root(rootfs)`, `of_binds(rootfs, &binds)` (the table of the `VolumeBind`s of the namespace rung, with no rule of the interposer), `new(&names, rootfs, &volumes)` with the refusals of more than `MAX_MAP_PAIRS - 1` volumes (by the name of `names.maps`), of a guest side that is not absolute, is `/`, is not a normal path, repeats one, or is a reserved path of `names` or is under one (by name), and of a host side that is not absolute; `rootfs`, `volumes`, `volume_at`), `userland_maps_table(&names, &maps)` (the root pair, then each volume pair), `userland_volume_table(&names, &maps)` (the volume pairs only), `MAX_MAP_SIDE` (4096 bytes), `MAX_MAP_PAIRS` (32): each side that the variable cannot spell is refused with its own name (the rootfs path, the guest side or the host side of the volume) |
| `binfmt` | `support_for`, `Support`, `copy_in_target`, `copy_interpreter_in`, and the reader of `pod_hostprobe::binfmt` |
| `abi` | `Elf` (`read`, `parse`, `flavour`), `Flavour`, `Versioned`, `Unreadable`, `Verdict`, `admits`, `admits_set`, `interp_of` (head-safe), `interp_of_file`, `HEAD_BYTES`, `READ_CEILING`, `PHDR_TABLE_MAX`, `libc_beside`, `resolve_in`, `resolve_dir_in` (each takes a `GuestRoot`), `Search` (`read`, `parse`: `DT_NEEDED`, `DT_RUNPATH`, `DT_RPATH` and the machine of a file, with no read of its symbols; `dirs`, the list that the loader searches), `GuestRoot` (`root`, `host_of`, `above_mount`, `guest_of` (a host path back to its guest path); for `Path`, `PathBuf`, `str`, `String` and `userland::Maps`), `ResolveKind`, `link_set`, `LinkSet`, `Member`, `guest_lib_dirs` |

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
| `Plan { .. }` | `Plan { .., names: podvm_enter::NAMES, limits: Vec::new(), no_new_privs: false, die_with_parent: false, confine: None, hostname: None, volumes: Vec::new(), userns_ids: None }` (`--ulimit` and `--security-opt no-new-privileges` fill the limits; the launcher of a detached run sets `die_with_parent`; podvm's record of a detached run keeps them in its own fields) |
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
| the image's `USER` or `--user` of `run -d` and `create` on the namespace rung (PodUtils/podvm#447) | `Plan::userns_ids: Some((uid, gid))` with the ids that podvm resolves in the tree; `--user 0` is `None`. `Plan::env_for` adds `HOME=/root` where the image and the caller set no `HOME`: give the `HOME` of the user (docker reads the passwd of the image) in the caller's variables. Each file of the tree of the caller then shows as owned by the user |
| the image's `USER` or `--user` of a foreground `run`, where podvm is uid 0 of a user namespace of its own (PodUtils/podvm#452) | the same `Plan::userns_ids: Some((uid, gid))`: the payload gets the ids through a nested user namespace (`<uid> 0 1`), on the namespace rung and the chroot rung, with no capability. Nothing more to give |

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
`tests/userns.rs` runs the namespace rung as a caller with no
capabilities (as root, the test starts again as uid 65534): the payload
sees uid 0, the host name of the plan, a `/tmp` that is not the image's,
the word `namespace`, and one banner; with the `/dev` that `pod-rootfs`
leaves there, the payload reads 2000000 bytes of `/dev/urandom` and of
`/dev/random`, `/dev/null` and `/dev/tty` are character devices, a write
to `/dev/full` fails with `ENOSPC`, the bind of `/dev/urandom` is
`nosuid` and `noexec` and not `nodev`, and a link at `/dev/zero` stays a
link with one line. With a `/dev/pts` directory and no `/dev/ptmx` in the
image (`tests/userns/pts.rs`), `/dev/ptmx` is a character device whose
bind is `nosuid` and `noexec`, `/dev/pts` is not on the device of the
host's, its `ptmx` has mode 0666, an open of `/dev/ptmx` gives
`/dev/pts/0` as the one pty, and no line of a refusal goes out. Its test of the volumes (`tests/userns/volumes.rs`)
runs as root and as the caller with no capabilities, each over a tmpfs
with `nosuid`, `nodev`, `noexec` and `noatime` that holds the host sides
(`pod_testkit::nobody::again_over_a_flagged_tmpfs`): the payload reads and
writes a volume (the host sees the write), a write into a read-only
volume fails with `EROFS` and its mount holds `ro` and the four flags, a
volume on a path that the image does not hold and a file volume work (the
image keeps an empty mount point), a guest path through a link of the
image is refused with its step, and a host path that does not exist is
refused before the banner. A confined payload with volumes (T-739) reads
a volume that the image does not hold under a read rule, writes a
directory of the host side alone under a write rule, reads a volume under
a rule on a directory that the child makes above it, and gets `EACCES` in
a volume that no rule names and for a write outside the rules; a rule on
a path in no volume and not in the image is refused by name before the
banner. On an overlay root with an upper directory on that tmpfs (T-750,
`tests/userns/upper.rs`), `-w` and a write rule on a directory that only
the upper directory holds pass (the write lands in it, and a write
outside the rules fails), a working directory and a rule on a path that
a whiteout of the upper directory removes are refused by name before the
banner, a volume over a directory of the upper directory wins for `-w`
and a rule, and a joined exec takes `-w` of the upper directory. Its test of the ids
inside (T-786, `tests/userns/ids.rs`): with `Plan::userns_ids`
1001:1001 the payload sees `id -u` and `id -g` 1001, the maps
`1001 <caller> 1`, a file of the caller as owned by 1001:1001, a file
that it makes in `/tmp` as owned by 1001, the host name of the plan,
and `CapEff` and `CapPrm` 0; with no ids it sees 0 and its capabilities;
an exec that joins the run with the ids is 1001 with no capability. Its
test of the nested namespace (T-787, `tests/userns/nested.rs`) starts
again as uid 0 of a user namespace of its own (`0 <caller> 1`, over uid
65534 as root; `pod_testkit::nobody::again_as_root_of_a_user_namespace`):
with the ids 1001:1001 the payload sees `id -u` and `id -g` 1001, the maps
`1001 0 1`, a file of the caller as owned by 1001:1001, a file of `/tmp`
as 1001, the host name, and `CapEff` and `CapPrm` 0; with no ids it is 0
with its capabilities; the chroot rung gives 1001; an exec that joins the
run is 1001 with no capability; with `max_user_namespaces` 0 the entry
names `unshare(CLONE_NEWUSER)` and `ENOSPC` and does not fall back. As a
host root before that, the payload with the ids stays 0.
`scripts/ci/userns-tests.sh` runs it
in CI and fails on a skip. The plants `enter-userns-not-tried`,
`enter-userns-no-maps`, `enter-userns-banner-twice`,
`enter-userns-dev-bind-over-link`, `enter-userns-dev-no-remount`,
`enter-userns-no-own-devpts`,
`enter-volume-skipped`, `enter-volume-ro-writable`,
`enter-volume-flags-dropped`, `enter-volume-through-a-link`,
`enter-confine-volume-in-the-image`,
`enter-confine-volume-missing-accepted`,
`enter-confine-volume-descriptors-kept`, `enter-upper-workdir-in-the-lower`,
`enter-upper-rule-in-the-lower`, `enter-upper-hidden-read`,
`enter-upper-volume-under-the-layers`, `enter-upper-join-in-the-lower`,
`enter-upper-whiteout-read`, `enter-upper-opaque-read`,
`enter-overlay-no-userxattr`, `enter-view-reads-both-opaque`,
`enter-userns-ids-ignored` and `enter-userns-nested-ids-ignored` prove it.
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
(`scripts/plants/`) prove the checks. `abi::tests::labels` checks that no
reason of `admits`, `admits_set` and `Unreadable` holds a label of a
printer, and that a static object is refused as static; the plants
`enter-abi-reason-carries-a-label` and
`enter-abi-static-object-has-no-reason` prove it.
`limits::tests` checks that a plan with two limits of one resource
(`nofile=64`, then `nofile=128`) is refused before the fork with
`Error::LimitTwice(Resource::Nofile)`, and that the payload does not run;
the plant `enter-limit-twice-reaches-the-child` proves it.
`confine::tests` (PodUtils/podvm#344, on the entry with no chroot with
host paths): `check_exec` refuses a static ELF, a dynamic ELF and a script
outside the execute paths and takes them inside, also through a link;
the kernel runs the three inside and refuses them outside, and the loader,
the interpreter and a memfd run them outside (the gap that the check
closes); the payload gets the exec table, and a value of the caller never
reaches it. The plants `enter-check-exec-allows-each`,
`enter-exec-table-not-set` and `enter-exec-table-of-the-caller` prove
them.
