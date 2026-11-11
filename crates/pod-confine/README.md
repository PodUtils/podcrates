# pod-confine

The confinement of a payload with no privilege on Linux: Landlock rules
for the paths and the TCP ports, a seccomp filter that denies a list of
system calls, and `no_new_privs`, from one `Spec`. New code for the ask of
podvm (PodUtils/podvm#259, A-V-01 of GitHub #35). Linux only: on another
target the crate is empty.

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
   `landlock_create_ruleset`, `landlock_add_rule` for each path and port,
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
TCP rules; Landlock in the list of LSMs of the host.

## API

| Item | What |
| --- | --- |
| `Spec { paths, connect, bind, deny }` | `paths`: `PathRule { path, access }`; `connect`, `bind`: `None` leaves the TCP call free, `Some(ports)` allows those ports only; `deny`: the calls that fail. `Spec::new()` (no path, TCP free, `Deny::defaults()`), `allow(path, access)`, `connect_to(port)`, `bind_to(port)` |
| `Access` | `READ` (read a file, list a directory), `WRITE` (write, truncate, make, remove, rename, link; an `ioctl` on a device), `EXECUTE`, `NONE`, and their union (`READ \| EXECUTE`); `rights()` gives the Landlock rights |
| `Deny` | `{ name, nr, errno, flags }`: `Deny::call(name, SYS_*, errno)`, `when_flags(index, mask)` (only where the low 32 bits of the argument have a bit of the mask), `Deny::defaults()` |
| `deny::program(&[Deny])` | the BPF program: the audit arch (a foreign ABI and x32 get `ENOSYS`), one block for each call (the first rule of a number wins), `ALLOW`; `NAMESPACE_FLAGS`, `CLONE_NEWTIME` |
| `Spec::compile(abi)`, `Spec::compile_in(abi, root_fd)` | a `Compiled` (`abi`, `handled_fs`, `handled_net`, `rules` with `fd`, `allowed`, `path`, `ports`, `filter`; it closes its descriptors when it drops); `keep_clear_of(&numbers)` moves each rule off a number that the child takes for a `dup2` |
| `apply(&Compiled)` | the confinement of this thread and of each process that it makes later; `Err(Refusal { call, index, errno })` |
| `Call`, `Refusal` | the call (`NoNewPrivs`, `CreateRuleset`, `AddPathRule`, `AddPortRule`, `RestrictSelf`, `Seccomp`; `name`, `code`, `from_code`); `Refusal::to_bytes` and `from_bytes` (five bytes for a pipe from a child), `describe(&compiled)` (the call, the path or the port of the rule, the errno), `failure(&compiled)` |
| `check()`, `probe()` | the ABI where a confinement loads here; `check` names the call and the errno of a refusal |
| `MIN_ABI` (3), `NET_ABI` (4), `spec::handled_fs(abi)`, `spec::FILE_RIGHTS`, `spec::CONNECT_TCP`, `spec::BIND_TCP`, `MOST_RULES` | the bounds and the rights of each ABI |

The ruleset handles each file right that the ABI of the kernel knows (ABI
1: thirteen; ABI 2 adds `REFER`, ABI 3 `TRUNCATE`, ABI 5 `IOCTL_DEV`), so a
right that no rule gives is denied for each path. A rule on a file keeps
the rights of a file (`EXECUTE`, `WRITE_FILE`, `READ_FILE`, `TRUNCATE`,
`IOCTL_DEV`). Landlock does not restrict UDP or a Unix socket.

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
| the confine step in the child of the entry | `pod_enter::Plan::confine: Some(spec)`: guest paths on a rung with a chroot, host paths on the userland rung (from the path map of podvm) |
| `podvm probe`: the ABI | `pod_confine::probe()`, or `check()` for the reason |
| the `errand` profile | a `Spec` of podvm |

## Tests

`cargo test -p pod-confine --all-features`: the unit tests (the rights of
each ABI, the compile and its refusals, a guest path under a root, the
move of a rule off a taken number, a refusal and its bytes, a run of the
BPF program over each call of the list, a foreign ABI, the namespace
flags), and `tests/apply.rs`: a child applies a compiled spec and makes
the checks of podvm's experiment 414 with no allocation (a read and a
write outside their lists fail with `EACCES`, a TCP connect and a bind
outside their lists fail with `EACCES` and a connect inside passes,
`NoNewPrivs: 1` and `Seccomp: 2`, `unshare` and `setns` fail with `EPERM`,
an exec outside the execute list fails with `EACCES`); `apply` refuses
where a seccomp filter stands in for a kernel with no Landlock; the probe
leaves the caller free. A host with no Landlock skips the checks of a rule
with a line `skipping, REASON`. The plants `confine-write-unhandled`,
`confine-filter-foreign-abi-passes` and `confine-absent-runs-unconfined`
(`scripts/plants/`) prove the checks. `pod-enter`'s `tests/confine.rs`
runs the same checks through the entry.
