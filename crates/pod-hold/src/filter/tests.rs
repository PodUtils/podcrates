use super::*;
use crate::abi::SeccompData;

/// Run the BPF program over `data`, as the kernel does for the subset of
/// the program: loads of a word, the jumps on a constant, the returns.
fn run(filter: &[libc::sock_filter], data: &SeccompData) -> u32 {
    let mut bytes = Vec::with_capacity(64);
    bytes.extend_from_slice(&data.nr.to_ne_bytes());
    bytes.extend_from_slice(&data.arch.to_ne_bytes());
    bytes.extend_from_slice(&data.instruction_pointer.to_ne_bytes());
    for arg in data.args {
        bytes.extend_from_slice(&arg.to_ne_bytes());
    }
    let mut acc = 0u32;
    let mut pc = 0usize;
    loop {
        let ins = filter[pc];
        pc += 1;
        match ins.code {
            BPF_LD_ABS_W => {
                let at = ins.k as usize;
                acc = u32::from_ne_bytes(bytes[at..at + 4].try_into().expect("a word"));
            }
            BPF_JEQ_K => pc += usize::from(if acc == ins.k { ins.jt } else { ins.jf }),
            BPF_JGE_K => pc += usize::from(if acc >= ins.k { ins.jt } else { ins.jf }),
            BPF_ALU_AND_K => acc &= ins.k,
            BPF_RET_K => return ins.k,
            code => panic!("the test runs no BPF code {code:#x}"),
        }
    }
}

fn call(nr: libc::c_long, args: [u64; 6]) -> SeccompData {
    SeccompData { nr: nr as i32, arch: AUDIT_ARCH_NATIVE, instruction_pointer: 0, args }
}

#[test]
fn the_filter_covers_the_bypass_surfaces() {
    let calls = Calls::sockets();
    for required in [
        libc::SYS_socket,
        libc::SYS_connect,
        libc::SYS_sendto,
        libc::SYS_sendmsg,
        libc::SYS_sendmmsg,
        libc::SYS_setsockopt,
        libc::SYS_listen,
        SYS_ACCEPT,
        libc::SYS_accept4,
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
    ] {
        assert!(calls.contains(required), "syscall {required} is not intercepted");
    }
}

// Each stopped call goes to the supervisor, each other call runs, and a
// call of a foreign ABI fails with ENOSYS.
#[test]
fn the_program_stops_the_listed_calls_and_refuses_a_foreign_abi() {
    let calls = Calls::sockets();
    let filter = calls.program();
    for &nr in calls.numbers() {
        // recvfrom with a source buffer.
        assert_eq!(run(&filter, &call(nr, [0, 0, 0, 0, 0x1000, 0])), RET_USER_NOTIF, "call {nr}");
    }
    assert_eq!(run(&filter, &call(libc::SYS_getpid, [0; 6])), RET_ALLOW);
    assert_eq!(run(&filter, &call(libc::SYS_write, [0; 6])), RET_ALLOW);
    let mut foreign = call(libc::SYS_connect, [0; 6]);
    foreign.arch ^= 0x8000_0000;
    assert_eq!(run(&filter, &foreign), RET_ERRNO | libc::ENOSYS as u32);
    if X32_SYSCALL_BIT != 0 {
        let x32 = call(libc::SYS_connect | X32_SYSCALL_BIT as libc::c_long, [0; 6]);
        assert_eq!(run(&filter, &x32), RET_ERRNO | libc::ENOSYS as u32);
    }
}

// A recvfrom with no source buffer runs in the kernel; a pointer in the low
// or the high word stops it.
#[test]
fn a_recvfrom_with_no_source_runs_in_the_kernel() {
    let filter = Calls::sockets().program();
    let recv = |source: u64| run(&filter, &call(libc::SYS_recvfrom, [3, 0x1000, 64, 0, source, 0]));
    assert_eq!(recv(0), RET_ALLOW);
    assert_eq!(recv(0x1000), RET_USER_NOTIF);
    assert_eq!(recv(1 << 40), RET_USER_NOTIF);
    // The call after recvfrom in the list still stops.
    assert_eq!(run(&filter, &call(libc::SYS_recvmsg, [0; 6])), RET_USER_NOTIF);
    // A list of the caller stops each recvfrom.
    let own = Calls::of(&[libc::SYS_recvfrom]).program();
    assert_eq!(run(&own, &call(libc::SYS_recvfrom, [0; 6])), RET_USER_NOTIF);
}

// socket(AF_INET or AF_INET6, SOCK_RAW with any flag, any protocol) and a
// ping socket get EPERM in the kernel (PodUtils/podnet#538); the other
// sockets and the other calls go on as before. The opt-in lets them go to
// the supervisor.
#[test]
fn a_raw_socket_and_an_icmp_socket_get_eperm_unless_allowed() {
    let deny = Calls::sockets();
    let allow = Calls::sockets().allow_raw();
    assert!(deny.denies_raw() && !allow.denies_raw() && !Calls::of(&[libc::SYS_socket]).denies_raw());
    let socket = |filter: &[libc::sock_filter], family: i32, kind: i32, protocol: i32| {
        // A high word of garbage: the kernel reads an int.
        let arg = |v: i32| u64::from(v as u32) | 0xdead_0000_0000_0000;
        run(filter, &call(libc::SYS_socket, [arg(family), arg(kind), arg(protocol), 0, 0, 0]))
    };
    let eperm = RET_ERRNO | libc::EPERM as u32;
    let flags = libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK;
    let refused = [
        (libc::AF_INET, libc::SOCK_RAW, libc::IPPROTO_ICMP),
        (libc::AF_INET, libc::SOCK_RAW | flags, libc::IPPROTO_ICMP),
        (libc::AF_INET6, libc::SOCK_RAW | libc::SOCK_CLOEXEC, libc::IPPROTO_ICMPV6),
        (libc::AF_INET, libc::SOCK_RAW, libc::IPPROTO_RAW),
        (libc::AF_INET6, libc::SOCK_RAW, libc::IPPROTO_TCP),
        (libc::AF_INET, libc::SOCK_DGRAM | flags, libc::IPPROTO_ICMP),
        (libc::AF_INET6, libc::SOCK_DGRAM, libc::IPPROTO_ICMPV6),
    ];
    for (family, kind, protocol) in refused {
        let what = format!("socket({family}, {kind:#x}, {protocol})");
        assert_eq!(socket(&deny.program(), family, kind, protocol), eperm, "{what}");
        assert_eq!(socket(&allow.program(), family, kind, protocol), RET_USER_NOTIF, "{what} with the opt-in");
    }
    for (family, kind, protocol) in [
        (libc::AF_INET, libc::SOCK_STREAM, 0),
        (libc::AF_INET6, libc::SOCK_DGRAM | flags, libc::IPPROTO_UDP),
        (libc::AF_UNIX, libc::SOCK_DGRAM, 0),
        (libc::AF_NETLINK, libc::SOCK_RAW, 0),
    ] {
        assert_eq!(socket(&deny.program(), family, kind, protocol), RET_USER_NOTIF, "socket({family}, {kind:#x})");
    }
    // The rule gives the number of the call back to the list.
    let filter = deny.program();
    let raw = [libc::AF_INET as u64, libc::SOCK_RAW as u64, 1, 0, 0, 0];
    assert_eq!(run(&filter, &call(libc::SYS_connect, raw)), RET_USER_NOTIF);
    assert_eq!(run(&filter, &call(libc::SYS_write, raw)), RET_ALLOW);
    // A list of the caller refuses no socket.
    let own = Calls::of(&[libc::SYS_socket]).program();
    assert_eq!(socket(&own, libc::AF_INET, libc::SOCK_RAW, libc::IPPROTO_ICMP), RET_USER_NOTIF);
}
