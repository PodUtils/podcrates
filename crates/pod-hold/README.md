# pod-hold

The hold of each connect of a payload, with no root: the generic core of
podnet's `podnet pod` (`PodUtils/podnet:crates/podnet-netns`), as
PodUtils/podnet#414 and #422 cut it. Linux only: on another target the
crate is empty.

Two holds. Each reports its level and a proof ([`Hold`](src/level.rs)):

| Hold | Level | How |
| --- | --- | --- |
| seccomp | `advisory` | A filter that returns `SECCOMP_RET_USER_NOTIF` stops the socket calls of the payload. The supervisor answers each one. A payload can pass around it with a descriptor that it inherits, or with a call or a family that the filter or the handler gives back to the kernel. |
| netns | `enforced` | The payload runs in a new network namespace (in a new user namespace with no privilege). Its only route is a TUN that the supervisor holds. |
| none | `none` | No hold; the proof names the reason. |

The decisions are the consumer's: where a connect goes, the rules, the
upstreams, the relays, the fake DNS and the stack on the TUN. The crate
reads no environment variable, prints nothing, keeps no global state, and
each failure is a `pod_core::Failure` at the stage `host`.

Needs: Linux 5.14 for the seccomp hold (`SECCOMP_ADDFD_FLAG_SEND`; the
user notification itself is 5.0); unprivileged user namespaces and
`/dev/net/tun` for the netns hold.

## API

| Module | Contents |
| --- | --- |
| `abi` | The structures of the kernel (`SeccompNotif`, `SeccompNotifResp`, `SeccompNotifAddfd`), the ioctl numbers from their encoding (`Encoding::NATIVE`: powerpc and mips put the direction at bit 29), `TUNSETIFF`, `AUDIT_ARCH_NATIVE`, `X32_SYSCALL_BIT`, `SYS_ACCEPT`. |
| `filter` | `Calls::sockets()` (the 21 calls of podnet's pod), `Calls::of(&[nr])`, `Calls::program()` (the BPF program: a foreign ABI and x32 get `ENOSYS`; `socketcall` gets `ENOSYS` on i386 and powerpc64; a `recvfrom` with no source runs in the kernel), `install(&calls)` (`no_new_privs`, then the filter; the notification descriptor). |
| `spawn` | `resolve(name, path)` (as `execvp`, with the `PATH` of the caller), `Program::new` and `Program::with_env` (made before the fork), `start(program, &calls)` (a helper thread installs the filter and forks; the child calls only `execve`): `Started { listener, pid, hold }`. |
| `listen` | `Listener::new(fd, access)` on the reactor of tokio; `recv().await` gives the next `Notification { id, pid, nr, args, memory }`, or `None` when no task uses the filter. The memory is opened for each call, and `SECCOMP_IOCTL_NOTIF_ID_VALID` after the open checks that the task still waits. `filter_unused(fd)`. |
| `mem` | `Memory::open(pid, access)` (`/proc/PID/mem` read-write, else read-only with `process_vm_writev`), `Memory::of_fd` (a memfd stands in for the task), `read`, `write`, `read_word`, `write_to`, `process_vm_writable`; `Access` (`write_open`, `vm_write`); `memfd(bytes)` with the feature `fuzzing` or in tests. |
| `sockaddr` | `encode`, `parse` (no byte past the length of the family), `read(memory, ptr, len)` (at most the length of the task and 128 bytes), `Address`. |
| `reply` | The seam: `Answer` (`respond` with the negative errno, `proceed`); `respond`, `proceed`, `addfd_send`, `addfd_setfd`, `id_valid`, `child_fd_flags`, `set_nonblocking`; `Recorded` keeps the answers in memory for a test or a fuzz target. |
| `table` | `Table<T>` of the sockets of the payload, keyed by the inode of the socket (a copy finds its entry; a sweep removes the sockets that closed with no `close`); `has_room` at the ceiling (the flow budget); `handle_close`; `derived_ceiling(RLIMIT_NOFILE)`, `nofile`, `raise_nofile`; `Refusal` and `Refusals` (a `const fn new` for a static of the consumer). |
| `connect` | The state of a held `connect` while the consumer dials: `Dials<K>` (keyed by the consumer's key of a socket): `join` (a dial in progress: a blocking call waits, a nonblocking one gets `EALREADY`), `start` (`EINPROGRESS` for a nonblocking call), `wait_error` (a `getsockopt(SO_ERROR)` waits), `finish` (`None`: a blocking dial with no waiting call; drop it); `Finished::answer` (a `connect` takes the error, else the first `SO_ERROR`), `Finished::taker` (the call that can take the upstream socket); `write_error`, `write_error_to_task`; `Caller`, `ErrorCall`. The dial, the rules, the hand-off and the relay are the consumer's. |
| `netns` | `enter()` (`CLONE_NEWNET`, else `CLONE_NEWUSER\|CLONE_NEWNET`), the handshake `announce` (child) and `answer` (parent, the id maps), `write_id_maps`, `bringup_loopback`, `Tun` (podnet's defaults) and `create_tun`, `send_fd`, `send_fd_raw` (no allocation, for a child after `fork`), `recv_fd` (one descriptor, else a refusal), `socketpair`, `read_exact_fd`, `write_all_fd`. |
| `packet` | The netns hold of a SYN: `parse_tcp_syn` (a bare IPv4 TCP SYN; a header under 20 bytes or a later fragment is none), `rst_reply_for_syn` (the RST\|ACK that fails the `connect` of the payload with `ECONNREFUSED`), `ipv4_checksum`. The hold of each SYN on the TUN (dial first, then accept or RST) stays the consumer's until T-170, step 2. |
| `level` | `Level` (`enforced`, `advisory`, `none`) and `Hold { level, proof, not_held }`: `Hold::seccomp`, `Hold::netns`, `Hold::none`. |

## Features

* `fuzzing`: `mem::memfd`, for the fuzz target `hold_sockaddr` of `fuzz/`.
  No consumer turns it on.

## Settings

| Setting | Default | podnet |
| --- | --- | --- |
| `Calls` | `Calls::sockets()` | the same |
| `mem::Access` | write open, then `process_vm_writev` | `PODNET_NETNS_TEST_MEM` (`no-fd`, `none`) stays a variable of podnet and gives the `Access` |
| the ceiling of `Table::new(root, ceiling)` | none: the caller gives it (`derived_ceiling(nofile().1)` after `raise_nofile`) | `--flow-budget N`, else the derived ceiling |
| `netns::Tun` | `eth0`, 10.66.255.255/31, gateway 10.66.255.254, MTU 1500 | the same |
| the `PATH` of `spawn::resolve` | `/usr/local/bin:/usr/bin:/bin` | the `PATH` of podnet's process |

## Parity

With podnet `crates/podnet-netns` at `9e8d177`:

* Each item of the cut is here, with its behaviour: the filter and its
  list, the start on a helper thread, the receive (poll, then
  `NOTIF_RECV`; `EAGAIN`, `EINTR` and `ENOENT` go on; `POLLHUP` ends), the
  open of the memory for each call and the check of the id after it, the
  read and the write of the memory, the reply with the negative errno
  (issue 98), `ADDFD` with `SEND` and `SETFD`, the codec of an address,
  the table with its sweep and its budget (PodUtils/podnet@T-064, PodUtils/podnet@T-181), the namespace,
  the id maps, the TUN, the descriptor passing.
* Changes:
  * The ioctl numbers follow the encoding of the target. podnet wrote the
    x86_64 numbers: on powerpc64le, `NOTIF_ADDFD`, `NOTIF_ID_VALID` and
    `TUNSETIFF` were wrong numbers.
  * `recv_fd` refuses a message with no descriptor, with more than one
    (podnet kept the others open), or with a cut control part.
  * `read_word` gives `None` past the bytes (podnet panicked); an address
    past `i64::MAX` is `EFAULT` with no overflow.
  * The id of a notification is checked after each open of the memory,
    also when the open failed.
  * The pid of `process_vm_writev` is in the `Memory` (podnet: a static).
    The ceiling and the refusals are values of the caller (podnet:
    statics). The summary of the refusals ends with the budget; podnet
    adds ` (--flow-budget)`.
  * The descriptor limit comes from `prlimit64`: 64-bit values on a 32-bit
    target.
* Not here (stays in podnet): the dispatch of each call to its handler
  (`socket`, `connect`, the sends, the receives, the options, `bind`,
  `listen`, `accept`, ICMP), the dials and the hand-off, the relays, the
  rules, the upstreams, the fake DNS, the flow log, `--learn`, the
  inbound listeners, the sessions, the router of the netns backend (its
  smoltcp stack and UDP flows), the reaper and the outlive of the pod,
  `pod doctor` and the CLI.

## Adoption map

### podnet

podnet took these rows at `b759937` (tag `v0.15.0`):

| Old (`crates/podnet-netns/src/`) | New |
| --- | --- |
| `seccomp/filter.rs`: the structures, the constants, `intercepted_syscalls`, `install_notif_filter` | `pod_hold::abi`, `pod_hold::filter::{Calls::sockets, install}`; `install_notif_filter` stays as a test helper over `install` |
| `seccomp/notif.rs`: `notif_respond`, `notif_continue`, `notif_addfd_send`, `notif_addfd_setfd`, `notif_id_valid`, `Answer`, `sockaddr_in`, `parse_sockaddr`, `set_nonblocking`, `child_fd_flags` | the same names, as glue over `pod_hold::reply` and `pod_hold::sockaddr` |
| `seccomp/mem.rs`: `read_mem`, `read_word`, `write_mem`, `write_mem_to`, `open_mem_of`, `process_vm_writable`; `open_mem` | glue over `pod_hold::mem`; `MEM_PID`, the warning and `PODNET_NETNS_TEST_MEM` (as `mem_access()`) stay; `open_mem` becomes `note_task(pid, writable)` |
| `seccomp/mod.rs`: `resolve_program`, the helper thread of `supervise_pod`, the receive of `supervise_seccomp`, `filter_unused` | `pod_hold::spawn::{resolve, Program::with_env, start}`, `pod_hold::listen::{Listener, filter_unused}` |
| `fdtable.rs`: `Table`, `Key`, `Refusal`, `Refusals`, `derived_ceiling`, `nofile`, `raise_nofile`, `inode_of`, `handle_close`, `MIN_CEILING`, `MAX_CEILING` | `pod_hold::table`; `Table` is `pod_hold::table::Table<Sock>`; `CEILING`, `REFUSALS`, `refuse` and `report_refusals` stay |
| `netns/fdpass.rs` | glue over `pod_hold::netns::{send_fd, send_fd_raw, recv_fd}` |
| `netns/tun.rs`: `TUNSETIFF`, `IFF_TUN`, `IFF_NO_PI`, `ifreq_for`, `bringup_loopback`, `create_tun` | `pod_hold::abi::TUNSETIFF`, `pod_hold::netns::{..}` with `Tun` of podnet's constants; the smoltcp device stays |
| `netns/mod.rs`: `write_proc`, `write_id_maps`, the handshake in `run_router` and `child_setup`, `write_all_fd`, `read_exact_fd`, `socketpair` | `pod_hold::netns::{write_id_maps, answer, enter, announce, write_all_fd, read_exact_fd, socketpair}` |
| `fuzzing.rs`: `Answers` | `pod_hold::reply::Recorded` |
| tests `the_filter_covers_the_bypass_surfaces` (`lib.rs`), `refusals_are_counted_by_class`, `the_ceiling_follows_rlimit_nofile`, `a_socket_link_gives_its_inode`, `a_copy_of_a_socket_has_the_inode_of_the_socket` (`fdtable.rs`) | here, with the same names |
| `src/warp/tun.rs` (podnet, not of the cut): `TUNSETIFF = 0x4004_54CA` | `pod_hold::abi::TUNSETIFF`: the same defect of powerpc64le |
| `crates/podnet-netns/README.md`: "Linux 5.0 or newer" | Linux 5.14 (`SECCOMP_ADDFD_FLAG_SEND`) |
| `podnet-netns/Cargo.toml`, `Cargo.toml` | add `pod-hold` (tag `v0.15.0`) |

podnet took these rows at `e4d4049` (tag `v0.17.0`; T-169, T-170 step
1, GitHub #59):

| Old (`crates/podnet-netns/src/`) | New |
| --- | --- |
| `connect.rs`: `ErrorCall`, `Waiting`, the state in `Dials` (`in_progress`, `nonblocking`, `wait_error`), the waits of `handle_connect` (the join, `EALREADY`, `EINPROGRESS`), the drop, the answers and the taker of `finish`, `answer_error` | `pod_hold::connect::{Dials<Key>, ErrorCall}`: `join`, `start`, `finish`, `Finished::answer`, `Finished::taker`, `write_error_to_task` with `mem_access()`; `Dialed`, the channel of the ends of the dials, the dial, the rules, the entry of the socket, the hand-off and the relay stay |
| `handoff.rs`: `Caller` | `pod_hold::connect::Caller` |
| `netns/packet.rs`: `parse_tcp_syn`, `rst_reply_for_syn`, `ipv4_checksum` | `pod_hold::packet`; `dns_reply_packet` stays |
| tests `parses_a_tcp_syn`, `the_rst_reply_answers_the_syn`, `a_header_shorter_than_20_bytes_is_no_syn`, `a_fragment_past_the_first_is_no_syn` (`lib.rs`) | here, with the same names |
| each `pod-*` dependency | tag `v0.17.0` |

From the import, `connect.rs` and `netns/packet.rs` of podnet take fixes
only, until podnet takes the patch.

### Later consumers

* podvm (`--net`, A-V-02, PodUtils/podvm@T-1665): the netns hold, or the
  seccomp hold with its own handler of `connect`.
* podrunner (G-04, T-160): the level and the proof of the hold of each
  unit, for its run report.

## Tests

`cargo test -p pod-hold`: the unit tests of each module (a BPF run of the
program, the ioctl numbers of both encodings, the memory of a memfd and of
this process, the codec and the read of an address, the table and its
sweep, the descriptor passing, the levels). `tests/hold.rs` (no libtest:
the binary is the payload too): a refused connect fails with the errno of
the reply, a socket call gets the descriptor of the supervisor, and a
namespace gives its TUN to the parent. A host that refuses a hold skips its
test with a line `SKIP:`. The fuzz target `hold_sockaddr` reads an address
from a memfd. The plants `hold-reply-errno-positive`,
`hold-address-read-past-length` and `hold-filter-foreign-abi-passes`
(`scripts/plants/`) prove the core checks.

## Sources

`SOURCES` lists each imported file with its blob.
