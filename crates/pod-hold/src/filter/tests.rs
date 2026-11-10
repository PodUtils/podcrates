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
