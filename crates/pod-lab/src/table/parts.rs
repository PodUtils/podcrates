//! The parts and the filters of the classes of podssh
//! (PodUtils/podssh#268) and podnet (PodUtils/podnet#585). The network
//! classes are in `network.rs`.

use super::*;

const ALL: u32 = u32::MAX;
const TYPE: u32 = k::SOCK_TYPE_MASK as u32;
const INET: u32 = k::AF_INET as u32;
const INET6: u32 = k::AF_INET6 as u32;
const DGRAM: u32 = k::SOCK_DGRAM as u32;
const RAW: u32 = k::SOCK_RAW as u32;
const EAFNOSUPPORT: i32 = e::EAFNOSUPPORT as i32;
const ENETUNREACH: i32 = e::ENETUNREACH as i32;

/// A ring makes a socket with no `socket(2)`: a rule on `socket` holds
/// only with no ring.
const IO_URING: Call = Call { name: "io_uring_setup", nr: k::SYS_IO_URING_SETUP, errno: EPERM, when: &[] };

/// `socket(2)` of a family (argument 0) and a type (argument 1, with no
/// flag bit) with `EPERM`.
const fn socket(name: &'static str, when: &'static [When]) -> Call {
    Call { name, nr: k::SYS_SOCKET, errno: EPERM, when }
}

const fn family(value: u32) -> When {
    When::Equal { arg: 0, mask: ALL, value }
}

const fn kind(value: u32) -> When {
    When::Equal { arg: 1, mask: TYPE, value }
}

const INET_DGRAM: &[When] = &[family(INET), kind(DGRAM)];
const INET6_DGRAM: &[When] = &[family(INET6), kind(DGRAM)];
const INET_RAW: &[When] = &[family(INET), kind(RAW)];
const INET6_RAW: &[When] = &[family(INET6), kind(RAW)];
const PACKET: &[When] = &[family(k::AF_PACKET as u32)];

/// `listen(2)` with `EACCES`, as a policy of the kernel gives it.
const LISTEN: &[Call] = &[Call { name: "listen", nr: k::SYS_LISTEN, errno: EACCES, when: &[] }];

const UDP: &[Call] =
    &[socket("socket(AF_INET, SOCK_DGRAM)", INET_DGRAM), socket("socket(AF_INET6, SOCK_DGRAM)", INET6_DGRAM), IO_URING];

const RAW_CALLS: &[Call] = &[
    socket("socket(AF_INET, SOCK_RAW)", INET_RAW),
    socket("socket(AF_INET6, SOCK_RAW)", INET6_RAW),
    socket("socket(AF_PACKET)", PACKET),
    IO_URING,
];

const NAMESPACES: u32 = pod_confine::deny::NAMESPACE_FLAGS;

/// The calls of a seccomp pod: the namespaces, the mounts, `chroot` and
/// `ptrace`.
const NNP: &[Call] = &[
    Call {
        name: "unshare",
        nr: k::SYS_UNSHARE,
        errno: EPERM,
        when: &[When::Bits { arg: 0, mask: NAMESPACES | pod_confine::deny::CLONE_NEWTIME }],
    },
    Call {
        name: "clone",
        nr: k::SYS_CLONE,
        errno: EPERM,
        when: &[When::Bits { arg: k::CLONE_FLAGS_ARG as u8, mask: NAMESPACES }],
    },
    Call { name: "clone3", nr: k::SYS_CLONE3, errno: ENOSYS, when: &[] },
    Call { name: "setns", nr: k::SYS_SETNS, errno: EPERM, when: &[] },
    Call { name: "mount", nr: k::SYS_MOUNT, errno: EPERM, when: &[] },
    Call { name: "umount2", nr: k::SYS_UMOUNT2, errno: EPERM, when: &[] },
    Call { name: "pivot_root", nr: k::SYS_PIVOT_ROOT, errno: EPERM, when: &[] },
    Call { name: "fsopen", nr: k::SYS_FSOPEN, errno: EPERM, when: &[] },
    Call { name: "fsmount", nr: k::SYS_FSMOUNT, errno: EPERM, when: &[] },
    Call { name: "move_mount", nr: k::SYS_MOVE_MOUNT, errno: EPERM, when: &[] },
    Call { name: "open_tree", nr: k::SYS_OPEN_TREE, errno: EPERM, when: &[] },
    Call { name: "chroot", nr: k::SYS_CHROOT, errno: EPERM, when: &[] },
    Call { name: "ptrace", nr: k::SYS_PTRACE, errno: EPERM, when: &[] },
];

pub(super) const BIND_ALLOWED: Part = Part {
    name: "bind-allowed",
    about: "a TCP bind of 127.0.0.1:0 succeeds",
    observe: &[row("bind(127.0.0.1:0)", Expect::Allowed)],
    hold: Hold::Host,
};

pub(super) const LISTEN_REFUSED: Part = Part {
    name: "listen-refused",
    about: "listen after a bind gives EACCES or EPERM",
    // The row of the bind alone and the row of a listen after it: a row of
    // both steps cannot tell a refused bind from a refused listen.
    observe: &[row("listen after bind(127.0.0.1:0)", Expect::Refused(&[EACCES, EPERM]))],
    hold: Hold::Seccomp(LISTEN),
};

pub(crate) const UDP_REFUSED: Part = Part {
    name: "udp-refused",
    about: "a UDP socket of AF_INET and AF_INET6 gives EPERM",
    // A host with no IPv6 refuses the socket of the family: no UDP of it
    // can pass.
    observe: &[
        row("socket(AF_INET, SOCK_DGRAM)+connect", Expect::Refused(&[EPERM, EACCES])),
        row("socket(AF_INET6, SOCK_DGRAM)+connect", Expect::Refused(&[EPERM, EACCES, EAFNOSUPPORT])),
    ],
    hold: Hold::Seccomp(UDP),
};

pub(super) const RAW_REFUSED: Part = Part {
    name: "raw-refused",
    about: "a raw socket and a packet socket give EPERM",
    // A kernel with no packet family refuses the socket with EAFNOSUPPORT.
    observe: &[
        row("socket(AF_INET, SOCK_RAW)", Expect::Refused(&[EPERM])),
        row("socket(AF_PACKET, SOCK_RAW)", Expect::Refused(&[EPERM, EAFNOSUPPORT])),
    ],
    hold: Hold::Seccomp(RAW_CALLS),
};

pub(super) const PTMX_ABSENT: Part = Part {
    name: "ptmx-absent",
    about: "no /dev/ptmx and no /dev/pts",
    observe: &[
        row("stat(/dev/ptmx)", Expect::Refused(&[ENOENT])),
        row("open(/dev/ptmx, O_RDWR)", Expect::Refused(&[ENOENT])),
        row("stat(/dev/pts)", Expect::Refused(&[ENOENT])),
    ],
    hold: Hold::DevNoPty,
};

pub(super) const NO_CTTY: Part = Part {
    name: "no-ctty",
    about: "the process has no controlling terminal",
    observe: &[row("tty_nr of /proc/self/stat", Expect::Value { key: "tty_nr", value: "0" })],
    hold: Hold::NoCtty,
};

pub(super) const TTY_OPENS: Part = Part {
    name: "tty-opens",
    about: "/dev/tty opens, and a read of it waits",
    observe: &[row("open(/dev/tty, O_NONBLOCK)+poll", Expect::Value { key: "read", value: "waits" })],
    hold: Hold::DevNoPty,
};

pub(super) const NO_ID_LINES: Part = Part {
    name: "no-id-lines",
    about: "/etc/passwd has no line of the uid, and /etc/group none of the gid",
    observe: &[
        row("line of getuid() in /etc/passwd", Expect::Value { key: "lines", value: "0" }),
        row("line of getgid() in /etc/group", Expect::Value { key: "lines", value: "0" }),
    ],
    hold: Hold::NoPasswdEntry,
};

pub(super) const HOME_NOEXEC: Part = Part {
    name: "home-noexec",
    about: "HOME is writable and on a noexec mount",
    observe: &[row("creat($HOME/.pod-hostprobe)", Expect::Value { key: "noexec", value: "1" })],
    hold: Hold::NoexecHome,
};

pub(super) const TMP_EXEC: Part = Part {
    name: "tmp-exec",
    about: "a file in /tmp runs",
    observe: &[row("exec(/tmp/execprobe)", Expect::Allowed)],
    hold: Hold::Host,
};

pub(super) const PROC_ABSENT: Part = Part {
    name: "proc-absent",
    about: "no /proc: the path of the running binary is not known",
    // The row runs in the probe process: a host with no /proc starts no
    // probe child.
    observe: &[row("readlink(/proc/self/exe)", Expect::Refused(&[ENOENT]))],
    hold: Hold::NoProc,
};

pub(super) const NNP_SET: Part = Part {
    name: "no-new-privs",
    about: "NoNewPrivs is 1",
    observe: &[Observe::Identity(IdFact::NoNewPrivs)],
    hold: Hold::NoNewPrivs,
};

pub(super) const NO_CAPS: Part = Part {
    name: "no-caps",
    about: "no effective capability, and mknod of a device fails",
    observe: &[Observe::Identity(IdFact::NoCaps), row("mknod(chr 1:3 /tmp/nodprobe)", Expect::Refused(&[EPERM]))],
    hold: Hold::DropCaps,
};

pub(super) const CALLS_REFUSED: Part = Part {
    name: "calls-refused",
    about: "a seccomp filter refuses unshare, mount, chroot and ptrace",
    observe: &[
        Observe::Identity(IdFact::SeccompFilter),
        row("unshare(CLONE_NEWUSER)", Expect::Refused(&[EPERM])),
        row("unshare(CLONE_NEWNS)", Expect::Refused(&[EPERM])),
        row("mount(2) bogus target", Expect::Refused(&[EPERM])),
        row("chroot(/tmp)", Expect::Refused(&[EPERM])),
        row("ptrace(PTRACE_TRACEME)", Expect::Refused(&[EPERM])),
    ],
    hold: Hold::Seccomp(NNP),
};

pub(super) const TUN_ABSENT: Part = Part {
    name: "tun-absent",
    about: "/dev/net/tun does not open",
    observe: &[row("open(/dev/net/tun, O_RDWR)", Expect::Refused(&[ENOENT, EACCES]))],
    hold: Hold::Hide { path: "/dev/net/tun", rights: READ_FILE | WRITE_FILE },
};

pub(super) const NO_NET_ADMIN: Part = Part {
    name: "no-net-admin",
    about: "no CAP_NET_ADMIN",
    observe: &[Observe::Identity(IdFact::NoNetAdmin)],
    hold: Hold::DropCaps,
};

pub(super) const ETC_SSL_READ_ONLY: Part = Part {
    name: "etc-ssl-read-only",
    about: "/etc/ssl is on a read-only mount",
    observe: &[row("statfs(/etc/ssl)", Expect::Value { key: "rdonly", value: "1" })],
    hold: Hold::ReadOnly { path: "/etc/ssl" },
};

pub(super) const LOOPBACK_ABSENT: Part = Part {
    name: "loopback-absent",
    about: "the loopback is down: a connect to 127.0.0.1 fails",
    // A fresh network namespace still takes a bind of 127.0.0.1: no row of
    // a bind shows the class.
    observe: &[row("connect(127.0.0.1:1)", Expect::Refused(&[ENETUNREACH]))],
    hold: Hold::NetNs,
};

/// `unshare` and `clone` with `CLONE_NEWNET` (`clone3` gets `ENOSYS`, as
/// in [`NNP`]).
const NETNS: u32 = k::CLONE_NEWNET as u32;
const NETNS_OFF: &[Call] = &[
    Call {
        name: "unshare(CLONE_NEWNET)",
        nr: k::SYS_UNSHARE,
        errno: EPERM,
        when: &[When::Bits { arg: 0, mask: NETNS }],
    },
    Call {
        name: "clone(CLONE_NEWNET)",
        nr: k::SYS_CLONE,
        errno: EPERM,
        when: &[When::Bits { arg: k::CLONE_FLAGS_ARG as u8, mask: NETNS }],
    },
    Call { name: "clone3", nr: k::SYS_CLONE3, errno: ENOSYS, when: &[] },
];

const VM_WRITE: &[Call] = &[Call { name: "process_vm_writev", nr: k::SYS_PROCESS_VM_WRITEV, errno: EPERM, when: &[] }];

pub(super) const TCP_BIND_ONLY_REFUSED: Part = Part {
    name: "tcp-bind-refused",
    about: "a TCP bind of each family gives EACCES; a TCP connect passes",
    observe: &[
        row("bind(127.0.0.1:0)", Expect::Refused(&[EACCES])),
        Observe::Lab(LabFact::Inet6BindRefused),
        // The row reads ECONNREFUSED (nothing listens) as a connect that
        // reached the kernel.
        row("connect(127.0.0.1:1)", Expect::Allowed),
    ],
    hold: Hold::LandlockBind,
};

pub(super) const NETNS_REFUSED: Part = Part {
    name: "netns-refused",
    about: "no network namespace: clone with CLONE_NEWUSER and CLONE_NEWNET fails",
    observe: &[row("clone(CLONE_NEWUSER|NEWNET)", Expect::Refused(&[EPERM, ENOSPC]))],
    hold: Hold::Seccomp(NETNS_OFF),
};

pub(super) const VM_WRITE_REFUSED: Part = Part {
    name: "vm-write-refused",
    about: "process_vm_writev gives EPERM, also to the own process",
    observe: &[row("process_vm_writev(own pid)", Expect::Refused(&[EPERM]))],
    hold: Hold::Seccomp(VM_WRITE),
};

pub(super) const UNIX_ALLOWED: Part = Part {
    name: "unix-allowed",
    about: "socket, bind and connect of AF_UNIX succeed",
    observe: &[Observe::Lab(LabFact::UnixAllowed)],
    hold: Hold::Host,
};
