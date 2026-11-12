# pod-hold

The hold of each connect of a payload, with no root: the generic core of
podnet's `podnet pod` (`PodUtils/podnet:crates/podnet-netns`), as
PodUtils/podnet#414 and #422 cut it. Linux only: on another target the
crate is empty.

Two holds. Each reports its level and a proof ([`Hold`](src/level.rs)):

| Hold | Level | How |
| --- | --- | --- |
| seccomp | `advisory` | A filter that returns `SECCOMP_RET_USER_NOTIF` stops the socket calls of the payload. The supervisor answers each one. A payload can pass around it with a descriptor that it inherits, or with a call or a family that the filter or the handler gives back to the kernel. |
| netns | `enforced` | The payload runs in a new network namespace (in a new user namespace with no privilege). Its only route is its wire, which the supervisor holds: a TUN, else a veth pair whose frames only the packet socket of the supervisor reads. |
| none | `none` | No hold; the proof names the reason. |

The decisions are the consumer's: where a connect goes, the rules, the
upstreams, the relays, the fake DNS, and the stack on the wire with its
device. The crate reads no environment variable, prints nothing, keeps no
global state, and each failure is a `pod_core::Failure` at the stage
`host`.

With the feature `syn-hold`, the netns hold also holds each SYN of the
payload on the stack of `pod-netstack` until the consumer dialed its
upstream (the section "The hold of a SYN").

Needs: Linux 5.14 for the seccomp hold (`SECCOMP_ADDFD_FLAG_SEND`; the
user notification itself is 5.0); unprivileged user namespaces, and
`/dev/net/tun` or a veth pair (the `veth` module, packet sockets) in the
namespace, for the netns hold.

## The wire of the netns hold

The child of the netns hold makes its wire after `lo`, with
`open_wire(&tun, choice)`, and sends the descriptor to the parent:

* The TUN (`create_tun`): each read and each write is one IP packet.
* The veth pair (`create_veth`), when the namespace cannot open
  `/dev/net/tun` (`WireChoice::Auto`), or always (`WireChoice::Veth`).
  One `RTM_NEWLINK` makes the program end (`tun.name`, MAC `GUEST_MAC`)
  and the wire end (`WIRE_NAME`, `wire0`). The program end gets the
  address, the MTU and the default route of the `Tun`, as a TUN does, and
  its offloads (checksum, TSO, GSO) go off, so that each frame is at most
  the MTU. The descriptor is a packet socket on the wire end.

The parent reads the wire from the descriptor (`Wire::of`: a socket is
the veth wire). On the veth wire the device of the stack calls
`take_frame(frame, gateway, mtu)` for each frame that it reads: an IP
packet goes to the stack (`Frame::Ip`, with no Ethernet padding), an ARP
request for the gateway gets its reply (`Frame::Reply`, to write back at
once, from `GATEWAY_MAC`), and each other frame is dropped (another MAC,
the own frames, a frame larger than the MTU). Each packet of the stack
goes out as `frame_for(packet)`. The stack keeps its IP medium; it
computes the checksums and checks none (`Wire::checks_checksums`), as the
program end can send a partial sum. A read takes `Wire::read_buffer(mtu)`
bytes.

The child closes its copy of the packet socket after it sends it: each
open packet socket of the wire end gets a copy of each frame. A child
keeps its copy of a TUN. `create_tun`, `create_veth` and `open_wire`
allocate: call them in a child that forked from a process with one thread.

## The hold of a SYN

`syn_hold::SynHold` (the feature `syn-hold`; call C90) is the hold of
each `connect` of the payload on the wire of the netns hold: podnet's
router (PodUtils/podnet#424, #451). A stack of `pod-netstack` answers for
each destination of the payload (`any_ip`), and it accepts a connection
only after the consumer dialed its upstream:

1. The consumer reads the IP packets of the payload from the wire, takes
   its DNS and its UDP flows, and gives the rest to
   `pump(&mut from_payload, &mut to_payload, &dial)`.
2. The pump holds each new TCP SYN of IPv4 and starts `dial.dial(syn)` in
   a task. A SYN again while its dial runs is dropped. A new SYN past
   `Settings::max_flows` (held, dialed and open) is dropped with no RST:
   the payload sends it again (call C91).
3. When the dial succeeds, the pump gives the SYN to the stack: a listener
   on any address of the port (one for each port, closed when its last
   handshake and its last flow end) accepts it, and `next_flow()` gives a
   `Flow { syn, stream, upstream, open }`. The `local_addr` of the stream
   is the target. The consumer copies the flow; the drop of `open` ends it
   in the hold.
4. When the dial fails, the pump puts the RST of the SYN
   (`packet::rst_reply_for_syn`) into `to_payload`: the `connect` of the
   payload fails with `ECONNREFUSED` at once, the stack never sees the
   SYN, and `dial.refused(syn, Refused::Reset, error)` gets the failure.
5. The consumer writes `to_payload` (the RSTs and the packets of the
   stack) to the wire, and waits for the wire, `activity()` (a packet of
   the stack, an accepted stream, the end of a dial or of a flow) or a
   timer (the time limits of `Settings`).

A stream that the stack accepted with no upstream (its upstream waited
past `dialed_ttl`) gets a late dial: it waits in the hold, then becomes a
flow, or it is aborted (`Refused::Aborted`; call C100).

The dial, the rules, the names of the targets, the copy of a flow, the
flow log, the DNS and the UDP flows are the consumer's. A binary that
turns the feature on sets `SMOLTCP_IFACE_MAX_ADDR_COUNT = "8"` in its
`.cargo/config.toml` (`pod-netstack`).

## API

| Module | Contents |
| --- | --- |
| `abi` | The structures of the kernel (`SeccompNotif`, `SeccompNotifResp`, `SeccompNotifAddfd`), the ioctl numbers from their encoding (`Encoding::NATIVE`: powerpc and mips put the direction at bit 29), `TUNSETIFF`, `AUDIT_ARCH_NATIVE` and `X32_SYSCALL_BIT` (from `pod_hostprobe::sys`, the one table of the workspace), `SYS_ACCEPT`. |
| `filter` | `Calls::sockets()` (the 21 calls of podnet's pod), `Calls::of(&[nr])`, `Calls::program()` (the BPF program: a foreign ABI and x32 get `ENOSYS`; `socketcall` gets `ENOSYS` on i386 and powerpc64; a `recvfrom` with no source runs in the kernel), `install(&calls)` (`no_new_privs`, then the filter; the notification descriptor). |
| `spawn` | `resolve(name, path)` (as `execvp`, with the `PATH` of the caller), `Program::new` and `Program::with_env` (made before the fork), `start(program, &calls)` (a helper thread installs the filter and forks; the child calls only `execve`): `Started { listener, pid, hold }`. |
| `listen` | `Listener::new(fd, access)` on the reactor of tokio; `recv().await` gives the next `Notification { id, pid, nr, args, memory }`, or `None` when no task uses the filter. The memory is opened for each call, and `SECCOMP_IOCTL_NOTIF_ID_VALID` after the open checks that the task still waits. `filter_unused(fd)`. |
| `mem` | `Memory::open(pid, access)` (`/proc/PID/mem` read-write, else read-only with `process_vm_writev`), `Memory::of_fd` (a memfd stands in for the task), `read`, `write`, `read_word`, `write_to`, `process_vm_writable`; `Access` (`write_open`, `vm_write`); `memfd(bytes)` with the feature `fuzzing` or in tests. |
| `sockaddr` | `encode`, `parse` (no byte past the length of the family), `read(memory, ptr, len)` (at most the length of the task and 128 bytes), `Address`. |
| `reply` | The seam: `Answer` (`respond` with the negative errno, `proceed`); `respond`, `proceed`, `addfd_send`, `addfd_setfd`, `id_valid`, `child_fd_flags`, `set_nonblocking`; `Recorded` keeps the answers in memory for a test or a fuzz target. |
| `table` | `Table<T>` of the sockets of the payload, keyed by the inode of the socket (a copy finds its entry; a sweep removes the sockets that closed with no `close`); `has_room` at the ceiling (the flow budget); `handle_close`; `derived_ceiling(RLIMIT_NOFILE)`, `nofile`, `raise_nofile`; `Refusal` and `Refusals` (a `const fn new` for a static of the consumer). |
| `connect` | The state of a held `connect` while the consumer dials: `Dials<K>` (keyed by the consumer's key of a socket): `join` (a dial in progress: a blocking call waits, a nonblocking one gets `EALREADY`), `start` (`EINPROGRESS` for a nonblocking call), `wait_error` (a `getsockopt(SO_ERROR)` waits), `finish` (`None`: a blocking dial with no waiting call; drop it); `Finished::answer` (a `connect` takes the error, else the first `SO_ERROR`), `Finished::taker` (the call that can take the upstream socket); `write_error`, `write_error_to_task`; `Caller`, `ErrorCall`. The dial, the rules, the hand-off and the relay are the consumer's. |
| `netns` | `enter()` (`CLONE_NEWNET`, else `CLONE_NEWUSER\|CLONE_NEWNET`), the handshake `announce` (child) and `answer` (parent, the id maps), `write_id_maps`, `bringup_loopback`, `Tun` (podnet's defaults) and `create_tun`; the wire (the section above): `Wire` (`Tun`, `Veth`; `of(fd)`, `word`, `checks_checksums`, `read_buffer`), `WireChoice` (`Auto`, `Tun`, `Veth`), `open_wire`, `create_veth`, `take_frame`, `Frame` (`Ip`, `Reply`, `Drop`), `frame_for`, `GUEST_MAC`, `GATEWAY_MAC`, `WIRE_NAME`, `ETH_HEADER`; `send_fd`, `send_fd_raw` (no allocation, for a child after `fork`), `recv_fd` (one descriptor, else a refusal), `socketpair`, `read_exact_fd`, `write_all_fd`. |
| `packet` | The netns hold of a SYN: `parse_tcp_syn` (a bare IPv4 TCP SYN; a header under 20 bytes or a later fragment is none), `rst_reply_for_syn` (the RST\|ACK that fails the `connect` of the payload with `ECONNREFUSED`), `ipv4_checksum`. `syn_hold` uses them. |
| `syn_hold` | The feature `syn-hold` (the section "The hold of a SYN"): `SynHold<U, E>` (`new(StackConfig, Settings)`, `pump`, `activity`, `next_flow`, `shutdown`, `stack`, `stats`, `held_count`, `flow_count`, `listening_ports`); `Dial` (`type Upstream`, `type Error: From<JoinError>`, `dial(syn)`, `refused(syn, Refused, error)`; a closure `Fn(Syn) -> Future` is one); `Syn` (`source`, `target`; `Syn::of(packet)`); `Flow<U>` (`syn`, `stream`, `upstream`, `open`); `Open`; `Refused` (`Reset`, `Aborted`); `Settings`; `Stats` (`held`, `released`, `reset`, `again`, `over_budget`, `no_listener`, `late`, `aborted`, `flows`). |
| `level` | `Level` (`enforced`, `advisory`, `none`) and `Hold { level, proof, not_held }`: `Hold::seccomp`, `Hold::netns(pid, userns, wire, name)` (the proof names the TUN or the veth pair), `Hold::none`. |

## Features

* `syn-hold`: the module `syn_hold`, on `pod-netstack` (call C90). The
  binary sets `SMOLTCP_IFACE_MAX_ADDR_COUNT = "8"`.
* `fuzzing`: `mem::memfd`, for the fuzz target `hold_sockaddr` of `fuzz/`.
  No consumer turns it on.

## Settings

| Setting | Default | podnet |
| --- | --- | --- |
| `Calls` | `Calls::sockets()` | the same |
| `mem::Access` | write open, then `process_vm_writev` | `PODNET_NETNS_TEST_MEM` (`no-fd`, `none`) stays a variable of podnet and gives the `Access` |
| the ceiling of `Table::new(root, ceiling)` | none: the caller gives it (`derived_ceiling(nofile().1)` after `raise_nofile`) | `--flow-budget N`, else the derived ceiling |
| `netns::Tun` | `eth0`, 10.66.255.255/31, gateway 10.66.255.254, MTU 1500 | the same |
| `netns::WireChoice` | `Auto`: the TUN, else the veth pair | `Auto`; `Veth` when the test variable `PODNET_NETNS_TEST_VETH` (podnet's) is set and not empty |
| the `PATH` of `spawn::resolve` | `/usr/local/bin:/usr/bin:/bin` | the `PATH` of podnet's process |
| `syn_hold::Settings::max_flows` | 4096 | `MAX_TRACKED_FDS` |
| `syn_hold::Settings::dialed_ttl` | 30 s | `PREDIALED_TTL` |
| `syn_hold::Settings::handshake_ttl` | 10 s | `HANDSHAKE_TTL` |
| the `StackConfig` of `SynHold::new` | the caller gives it; `any_ip` is always on | the gateway `10.66.255.254/31`, MTU 1500, buffers of 64 KiB, `Checksum::Tx` on the veth wire |

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
  device on both wires, its DNS and its UDP flows; its hold of a SYN is
  `syn_hold`, with the parity of `a7350b2` below), the reaper
  and the outlive of the pod, `pod doctor` (with its row `veth wire (no
  TUN)`) and the CLI.

With podnet `crates/podnet-netns` at `db34bae`, the veth wire (T-201,
PodUtils/podnet#446 and #451; podnet's PodUtils/podnet@T-174):

* Each item is here, with its behaviour: the pair from one `RTM_NEWLINK`
  (the names, `GUEST_MAC`, the MTU of both ends), the setup of the program
  end, the offloads off (a refusal is not fatal), the wire end up, the
  packet socket of each protocol with `PACKET_IGNORE_OUTGOING`, the read of
  the wire from `fstat`, the ARP answer for the gateway, the frames of IPv4
  and IPv6 with no padding, the drop of the own frames, of another MAC and
  of a frame larger than the MTU, the frame of a packet, the read buffer of
  64 KiB and the checksums that the stack computes and does not check.
* Changes:
  * `WireChoice` is a setting of the caller; podnet read the process
    variable `PODNET_NETNS_TEST_VETH` (it stays podnet's, as the choice).
  * The program end takes the `configure` of the TUN (one copy of the
    ioctls; the MTU is set by the ioctl too). podnet had a second copy in
    `veth.rs`.
  * `take_frame` takes the gateway and the MTU; podnet read its constant
    gateway.
  * Each name (both ends, the TUN) is checked before a system call: empty,
    too long, a NUL byte, the same name for both ends (`Class::Usage`).
  * With `Auto`, the failure of both wires names both:
    `no TUN (...) and no veth wire: ...` (podnet:
    `...; the veth wire: ...`).
  * `Hold::netns` takes the wire: the proof of a veth hold names the veth
    pair, not a TUN.

With podnet `crates/podnet-netns` at `a7350b2`, the hold of a SYN of the
router of the netns backend on `pod-netstack` (T-170, step 2;
PodUtils/podnet#451, podnet's PodUtils/podnet@T-249):

* Each item is here, with its behaviour: the hold of each new SYN with
  its dial in a task, the drop of a SYN again while its dial runs, the
  release of the SYN to the stack after a dial that succeeded (first in
  the queue of the pass), the RST of a failed dial, the upstreams of the
  released SYNs (30 s) and the handshakes (10 s), a listener on any address
  for each port with its task of accepts, the close of the listener of an
  idle port, the match of an accepted stream to its upstream, the late
  dial, the abort of a stream of IPv6, the budget of 4096.
* Changes:
  * The dial is the consumer's `Dial`, given to each pass (call C90);
    podnet called `spawn_dial` (its rules, its flow log, its proxies) and
    `spawn_gateway_dial`, and printed a refusal with `-v` (now `refused`).
  * A new SYN past the budget is dropped and counted (`over_budget`); the
    payload sends it again. podnet gave it to the stack, which accepted it
    with no upstream, and the dial ran after the accept (call C91). The
    budget counts the dialed upstreams too (podnet: held and open).
  * A late dial waits in the hold, and its failure aborts the stream with
    `Refused::Aborted` (call C100); podnet's flow task waited for it. The
    late dial is the one `dial` of the consumer (podnet: `spawn_dial` also
    for a connection to the gateway).
  * The end of a dial and the end of a flow wake `activity` (call C100);
    podnet found a finished dial at its next pass (a timer of 5 ms).
  * The table of the open flows counts each accept of a connection: a
    connection that the stack accepts again while its first flow lives
    stays until both flows end (podnet's map kept one entry, and the end of
    the first flow removed it).
  * A listener that the stack refuses is counted (`no_listener`); podnet
    printed it with `-v`.

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

podnet took these rows at `f107766` (tag `v0.26.0`; the trial patch
`pod-hold-veth` of T-201, at podnet `db34bae`):

| Old (`crates/podnet-netns/src/`) | New |
| --- | --- |
| `netns/veth.rs`: `create_veth`, `new_link`, `newlink_request`, `netlink_error`, `configure`, `ethtool_off`, `packet_socket`, `is_veth`, `Frame`, `take_frame`, `arp_reply`, `frame_for`, `GUEST_MAC`, `GATEWAY_MAC`, `WIRE_NAME`, `ETH_HEADER` | glue over `pod_hold::netns`: `open_wire(name)` gives `open_wire(&hold_tun(name), choice)`, `create_veth(guest, wire)` gives `create_veth(&hold_tun(guest), wire)`, `is_veth` gives `Wire::of`, `take_frame(frame, mtu)` gives `take_frame(frame, TUN_GW, mtu)`; `Frame` and `frame_for` are the crate's; `TEST_VETH_VARIABLE` and `veth_forced` stay and give the `WireChoice` |
| `netns/tun.rs`: `Wire`, the `Tun` of pod-hold in `create_tun`, the buffer of `read_frames`, the checksums of `capabilities` | `pod_hold::netns::Wire`, `hold_tun(name)` (also for the veth wire), `Wire::Veth.read_buffer(mtu)`, `Wire::checks_checksums`; the smoltcp device stays |
| `doctor.rs`: the doc comment of `probe_veth` above `close_inherited` | above `probe_veth`; the probe row stays |
| tests `the_wire_answers_arp_for_the_gateway`, `the_wire_strips_the_header_and_the_padding_of_an_ip_frame`, `the_newlink_request_names_both_ends` (`netns/veth/tests.rs`) | here, with the same names; podnet's `tests/veth.rs` stays and runs its pod on the wire of the crate |
| `crates/podnet-netns/README.md` | names `pod_hold::netns` for the wire |
| each `pod-*` dependency | tag `v0.25.0` |

podnet took these rows at `98d8a71` (tag `v0.29.0`; the trial patch
`pod-hold-syn` of T-170 step 2, at podnet `a7350b2`):

| Old (`crates/podnet-netns/src/`) | New |
| --- | --- |
| `netns/backend.rs`: `SynKey`, `HeldSyn`, `DialTask`, `PREDIALED_TTL`, `HANDSHAKE_TTL`, `Accepted`, `Source`, the fields `stack`, `link_in`, `link_out`, `ports`, `accepted_tx`, `accepted_rx`, `accepted`, `handshakes`, `held` and `predialed` of `Backend`, `activity`, `has_connection`, `open_count`, `hold_syns`, `feed`, `listen_on`, `close_idle_ports`, `collect`, the match of a stream to its upstream in `accept_flows`, the wait of `Source::Dial` in `Flow::run`, `MAX_TRACKED_FDS` in `hold_syns` | `pod_hold::syn_hold`: the field `hold: SynHold<Dialed, anyhow::Error>` (`SynHold::new(config, Settings { max_flows: MAX_TRACKED_FDS, .. })`), `pump(&mut device.rx, &mut device.tx, &tcp)` in `tick`, `activity`, `next_flow` in `accept_flows`, `Syn` for `SynKey` |
| `spawn_dial`, `spawn_gateway_dial`, `target_of` | a `TcpRoute` (the rules, the proxies, the fake DNS, `--host-gateway`, `-v`) implements `Dial`: `dial` is `spawn_dial` with no spawn (the gateway for `TUN_GW`), `refused` prints the line of `-v`; `Dialed` carries the upstream, the target and the number of the flow log |
| `FlowRecord`, `Flow`, `relay`, the close lines of `Drop` | stay; `OpenFlows` keeps the records only, and `Flow` holds the `Open` of the hold |
| `netns/packet.rs`: the uses of `parse_tcp_syn` and `rst_reply_for_syn` | gone (`syn_hold` calls them); `ipv4_checksum` and `dns_reply_packet` stay |
| `crates/podnet-netns/README.md`; the citations of `backend.rs` and of that README in `TODO/` | names `pod_hold::syn_hold` and the budget; remapped |
| `podnet-netns/Cargo.toml`, `Cargo.toml` | `pod-hold` with the feature `syn-hold`; each `pod-*` dependency at the tag of the release |

### Later consumers

* podvm (`--net`, A-V-02, PodUtils/podvm@T-1665): the netns hold with the
  hold of a SYN (`syn_hold`, its own `Dial`), or the seccomp hold with its
  own handler of `connect`.
* podrunner (G-04, T-160): the level and the proof of the hold of each
  unit, for its run report.

## Tests

`cargo test -p pod-hold`: the unit tests of each module (a BPF run of the
program, the ioctl numbers of both encodings, the memory of a memfd and of
this process, the codec and the read of an address, the table and its
sweep, the descriptor passing, the levels, the frames and the ARP answer
of the veth wire, the `RTM_NEWLINK` request, the names of the wire). No
unit test makes an interface: each name check comes before a system call.
`tests/hold.rs` (no libtest: the binary is the payload too): a refused
connect fails with the errno of the reply, a socket call gets the
descriptor of the supervisor, and a namespace gives its TUN to the parent.
`tests/veth.rs` (no libtest): `veth_wire_carries_a_tcp_flow`, podnet's
test with a stack of `pod-netstack` in the place of the pod: the child
makes the veth wire in a new namespace, sends a datagram to the gateway,
connects to an address past it, and sends 256 KiB that the stack echoes;
no frame is larger than the MTU. A host that refuses a hold, a namespace,
`lo` or a veth pair skips its test with a line `SKIP:`; on a GitHub runner
a user has no user namespace, so `scripts/root-netns-tests.sh` (the step
"Netns tests as root" of `ci.yml`) runs both binaries as root, and a line
`SKIP:` fails it. The fuzz target `hold_sockaddr` reads an address from a
memfd.

`cargo test -p pod-hold --features syn-hold`: the hold of a SYN
(`src/syn_hold/tests.rs`, `src/syn_hold/tests/many.rs`) over a link in the
process: a stack of `pod-netstack` stands in for the payload behind its
wire, a pump moves its packets as podnet's loop moves the packets of the
TUN, and the dial gives an upstream in memory whose target greets and
echoes. A dial that succeeds gives a flow that carries 256 KiB both ways
(`a_syn_whose_dial_succeeds_is_accepted_and_carries_bytes_both_ways`, a
closure as the dial); a dead upstream gets one RST on the link and
`ECONNREFUSED` in less than 3 s, and the stack sees no SYN
(`netns_backend_refuses_connect_when_the_upstream_is_dead`); the SYN-ACK
of a held port comes after the SYN-ACK of a later port
(`the_stack_answers_a_syn_only_after_its_dial`); a SYN sent two times has
one dial (`a_syn_sent_again_while_its_dial_waits_is_held_once`); 48
connects at once to one port get 48 greetings and no RST
(`netns_backend_takes_a_burst_of_connects_to_one_port`); 300 ports at one
time, then each listener closes and six ports work again
(`netns_backend_has_no_cap_on_destination_ports`); a SYN past a budget of
one waits for room with no late dial
(`a_syn_past_the_budget_waits_for_room`). The tests of podnet keep their
names. The pump of the tests has no timer: a wake that the hold misses
stops a test at its time limit.

The plants `hold-reply-errno-positive`, `hold-address-read-past-length`,
`hold-filter-foreign-abi-passes`, `hold-veth-arp-any-address` and
`hold-veth-offloads-on` (`scripts/plants/`) prove the core checks;
`hold-syn-accepted-before-the-dial` and
`hold-syn-past-the-budget-to-the-stack` prove the tests of the hold of a
SYN.

## Sources

`SOURCES` lists each imported file with its blob.
