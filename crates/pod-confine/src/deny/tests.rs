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
    for d in Deny::defaults().iter().filter(|d| d.flags.is_none()) {
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

#[test]
fn the_first_rule_of_a_number_wins() {
    let filter = program(&[
        Deny::call("getpid", k::SYS_GETPID, 13),
        Deny::call("getpid again", k::SYS_GETPID, 1),
        Deny::call("unshare", k::SYS_UNSHARE, 1).when_flags(0, NAMESPACE_FLAGS),
        Deny::call("unshare again", k::SYS_UNSHARE, 13),
    ])
    .unwrap();
    assert_eq!(run(&filter, &call(k::SYS_GETPID, [0; 6])), errno(13));
    assert_eq!(run(&filter, &call(k::SYS_UNSHARE, [0; 6])), ALLOW);
    assert_eq!(run(&filter, &call(k::SYS_UNSHARE, [0x1000_0000, 0, 0, 0, 0, 0])), errno(1));
}

#[test]
fn a_rule_that_the_kernel_cannot_take_is_refused_by_name() {
    let e = program(&[Deny::call("getpid", k::SYS_GETPID, 0)]).unwrap_err();
    assert!(e.to_string().contains("getpid") && e.to_string().contains("1 to 4095"), "{e}");
    let e = program(&[Deny::call("getpid", k::SYS_GETPID, 4096)]).unwrap_err();
    assert_eq!(e.class, Class::Usage);
    let e = program(&[Deny::call("clone", k::SYS_CLONE, 1).when_flags(6, 1)]).unwrap_err();
    assert!(e.to_string().contains("argument 6"), "{e}");
    let many: Vec<Deny> = (0..k::BPF_MAXINSNS as i64).map(|n| Deny::call(format!("call {n}"), n, 1)).collect();
    let e = program(&many).unwrap_err();
    assert!(e.to_string().contains("BPF instructions"), "{e}");
}
