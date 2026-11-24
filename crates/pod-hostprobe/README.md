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
| `verdict` | `Verdict` (`Ok`, `Denied`, `Skip`), `Outcome` (`value(key)`: an item of the reason of a row of a fact), `items` (the reason of a row of a fact: `KEY VALUE` items with `; ` between them), the wire line `pod-hostprobe/1` (`encode`, `decode`), `Outcome::row` (the row format of the Go reference instrument) |
| `child` | `Spawner`: `clone_accepted`, `run_probe` |
| `probes` | `PROBES` (90 rows: census, attribution, machine, supervise), `Kind` (`Clone`, `Child`, and `Here`: the body in the probe process, for `readlink(/proc/self/exe)` alone), `Probe::measure` (runs a row as its kind says), `Group`, `find`, `USERNS_LEG` (the namespace rung through a user namespace: the sequence of `userns::enter`, then `unshare(CLONE_NEWNS)`, `/` private and a tmpfs on `/tmp`; the denial names the refused call), `MACHINE_LEGS`, `SUPERVISE_LEGS`, `SUPERVISION_LEGS`, `chroot_usable`, `ptmx_usable`, `fuse_usable`, `PTMX`, `FUSE` |
| `select` | `Rung` (`word`, `requirement`, `must_never_claim`), `Strictness`, `Rejection`, `Selection` (`choose`, `meets`, `exit_code`, `STRICT_FLOOR`), `Provides`, `control_rows`. The namespace rung needs a mount namespace and a mount in the user namespace of the caller, or the leg `USERNS_LEG`; `Provides::namespaces` is `mount-only`, or `user+mount` where only the leg passes |
| `userns` | `Maps` (`new(uid, gid)` and `of_caller` map to 0; `with_inside((uid, gid), uid, gid)` and `of_caller_inside((uid, gid))` map to the ids inside; `uid_line`, `gid_line`), `map_line(inside, id)` (`<inside> <id> 1`), `enter(&Maps)`, `Step` (`name`), `is_initial_map(text)` (the `uid_map` of the initial user namespace, one line `0 0 4294967295`): the user namespace of a caller with no capabilities, as `unshare -Ur` makes it, async-signal-safe after the fork; the namespace rung of `pod-enter` and the leg call the one sequence; the probe legs map to 0, the rung to `Plan::userns_ids` of `pod-enter` |
| `report` (feature `report`) | `write(&Findings, &Selection) -> String`, `read(&str) -> Result<(Findings, Selection), ReadError>`, `SCHEMA`, `ReadError` (`Form`, `Version`, `UnknownRow`, `DuplicateRow`, `Word`, `DenialWithoutErrno`); `schema() -> serde_json::Value` and `SCHEMA_ID` (feature `schema`): the JSON object of one run and its JSON Schema (see "The report") |
| `mounts` | `Mounts`: create, configure, attach, use; `summary`, `capability_note` |
| `machine` | `assess`, `Assessment`, `Profile` (`Full`, `Tcg`), `Leg`, the leg names, `accelerators_in`. The KVM leg opens `/dev/kvm`, then asks the device for a guest of the build's architecture (`KVM_GET_API_VERSION`, `KVM_CREATE_VM`, then `KVM_GET_SUPPORTED_CPUID` on x86_64 or `KVM_ARM_PREFERRED_TARGET` on aarch64): a device that QEMU user mode passes through from another architecture is a denial with `ENOTTY` or `EINVAL` and the reason (PodUtils/podvm#407). `Assessment::refusal` is one sentence with its verdict (`machine tier refused: LEG; LEG`): a printer adds no verdict word before it |
| `supervise` | `assess` (the mediation legs), `assess_supervision` (the lifecycle legs), `Assessment`, `Leg`. `Assessment::refusal` has the same shape (`supervise tier refused: LEG; LEG`) |
| `identity` | `Identity`, `read`, `is_single_id_map`, `ConfinementKey`, `confinement_key(&Identity, &Interpreter)` |
| `interp` | `detect(is_set) -> Interpreter`, `QEMU_VARS`, `qemu_comm`: is an emulator in the way (a variable of qemu-user, the name `qemu-*` of the process, or a `binfmt_misc` registration of this machine) |
| `binfmt` | `registrations`, `Registration`, `machine_for`, `SELF_MACHINE`, `BINFMT_DIR` |
| `writable` | `candidates(named)`, `probe(candidates)`, `WriteProbe`, `FIXED`, `VARIABLES` |
| `cgroup` | `probe() -> Leg`, `assess(magic, membership, root, writable)`, `Leg` (`own`, `verdict`), `Delegation` (`Delegated { path, controllers, enabled }`, `None(Reason)`; `words`, `is_delegated`), `Reason` (`NoMount`, `NotCgroup2`, `NoMembership`, `NotWritable`, `MissingControllers`), `ROOT`, `WANTED`, `CGROUP2_SUPER_MAGIC`: the nearest directory at or above the cgroup of the process that the user can write (a `mkdir` and `rmdir` of a child, an open of `cgroup.subtree_control` for write) with the controllers `memory`, `cpu` and `pids`, or the reason that there is none. It writes no limit (PodUtils/podvm#390) |
| `usage` | `Meter` (`open(v2, v1_memory, v1_cpuacct)`, `v2(dir) -> Result<Meter, Failure>`, `v1_memory(dir)`, `v1_cpu(dir)`, `note`, `version`, `read() -> Usage`), `Usage` (`cpu_usec`, `peak_bytes`, `oom_kills`: each an `Option`; `why`: the reason of each `None`), `Version` (`V2`, `V1`), `read(v2, v1_memory, v1_cpuacct)`, the parsers `cpu_usec_v2`, `cpu_usec_v1` (nanoseconds, rounded down to microseconds), `bytes`, `oom_kills_in`, and `oom_kills_of(dir)`: the CPU time, the peak of memory and the OOM kills of a cgroup at the end of a run. The files stay open and each read starts at offset 0. cgroup2 serves only where its memory controller is there (`memory.events`), else cgroup v1: on a host with both versions, a cgroup2 with no controllers gives the `cpu.stat` of the host (PodUtils/podvm#650) |
| `space` | `read`, `require(&Settings, ...)`, `choose`, `file_ceiling`, `dir_bytes`, `mib`: blocks and inodes before a write |
| `exec` | `probe_dir_exec(&Settings, dir)`, `try_exec`: can a file created under a directory execute. On `EACCES` the detail reads the `noexec` flag of the mount (`statfs`): only a `noexec` mount gets the move off the filesystem; a mount that is not `noexec` names a policy on the path or on the file (an LSM, a seccomp filter, a sandbox); a flag that cannot be read names both |
| `sys` | the raw system calls: `Errno`, the `SYS_*` numbers, the constants, `CBuf` (`new` of a text, `from_bytes` of a name that need not be UTF-8), the wrappers, `clone_fork`, `shed_after_fork`, `close_in_children`; `pidfd_send_signal` (a signal that a reaped process answers with `ESRCH`, never a process that got its pid); `sigblock(set, &mut old)` (block any set in the calling thread; `sigrestore` gives the earlier mask back), `signalfd(set)`, `sigbit(sig)`, `SIGHUP`, `SIGQUIT`; `PROXIED_SIGNALS`, the one list of the signals that each relay of a payload blocks and gives to it (SIGHUP, SIGINT, SIGQUIT, SIGUSR1, SIGUSR2, SIGTERM, SIGALRM and SIGPWR, as docker's `--sig-proxy` with no tty), `proxied_forwards(signo, code)` (its rule: SIGINT and SIGQUIT only from a process, `ssi_code` 0 or less), `sigblock_proxied(&mut old)` (the block of the list for each thread of a relay) and `signalfd_proxied()`; each signal call takes signals 1 to 64 as a `u64` and gives the kernel its `kernel_sigset_t` word by word (`to_kernel`, `from_kernel`, `SIGSET_BYTES`: 16 bytes on mips), and `SIG_BLOCK`, `SIG_SETMASK`, `SIGCHLD`, `SIGINT`, `SIGTERM` and `SFD_CLOEXEC` come from `linux_raw_sys` for the architecture (mips and sparc differ); the errno numbers (`EPERM` to `ELOOP`, `Errno::name`) and the open flags (`O_RDONLY` to `O_NONBLOCK`) come from `linux_raw_sys::errno` and `linux_raw_sys::general` for the architecture (`sys/numbers.rs`; a constant assertion checks the values of mips and sparc); `mount_data` is `mount(2)` with the data of the file system; `fgetxattr` and `fsetxattr` read and set an extended attribute through a descriptor, and `lsetxattr` sets one by a path with no follow of its last component (a symlink); `statfs` gives `Statfs { f_bsize, f_blocks, f_bavail, f_bfree, f_files, f_ffree, f_flags, f_type }` (`f_bfree` holds the blocks that the file system keeps for root, `f_bavail` does not) from the `struct statfs64` of the kernel on each target (`statfs64(2)` on a 32-bit target), with `Statfs::flag` and the mount flags `ST_RDONLY`, `ST_NOSUID`, `ST_NODEV`, `ST_NOEXEC`, `ST_SYNCHRONOUS`, `ST_VALID`, `ST_MANDLOCK`, `ST_NOATIME`, `ST_NODIRATIME`, `ST_RELATIME`, and the magics `OVERLAYFS_SUPER_MAGIC` and `PROC_SUPER_MAGIC` of `f_type`. Landlock (`sys/landlock.rs`): `landlock_abi`, `landlock_create_ruleset`, `landlock_add_path_rule`, `landlock_add_port_rule`, `landlock_restrict_self`, the rights `LANDLOCK_ACCESS_*` and the structs of `linux_raw_sys::landlock`; `connect`, `sockaddr_in`. seccomp (`sys/seccomp.rs`): the one table of the audit arch of the build target (`AUDIT_ARCH_NATIVE`, `X32_SYSCALL_BIT`; `pod-hold` and `pod-confine` read it), `SockFilter`, `SockFprog`, the BPF opcodes, `bpf_stmt`, `bpf_jump`, the offsets of `seccomp_data` (`seccomp_arg_low` follows the byte order), `CLONE_FLAGS_ARG`, `seccomp_set_filter`. Each wrapper is one system call on a struct of the caller, so a child between its fork and its exec can call it |

A failure of `space` and `exec` is a `pod_core::Failure` at
`Stage::Host`: `Class::Usage` for a path with a NUL, `Class::Unavailable`
for no space or an unknown free space, the class of the I/O error
otherwise.

The crate reads no variable of the environment for its own work. The
caller gives the named directories of the write probe
(`writable::candidates`), and a function that tells which variables are
set (`interp::detect`). A probe child gets the environment of its parent
as it is. Three rows measure a variable, as a program of the host reads
it: `creat($HOME/.pod-hostprobe)` reads `HOME`, `getenv(HTTPS_PROXY)`
reads `https_proxy`, `HTTPS_PROXY`, `all_proxy` and `ALL_PROXY` (the order
of curl), and `getenv(SSL_CERT_FILE)+read` reads `SSL_CERT_FILE`.

## The rows of the host classes

The rows of the classes of podssh and podnet (T-714, T-715, T-745) are
census rows, each in its own probe child. None sends a packet past the
loopback. A row of a fact gives `ok` with items `KEY VALUE` in its reason
(`Outcome::value` reads one). A reason holds no path of the user, no host
of a proxy and no credential.

| Row | What it reads | The items or the denial |
| --- | --- | --- |
| `bind(127.0.0.1:0)` | a TCP bind alone | the errno of `bind` |
| `listen after bind(127.0.0.1:0)` | `listen(2)` after the bind | the errno of `listen`; a skip where the bind fails |
| `socket(AF_INET, SOCK_DGRAM)+connect`, `socket(AF_INET6, SOCK_DGRAM)+connect` | a UDP socket and its connect to the loopback (no packet) | the errno of the step that failed (`EAFNOSUPPORT` with no IPv6) |
| `socket(AF_INET, SOCK_RAW)`, `socket(AF_PACKET, SOCK_RAW)` | a raw ICMP socket and a packet socket of protocol 0 | the errno |
| `connect(127.0.0.1:1)` | a TCP connect that waits at most 2 s | `ok` with `ECONNREFUSED` in the reason (the loopback answers); `ENETUNREACH` or `EACCES`; a skip with no answer |
| `stat(/dev/pts)` | the path | the kind and the mode; `ENOENT` |
| `readlink(/proc/self/exe)` | in the probe process (`Kind::Here`): a host with no `/proc` starts no child | `ENOENT` |
| `tty_nr of /proc/self/stat` | the controlling terminal of the probe | `tty_nr N` |
| `open(/dev/tty, O_NONBLOCK)+poll` | an open with no wait and a poll with no wait: no byte is read | `read waits` or `read ready`; `ENXIO` with no controlling terminal |
| `line of getuid() in /etc/passwd`, `line of getgid() in /etc/group` | the lines whose third field is the id | `uid N; lines K` (`file absent` where there is no file) |
| `creat($HOME/.pod-hostprobe)` | `statfs` of `HOME`, then a new file in it, removed | `noexec 1` or `0`; the errno of the file; a skip with no `HOME` |
| `statfs(/etc/ssl)` | the mount flags | `rdonly 1; noexec 0` |
| `getenv(HTTPS_PROXY)` | the first variable of a proxy that is set | `set 0`, or `set 1; variable V; scheme S; credential 0; port P` |
| `getenv(SSL_CERT_FILE)+read` | the certificate file | `set 0`, or `set 1; certificates N`; the errno of the read |
| `read(/etc/resolv.conf)` | the `nameserver` lines; with none, a TCP connect to `127.0.0.1:53` | `nameservers N`, with `127.0.0.1:53 refused`, `answers`, `silent` or the errno |
| `read(/proc/net/route)`, `read(/proc/net/ipv6_route)` | a default route that is up (no reject route) in the main table | `default 1` or `0`; `ipv6 absent` where the kernel has no IPv6 |


### The rows that send a packet out

The rows of `probes::NETWORK_ROWS` (`Kind::Network`) send a packet past
the host. With no `Settings::network` (the default), each is a skip that
names the setting, and no child runs. The spawner gives the targets
(`Network`: a name and its port, another name, a closed port, an IP
literal, a time limit) to the child in `POD_HOSTPROBE_NETWORK`, and drops
that variable of the caller. A reason holds no name of a target, no host
of a proxy, no credential and no byte of a body.

| Row | What it does | The items |
| --- | --- | --- |
| `CONNECT(HTTPS_PROXY, name)`, `... with no credential`, `CONNECT(HTTPS_PROXY, other)`, `CONNECT(HTTPS_PROXY, literal)` | a `CONNECT` through the proxy of the host; the head of the answer alone | `status N`, or `status none; end WORD` |
| `CONNECT(HTTPS_PROXY, name:closed port)` | the same to the closed port; after a 200, an SSH banner and one read of at most 1 s | `status 200; bytes 0` or `1` |
| `TLS(name)`, `TLS(other)`, `TLS(literal) with no name` | a ClientHello of TLS 1.2, through the proxy where a variable names one, else direct; the certificates of the server, with no check of a signature | `path proxy` or `direct`; `hello 1; certs N; issuer file; ca K` (the certificate of `SSL_CERT_FILE` that issued the chain), `issuer other` or `issuer none`; else `hello 0; end WORD` |
| `getaddrinfo(name)` | a lookup by the C library, in a thread with the time limit | `resolved 1; addresses N`, `resolved 0`, or `resolved 0; end silent` |

## The report

The feature `report` (serde and serde_json) gives the module `report`.
`report::write` gives one line of JSON with no newline at the end: an
object of the findings and the selection of one run. `report::read` gives
them back, and `write`, then `read`, then `write` gives the same text.
`read` accepts the keys in each order and each white space. A caller that
writes a document of its own puts the object under one key and reads that
key back with `read`, so the caller keeps one writer.

The object holds what the run measured. It holds no constant of the crate
and no value that another field gives: the name of an errno
(`Errno::name`), the group of a row (`probes::find`), the requirement of a
rejected rung (`Rung::requirement`), what a rung must never claim.

| Field | Type | What |
| --- | --- | --- |
| `schema` | number | the version of the object, `report::SCHEMA` (1); the first key |
| `findings.rows[]` | object | one for each row of `Findings::rows`, in its order |
| `findings.rows[].name` | string | the name of a row of `probes::PROBES` |
| `findings.rows[].verdict` | string | `ok`, `denied` or `skip` (`Verdict::word`) |
| `findings.rows[].errno` | number or null | the errno; never null for `denied` |
| `findings.rows[].reason` | string | the reason; empty for none |
| `findings.identity` | object | `Identity`: `uid`, `gid` (numbers), `groups` (numbers), `uid_map`, `gid_map`, `setgroups`, `cap_eff`, `cap_prm`, `cap_bnd`, `seccomp`, `seccomp_filters`, `no_new_privs`, `threads` (string or null), `unreadable` (strings) |
| `findings.writable[]` | object | one for each write probe: `path`, `source`, `verdict`, `errno`, `reason` (as a row), `space` (null, or `blocks_free`, `block_size`, `inodes_free`), `space_errno` (number or null) |
| `findings.self_exe` | string | the path from which the probe children started |
| `selection.rung` | string | the rung (`Rung::word`) |
| `selection.rejected[]` | object | one for each rejected rung: `rung`, `evidence` |
| `selection.controls_answered` | boolean | each control answered |
| `selection.control_notes` | strings | the notes of the controls |

`read` refuses, with a `ReadError`: a text that is not a JSON object, or
an object with a missing or an unknown field (`Form`); a `schema` that is
absent, not a number or not `SCHEMA` (`Version`, before the form); a row
name that `probes::PROBES` does not name (`UnknownRow`) or that comes two
times (`DuplicateRow`); a verdict or a rung that is not a word of the
crate (`Word`); a `denied` with no errno (`DenialWithoutErrno`). Each row
name of the result is the `&'static str` of its entry of `PROBES`. A
change of the form of the object changes `SCHEMA`.

The JSON Schema (2020-12) of the object is
`schemas/hostprobe-report.v1.schema.json` of the repository, released
with each tag. The feature `schema` (the feature `report` and schemars)
gives `report::schema()`, the same schema as a `serde_json::Value`, and
`report::SCHEMA_ID`, its `$id`: a consumer validates a report against it
with no copy of the file. The schema comes from the wire types: `schema`
is the constant `SCHEMA`, a verdict, a rung and the name of a row are an
`enum` of the words of the crate, a `denied` row or write probe needs an
integer `errno`, and an object has no other field. Only a row name that
comes two times passes the schema and fails `read`. A test holds the file
to the types: after a change of a wire type, write it again with
`POD_HOSTPROBE_WRITE_SCHEMAS=1 cargo test -p pod-hostprobe --all-features
schema`. `tests/examples/report/` has three valid reports (`valid-host-classes.json` holds each row of the host classes) and ten invalid
ones; `tests/examples/expect.json` gives the reason of each, and each
example gets the same verdict from `read` and from a validator of JSON
Schema (boon).

## Parity

Against podvm `crates/podvm-probe`, `crates/podvm-image/src/space.rs` and
`crates/podvm-image/src/exec_probe.rs` at `2c03085`:

| Capability of podvm | Here |
| --- | --- |
| [x] One disposable child for each probe, the line and the exit status cross-checked | `child` |
| [x] The three states, the wire line | `verdict` (magic `pod-hostprobe/1`) |
| [x] The probe table and the bodies (66 rows; 67 with the leg `USERNS_LEG` of T-670) | `probes`, with the rows of T-744, T-714, T-715, T-745 and T-866 (90 rows) |
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
| [x] `exit`, `json`, `report`, `nongoals` | stay in podvm: its CLI and its report; podvm's report embeds the object of `report` (feature `report`) |
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
| the findings and the selection in podvm's `report::document`, read back by the probe cache | `pod_hostprobe::report::write` under one key of the document, and `report::read` of that key (feature `report`) |
| `podvm::incus::usage::{cpu_usec_v2, cpu_usec_v1, bytes}` | `pod_hostprobe::usage::{cpu_usec_v2, cpu_usec_v1, bytes}` |
| `podvm::incus::usage::Meter` (`open`, `read`) and the cgroup part of `Measure` | `pod_hostprobe::usage::{Meter, Usage}`: podvm keeps its mount at `/.podvm/.meter`; in the mount of `cgroup2` it calls `Meter::v2(MOUNT)`, on a failure `Meter::default()` with `note(failure.chain())`, then `v1_memory(MOUNT)` and `v1_cpu(MOUNT)` in the mounts of `memory` and `cpuacct`. `read()` gives `Usage` (`cpu_usec`, `peak_bytes`, `oom_kills`, `why`); `Measure`, its JSON, `seal`, `Anchor` and `end_tree` stay in podvm. The sign of the memory controller is `memory.events`, not `memory.peak`: a kernel 5.8 to 5.18 stays on cgroup2 with a peak of `None` and its reason |
| `podvm_supervise::report::{oom_kills_in, oom_kills_of}` | `pod_hostprobe::usage::{oom_kills_in, oom_kills_of}` |
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
private mount namespace where it can get one. No test under libtest runs
the probes: a child of a libtest binary refuses the child flag. With the
feature `report`, `tests/children.rs` also writes the report of this host,
reads it back, and compares the rows, the rung and the text; the plant
`hostprobe-t625-rung-not-read-back` proves it.

The unit tests of `usage` hold the parsers in tables (a count, a missing
line, a value that is not a count, the nanoseconds of `cpuacct.usage`
rounded down to microseconds) and read made-up cgroup trees in the
temporary directory:
`usage::tests::cgroup2_with_no_memory_controller_gives_way_to_v1` (a
host with both versions), `cgroup2_with_its_memory_controller_serves`,
`cgroup2_with_no_memory_peak_stays_on_cgroup2` (Linux 5.8 to 5.18),
`each_missing_value_gives_its_reason`, and
`a_meter_reads_its_open_files_again_from_the_start` (a read after the
remove of the directory).

`exec::tests::the_flag_of_the_mount_decides_the_remedy` checks the
sentence of each value of the `noexec` flag, and the plant
`hostprobe-exec-always-names-the-filesystem` proves it.
`the_noexec_flag_agrees_with_the_mount_table` checks the flag against
`/proc/self/mounts`, and `a_noexec_mount_is_named_by_the_probe` (root)
probes a `noexec` tmpfs.
`tests/host_rows.rs` (no libtest) runs each row of the host classes
against what the test reads itself (the terminal, the lines of the ids,
the mount of `HOME`, the variables of a proxy and of the certificate file,
the resolver), and as root again as uid 65534, where a raw socket gives
`EPERM` and a file in a `HOME` of root gives `EACCES`;
`scripts/ci/userns-tests.sh` runs it too. The unit tests of
`probes/files.rs` and `probes/network/tests.rs` hold the parsers (the
field `tty_nr`, the lines of an id, the items of a proxy URL with no host
and no credential, the `nameserver` lines, the default routes). The plants
`hostprobe-listen-row-measures-the-bind`, `hostprobe-connect-reset-denied`,
`hostprobe-tty-nr-field-shifted`, `hostprobe-proxy-host-in-reason`,
`hostprobe-route-reject-default` and `hostprobe-proc-row-in-a-child`
prove them.
`machine::tests::the_legs_of_a_refusal_are_separated_where_a_reason_ends`
and `supervise::tests::the_refusal_names_every_missing_leg_in_run_order`
check that `; ` separates the legs of a refusal and that the verdict comes
one time; the plants `hostprobe-machine-legs-run-together` and
`hostprobe-supervise-legs-run-together` prove them.
`select::tests::the_leg_through_a_user_namespace_selects_namespace`
checks the selection through the leg, and the plant
`hostprobe-userns-leg-not-selected` proves it. `tests/userns_leg.rs` (no
libtest) runs the leg as a caller with no capabilities (as root, the
binary starts again as uid 65534); `scripts/ci/userns-tests.sh` runs it in
CI and fails on a skip, and the plant `hostprobe-userns-leg-no-user`
proves it.
`sys::tests::the_kernel_mask_has_the_kernels_size_and_keeps_signals_1_to_64`
checks the size of the mask against `_NSIG`, the order of signals 1 to
64 through the words of the kernel, and a `signalfd` with the mask; the
plants `hostprobe-t618-mask-not-the-kernels-size` and
`hostprobe-t618-mask-words-out-of-order` prove it.
`sys::tests::the_creation_flags_and_the_errno_numbers_are_the_kernels`
makes the kernel refuse a create with `O_CREAT | O_EXCL`, empty a file
with `O_TRUNC`, and refuse a name of 300 bytes, and compares each errno
with the constant; the plants `hostprobe-t619-o-excl-of-another-target`
and `hostprobe-t619-errno-of-another-target` prove it.

## Sources

`SOURCES` lists each imported file of podvm with its blob, and the
`check-sources.py` check compares them with podvm's `main`. The files of
podvm that stayed (`exit.rs`, `json.rs`, `report.rs`, `report/`,
`nongoals.rs`) are `skip` lines. The second block of podvm names the files of the
reader of the usage of a cgroup (`usage`), of which a part came.
