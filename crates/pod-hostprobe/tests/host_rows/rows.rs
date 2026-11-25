//! The checks of [`check`]: each row of the classes of podssh and podnet
//! against what this process reads of the host itself.

use pod_hostprobe::child::Spawner;
use pod_hostprobe::probes;
use pod_hostprobe::sys::{self, Errno};
use pod_hostprobe::verdict::{Outcome, Verdict};
use pod_hostprobe::{Settings, identity};

/// The argument of the run again as uid 65534.
pub const AGAIN: &str = "--again-as-nobody";

/// `CAP_NET_RAW` in a capability set.
const CAP_NET_RAW: u64 = 1 << 13;

fn row(spawner: &Spawner, name: &str) -> Outcome {
    let p = probes::find(name).unwrap_or_else(|| panic!("no row {name}"));
    let o = p.measure(spawner);
    println!("host_rows: {}", o.row(name));
    if !o.reason.is_empty() {
        println!("host_rows:     {}", o.reason);
    }
    o
}

fn denied(o: &Outcome, e: Errno, name: &str) {
    assert_eq!((o.verdict, o.errno), (Verdict::Denied, Some(e)), "{name}: {o:?}");
}

/// Set or remove a variable that the next [`Spawner`] gives its children.
fn set(name: &str, value: Option<&str>) {
    // SAFETY: the test binary has one thread (no libtest).
    unsafe {
        match value {
            Some(v) => std::env::set_var(name, v),
            None => std::env::remove_var(name),
        }
    }
}

pub fn check(settings: &Settings, again: bool) {
    let id = identity::read();
    let who = if again { "uid 65534" } else { "this user" };
    println!("host_rows: as {who} (uid {})", id.uid);
    sockets(settings, &id);
    files(settings, &id);
    variables(settings);
    println!("host_rows: as {who}: ok");
}

fn sockets(settings: &Settings, id: &identity::Identity) {
    let s = Spawner::new(settings);
    // The old row binds and listens: where it passes, each step passes.
    if row(&s, "bind(127.0.0.1:0)+listen").verdict == Verdict::Ok {
        assert_eq!(row(&s, "bind(127.0.0.1:0)").verdict, Verdict::Ok);
        assert_eq!(row(&s, "listen after bind(127.0.0.1:0)").verdict, Verdict::Ok);
    }
    if pod_testkit::host::loopback().is_ok() {
        let o = row(&s, "connect(127.0.0.1:1)");
        assert_eq!(o.verdict, Verdict::Ok, "{o:?}");
        assert_eq!(row(&s, "socket(AF_INET, SOCK_DGRAM)+connect").verdict, Verdict::Ok);
    }
    let v6 = row(&s, "socket(AF_INET6, SOCK_DGRAM)+connect");
    assert!(v6.verdict == Verdict::Ok || v6.errno == Some(sys::EAFNOSUPPORT), "{v6:?}");
    let caps = id.cap_eff.as_deref().and_then(|c| u64::from_str_radix(c.trim(), 16).ok()).unwrap_or(0);
    let raw = row(&s, "socket(AF_INET, SOCK_RAW)");
    let packet = row(&s, "socket(AF_PACKET, SOCK_RAW)");
    if caps & CAP_NET_RAW == 0 {
        // No capability: the kernel refuses each one.
        denied(&raw, sys::EPERM, "a raw socket with no CAP_NET_RAW");
        assert!(matches!(packet.errno, Some(sys::EPERM | sys::EAFNOSUPPORT)), "{packet:?}");
    } else if id.seccomp.as_deref() == Some("0") {
        assert_eq!(raw.verdict, Verdict::Ok, "a raw socket with CAP_NET_RAW: {raw:?}");
    }
}

fn files(settings: &Settings, id: &identity::Identity) {
    let s = Spawner::new(settings);
    let pts = row(&s, "stat(/dev/pts)");
    assert_eq!(pts.verdict == Verdict::Ok, std::path::Path::new("/dev/pts").exists(), "{pts:?}");
    assert_eq!(row(&s, "readlink(/proc/self/exe)").verdict, Verdict::Ok);
    // A probe child keeps the terminal of this process.
    let stat = std::fs::read_to_string("/proc/self/stat").expect("/proc/self/stat");
    let tty = pod_core::proc_stat::after_name(&stat).and_then(|f| f.get(4).map(|t| t.to_string())).expect("tty_nr");
    assert_eq!(row(&s, "tty_nr of /proc/self/stat").value("tty_nr"), Some(tty.as_str()));
    let poll = row(&s, "open(/dev/tty, O_NONBLOCK)+poll");
    if tty == "0" {
        denied(&poll, sys::ENXIO, "/dev/tty with no controlling terminal");
    } else {
        assert_eq!(poll.verdict, Verdict::Ok, "{poll:?}");
    }
    for (name, file, key, n) in [
        ("line of getuid() in /etc/passwd", "/etc/passwd", "uid", id.uid),
        ("line of getgid() in /etc/group", "/etc/group", "gid", id.gid),
    ] {
        let o = row(&s, name);
        let text = std::fs::read_to_string(file).unwrap_or_default();
        let lines = text.lines().filter(|l| l.split(':').nth(2) == Some(n.to_string().as_str())).count();
        assert_eq!(o.value(key), Some(n.to_string().as_str()), "{name}");
        assert_eq!(o.value("lines"), Some(lines.to_string().as_str()), "{name}");
    }
    let home = row(&s, "creat($HOME/.pod-hostprobe)");
    if let Ok(dir) = std::env::var("HOME") {
        let path = sys::CBuf::new(&dir).expect("no NUL");
        let writable = sys::faccessat(sys::AT_FDCWD as i64, &path, 2, 0).is_ok();
        if nix_uid() != 0 && !writable {
            denied(&home, sys::EACCES, "a file in a HOME that is not writable");
        } else if writable {
            let flag = sys::statfs(&path).expect("statfs").flag(sys::ST_NOEXEC);
            assert_eq!(home.value("noexec"), Some(flag_text(flag)), "{home:?}");
            assert_eq!(home.verdict, Verdict::Ok, "{home:?}");
        }
    }
    let ssl = row(&s, "statfs(/etc/ssl)");
    match sys::statfs(&sys::CBuf::new("/etc/ssl").expect("no NUL")) {
        Ok(st) => assert_eq!(ssl.value("rdonly"), Some(flag_text(st.flag(sys::ST_RDONLY))), "{ssl:?}"),
        Err(e) => denied(&ssl, e, "statfs(/etc/ssl)"),
    }
}

/// The value of a mount flag in a row: `unknown` where the kernel gave no
/// `ST_VALID` (qemu-user gives none).
fn flag_text(flag: Option<bool>) -> &'static str {
    match flag {
        Some(true) => "1",
        Some(false) => "0",
        None => "unknown",
    }
}

fn nix_uid() -> i64 {
    // SAFETY: no pointer.
    unsafe { sys::syscall6(sys::SYS_GETUID, 0, 0, 0, 0, 0, 0) }
}

/// The variables of a proxy and of the certificate file, as a child
/// inherits them; then the resolver and the routes of this host.
fn variables(settings: &Settings) {
    const PROXY: &str = "getenv(HTTPS_PROXY)";
    const CERT: &str = "getenv(SSL_CERT_FILE)+read";
    let saved: Vec<(&str, Option<String>)> = ["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY", "SSL_CERT_FILE"]
        .into_iter()
        .map(|v| (v, std::env::var(v).ok()))
        .collect();
    saved.iter().for_each(|(v, _)| set(v, None));
    let none = Spawner::new(settings);
    assert_eq!(row(&none, PROXY).reason, "set 0");
    assert_eq!(row(&none, CERT).reason, "set 0");

    let pem = std::env::temp_dir().join(format!("pod-hostprobe-rows-{}.pem", std::process::id()));
    let block = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";
    std::fs::write(&pem, block.repeat(2)).expect("a certificate file");
    set("HTTPS_PROXY", Some("http://127.0.0.1:3128"));
    set("SSL_CERT_FILE", pem.to_str());
    let some = Spawner::new(settings);
    let proxy = row(&some, PROXY);
    assert_eq!(proxy.reason, "set 1; variable HTTPS_PROXY; scheme http; credential 0; port 3128");
    assert_eq!(row(&some, CERT).reason, "set 1; certificates 2");
    std::fs::remove_file(&pem).expect("the removal of the certificate file");
    let gone = Spawner::new(settings);
    denied(&row(&gone, CERT), sys::ENOENT, "a certificate file that is not there");
    for (v, value) in &saved {
        set(v, value.as_deref());
    }

    let s = Spawner::new(settings);
    let resolver = row(&s, "read(/etc/resolv.conf)");
    let text = std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default();
    let n = text.lines().filter(|l| l.split_whitespace().next() == Some("nameserver")).count();
    assert_eq!(resolver.value("nameservers"), Some(n.to_string().as_str()), "{resolver:?}");
    assert_eq!(resolver.value("127.0.0.1:53").is_some(), n == 0, "{resolver:?}");
    for name in ["read(/proc/net/route)", "read(/proc/net/ipv6_route)"] {
        let o = row(&s, name);
        assert!(matches!(o.value("default"), Some("0" | "1")), "{name}: {o:?}");
    }
}
