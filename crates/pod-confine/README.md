# pod-confine

The confinement of a payload with no privilege on Linux: Landlock rules
for the paths and the TCP ports, the Landlock scopes (signals and abstract
Unix sockets), a seccomp filter that denies a list of system calls (and
UDP and the sockets that Landlock does not check where the spec closes a
socket), and `no_new_privs`, from one `Spec`. New
code for the ask of podvm (PodUtils/podvm#259, A-V-01 of GitHub #35).
Linux only: on another target the crate is empty.

The work has three steps, so that the child between its fork and its exec
only issues system calls on data that exists:

1. `check()` (or `probe()`) before the fork: the Landlock ABI of the kernel
   (`landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)`),
   and an `apply` of a minimal ruleset and filter in a disposable child,
   so that a nested ruleset under the policy of the host is proven. The
   caller is not restricted by it.
2. `Spec::compile(abi)` (host paths) or `Spec::compile_in(abi, root)`
   (guest paths, resolved under a root descriptor with `openat2` and
   `RESOLVE_IN_ROOT`, as after a `chroot` there) before the fork: each
   path opens as `O_PATH | O_CLOEXEC`, the BPF program is built. A path
   that does not exist is a refusal by name. Where the child mounts on a
   path after the compile or changes its root (a mount namespace, a
   `chroot`), `Compiled::close_paths()` closes the descriptors after the
   checks, and the child calls `Compiled::open_paths()` after its mounts
   and its root change: each path opens again in the view of the payload,
   with no allocation. A failed open is a `Refusal` of `open(O_PATH)` with
   the index of its rule.
3. `apply(&compiled)` in the child: `PR_SET_NO_NEW_PRIVS`, the drop of
   each capability that the spec does not keep (`capget`,
   `PR_CAPBSET_DROP`, `capset`, `PR_CAP_AMBIENT_CLEAR_ALL`),
   `landlock_create_ruleset` (with the scopes), `landlock_add_rule` for
   each path and port,
   `landlock_restrict_self`, then `seccomp(SECCOMP_SET_MODE_FILTER)`
   (mode 2). No allocation, no lock. A `Refusal` names the call, the rule
   and the errno.

There is no unconfined way through: a compile refuses a kernel with no
Landlock and an ABI under 3 (ABI 1 and 2 do not confine `truncate(2)`),
and `apply` refuses each failed call, so the caller does not exec. A rule
holds the inode of its path: a rule on a directory covers the tree under
it, also the mounts in that tree. It does not cover a mount on the
directory itself. The walk of Landlock goes from the root of a mount to
the parent of its mount point, and does not read a rule on the mount
point, which the mount hides. Measured on Linux 6.18 (ABI 7): a rule on a
directory, opened before a tmpfs mount on it, does not cover the tmpfs
(`EACCES`); a rule on the root of the tmpfs, or on the parent of the
directory, does; a rule on the target of a bind mount does not cover the
bind, and a rule on its source does. Thus a rule must open in the view of
the payload, after each mount on its path: `close_paths` and
`open_paths`.

The crate reads no environment variable, prints nothing and keeps no
global state. A failure of a compile or of the probe is a
`pod_core::Failure` at the stage `host`, with the class of its errno
(`ENOENT` `NotFound`, `EPERM`/`EACCES` `Refused`, `ENOSYS`/`EOPNOTSUPP`
and no Landlock `Unsupported`, a bad rule `Usage`).

Needs: Linux 6.2 (Landlock ABI 3) for the file rules, 6.7 (ABI 4) for the
TCP rules, 6.12 (ABI 6) for the scopes; Landlock in the list of LSMs of
the host.

## API

| Item | What |
| --- | --- |
| `Spec { paths, connect, bind, scope, udp, deny, keep_caps }` | `paths`: `PathRule { path, access }`; `connect`, `bind`: `None` leaves the TCP call free, `Some(ports)` allows those ports only; `scope`: the Landlock scopes; `udp`: `false` closes UDP; `deny`: the calls that fail. `Spec::new()` (no path, TCP and UDP free, the scopes on, `Deny::defaults()`), `allow(path, access)`, `connect_to(port)`, `bind_to(port)`, `close_udp()`, `no_scope()`, `no_caps()`, `keep_cap(n)`; `keep_caps`: the mask of the capabilities that the payload keeps (`DOCKER_CAPS`, docker's default set, in `Spec::new()`; `no_caps()` keeps none); `deny_rules()` (the rules of the filter: `deny`, then the rules of the switches) |
| `Access` | `READ` (read a file, list a directory), `WRITE` (write, truncate, make, remove, rename, link; an `ioctl` on a device), `EXECUTE`, `NONE`, and their union (`READ \| EXECUTE`); `rights()` gives the Landlock rights |
| `Deny` | `{ name, nr, errno, when }`: `Deny::call(name, SYS_*, errno)`; each condition of `when` is an `Arg` on the low 32 bits of one argument, and all must hold: `when_flags(index, mask)` (a bit of the mask is set), `when_equal(index, mask, value)` (the argument masked is the value), `when_not_equal(index, mask, value)`; `Deny::defaults()`, `Deny::io_uring()`, `Deny::udp()`, `Deny::mptcp()`, `Deny::families()` |
| `deny::program(&[Deny])` | the BPF program: the audit arch (a foreign ABI and x32 get `ENOSYS`), one block for each rule, `ALLOW`. The first rule of a number that holds gives its errno: a rule with conditions that do not hold gives the call to the next rule of its number, and a rule after a rule of its number with no condition is left out. A rule with no condition takes two instructions, one with one flag condition five. `NAMESPACE_FLAGS`, `CLONE_NEWTIME` |
| `Spec::compile(abi)`, `Spec::compile_in(abi, root_fd)` | a `Compiled` (`abi`, `handled_fs`, `handled_net`, `scoped`, `rules` with `fd`, `allowed`, `path`, `ports`, `filter`; it closes its descriptors when it drops); `keep_clear_of(&numbers)` moves each rule off a number that the child takes for a `dup2`; `close_paths()` closes the descriptor of each rule (`fd` is then -1) and keeps its path; `open_paths()` opens the path of each rule with no descriptor in the view of the caller (in the child, after its mounts and its root change), with no allocation, or gives `Err(Refusal)` of `OpenPath` |
| `apply(&Compiled)` | the confinement of this thread and of each process that it makes later; `Err(Refusal { call, index, errno })` |
| `Call`, `Refusal` | the call (`NoNewPrivs`, `CreateRuleset`, `AddPathRule`, `AddPortRule`, `RestrictSelf`, `Seccomp`, `CapGet`, `CapBoundingDrop`, `CapSet`, `CapAmbientClear`, `OpenPath`; `name`, `code` 1 to 11, `from_code`); `Refusal::to_bytes` and `from_bytes` (five bytes for a pipe from a child), `describe(&compiled)` (the call, the path or the port of the rule, the errno), `failure(&compiled)` |
| `check()`, `probe()` | the ABI where a confinement loads here; `check` names the call and the errno of a refusal |
| `MIN_ABI` (3), `NET_ABI` (4), `SCOPE_ABI` (6), `UNIX_ABI` (9), `UDP_ABI` (10), `spec::handled_fs(abi)`, `spec::FILE_RIGHTS`, `spec::CONNECT_TCP`, `spec::BIND_TCP`, `spec::UDP_RIGHTS`, `spec::SCOPES`, `MOST_RULES` | the bounds, the rights and the scopes of each ABI |

The ruleset handles each file right that the ABI of the kernel knows (ABI
1: thirteen; ABI 2 adds `REFER`, ABI 3 `TRUNCATE`, ABI 5 `IOCTL_DEV`, ABI
9 `RESOLVE_UNIX`), so a right that no rule gives is denied for each path.
A rule on a file keeps the rights of a file (`EXECUTE`, `WRITE_FILE`,
`READ_FILE`, `TRUNCATE`, `IOCTL_DEV`, `RESOLVE_UNIX`). From ABI 9 a
connect, or a send to an address, to a Unix socket of a path that a
process outside the domain made (a system bus, the socket of a container
engine) needs a rule with write on the socket or on a directory above it
(`EACCES` else); a socket that the payload makes stays free to it. From
ABI 10 a closed UDP also handles the UDP rights of Landlock with no port
rule, so the kernel denies a bind, a connect and a send of each UDP
socket, also one that the filter does not see. The values of the rights
of ABI 9 and 10 come from `linux/landlock.h` of Linux 7.4; this host has
ABI 7, so only the compile is tested for them.

### The capabilities

`apply` drops each capability that `keep_caps` does not name: from the
bounding set (where the thread holds `CAP_SETPCAP`; else the drop needs
no step there), then from the effective, permitted and inheritable sets
(`capset`), then the ambient set. With `no_new_privs`, an `execve` as root
gives no more than the permitted set before it, so the payload keeps the
mask and nothing else. A payload of another user holds no capability, and
the drop changes nothing for it. `Spec::new()` keeps `DOCKER_CAPS`, so a
root payload that installs packages works (`CHOWN`, `DAC_OVERRIDE`,
`FOWNER`, `FSETID`, `KILL`, `SETGID`, `SETUID`, `SETPCAP`,
`NET_BIND_SERVICE`, `NET_RAW`, `SYS_CHROOT`, `MKNOD`, `AUDIT_WRITE`,
`SETFCAP`); `no_caps()` keeps none. Measured on Linux 6.18 (ABI 7), as root: a confined payload
that keeps `CAP_SYS_ADMIN` or `CAP_PERFMON` reads the environment of its
unconfined ancestors of the same uid (the supervisor, the caller) past the
Landlock domain; with no capability, or with `DOCKER_CAPS`, the read
fails with `EACCES`. Keep neither of the two.

### The scopes, UDP and MPTCP

| Field | Effect | ABI under which it gives nothing |
| --- | --- | --- |
| `scope` (on in `Spec::new()`) | `LANDLOCK_SCOPE_SIGNAL` and `LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET`: a signal to a process outside the domain of the payload (its parent, each other process of its user) and a connect or a send to an abstract Unix socket made outside it (a session bus, an X server) fail with `EPERM`. A process that the payload starts stays in its domain. `Compiled::scoped()` is 0 where the field gives nothing. | ABI 6 (Linux 6.12) |
| `udp: false` (`close_udp()`) | `socket(2)` of `AF_INET` and `AF_INET6` with the type `SOCK_DGRAM` fails with `EACCES` (`Deny::udp()`; the type is masked with `SOCK_TYPE_MASK`, as `SOCK_NONBLOCK` and `SOCK_CLOEXEC` are bits of it; a ping socket is a datagram socket too), and the io_uring calls fail with `EPERM`. TCP and Unix sockets stay free. | none: it is a seccomp rule |
| `connect` or `bind` set | `socket(2)` of `AF_INET` and `AF_INET6` with `IPPROTO_MPTCP` fails with `ENOPROTOOPT` (`Deny::mptcp()`), the answer of a kernel with MPTCP off, so a payload falls back to TCP; and the io_uring calls fail with `EPERM`. Landlock checks TCP sockets only: an MPTCP connect passes each port rule (measured on Linux 6.18, ABI 7). | none |
| `connect` or `bind` set, or `udp: false` | the sockets that Landlock does not check (`Deny::families()`): `socket(2)` and `socketpair(2)` of a family other than `AF_UNIX`, `AF_INET`, `AF_INET6` and `AF_NETLINK` fail with `EAFNOSUPPORT` (packet, VSOCK, SMC, RDS, RxRPC); `socket(2)` of `AF_INET` and `AF_INET6` of a type other than `SOCK_STREAM` and `SOCK_DGRAM` fails with `ESOCKTNOSUPPORT` (raw, SCTP over `SOCK_SEQPACKET`); a `SOCK_STREAM` of a protocol other than 0 and `IPPROTO_TCP` fails with `EPROTONOSUPPORT` (SCTP, SMC). Each answer is the one of a kernel with no such family, type or protocol. | none |

Why io_uring: a ring makes a socket (`IORING_OP_SOCKET`, Linux 5.19) with
no `socket(2)` call, so a seccomp rule on `socket` is void while a ring is
open; Landlock still checks the operations of a ring. The default list
denies the three io_uring calls; a spec that closes a socket adds them
again, also where its own list leaves them out.

Why a refusal on i686, s390x and powerpc64: the kernel of these targets
has `socketcall(2)`, which makes a socket with its arguments behind a
pointer, so a filter cannot read the family and the type. A payload would
make a UDP socket through it. A deny of each `socketcall` would stop each
socket of a payload that makes its sockets only through it (Go on 386 and
s390x; glibc on i386 for a kernel under 4.3; musl since 1.2.2 tries the
direct call first). So a compile of a spec with `udp: false` there is a
refusal by name (`Class::Unsupported`). The MPTCP rule and the rules of
the families there have the same gap, so a compile of a spec with a TCP
list (`connect` or `bind`) there is a refusal by name too (T-262,
PodUtils/podvm#276: each consumer gets the same answer, and a payload that
asks for the layer does not run without it). From ABI 10 the UDP rights
of Landlock close UDP there too, and a later version can take a closed
UDP there.

Not covered: under ABI 10, a UDP socket that the payload gets from
another process (a passed descriptor, `SCM_RIGHTS`); under ABI 9, a
connect to a Unix socket of a path; a socket of another family that the
payload gets from another process.

### The default deny list

`Deny::defaults()`; each call gets `EPERM` but `clone3`:

| Calls | Why |
| --- | --- |
| `unshare` and `clone` with a namespace flag (`CLONE_NEWNS`, `NEWCGROUP`, `NEWUTS`, `NEWIPC`, `NEWUSER`, `NEWPID`, `NEWNET`; `NEWTIME` for `unshare`); `setns` | a new namespace; a thread and a fork still start |
| `clone3` with `ENOSYS` | a filter cannot read its flags behind a pointer; the C library then uses `clone` |
| `mount`, `umount2`, `umount` (i386, s390x, powerpc64), `pivot_root`, `open_tree`, `open_tree_attr`, `move_mount`, `fsopen`, `fsconfig`, `fsmount`, `fspick`, `mount_setattr` | the mounts |
| `ptrace`, `process_vm_readv`, `process_vm_writev` | the memory of another process |
| `keyctl`, `add_key`, `request_key` | the keyrings |
| `bpf`, `perf_event_open` | the BPF and the events of the kernel |
| `kexec_load`, `kexec_file_load` (not on i386), `init_module`, `finit_module`, `delete_module` | the kernel and its modules |
| `open_by_handle_at`, `name_to_handle_at` | a file by its handle, past the paths |
| `io_uring_setup`, `io_uring_enter`, `io_uring_register` | a ring does work with no system call of its own (a socket), so a filter does not see it; a large surface of the kernel (docker's default profile denies them too) |

## Features

None.

## Settings

The `Spec` of the caller. The crate has no other setting.

## Adoption map

### podvm (PodUtils/podvm@T-1658, issue 170)

| podvm | pod-confine |
| --- | --- |
| the profile keys `fs.read`, `fs.write`, `fs.execute` (paths) | `Spec::allow(path, Access::READ)`, `Access::WRITE`, `Access::EXECUTE` (a path in two keys: their union) |
| `net.ports` (TCP connect; bind where a profile asks) | `Spec::connect_to(port)`, `Spec::bind_to(port)` |
| none (the scopes, UDP; the question goes to podvm with the release of T-209) | `Spec::scope` (on), `Spec::udp` (free); `no_scope()`, `close_udp()` |
| the confine step in the child of the entry | `pod_enter::Plan::confine: Some(spec)`: guest paths on a rung with a chroot, host paths on the userland rung (from the path map of podvm) |
| `podvm probe`: the ABI | `pod_confine::probe()`, or `check()` for the reason |
| the `errand` profile | a `Spec` of podvm |

## Tests

`cargo test -p pod-confine --all-features`: the unit tests (the rights of
each ABI, the scopes of each ABI, the compile and its refusals, the rules
of the switches, the refusal of a closed UDP where the kernel has
`socketcall`, a guest path under a root, the move of a rule off a taken
number, the close of the descriptors and the open of the paths again (a
path that went away is a refusal of `OpenPath` with its index), a refusal
and its bytes, a run of the BPF program over each call
of the list, a foreign ABI, the namespace flags, the fall-through of a
rule that does not hold, the AND of two conditions, a "not equal"
condition, the masked type of a socket, the rules of the families, the
rights of ABI 9 and 10), `tests/sockets.rs` (under a TCP list a packet,
VSOCK, raw IP, SCTP and SCTP stream socket fail with the answer of a
kernel with none, and TCP, UDP, Unix and netlink sockets open), and
`tests/apply.rs`: a child applies a compiled spec and makes
the checks of podvm's experiment 414 with no allocation (a read and a
write outside their lists fail with `EACCES`, a TCP connect and a bind
outside their lists fail with `EACCES` and a connect inside passes,
`NoNewPrivs: 1` and `Seccomp: 2`, `unshare` and `setns` fail with `EPERM`,
an exec outside the execute list fails with `EACCES`); `apply` refuses
where a seccomp filter stands in for a kernel with no Landlock; the probe
leaves the caller free; with the scopes a `kill` of the parent and a
connect to an abstract Unix socket of the parent fail with `EPERM`; with
UDP closed a UDP socket of IPv4 and IPv6 fails with `EACCES` and
`io_uring_setup` with `EPERM`, and TCP still works; under a TCP list an
MPTCP socket fails with `ENOPROTOOPT`. A host with no Landlock, an ABI
under the one of a check, or no MPTCP skips the check with a line
`skipping, REASON`. The plants of the group `confine` (`scripts/plants/`)
prove the checks. `pod-enter`'s `tests/confine.rs`
runs the same checks through the entry.
