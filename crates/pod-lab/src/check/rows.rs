//! The check of made-up reports of each part that a row of the socket, file
//! and network classes sees: each through the JSON of `pod_hostprobe::report`
//! where the feature `report` is on.

use pod_hostprobe::sys::{EACCES, EAFNOSUPPORT, ECONNREFUSED, ENETUNREACH, ENOENT, ENXIO, EPERM};
use pod_hostprobe::verdict::Outcome;

use super::*;
use crate::table::{PROFILES, find};

/// The verdict of `name` against made-up rows, through a report.
fn verdict(name: &str, rows: &[(&'static str, Outcome)]) -> Verdict {
    let p = find(PROFILES, name).expect("a profile of the table");
    let mut f = Findings::empty();
    f.rows = rows.to_vec();
    #[cfg(feature = "report")]
    {
        use pod_hostprobe::select::Selection;
        let text = pod_hostprobe::report::write(&f, &Selection::choose(&f));
        let v = check_report(p, &text).expect("the report reads");
        assert_eq!(v, check(p, &f), "the report and the findings disagree");
        v
    }
    #[cfg(not(feature = "report"))]
    check(p, &f)
}

fn state(v: &Verdict, part: &str) -> State {
    v.parts.iter().find(|p| p.part == part).unwrap_or_else(|| panic!("no part {part}: {v:?}")).state
}

fn ok(reason: &str) -> Outcome {
    Outcome::ok_with(reason)
}

const BIND: &str = "bind(127.0.0.1:0)";
const LISTEN: &str = "listen after bind(127.0.0.1:0)";
const CONNECT: &str = "connect(127.0.0.1:1)";

#[test]
fn listen_denied_reads_the_bind_and_the_listen_apart() {
    let v = verdict("listen-denied", &[(BIND, Outcome::ok()), (LISTEN, Outcome::denied(EACCES))]);
    assert!(v.holds(), "{v:?}");
    let v = verdict("listen-denied", &[(BIND, Outcome::ok()), (LISTEN, Outcome::ok())]);
    assert_eq!((state(&v, "bind-allowed"), state(&v, "listen-refused")), (State::Holds, State::Fails));
    // A refused bind: the listen did not run.
    let skip = Outcome::skip(Some(EACCES), "bind(127.0.0.1:0) failed");
    let v = verdict("listen-denied", &[(BIND, Outcome::denied(EACCES)), (LISTEN, skip)]);
    assert_eq!((state(&v, "bind-allowed"), state(&v, "listen-refused")), (State::Fails, State::Unseen));
}

#[test]
fn udp_denied_takes_a_host_with_no_ipv6() {
    let (v4, v6) = ("socket(AF_INET, SOCK_DGRAM)+connect", "socket(AF_INET6, SOCK_DGRAM)+connect");
    let v = verdict("udp-denied", &[(v4, Outcome::denied(EPERM)), (v6, Outcome::denied(EAFNOSUPPORT))]);
    assert!(v.holds(), "{v:?}");
    let v = verdict("udp-denied", &[(v4, Outcome::ok()), (v6, Outcome::denied(EPERM))]);
    assert!(v.fails(), "{v:?}");
    let v = verdict("udp-denied", &[(v4, Outcome::denied(EPERM)), (v6, Outcome::ok())]);
    assert!(v.fails(), "{v:?}");
}

#[test]
fn raw_denied_wants_eperm_of_each_socket() {
    let (raw, packet) = ("socket(AF_INET, SOCK_RAW)", "socket(AF_PACKET, SOCK_RAW)");
    assert!(verdict("raw-denied", &[(raw, Outcome::denied(EPERM)), (packet, Outcome::denied(EPERM))]).holds());
    assert!(verdict("raw-denied", &[(raw, Outcome::ok()), (packet, Outcome::denied(EPERM))]).fails());
    assert!(verdict("raw-denied", &[(raw, Outcome::denied(EPERM)), (packet, Outcome::ok())]).fails());
}

#[test]
fn the_connect_to_the_loopback_tells_no_loopback_from_a_reset() {
    let reset = ok(&format!("ECONNREFUSED ({}): the loopback answers", ECONNREFUSED.0));
    let v = verdict("no-loopback", &[(CONNECT, Outcome::denied(ENETUNREACH))]);
    assert_eq!(state(&v, "loopback-absent"), State::Holds);
    let v = verdict("no-loopback", &[(CONNECT, reset.clone())]);
    assert_eq!(state(&v, "loopback-absent"), State::Fails);
    let v = verdict("bind-denied", &[(CONNECT, Outcome::denied(EACCES))]);
    assert_eq!(state(&v, "tcp-connect-refused"), State::Holds);
    let v = verdict("bind-denied", &[(CONNECT, reset)]);
    assert_eq!(state(&v, "tcp-connect-refused"), State::Fails);
}

#[test]
fn no_ptmx_reads_the_terminal_of_the_rows() {
    let rows = |tty: &str, read: Outcome| {
        vec![
            ("stat(/dev/ptmx)", Outcome::denied(ENOENT)),
            ("open(/dev/ptmx, O_RDWR)", Outcome::denied(ENOENT)),
            ("stat(/dev/pts)", Outcome::denied(ENOENT)),
            ("tty_nr of /proc/self/stat", ok(&format!("tty_nr {tty}"))),
            ("open(/dev/tty, O_NONBLOCK)+poll", read),
        ]
    };
    assert!(verdict("no-ptmx", &rows("0", ok("read waits"))).holds());
    let v = verdict("no-ptmx", &rows("34816", ok("read ready")));
    assert_eq!((state(&v, "no-ctty"), state(&v, "tty-opens")), (State::Fails, State::Fails));
    // The node of the kernel with no controlling terminal: no open.
    let v = verdict("no-ptmx", &rows("0", Outcome::denied(ENXIO)));
    assert_eq!(state(&v, "tty-opens"), State::Fails);
    let mut no_field = rows("0", ok("read waits"));
    no_field[3].1 = ok("something else");
    assert_eq!(state(&verdict("no-ptmx", &no_field), "no-ctty"), State::Unseen);
}

#[test]
fn no_passwd_entry_wants_no_line_of_each_id() {
    let rows = |uid: &str, gid: &str| {
        vec![
            ("line of getuid() in /etc/passwd", ok(&format!("uid 1000; lines {uid}"))),
            ("line of getgid() in /etc/group", ok(&format!("gid 1000; lines {gid}"))),
        ]
    };
    assert!(verdict("no-passwd-entry", &rows("0", "0")).holds());
    assert!(verdict("no-passwd-entry", &rows("1", "0")).fails());
    assert!(verdict("no-passwd-entry", &rows("0", "1")).fails());
}

#[test]
fn noexec_home_wants_a_new_file_on_a_noexec_mount() {
    let home = "creat($HOME/.pod-hostprobe)";
    let tmp = ("exec(/tmp/execprobe)", Outcome::ok());
    assert!(verdict("noexec-home", &[(home, ok("noexec 1")), tmp.clone()]).holds());
    let v = verdict("noexec-home", &[(home, ok("noexec 0")), tmp.clone()]);
    assert_eq!(state(&v, "home-noexec"), State::Fails);
    let refused = Outcome { reason: "noexec 1".to_string(), ..Outcome::denied(EACCES) };
    let v = verdict("noexec-home", &[(home, refused), tmp]);
    assert_eq!(state(&v, "home-noexec"), State::Fails);
}

#[test]
fn no_proc_and_ro_etc_ssl_read_their_rows() {
    let exe = "readlink(/proc/self/exe)";
    assert!(verdict("no-proc", &[(exe, Outcome::denied(ENOENT))]).holds());
    assert!(verdict("no-proc", &[(exe, Outcome::ok())]).fails());
    let ssl = "statfs(/etc/ssl)";
    assert!(verdict("ro-etc-ssl", &[(ssl, ok("rdonly 1; noexec 0"))]).holds());
    assert!(verdict("ro-etc-ssl", &[(ssl, ok("rdonly 0; noexec 0"))]).fails());
    assert!(verdict("ro-etc-ssl", &[(ssl, Outcome::denied(ENOENT))]).fails());
    assert_eq!(state(&verdict("ro-etc-ssl", &[(ssl, ok("rdonly unknown"))]), "etc-ssl-read-only"), State::Fails);
}

const PROXY: &str = "getenv(HTTPS_PROXY)";
const ROUTE4: &str = "read(/proc/net/route)";
const ROUTE6: &str = "read(/proc/net/ipv6_route)";
const HTTP_PROXY: &str = "set 1; variable HTTPS_PROXY; scheme http; credential 1; port 3128";

#[test]
fn proxy_only_sees_the_routes_and_the_variable_and_leaves_out_the_lab_check() {
    let rows = |proxy: &str, route: &str| vec![(PROXY, ok(proxy)), (ROUTE4, ok(route)), (ROUTE6, ok("default 0"))];
    let v = verdict("proxy-only", &rows(HTTP_PROXY, "default 0"));
    assert_eq!((state(&v, "direct-refused"), state(&v, "proxy-variable")), (State::Holds, State::Holds));
    // The connect out of the lab is no observation of a report.
    let direct = v.parts.iter().find(|p| p.part == "direct-refused").expect("the part");
    assert_eq!(direct.seen.len(), 2, "{direct:?}");
    // A default route does not contradict the class: a filter can refuse.
    let v = verdict("proxy-only", &rows("set 0", "default 1"));
    assert_eq!((state(&v, "direct-refused"), state(&v, "proxy-variable")), (State::Unseen, State::Fails));
    let socks = "set 1; variable ALL_PROXY; scheme socks5h; credential 0; port 1080";
    assert_eq!(state(&verdict("proxy-only", &rows(socks, "default 0")), "proxy-variable"), State::Fails);
    // The parts of a CONNECT through the proxy stay unseen in a report.
    assert_eq!(state(&v, "proxy-reaches"), State::Unseen);
}

#[test]
fn the_proxy_classes_and_the_direct_classes_read_the_variable() {
    for name in ["proxy-tls-reterminate", "proxy-per-name", "ws-upgrade-refused"] {
        assert_eq!(state(&verdict(name, &[(PROXY, ok(HTTP_PROXY))]), "proxy-variable"), State::Holds, "{name}");
        assert_eq!(state(&verdict(name, &[(PROXY, ok("set 0"))]), "proxy-variable"), State::Fails, "{name}");
    }
    for name in ["sni-allowlist", "no-proxy-no-dns"] {
        assert_eq!(state(&verdict(name, &[(PROXY, ok("set 0"))]), "no-proxy"), State::Holds, "{name}");
        assert_eq!(state(&verdict(name, &[(PROXY, ok(HTTP_PROXY))]), "no-proxy"), State::Fails, "{name}");
    }
    let cert = "getenv(SSL_CERT_FILE)+read";
    let v = verdict("proxy-tls-reterminate", &[(cert, ok("set 1; certificates 2"))]);
    assert_eq!(state(&v, "cert-file"), State::Holds);
    // A CA of the proxy in the store of the system is not in the variable.
    let v = verdict("proxy-tls-reterminate", &[(cert, ok("set 0"))]);
    assert_eq!(state(&v, "cert-file"), State::Unseen);
}

#[test]
fn no_resolver_shows_no_server_and_a_refused_fallback() {
    let resolver = "read(/etc/resolv.conf)";
    let v = verdict("no-proxy-no-dns", &[(resolver, ok("nameservers 0; 127.0.0.1:53 refused"))]);
    assert_eq!(state(&v, "no-resolver"), State::Holds);
    let v = verdict("no-proxy-no-dns", &[(resolver, ok("nameservers 2"))]);
    assert_eq!(state(&v, "no-resolver"), State::Unseen);
    let v = verdict("no-proxy-no-dns", &[(resolver, ok("nameservers 0; 127.0.0.1:53 answers"))]);
    assert_eq!(state(&v, "no-resolver"), State::Unseen);
}

#[test]
fn ipv6_only_sees_no_ipv4_route_and_the_loopback() {
    let reset = ok("ECONNREFUSED (111): the loopback answers");
    let v = verdict("ipv6-only", &[(ROUTE4, ok("default 0")), (CONNECT, reset)]);
    assert_eq!(state(&v, "no-ipv4-route"), State::Holds);
    let v = verdict("ipv6-only", &[(ROUTE4, ok("default 0")), (CONNECT, Outcome::denied(ENETUNREACH))]);
    assert_eq!(state(&v, "no-ipv4-route"), State::Fails);
    assert_eq!(state(&v, "ipv6-reaches"), State::Unseen);
}

/// A lab measures its own check beside the rows; a check of findings
/// leaves it out.
#[test]
fn a_lab_check_counts_in_a_lab_alone() {
    let p = find(PROFILES, "proxy-only").expect("a profile");
    let mut f = Findings::empty();
    f.rows = vec![(ROUTE4, ok("default 0")), (ROUTE6, ok("default 0"))];
    let fails = |fact: LabFact| Seen { what: fact.name(), state: State::Fails, errno: None, detail: String::new() };
    let v = check_with(p, &f, &fails);
    let direct = v.parts.iter().find(|x| x.part == "direct-refused").expect("the part");
    assert_eq!((direct.state, direct.seen.len()), (State::Fails, 3), "{direct:?}");
    assert_eq!(state(&check(p, &f), "direct-refused"), State::Holds);
}

/// The rows that send a packet out decide the parts of a CONNECT and of a
/// handshake in a report; a report with no such row (no
/// `Settings::network`) leaves them unseen, and a skip of a row is unseen.
#[test]
fn the_rows_that_send_a_packet_out_decide_the_network_parts() {
    use pod_hostprobe::probes::{CONNECT_CLOSED, CONNECT_LITERAL, CONNECT_NAME, CONNECT_OTHER, TLS_NAME};
    let reaches = |connect: &str, hello: &str| vec![(CONNECT_NAME, ok(connect)), (TLS_NAME, ok(hello))];
    let v = verdict("proxy-only", &reaches("status 200", "path proxy; hello 1; certs 2; issuer other"));
    assert_eq!(state(&v, "proxy-reaches"), State::Holds);
    let v = verdict("proxy-only", &reaches("status 403", "path proxy; status 403"));
    assert_eq!(state(&v, "proxy-reaches"), State::Fails);
    let off = Outcome::skip(None, pod_hostprobe::probes::send::OFF);
    let v = verdict("proxy-only", &[(CONNECT_NAME, off.clone()), (TLS_NAME, off)]);
    assert_eq!(state(&v, "proxy-reaches"), State::Unseen);
    assert_eq!(state(&verdict("proxy-only", &[]), "proxy-reaches"), State::Unseen);

    let v = verdict("proxy-only", &[(CONNECT_CLOSED, ok("status 403"))]);
    assert_eq!(state(&v, "port-refused"), State::Holds);
    let v = verdict("proxy-tls-reterminate", &[(CONNECT_CLOSED, ok("status 200; bytes 0"))]);
    assert_eq!(state(&v, "ssh-port-silent"), State::Holds);
    let v = verdict("proxy-tls-reterminate", &[(CONNECT_CLOSED, ok("status 200; bytes 1"))]);
    assert_eq!(state(&v, "ssh-port-silent"), State::Fails);
    let per_name = [
        (CONNECT_NAME, ok("status 200")),
        (CONNECT_OTHER, ok("status 504")),
        (CONNECT_LITERAL, ok("status none; end reset")),
    ];
    assert_eq!(state(&verdict("proxy-per-name", &per_name), "answer-per-name"), State::Holds);
}

/// `bind-netns-vm-denied` on a report: the network namespace and the write
/// of memory each read their row; a refused connect or an allowed write
/// fails the profile (PodUtils/podnet#649).
#[test]
fn bind_netns_vm_denied_reads_each_row() {
    const NETNS: &str = "clone(CLONE_NEWUSER|NEWNET)";
    const WRITE: &str = "process_vm_writev(own pid)";
    let host = [
        (BIND, Outcome::denied(EACCES)),
        (CONNECT, Outcome::ok()),
        (NETNS, Outcome::denied(EPERM)),
        (WRITE, Outcome::denied(EPERM)),
    ];
    let v = verdict("bind-netns-vm-denied", &host);
    assert_eq!((state(&v, "netns-refused"), state(&v, "vm-write-refused")), (State::Holds, State::Holds), "{v:?}");
    assert!(!v.fails(), "{v:?}");
    let mut write = host.clone();
    write[3] = (WRITE, Outcome::ok());
    assert_eq!(state(&verdict("bind-netns-vm-denied", &write), "vm-write-refused"), State::Fails);
    let mut connect = host.clone();
    connect[1] = (CONNECT, Outcome::denied(EACCES));
    assert_eq!(state(&verdict("bind-netns-vm-denied", &connect), "tcp-bind-refused"), State::Fails);
    let mut netns = host;
    netns[2] = (NETNS, Outcome::ok());
    assert_eq!(state(&verdict("bind-netns-vm-denied", &netns), "netns-refused"), State::Fails);
}
