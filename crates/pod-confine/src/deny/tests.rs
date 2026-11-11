use super::*;

/// One call as the kernel gives it to the filter.
struct Data {
    nr: u32,
    arch: u32,
    args: [u64; 6],
}

/// Run the BPF program over `data`, as the kernel does for the subset of
/// classic BPF that the program uses.
fn run(filter: &[SockFilter], data: &Data) -> u32 {
    let mut bytes = Vec::with_capacity(64);
    bytes.extend_from_slice(&data.nr.to_ne_bytes());
    bytes.extend_from_slice(&data.arch.to_ne_bytes());
    bytes.extend_from_slice(&0u64.to_ne_bytes());
    for arg in data.args {
        bytes.extend_from_slice(&arg.to_ne_bytes());
    }
    let (mut acc, mut pc) = (0u32, 0usize);
    loop {
        let ins = filter[pc];
        pc += 1;
        let jump = |taken: bool| usize::from(if taken { ins.jt } else { ins.jf });
        match ins.code {
            k::BPF_LD_W_ABS => {
                let at = ins.k as usize;
                acc = u32::from_ne_bytes(bytes[at..at + 4].try_into().expect("a word"));
            }
            k::BPF_JEQ_K => pc += jump(acc == ins.k),
            k::BPF_JGE_K => pc += jump(acc >= ins.k),
            k::BPF_JSET_K => pc += jump(acc & ins.k != 0),
            k::BPF_ALU_AND_K => acc &= ins.k,
            k::BPF_RET_K => return ins.k,
            code => panic!("the test runs no BPF code {code:#x}"),
        }
    }
}

fn call(nr: i64, args: [u64; 6]) -> Data {
    Data { nr: nr as u32, arch: k::AUDIT_ARCH_NATIVE, args }
}

const ALLOW: u32 = k::SECCOMP_RET_ALLOW;

fn errno(e: u16) -> u32 {
    k::SECCOMP_RET_ERRNO | u32::from(e)
}

fn defaults() -> Vec<SockFilter> {
    program(&Deny::defaults()).expect("the default program")
}

/// The calls that the ask of podvm (PodUtils/podvm#259) names.
#[test]
fn the_default_list_holds_each_call_of_the_ask() {
    let names: Vec<String> = Deny::defaults().into_iter().map(|d| d.name).collect();
    for name in [
        "unshare",
        "setns",
        "clone",
        "mount",
        "umount2",
        "pivot_root",
        "open_tree",
        "move_mount",
        "fsopen",
        "fsconfig",
        "fsmount",
        "ptrace",
        "process_vm_readv",
        "process_vm_writev",
        "keyctl",
        "add_key",
        "request_key",
        "bpf",
        "perf_event_open",
        "kexec_load",
        "init_module",
        "finit_module",
        "delete_module",
        "open_by_handle_at",
        "name_to_handle_at",
    ] {
        assert!(names.iter().any(|n| n == name), "the default list has no {name}");
    }
}

/// Each plain call of the list gets its errno, whatever its arguments.
#[test]
fn each_call_of_the_default_list_gets_its_errno() {
    let filter = defaults();
    for d in Deny::defaults().iter().filter(|d| d.when.is_empty()) {
        for args in [[0; 6], [u64::MAX; 6]] {
            let got = run(&filter, &Data { nr: d.nr, arch: k::AUDIT_ARCH_NATIVE, args });
            assert_eq!(got, errno(d.errno), "{}", d.name);
        }
    }
    assert_eq!(run(&filter, &call(k::SYS_SETNS, [0; 6])), errno(EPERM));
    assert_eq!(run(&filter, &call(k::SYS_CLONE3, [0; 6])), errno(ENOSYS));
}

/// A call of another ABI of the kernel (i386 on x86_64), and an x32 call,
/// has other numbers and would pass the list: it gets `ENOSYS`.
#[test]
fn a_foreign_abi_and_an_x32_call_get_enosys() {
    let filter = defaults();
    for nr in [k::SYS_GETPID, k::SYS_UNSHARE, k::SYS_WRITE] {
        let mut foreign = call(nr, [0; 6]);
        foreign.arch ^= 0x8000_0000;
        assert_eq!(run(&filter, &foreign), errno(ENOSYS), "call {nr} of a foreign ABI");
    }
    if k::X32_SYSCALL_BIT != 0 {
        let x32 = call(k::SYS_GETPID | i64::from(k::X32_SYSCALL_BIT), [0; 6]);
        assert_eq!(run(&filter, &x32), errno(ENOSYS));
    }
}

/// `unshare` and `clone` fail with a namespace flag and run without one:
/// a thread and a fork still start.
#[test]
fn unshare_and_clone_fail_with_a_namespace_flag_only() {
    let filter = defaults();
    let at = |nr: i64, flags: u64| {
        let mut args = [0u64; 6];
        let index = if nr == k::SYS_CLONE { k::CLONE_FLAGS_ARG as usize } else { 0 };
        args[index] = flags;
        run(&filter, &call(nr, args))
    };
    // CLONE_NEWUSER, CLONE_NEWNS, CLONE_NEWNET.
    for flag in [0x1000_0000u64, 0x0002_0000, 0x4000_0000] {
        assert_eq!(at(k::SYS_UNSHARE, flag), errno(EPERM), "unshare {flag:#x}");
        assert_eq!(at(k::SYS_CLONE, flag | 17), errno(EPERM), "clone {flag:#x}");
    }
    assert_eq!(at(k::SYS_UNSHARE, u64::from(CLONE_NEWTIME)), errno(EPERM));
    // CLONE_FILES, CLONE_FS; a fork (SIGCHLD); a thread
    // (CLONE_VM|CLONE_FS|CLONE_FILES|CLONE_SIGHAND|CLONE_THREAD|...).
    assert_eq!(at(k::SYS_UNSHARE, 0x400), ALLOW);
    assert_eq!(at(k::SYS_UNSHARE, 0x200), ALLOW);
    assert_eq!(at(k::SYS_CLONE, 17), ALLOW);
    assert_eq!(at(k::SYS_CLONE, 0x003d_0f00), ALLOW);
    // A flag in the high word is no namespace flag of clone.
    assert_eq!(at(k::SYS_CLONE, 1 << 40), ALLOW);
}

#[test]
fn each_other_call_runs() {
    let filter = defaults();
    for nr in [k::SYS_GETPID, k::SYS_READ, k::SYS_WRITE, k::SYS_EXECVE, k::SYS_OPENAT, k::SYS_EXIT_GROUP] {
        assert_eq!(run(&filter, &call(nr, [0; 6])), ALLOW, "call {nr}");
    }
}

/// The first rule of a number that holds gives the errno: a rule with no
/// condition holds for each call, and a rule after it never runs; a rule
/// with a condition that does not hold gives the call to the next rule.
#[test]
fn the_first_rule_of_a_number_wins() {
    let filter = program(&[
        Deny::call("getpid", k::SYS_GETPID, 13),
        Deny::call("getpid again", k::SYS_GETPID, 1),
        Deny::call("unshare", k::SYS_UNSHARE, 1).when_flags(0, NAMESPACE_FLAGS),
        Deny::call("unshare again", k::SYS_UNSHARE, 13),
        Deny::call("unshare never", k::SYS_UNSHARE, 2),
    ])
    .unwrap();
    assert_eq!(run(&filter, &call(k::SYS_GETPID, [0; 6])), errno(13));
    assert_eq!(run(&filter, &call(k::SYS_UNSHARE, [0; 6])), errno(13));
    assert_eq!(run(&filter, &call(k::SYS_UNSHARE, [0x1000_0000, 0, 0, 0, 0, 0])), errno(1));
    // Two blocks of two instructions and one of five: the rules after a
    // rule with no condition are not in the program.
    assert_eq!(filter.len(), defaults_head() + 2 + 5 + 2 + 1);
}

/// The instructions before the first rule: the check of the arch, and of
/// the x32 bit on x86_64.
fn defaults_head() -> usize {
    if k::X32_SYSCALL_BIT != 0 { 6 } else { 4 }
}

/// A rule with no condition takes two instructions, a rule with one flag
/// condition five: the default list keeps its size with the conditions
/// of more than one value.
#[test]
fn the_default_program_has_two_instructions_for_each_call_and_five_for_each_flag_rule() {
    let list = Deny::defaults();
    let flagged = list.iter().filter(|d| !d.when.is_empty()).count();
    assert_eq!(flagged, 2, "unshare and clone");
    let plain = list.len() - flagged;
    assert_eq!(defaults().len(), defaults_head() + 2 * plain + 5 * flagged + 1);
}

/// `socket(AF_INET, ...)` and `socket(AF_INET6, ...)`: two rules of one
/// number, each with two conditions.
fn two_socket_rules() -> Vec<SockFilter> {
    program(&[
        Deny::call("socket inet", k::SYS_SOCKET, 13).when_equal(0, u32::MAX, 2).when_equal(1, 0xf, 2),
        Deny::call("socket inet6", k::SYS_SOCKET, 13).when_equal(0, u32::MAX, 10).when_equal(1, 0xf, 2),
    ])
    .unwrap()
}

/// The second rule of a number runs where the first does not hold: the
/// block of the first loads the number again.
#[test]
fn a_rule_that_does_not_hold_falls_through_to_the_next_rule_of_its_number() {
    let filter = two_socket_rules();
    assert_eq!(run(&filter, &call(k::SYS_SOCKET, [2, 2, 0, 0, 0, 0])), errno(13), "AF_INET, SOCK_DGRAM");
    assert_eq!(run(&filter, &call(k::SYS_SOCKET, [10, 2, 0, 0, 0, 0])), errno(13), "AF_INET6, SOCK_DGRAM");
    assert_eq!(run(&filter, &call(k::SYS_SOCKET, [1, 2, 0, 0, 0, 0])), ALLOW, "AF_UNIX, SOCK_DGRAM");
    for nr in [k::SYS_GETPID, k::SYS_CONNECT] {
        assert_eq!(run(&filter, &call(nr, [2, 2, 0, 0, 0, 0])), ALLOW, "call {nr}");
    }
}

/// Each condition of a rule must hold; the mask takes the flags out of a
/// field, and the high word of an argument is not read.
#[test]
fn each_condition_of_a_rule_must_hold() {
    let filter = two_socket_rules();
    // SOCK_STREAM, SOCK_RAW: the family alone does not hold the rule.
    assert_eq!(run(&filter, &call(k::SYS_SOCKET, [2, 1, 0, 0, 0, 0])), ALLOW);
    assert_eq!(run(&filter, &call(k::SYS_SOCKET, [10, 3, 0, 0, 0, 0])), ALLOW);
    // SOCK_DGRAM with flags above the mask (SOCK_CLOEXEC, SOCK_NONBLOCK).
    assert_eq!(run(&filter, &call(k::SYS_SOCKET, [10, 2 | 0o2_004_000, 0, 0, 0, 0])), errno(13));
    assert_eq!(run(&filter, &call(k::SYS_SOCKET, [2 | 1 << 32, 2, 0, 0, 0, 0])), errno(13));
    // A value with a bit outside the mask never holds: it is refused.
    let e = program(&[Deny::call("socket", k::SYS_SOCKET, 13).when_equal(1, 0xf, 0x12)]).unwrap_err();
    assert!(e.to_string().contains("never holds"), "{e}");
}

/// Two flag conditions: both must hold.
#[test]
fn two_flag_conditions_must_both_hold() {
    let filter =
        program(&[Deny::call("unshare", k::SYS_UNSHARE, 1).when_flags(0, 0x1000_0000).when_flags(1, 1)]).unwrap();
    assert_eq!(run(&filter, &call(k::SYS_UNSHARE, [0x1000_0000, 1, 0, 0, 0, 0])), errno(1));
    assert_eq!(run(&filter, &call(k::SYS_UNSHARE, [0x1000_0000, 0, 0, 0, 0, 0])), ALLOW);
    assert_eq!(run(&filter, &call(k::SYS_UNSHARE, [0, 1, 0, 0, 0, 0])), ALLOW);
}

/// The rules of a closed UDP and of a TCP list, and the io_uring calls of
/// the default list.
#[test]
fn the_socket_rules_give_their_errno_for_their_sockets_only() {
    let mut list = Deny::udp();
    list.extend(Deny::mptcp());
    let filter = program(&list).unwrap();
    let socket =
        |family: u64, kind: u64, protocol: u64| run(&filter, &call(k::SYS_SOCKET, [family, kind, protocol, 0, 0, 0]));
    let cloexec = k::SOCK_CLOEXEC;
    for family in [k::AF_INET, k::AF_INET6] {
        assert_eq!(socket(family, k::SOCK_DGRAM, 0), errno(EACCES));
        assert_eq!(socket(family, k::SOCK_DGRAM | cloexec, 17), errno(EACCES));
        // A ping socket is a datagram socket too.
        assert_eq!(socket(family, k::SOCK_DGRAM, 1), errno(EACCES));
        assert_eq!(socket(family, k::SOCK_STREAM | cloexec, k::IPPROTO_MPTCP), errno(ENOPROTOOPT));
        assert_eq!(socket(family, k::SOCK_STREAM | cloexec, 0), ALLOW);
        assert_eq!(socket(family, k::SOCK_STREAM, 6), ALLOW);
    }
    assert_eq!(socket(k::AF_UNIX, k::SOCK_DGRAM, 0), ALLOW);
    let filter = defaults();
    for nr in [k::SYS_IO_URING_SETUP, k::SYS_IO_URING_ENTER, k::SYS_IO_URING_REGISTER] {
        assert_eq!(run(&filter, &call(nr, [0; 6])), errno(EPERM), "io_uring call {nr}");
    }
}

#[test]
fn a_rule_that_the_kernel_cannot_take_is_refused_by_name() {
    let e = program(&[Deny::call("getpid", k::SYS_GETPID, 0)]).unwrap_err();
    assert!(e.to_string().contains("getpid") && e.to_string().contains("1 to 4095"), "{e}");
    let e = program(&[Deny::call("getpid", k::SYS_GETPID, 4096)]).unwrap_err();
    assert_eq!(e.class, Class::Usage);
    let e = program(&[Deny::call("clone", k::SYS_CLONE, 1).when_flags(6, 1)]).unwrap_err();
    assert!(e.to_string().contains("argument 6"), "{e}");
    let e =
        program(&[Deny::call("socket", k::SYS_SOCKET, 1).when_equal(0, u32::MAX, 2).when_equal(7, 1, 1)]).unwrap_err();
    assert!(e.to_string().contains("socket") && e.to_string().contains("argument 7"), "{e}");
    let long = (0..90).fold(Deny::call("socket", k::SYS_SOCKET, 1), |d, i| d.when_equal(i % 6, 0xff, 1));
    let e = program(&[long]).unwrap_err();
    assert!(e.to_string().contains("over 255"), "{e}");
    let many: Vec<Deny> = (0..k::BPF_MAXINSNS as i64).map(|n| Deny::call(format!("call {n}"), n, 1)).collect();
    let e = program(&many).unwrap_err();
    assert!(e.to_string().contains("BPF instructions"), "{e}");
}
