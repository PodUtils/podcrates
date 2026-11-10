use super::*;
use crate::testing::{Scratch, no_in_root};

/// The write does not reach the canary outside the root.
#[test]
fn mtab_replaces_an_escaping_symlink_and_the_canary_survives() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("net-mtab");
    let outside = Scratch::new("net-mtab-canary");
    let canary = outside.at("canary");
    std::fs::write(&canary, b"host\n").unwrap();
    std::os::unix::fs::symlink(&canary, d.at("etc/mtab")).unwrap();
    let fs = run_mtab(&d.root(), Path::new("/proc/self/mounts")).unwrap();
    assert_eq!(fs[0].action, Action::Rewrote);
    assert!(fs[0].detail.contains("REPLACED the link"), "{:?}", fs[0]);
    assert_eq!(std::fs::read(&canary).unwrap(), b"host\n");
    let got = std::fs::read_to_string(d.at("etc/mtab")).unwrap();
    assert!(got.contains(" / "), "{got}");
}

/// An image that ships a working file keeps it.
#[test]
fn mtab_leaves_a_regular_file_alone() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("net-mtabreg");
    std::fs::write(d.at("etc/mtab"), b"mine\n").unwrap();
    let fs = run_mtab(&d.root(), Path::new("/proc/self/mounts")).unwrap();
    assert_eq!(fs[0].action, Action::Unchanged);
    assert_eq!(std::fs::read(d.at("etc/mtab")).unwrap(), b"mine\n");
}

/// An IPv6 address of an added host holds colons: a split at the last
/// one loses the address.
#[test]
fn add_host_splits_on_the_first_colon() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("net-hosts");
    let settings = Settings {
        container_name: Some("c1".into()),
        add_hosts: vec!["six:2001:db8::1".into(), "four:10.0.0.1".into()],
        ..Settings::default()
    };
    run_hosts(&d.root(), &settings).unwrap();
    let got = std::fs::read_to_string(d.at("etc/hosts")).unwrap();
    assert!(got.contains("2001:db8::1\tsix\n"), "{got}");
    assert!(got.contains("10.0.0.1\tfour\n"), "{got}");
    assert!(got.contains("127.0.0.1\tlocalhost\tc1\n"), "{got}");
}

#[test]
fn a_malformed_add_host_is_refused_by_name() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("net-badhost");
    let settings = Settings { add_hosts: vec!["nocolon".into()], ..Settings::default() };
    let e = run_hosts(&d.root(), &settings).unwrap_err();
    assert!(format!("{e}").contains("nocolon"), "{e}");
    assert_eq!(e.class, pod_core::Class::Usage);
}

/// A line end or a space in an added host or in the container name writes
/// a line that the caller did not give. podvm's code wrote
/// `6.6.6.6 bank.example` from `evil\n6.6.6.6 bank.example:10.0.0.1`.
#[test]
fn a_line_end_in_an_added_host_or_a_name_is_refused() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("net-inject");
    let root = d.root();
    for settings in [
        Settings { add_hosts: vec!["evil\n6.6.6.6 bank.example:10.0.0.1".into()], ..Settings::default() },
        Settings { add_hosts: vec!["name:10.0.0.1\n6.6.6.6\tbank.example".into()], ..Settings::default() },
        Settings { add_hosts: vec!["two words:10.0.0.1".into()], ..Settings::default() },
        Settings { container_name: Some("c1\n6.6.6.6 bank.example".into()), ..Settings::default() },
    ] {
        let e = run_hosts(&root, &settings).unwrap_err();
        assert_eq!(e.class, pod_core::Class::Usage, "{e}");
        assert!(!d.at("etc/hosts").exists(), "the file was written for {settings:?}");
    }
}

/// The resolver is written each time. An image whose resolver names the
/// libvirt gateway of a build host looks like a right one.
#[test]
fn the_resolver_is_overwritten_even_when_the_image_has_one() {
    if no_in_root() {
        return;
    }
    let d = Scratch::new("net-resolv");
    std::fs::write(d.at("etc/resolv.conf"), b"nameserver 192.168.122.1\n").unwrap();
    let host = Scratch::new("net-hostresolv");
    std::fs::write(host.at("resolv.conf"), b"nameserver 10.1.2.3\n").unwrap();
    let settings = Settings { host_resolv_conf: host.at("resolv.conf"), ..Settings::default() };
    let fs = run_resolver(&d.root(), &settings).unwrap();
    assert_eq!(fs[0].action, Action::Rewrote);
    assert_eq!(std::fs::read(d.at("etc/resolv.conf")).unwrap(), b"nameserver 10.1.2.3\n");
}

/// The longest mount point that holds the root gives the line of `/`, and
/// a mount under the root keeps its place under `/`.
#[test]
fn the_mount_table_holds_the_file_system_of_the_root() {
    let table = "/dev/sda1 / ext4 rw 0 0\n/dev/sdb1 /store xfs rw,noatime 0 0\ntmpfs /store/r/proc tmpfs rw 0 0\n\
                 tmpfs /elsewhere tmpfs rw 0 0\n";
    let got = topology(table, "/store/r");
    assert_eq!(got, "/dev/sdb1 / xfs rw,noatime 0 0\ntmpfs /proc tmpfs rw 0 0\n");
    assert_eq!(topology("", "/x"), "- / unknown rw 0 0\n");
}

/// The kernel writes a space of a mount point as `\040`. podvm's code
/// compared the escaped field with the path of the root, so a root under a
/// directory with a space got the line of `/`, the wrong file system.
#[test]
fn a_mount_point_with_a_space_holds_the_root() {
    let table =
        "/dev/sda1 / ext4 rw 0 0\n/dev/sdc1 /my\\040store xfs rw 0 0\nproc /my\\040store/r/my\\040proc proc rw 0 0\n";
    let got = topology(table, "/my store/r");
    assert_eq!(got, "/dev/sdc1 / xfs rw 0 0\nproc /my\\040proc proc rw 0 0\n");
}
