//! Landlock: the ABI of the kernel, a ruleset, its rules, and the
//! restriction of this thread; `connect(2)`, the call that a port rule
//! checks; and the values of `socket(2)` that a deny rule tests. Each
//! wrapper is one system call on a struct of the caller, so a child between
//! its fork and its exec can call each one. The layouts, the rights and the
//! scopes come from `linux_raw_sys::landlock`.

use super::*;

pub use linux_raw_sys::landlock::{
    LANDLOCK_ACCESS_FS_EXECUTE, LANDLOCK_ACCESS_FS_IOCTL_DEV, LANDLOCK_ACCESS_FS_MAKE_BLOCK,
    LANDLOCK_ACCESS_FS_MAKE_CHAR, LANDLOCK_ACCESS_FS_MAKE_DIR, LANDLOCK_ACCESS_FS_MAKE_FIFO,
    LANDLOCK_ACCESS_FS_MAKE_REG, LANDLOCK_ACCESS_FS_MAKE_SOCK, LANDLOCK_ACCESS_FS_MAKE_SYM,
    LANDLOCK_ACCESS_FS_READ_DIR, LANDLOCK_ACCESS_FS_READ_FILE, LANDLOCK_ACCESS_FS_REFER, LANDLOCK_ACCESS_FS_REMOVE_DIR,
    LANDLOCK_ACCESS_FS_REMOVE_FILE, LANDLOCK_ACCESS_FS_TRUNCATE, LANDLOCK_ACCESS_FS_WRITE_FILE,
    LANDLOCK_ACCESS_NET_BIND_TCP, LANDLOCK_ACCESS_NET_CONNECT_TCP, LANDLOCK_CREATE_RULESET_VERSION,
    LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET, LANDLOCK_SCOPE_SIGNAL,
};
pub use linux_raw_sys::landlock::{
    landlock_net_port_attr as LandlockNetPortAttr, landlock_path_beneath_attr as LandlockPathBeneathAttr,
    landlock_ruleset_attr as LandlockRulesetAttr,
};

use linux_raw_sys::landlock::landlock_rule_type;

/// `landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)`: the
/// ABI of the kernel (1 and up). `ENOSYS` or `EOPNOTSUPP`: no Landlock.
pub fn landlock_abi() -> Sysres {
    // SAFETY: no pointer; the call reads nothing of this process.
    unsafe { sys(SYS_LANDLOCK_CREATE_RULESET, [0, 0, u64::from(LANDLOCK_CREATE_RULESET_VERSION), 0, 0, 0]) }
}

/// `landlock_create_ruleset(attr, size, 0)`: a ruleset descriptor, which
/// the kernel makes close-on-exec. The size is the one of this struct; a
/// kernel with a shorter struct takes it when the extra fields are zero.
pub fn landlock_create_ruleset(attr: &LandlockRulesetAttr) -> Sysres {
    let size = core::mem::size_of::<LandlockRulesetAttr>() as u64;
    // SAFETY: `attr` lives until the call returns; the kernel reads `size`
    // bytes of it.
    unsafe { sys(SYS_LANDLOCK_CREATE_RULESET, [attr as *const LandlockRulesetAttr as u64, size, 0, 0, 0, 0]) }
}

/// `landlock_add_rule(ruleset, LANDLOCK_RULE_PATH_BENEATH, rule, 0)`.
pub fn landlock_add_path_rule(ruleset: i64, rule: &LandlockPathBeneathAttr) -> Sysres {
    let kind = landlock_rule_type::LANDLOCK_RULE_PATH_BENEATH as u64;
    // SAFETY: `rule` lives until the call returns; the kernel reads it.
    unsafe {
        sys(SYS_LANDLOCK_ADD_RULE, [ruleset as u64, kind, rule as *const LandlockPathBeneathAttr as u64, 0, 0, 0])
    }
}

/// `landlock_add_rule(ruleset, LANDLOCK_RULE_NET_PORT, rule, 0)` (ABI 4).
pub fn landlock_add_port_rule(ruleset: i64, rule: &LandlockNetPortAttr) -> Sysres {
    let kind = landlock_rule_type::LANDLOCK_RULE_NET_PORT as u64;
    // SAFETY: `rule` lives until the call returns; the kernel reads it.
    unsafe { sys(SYS_LANDLOCK_ADD_RULE, [ruleset as u64, kind, rule as *const LandlockNetPortAttr as u64, 0, 0, 0]) }
}

/// `landlock_restrict_self(ruleset, 0)`: this thread and each child that it
/// makes later are in the domain of the ruleset. It cannot be undone. An
/// unprivileged thread needs `no_new_privs` first.
pub fn landlock_restrict_self(ruleset: i64) -> Sysres {
    // SAFETY: no pointer.
    unsafe { sys(SYS_LANDLOCK_RESTRICT_SELF, [ruleset as u64, 0, 0, 0, 0, 0]) }
}

/// `connect(2)` of a socket to the address of the caller.
pub fn connect(fd: i64, addr: &[u8]) -> Sysres {
    // SAFETY: the kernel reads `addr.len()` bytes of `addr`, which lives
    // until the call returns.
    unsafe { sys(SYS_CONNECT, [fd as u64, addr.as_ptr() as u64, addr.len() as u64, 0, 0, 0]) }
}

// The values of `socket(2)` that a deny rule of a confinement tests. They
// come from `linux_raw_sys::net`: the type numbers are not the same on each
// architecture (mips).
pub const AF_UNIX: u64 = linux_raw_sys::net::AF_UNIX as u64;
pub const AF_INET6: u64 = linux_raw_sys::net::AF_INET6 as u64;
pub const SOCK_DGRAM: u64 = linux_raw_sys::net::SOCK_DGRAM as u64;
/// The bits of the type in the second argument of `socket(2)`
/// (`SOCK_TYPE_MASK` of `linux/net.h`); `SOCK_NONBLOCK` and `SOCK_CLOEXEC`
/// are flags above them.
pub const SOCK_TYPE_MASK: u64 = 0xf;
/// `IPPROTO_MPTCP`: a TCP stream over subflows that Landlock does not see.
pub const IPPROTO_MPTCP: u64 = linux_raw_sys::net::IPPROTO_MPTCP as u64;

/// The `struct sockaddr_in` of an IPv4 address and port: the family in the
/// byte order of the target, the port and the address in network order.
pub fn sockaddr_in(ip: [u8; 4], port: u16) -> [u8; 16] {
    let mut a = [0u8; 16];
    a[0..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
    a[2..4].copy_from_slice(&port.to_be_bytes());
    a[4..8].copy_from_slice(&ip);
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    // The layouts that the kernel reads: a path rule is packed (12 bytes),
    // a port rule is two u64, and the ruleset of ABI 6 has three u64.
    #[test]
    fn the_rules_have_the_layouts_of_the_kernel() {
        assert_eq!(core::mem::size_of::<LandlockPathBeneathAttr>(), 12);
        assert_eq!(core::mem::size_of::<LandlockNetPortAttr>(), 16);
        assert_eq!(core::mem::size_of::<LandlockRulesetAttr>(), 24);
        assert_eq!(LANDLOCK_CREATE_RULESET_VERSION, 1);
        assert_eq!(core::mem::offset_of!(LandlockRulesetAttr, scoped), 16);
        assert_eq!((LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET, LANDLOCK_SCOPE_SIGNAL), (1, 2));
    }

    // The values of linux/socket.h, linux/net.h and linux/in.h on each
    // target of docs/design.md, section 7.
    #[test]
    fn the_socket_values_are_the_ones_of_the_kernel() {
        assert_eq!((AF_UNIX, AF_INET, AF_INET6), (1, 2, 10));
        assert_eq!((SOCK_STREAM, SOCK_DGRAM, SOCK_TYPE_MASK), (1, 2, 0xf));
        assert_eq!(SOCK_CLOEXEC & SOCK_TYPE_MASK, 0);
        assert_eq!(IPPROTO_MPTCP, 262);
    }

    #[test]
    fn a_sockaddr_has_the_port_and_the_address_in_network_order() {
        let a = sockaddr_in([127, 0, 0, 1], 0x1234);
        assert_eq!(u16::from_ne_bytes([a[0], a[1]]), 2);
        assert_eq!(&a[2..8], &[0x12, 0x34, 127, 0, 0, 1]);
    }
}
