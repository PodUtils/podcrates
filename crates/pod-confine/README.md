# pod-confine

The confinement of a payload with no privilege on Linux: Landlock rules
for the paths and the TCP ports, the Landlock scopes (signals and abstract
Unix sockets), a seccomp filter that denies a list of system calls (and
UDP where the spec closes it), and `no_new_privs`, from one `Spec`. New
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
   that does not exist is a refusal by name.
3. `apply(&compiled)` in the child: `PR_SET_NO_NEW_PRIVS`,
   `landlock_create_ruleset` (with the scopes), `landlock_add_rule` for
   each path and port,
   `landlock_restrict_self`, then `seccomp(SECCOMP_SET_MODE_FILTER)`
   (mode 2). No allocation, no lock. A `Refusal` names the call, the rule
   and the errno.

There is no unconfined way through: a compile refuses a kernel with no
Landlock and an ABI under 3 (ABI 1 and 2 do not confine `truncate(2)`),
and `apply` refuses each failed call, so the caller does not exec. A rule
holds the inode of its path: a rule on a directory covers the tree under
it, and the open before the fork names the file that the payload sees.

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
| `Spec { paths, connect, bind, scope, udp, deny }` | `paths`: `PathRule { path, access }`; `connect`, `bind`: `None` leaves the TCP call free, `Some(ports)` allows those ports only; `scope`: the Landlock scopes; `udp`: `false` closes UDP; `deny`: the calls that fail. `Spec::new()` (no path, TCP and UDP free, the scopes on, `Deny::defaults()`), `allow(path, access)`, `connect_to(port)`, `bind_to(port)`, `close_udp()`, `no_scope()`; `deny_rules()` (the rules of the filter: `deny`, then the rules of the switches) |
| `Access` | `READ` (read a file, list a directory), `WRITE` (write, truncate, make, remove, rename, link; an `ioctl` on a device), `EXECUTE`, `NONE`, and their union (`READ \| EXECUTE`); `rights()` gives the Landlock rights |
| `Deny` | `{ name, nr, errno, when }`: `Deny::call(name, SYS_*, errno)`; each condition of `when` is an `Arg` on the low 32 bits of one argument, and all must hold: `when_flags(index, mask)` (a bit of the mask is set), `when_equal(index, mask, value)` (the argument masked is the value); `Deny::defaults()`, `Deny::io_uring()`, `Deny::udp()`, `Deny::mptcp()` |
| `deny::program(&[Deny])` | the BPF program: the audit arch (a foreign ABI and x32 get `ENOSYS`), one block for each rule, `ALLOW`. The first rule of a number that holds gives its errno: a rule with conditions that do not hold gives the call to the next rule of its number, and a rule after a rule of its number with no condition is left out. A rule with no condition takes two instructions, one with one flag condition five. `NAMESPACE_FLAGS`, `CLONE_NEWTIME` |
| `Spec::compile(abi)`, `Spec::compile_in(abi, root_fd)` | a `Compiled` (`abi`, `handled_fs`, `handled_net`, `scoped`, `rules` with `fd`, `allowed`, `path`, `ports`, `filter`; it closes its descriptors when it drops); `keep_clear_of(&numbers)` moves each rule off a number that the child takes for a `dup2` |
| `apply(&Compiled)` | the confinement of this thread and of each process that it makes later; `Err(Refusal { call, index, errno })` |
| `Call`, `Refusal` | the call (`NoNewPrivs`, `CreateRuleset`, `AddPathRule`, `AddPortRule`, `RestrictSelf`, `Seccomp`; `name`, `code`, `from_code`); `Refusal::to_bytes` and `from_bytes` (five bytes for a pipe from a child), `describe(&compiled)` (the call, the path or the port of the rule, the errno), `failure(&compiled)` |
| `check()`, `probe()` | the ABI where a confinement loads here; `check` names the call and the errno of a refusal |
| `MIN_ABI` (3), `NET_ABI` (4), `SCOPE_ABI` (6), `spec::handled_fs(abi)`, `spec::FILE_RIGHTS`, `spec::CONNECT_TCP`, `spec::BIND_TCP`, `spec::SCOPES`, `MOST_RULES` | the bounds, the rights and the scopes of each ABI |

The ruleset handles each file right that the ABI of the kernel knows (ABI
1: thirteen; ABI 2 adds `REFER`, ABI 3 `TRUNCATE`, ABI 5 `IOCTL_DEV`), so a
right that no rule gives is denied for each path. A rule on a file keeps
the rights of a file (`EXECUTE`, `WRITE_FILE`, `READ_FILE`, `TRUNCATE`,
`IOCTL_DEV`). Under ABI 10 Landlock does not restrict UDP, and under ABI 9
it does not restrict a connect to a Unix socket of a path; the ruleset of
this crate handles neither right yet (T-213).

### The scopes, UDP and MPTCP

| Field | Effect | ABI under which it gives nothing |
| --- | --- | --- |
| `scope` (on in `Spec::new()`) | `LANDLOCK_SCOPE_SIGNAL` and `LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET`: a signal to a process outside the domain of the payload (its parent, each other process of its user) and a connect or a send to an abstract Unix socket made outside it (a session bus, an X server) fail with `EPERM`. A process that the payload starts stays in its domain. `Compiled::scoped()` is 0 where the field gives nothing. | ABI 6 (Linux 6.12) |
| `udp: false` (`close_udp()`) | `socket(2)` of `AF_INET` and `AF_INET6` with the type `SOCK_DGRAM` fails with `EACCES` (`Deny::udp()`; the type is masked with `SOCK_TYPE_MASK`, as `SOCK_NONBLOCK` and `SOCK_CLOEXEC` are bits of it; a ping socket is a datagram socket too), and the io_uring calls fail with `EPERM`. TCP and Unix sockets stay free. | none: it is a seccomp rule |
| `connect` or `bind` set | `socket(2)` of `AF_INET` and `AF_INET6` with `IPPROTO_MPTCP` fails with `ENOPROTOOPT` (`Deny::mptcp()`), the answer of a kernel with MPTCP off, so a payload falls back to TCP; and the io_uring calls fail with `EPERM`. Landlock checks TCP sockets only: an MPTCP connect passes each port rule (measured on Linux 6.18, ABI 7). | none |

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
refusal by name (`Class::Unsupported`). The MPTCP rule there has the same
gap, and T-213 tracks it.

Not covered: a UDP socket that the payload gets from another process (a
passed descriptor, `SCM_RIGHTS`), a connect to a Unix socket of a path,
and the sockets that Landlock does not see past TCP (SCTP, SMC, and the
other families; T-213).

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
number, a refusal and its bytes, a run of the BPF program over each call
of the list, a foreign ABI, the namespace flags, the fall-through of a
rule that does not hold, the AND of two conditions, the masked type of a
socket), and `tests/apply.rs`: a child applies a compiled spec and makes
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
