use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::*;
use crate::spec::FILE_RIGHTS;
use crate::{Access, Call, Refusal};

/// The ABI of Linux 6.18, the host of the ask.
const ABI: u32 = 7;

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pod-confine-compile-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn text(p: &Path) -> String {
    p.to_str().unwrap().to_string()
}

/// The (device, inode) of the file of a descriptor.
fn file_of(fd: i64) -> (u64, u64) {
    let st = k::fstatat(fd, &k::cempty(), k::AT_EMPTY_PATH).unwrap();
    (st.st_dev, st.st_ino)
}

fn file_at(p: &Path) -> (u64, u64) {
    let m = std::fs::metadata(p).unwrap();
    (m.dev(), m.ino())
}

/// The ruleset handles each file right of the ABI, the write rights too:
/// a right that the ruleset does not handle is free for each path.
#[test]
fn the_ruleset_handles_each_right_of_the_abi() {
    let d = dir("handled");
    let c = Spec::new().allow(text(&d), Access::READ).compile(ABI).unwrap();
    assert_eq!(c.abi(), ABI);
    assert_eq!(c.handled_fs(), handled_fs(ABI));
    for right in [
        k::LANDLOCK_ACCESS_FS_WRITE_FILE,
        k::LANDLOCK_ACCESS_FS_MAKE_REG,
        k::LANDLOCK_ACCESS_FS_MAKE_DIR,
        k::LANDLOCK_ACCESS_FS_REMOVE_FILE,
        k::LANDLOCK_ACCESS_FS_TRUNCATE,
        k::LANDLOCK_ACCESS_FS_READ_FILE,
        k::LANDLOCK_ACCESS_FS_EXECUTE,
    ] {
        assert_ne!(c.handled_fs() & u64::from(right), 0, "the ruleset does not handle the right {right:#x}");
    }
    assert_eq!(c.handled_net(), 0, "TCP stays free with no port list");
    assert_eq!(c.rules().len(), 1);
    assert_eq!(c.rules()[0].allowed(), Access::READ.rights());
    assert_eq!(c.rules()[0].path(), text(&d));
    assert_eq!(file_of(c.rules()[0].fd()), file_at(&d));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_path_that_does_not_exist_is_refused_by_name() {
    let d = dir("missing");
    let gone = d.join("gone");
    let e = Spec::new().allow(text(&d), Access::READ).allow(text(&gone), Access::WRITE).compile(ABI).unwrap_err();
    let t = e.to_string();
    assert!(t.contains(&text(&gone)) && t.contains("does not exist") && t.contains("ENOENT"), "{t}");
    assert_eq!(e.class, Class::NotFound);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_relative_path_and_an_empty_access_are_refused() {
    let e = Spec::new().allow("usr", Access::READ).compile(ABI).unwrap_err();
    assert!(e.to_string().contains("not absolute"), "{e}");
    let e = Spec::new().allow("/", Access::NONE).compile(ABI).unwrap_err();
    assert!(e.to_string().contains("no access"), "{e}");
    let e = Spec::new().allow("/a\0b", Access::READ).compile(ABI).unwrap_err();
    assert!(e.to_string().contains("NUL"), "{e}");
}

/// No Landlock, or an ABI with no confinement of `truncate(2)`: no
/// compile, so the payload never runs unconfined in part.
#[test]
fn no_landlock_and_an_abi_under_3_are_refused() {
    for abi in [0, 1, 2] {
        let e = Spec::new().compile(abi).unwrap_err();
        assert_eq!(e.class, Class::Unsupported, "ABI {abi}");
        assert!(e.to_string().contains("unconfined"), "{e}");
    }
    assert!(Spec::new().compile(MIN_ABI).is_ok());
}

#[test]
fn tcp_rules_need_abi_4() {
    let spec = Spec::new().connect_to(443).bind_to(8080);
    let e = spec.compile(3).unwrap_err();
    assert!(e.to_string().contains("ABI 4"), "{e}");
    let c = spec.compile(4).unwrap();
    assert_eq!(c.handled_net(), CONNECT_TCP | BIND_TCP);
    assert_eq!(c.ports(), &[(443, CONNECT_TCP), (8080, BIND_TCP)]);
    // An empty list handles the call and allows no port.
    let c = Spec { connect: Some(Vec::new()), ..Spec::new() }.compile(4).unwrap();
    assert_eq!(c.handled_net(), CONNECT_TCP);
    assert!(c.ports().is_empty());
}

/// A rule on a file keeps the rights of a file only: the kernel refuses a
/// right of a directory on a file.
#[test]
fn a_file_rule_keeps_the_rights_of_a_file() {
    let d = dir("file");
    let f = d.join("f");
    std::fs::write(&f, b"x").unwrap();
    let c = Spec::new().allow(text(&f), Access::READ | Access::WRITE | Access::EXECUTE).compile(ABI).unwrap();
    let allowed = c.rules()[0].allowed();
    assert_eq!(allowed & !FILE_RIGHTS, 0, "a directory right on a file: {allowed:#x}");
    for right in [
        k::LANDLOCK_ACCESS_FS_READ_FILE,
        k::LANDLOCK_ACCESS_FS_WRITE_FILE,
        k::LANDLOCK_ACCESS_FS_EXECUTE,
        k::LANDLOCK_ACCESS_FS_TRUNCATE,
    ] {
        assert_ne!(allowed & u64::from(right), 0, "{right:#x}");
    }
    // At ABI 3 no right of ABI 5 goes into a rule.
    let c = Spec::new().allow(text(&f), Access::WRITE).compile(3).unwrap();
    assert_eq!(c.rules()[0].allowed() & u64::from(k::LANDLOCK_ACCESS_FS_IOCTL_DEV), 0);
    let _ = std::fs::remove_dir_all(d);
}

/// A guest path resolves in the root as the payload sees it after the
/// root change: an absolute link and `..` stay in the root, and a path of
/// the host only is not there.
#[test]
fn a_guest_path_resolves_in_the_root() {
    let d = dir("guest");
    std::fs::create_dir_all(d.join("real/etc")).unwrap();
    std::fs::write(d.join("real/etc/conf"), b"x").unwrap();
    std::os::unix::fs::symlink("/real/etc", d.join("etc")).unwrap();
    let root = k::open(&k::CBuf::new(&text(&d)).unwrap(), k::O_RDONLY | k::O_DIRECTORY | k::O_CLOEXEC, 0).unwrap();
    let spec = Spec::new().allow("/etc/conf", Access::READ).allow("/../../etc", Access::READ).allow("/", Access::READ);
    let c = spec.compile_in(ABI, root).unwrap();
    assert_eq!(file_of(c.rules()[0].fd()), file_at(&d.join("real/etc/conf")));
    assert_eq!(file_of(c.rules()[1].fd()), file_at(&d.join("real/etc")));
    assert_eq!(file_of(c.rules()[2].fd()), file_at(&d));
    // The temporary directory of the host is not in the root.
    let host = text(&std::env::temp_dir());
    let e = Spec::new().allow(host.clone(), Access::READ).compile_in(ABI, root).unwrap_err();
    assert!(e.to_string().contains(&host) && e.to_string().contains("does not exist"), "{e}");
    let _ = k::close(root);
    let _ = std::fs::remove_dir_all(d);
}

/// A descriptor on a number that the child takes for a `dup2` moves above
/// it, and keeps its file.
#[test]
fn a_rule_moves_off_a_taken_number_and_keeps_its_file() {
    let d = dir("clear");
    let mut c = Spec::new().allow(text(&d), Access::READ).compile(ABI).unwrap();
    let fd = c.rules()[0].fd();
    c.keep_clear_of(&[0, 1, 2, fd]).unwrap();
    let moved = c.rules()[0].fd();
    assert!(moved > fd, "{moved} <= {fd}");
    assert_eq!(file_of(moved), file_at(&d));
    let flags = k::fcntl(moved, k::F_GETFD, 0).unwrap() as u64;
    assert_ne!(flags & k::FD_CLOEXEC, 0, "a payload would hold the moved descriptor");
    let _ = std::fs::remove_dir_all(d);
}

/// A refusal of the child names the call, the path or the port of its
/// rule, and the errno, and crosses a pipe as five bytes.
#[test]
fn a_refusal_names_the_call_the_rule_and_the_errno() {
    let d = dir("refusal");
    let c = Spec::new().allow(text(&d), Access::READ).connect_to(443).compile(ABI).unwrap();
    let r = Refusal { call: Call::AddPathRule, index: 0, errno: k::EINVAL };
    assert_eq!(Refusal::from_bytes(r.to_bytes()), Some(r));
    let t = r.describe(&c);
    assert_eq!(t, format!("landlock_add_rule(LANDLOCK_RULE_PATH_BENEATH) for the path {:?}: EINVAL (22)", text(&d)));
    let r = Refusal { call: Call::AddPortRule, index: 0, errno: k::EINVAL };
    assert!(r.describe(&c).contains("for the TCP port 443"), "{}", r.describe(&c));
    let r = Refusal { call: Call::CreateRuleset, index: 0, errno: k::ENOSYS };
    assert_eq!(r.failure(&c).class, Class::Unsupported);
    assert!(r.failure(&c).to_string().contains("landlock_create_ruleset: ENOSYS (38)"));
    let r = Refusal { call: Call::OpenPath, index: 0, errno: k::ENOENT };
    assert_eq!(r.describe(&c), format!("open(O_PATH) for the path {:?}: ENOENT (2)", text(&d)));
    for call in [Call::NoNewPrivs, Call::CreateRuleset, Call::AddPathRule, Call::AddPortRule, Call::RestrictSelf] {
        assert_eq!(Call::from_code(call.code()), Some(call));
    }
    assert_eq!(Call::from_code(Call::Seccomp.code()), Some(Call::Seccomp));
    assert_eq!(Call::from_code(Call::OpenPath.code()), Some(Call::OpenPath));
    assert_eq!(Call::from_code(0), None);
    assert_eq!(Call::from_code(12), None);
    let _ = std::fs::remove_dir_all(d);
}

/// After [`Compiled::close_paths`] no rule holds a descriptor;
/// [`Compiled::open_paths`] opens each path again in the view of this
/// process, as `O_PATH` and close-on-exec. A path that went away is a
/// refusal of `open(O_PATH)` with the index of its rule.
#[test]
fn the_paths_close_after_the_compile_and_open_again() {
    let d = dir("again");
    std::fs::create_dir_all(d.join("a")).unwrap();
    std::fs::create_dir_all(d.join("b")).unwrap();
    let spec = Spec::new().allow(text(&d.join("a")), Access::READ).allow(text(&d.join("b")), Access::READ);
    let mut c = spec.compile(ABI).unwrap();
    c.close_paths();
    assert!(c.rules().iter().all(|r| r.fd() == -1), "{:?}", c.rules());
    c.keep_clear_of(&[0, 1, 2]).unwrap();
    assert!(c.rules().iter().all(|r| r.fd() == -1), "a closed rule moved: {:?}", c.rules());
    c.open_paths().unwrap();
    assert_eq!(file_of(c.rules()[0].fd()), file_at(&d.join("a")));
    assert_eq!(file_of(c.rules()[1].fd()), file_at(&d.join("b")));
    // `F_GETFL` of `fcntl(2)`.
    const F_GETFL: u64 = 3;
    for r in c.rules() {
        let flags = k::fcntl(r.fd(), F_GETFL, 0).unwrap() as u64;
        assert_ne!(flags & k::O_PATH, 0, "the rule {:?} is not O_PATH", r.path());
        let fd = k::fcntl(r.fd(), k::F_GETFD, 0).unwrap() as u64;
        assert_ne!(fd & k::FD_CLOEXEC, 0, "a payload would hold the rule {:?}", r.path());
    }
    // A rule with a descriptor keeps it.
    let held = c.rules()[0].fd();
    c.open_paths().unwrap();
    assert_eq!(c.rules()[0].fd(), held);
    // The second path goes away between the compile and the open.
    c.close_paths();
    std::fs::remove_dir(d.join("b")).unwrap();
    let r = c.open_paths().unwrap_err();
    assert_eq!((r.call, r.index, r.errno), (Call::OpenPath, 1, k::ENOENT));
    assert!(r.describe(&c).contains(&format!("for the path {:?}", text(&d.join("b")))), "{}", r.describe(&c));
    let _ = std::fs::remove_dir_all(d);
}

/// The scopes go into the ruleset from ABI 6, where the spec asks for
/// them; under ABI 6 the field gives nothing.
#[test]
fn the_scopes_need_abi_6() {
    assert_eq!(Spec::new().compile(SCOPE_ABI).unwrap().scoped(), SCOPES);
    assert_eq!(Spec::new().compile(ABI).unwrap().scoped(), SCOPES);
    assert_eq!(Spec::new().compile(SCOPE_ABI - 1).unwrap().scoped(), 0);
    assert_eq!(Spec::new().no_scope().compile(ABI).unwrap().scoped(), 0);
}

/// ABI 9 handles the lookup of a Unix socket of a path, and a rule with
/// write gives it, on a directory and on a file; a rule with read only does
/// not. ABI 10 handles the UDP rights where the spec closes UDP, with no
/// port rule for them; a free UDP leaves them free.
#[test]
fn the_rights_of_abi_9_and_10_follow_write_and_a_closed_udp() {
    let unix = u64::from(k::LANDLOCK_ACCESS_FS_RESOLVE_UNIX);
    assert_eq!(handled_fs(crate::UNIX_ABI - 1) & unix, 0);
    assert_eq!(handled_fs(crate::UNIX_ABI) & unix, unix);
    let d = dir("abi9");
    let f = d.join("sock");
    std::fs::write(&f, b"").unwrap();
    let spec = Spec::new().allow(text(&d), Access::WRITE).allow(text(&f), Access::WRITE).allow("/", Access::READ);
    let c = spec.clone().compile(crate::UNIX_ABI).unwrap();
    let allowed: Vec<u64> = c.rules().iter().map(|r| r.allowed() & unix).collect();
    assert_eq!(allowed, [unix, unix, 0], "write on a directory, write on a file, read");
    assert_eq!(spec.compile(ABI).unwrap().rules()[0].allowed() & unix, 0, "no right of ABI 9 at ABI {ABI}");
    let udp = crate::spec::UDP_RIGHTS;
    assert_eq!(udp, 0b1100);
    assert_eq!(Spec::new().close_udp().compile(crate::UDP_ABI).unwrap().handled_net(), udp);
    assert_eq!(Spec::new().close_udp().compile(crate::UDP_ABI - 1).unwrap().handled_net(), 0);
    let c = Spec::new().close_udp().connect_to(443).compile(crate::UDP_ABI).unwrap();
    assert_eq!(c.handled_net(), udp | CONNECT_TCP);
    assert_eq!(c.ports(), [(443, CONNECT_TCP)], "no port rule gives a UDP right");
    assert_eq!(Spec::new().connect_to(443).compile(crate::UDP_ABI).unwrap().handled_net(), CONNECT_TCP);
    let _ = std::fs::remove_dir_all(d);
}

fn has(rules: &[Deny], name: &str) -> bool {
    rules.iter().any(|d| d.name == name)
}

/// A closed UDP adds the UDP rules and the io_uring calls, also to an empty
/// deny list; a TCP list adds the MPTCP rules and the io_uring calls; each
/// adds the family rules; a free spec adds nothing.
#[test]
fn the_switches_of_the_spec_add_their_rules() {
    let empty = Spec { deny: Vec::new(), ..Spec::new() };
    assert_eq!(empty.deny_rules_on(false).unwrap(), Vec::new());
    let rules = empty.clone().close_udp().deny_rules_on(false).unwrap();
    assert!(has(&rules, "socket(AF_INET, SOCK_DGRAM)") && has(&rules, "socket(AF_INET6, SOCK_DGRAM)"), "{rules:?}");
    assert!(has(&rules, "io_uring_setup") && has(&rules, "io_uring_enter"), "{rules:?}");
    assert!(!has(&rules, "socket(AF_INET, IPPROTO_MPTCP)"), "{rules:?}");
    let rules = empty.clone().bind_to(80).deny_rules_on(false).unwrap();
    assert!(has(&rules, "socket(AF_INET6, IPPROTO_MPTCP)") && has(&rules, "io_uring_setup"), "{rules:?}");
    assert!(!has(&rules, "socket(AF_INET, SOCK_DGRAM)"), "{rules:?}");
    let families = ["socket(another family)", "socketpair(another family)", "socket(AF_INET6, another type)"];
    assert!(families.iter().all(|name| has(&rules, name)), "{rules:?}");
    let mptcp = rules.iter().position(|d| d.name == "socket(AF_INET, IPPROTO_MPTCP)").unwrap();
    let stream = rules.iter().position(|d| d.name == "socket(AF_INET, SOCK_STREAM, another protocol)").unwrap();
    assert!(mptcp < stream, "the MPTCP rule keeps its errno: {rules:?}");
    assert!(!has(&Spec::new().deny_rules_on(false).unwrap(), "socket(another family)"), "a free spec");
    // The list of the spec comes first: its rules win.
    let rules = Spec::new().close_udp().deny_rules_on(false).unwrap();
    assert_eq!(rules[..Deny::defaults().len()], Deny::defaults()[..]);
    let c = Spec::new().close_udp().connect_to(443).compile(ABI).unwrap();
    assert!(c.filter().len() > Spec::new().compile(ABI).unwrap().filter().len());
}

/// On a target with socketcall(2) a closed UDP is a refusal by name, not a
/// rule that a payload passes through socketcall.
#[test]
fn a_closed_udp_is_refused_where_the_kernel_has_socketcall() {
    let e = Spec::new().close_udp().deny_rules_on(true).unwrap_err();
    assert_eq!(e.class, Class::Unsupported);
    assert!(e.to_string().contains("socketcall(2)") && e.to_string().contains("UDP"), "{e}");
    assert!(Spec::new().deny_rules_on(true).is_ok(), "a free UDP is no refusal");
    let here = Spec::new().close_udp().compile(ABI);
    let socketcall = cfg!(any(target_arch = "x86", target_arch = "s390x", target_arch = "powerpc64"));
    assert_eq!(here.is_err(), socketcall, "{here:?}");
}

/// On a target with socketcall(2) a TCP list is a refusal by name too: the
/// MPTCP rule and the rules of the families do not see a socket made
/// through it (PodUtils/podvm#276).
#[test]
fn a_tcp_list_is_refused_where_the_kernel_has_socketcall() {
    for spec in [Spec::new().connect_to(443), Spec::new().bind_to(8080)] {
        let e = spec.deny_rules_on(true).unwrap_err();
        assert_eq!(e.class, Class::Unsupported);
        assert!(e.to_string().contains("socketcall(2)") && e.to_string().contains("TCP list"), "{e}");
        assert!(spec.deny_rules_on(false).is_ok(), "no socketcall: the rules");
    }
    let here = Spec::new().connect_to(443).compile(ABI);
    let socketcall = cfg!(any(target_arch = "x86", target_arch = "s390x", target_arch = "powerpc64"));
    assert_eq!(here.is_err(), socketcall, "{here:?}");
}
