use super::*;
use crate::profile::Observe;

#[test]
fn each_profile_of_the_asks_is_in_the_table() {
    let names: Vec<&str> = PROFILES.iter().map(|p| p.name).collect();
    for want in [
        "userns-off",
        "bind-denied",
        "getsockname-denied",
        "proxy-only",
        "small-tmp",
        "root-unreadable",
        "nonroot-nocaps",
        "no-kvm",
        "arm64-tcg",
        "inherited-fds",
        "chroot-denied",
        "userns-mount-denied",
        "cgroup-v1",
        "landlock-old",
        "proc-masked",
        "noexec-tmp",
        "listen-denied",
        "udp-denied",
        "raw-denied",
        "no-ptmx",
        "no-passwd-entry",
        "noexec-home",
        "no-proc",
        "seccomp-nonewprivs",
        "no-tun",
        "no-net-admin",
        "ro-etc-ssl",
        "no-loopback",
        "proxy-tls-reterminate",
        "proxy-per-name",
        "sni-allowlist",
        "no-proxy-no-dns",
        "ws-upgrade-refused",
        "ipv6-only",
        "bind-netns-vm-denied",
    ] {
        assert!(names.contains(&want), "the table has no profile {want}");
    }
    assert_eq!(names.len(), 35, "{names:?}");
}

#[test]
fn each_profile_names_a_part_and_each_part_an_observation() {
    for p in PROFILES {
        assert!(!p.parts.is_empty(), "{} has no part", p.name);
        assert!(!p.about.is_empty() && !p.source.is_empty(), "{}", p.name);
        for part in p.parts {
            assert!(!part.observe.is_empty(), "{}/{} has no observation", p.name, part.name);
        }
    }
}

#[test]
fn each_row_name_is_a_row_of_pod_hostprobe() {
    for p in PROFILES {
        for part in p.parts {
            for o in part.observe {
                if let Observe::Row { row, .. } = o {
                    assert!(pod_hostprobe::probes::find(row).is_some(), "{}/{}: no row {row:?}", p.name, part.name);
                }
            }
        }
    }
}

#[test]
fn the_names_are_unique_and_found() {
    for (i, p) in PROFILES.iter().enumerate() {
        assert!(PROFILES[i + 1..].iter().all(|q| q.name != p.name), "{} two times", p.name);
        assert_eq!(find(PROFILES, p.name).map(|q| q.name), Some(p.name));
        for (j, part) in p.parts.iter().enumerate() {
            assert!(p.parts[j + 1..].iter().all(|q| q.name != part.name), "{}/{} two times", p.name, part.name);
        }
    }
    assert!(find(PROFILES, "no-such-profile").is_none());
}

/// A part that no row observes names the entry that adds its row.
#[test]
fn each_fact_with_no_row_names_its_entry() {
    for p in PROFILES {
        for part in p.parts {
            for o in part.observe {
                let entry = match o {
                    Observe::Lab(f) => f.entry(),
                    Observe::Nothing { entry, .. } => *entry,
                    _ => continue,
                };
                assert!((692..=699).contains(&entry) || entry == 751, "{}/{}: {entry}", p.name, part.name);
            }
        }
    }
}

/// Each refused row names an errno that a seccomp filter can give, and
/// each errno of a filter call is one too.
#[test]
fn each_errno_is_one_that_a_filter_can_give() {
    let ok = |n: i32| (1..=4095).contains(&n);
    for p in PROFILES {
        for part in p.parts {
            for o in part.observe {
                if let Observe::Row { expect: Expect::Refused(e), .. } = o {
                    assert!(!e.is_empty() && e.iter().all(|&n| ok(n)), "{}/{}", p.name, part.name);
                }
            }
            if let Hold::Seccomp(calls) = part.hold {
                assert!(calls.iter().all(|c| ok(c.errno)), "{}/{}", p.name, part.name);
            }
        }
    }
}

/// The deny rules of each filter compile into one BPF program.
#[test]
fn the_filter_of_each_profile_compiles() {
    for p in PROFILES {
        let rules: Vec<_> = p.parts.iter().flat_map(|part| crate::hold::deny_rules(part.hold)).collect();
        if !rules.is_empty() {
            pod_confine::deny::program(&rules).unwrap_or_else(|f| panic!("{}: {f}", p.name));
        }
    }
}

/// The classes of podssh and podnet that a lab here holds: each part has a
/// hold here; each network class is a network lab with its stand-ins.
#[test]
fn the_classes_of_podssh_and_podnet_are_held_here() {
    let held = [
        "listen-denied",
        "udp-denied",
        "raw-denied",
        "no-ptmx",
        "no-passwd-entry",
        "noexec-home",
        "no-proc",
        "seccomp-nonewprivs",
        "no-tun",
        "no-net-admin",
        "ro-etc-ssl",
        "no-loopback",
        "bind-netns-vm-denied",
    ];
    for name in held {
        let p = find(PROFILES, name).expect("a profile");
        assert!(p.parts.iter().all(|x| x.hold.here()), "{name}");
    }
    for name in NETWORK_CLASSES {
        let p = find(PROFILES, name).expect("a profile");
        assert!(p.parts.iter().all(|x| x.hold.here()), "{name}");
        let nets: Vec<_> =
            p.parts.iter().filter_map(|x| if let Hold::LabNet(n) = x.hold { Some(n) } else { None }).collect();
        assert!(!nets.is_empty() && nets.iter().all(|n| *n == nets[0]), "{name}: one network lab");
    }
}

const NETWORK_CLASSES: [&str; 7] = [
    "proxy-only",
    "proxy-tls-reterminate",
    "proxy-per-name",
    "sni-allowlist",
    "no-proxy-no-dns",
    "ws-upgrade-refused",
    "ipv6-only",
];

/// The parts that podnet marked assumed (PodUtils/podnet#592) say so in
/// their text, and no other part does.
#[test]
fn each_assumed_part_says_so() {
    let assumed = [
        "tls-reterminated",
        "ssh-port-silent",
        "answer-per-name",
        "sni-reset",
        "literal-reaches",
        "upgrade-refused",
        "ipv6-reaches",
    ];
    for p in PROFILES {
        for part in p.parts {
            let says = part.about.contains(". Assumed: ");
            assert_eq!(says, assumed.contains(&part.name), "{}/{}: {}", p.name, part.name, part.about);
        }
    }
}

/// A network lab with a proxy names it; the others name none.
#[test]
fn the_words_of_each_network_lab() {
    for name in NETWORK_CLASSES {
        let p = find(PROFILES, name).expect("a profile");
        for part in p.parts {
            if let Hold::LabNet(n) = part.hold {
                assert_eq!(part.hold.words().contains("stand-in proxy"), n.proxy.is_some(), "{name}");
                assert!(part.hold.words().starts_with("a network namespace with no route out"), "{name}");
            }
        }
    }
}

/// The parts of the socket, file and network classes that a row of
/// `pod-hostprobe` sees: no lab fact and nothing unseen in a check of a
/// report.
#[test]
fn a_row_sees_each_part_of_the_socket_and_file_classes() {
    let seen = [
        ("listen-denied", "bind-allowed"),
        ("listen-denied", "listen-refused"),
        ("udp-denied", "udp-refused"),
        ("raw-denied", "raw-refused"),
        ("no-ptmx", "ptmx-absent"),
        ("no-ptmx", "no-ctty"),
        ("no-ptmx", "tty-opens"),
        ("no-passwd-entry", "no-id-lines"),
        ("noexec-home", "home-noexec"),
        ("no-proc", "proc-absent"),
        ("ro-etc-ssl", "etc-ssl-read-only"),
        ("no-loopback", "loopback-absent"),
        ("bind-denied", "tcp-connect-refused"),
        ("proxy-only", "direct-refused"),
        ("proxy-only", "proxy-variable"),
        ("proxy-only", "udp-refused"),
        ("proxy-tls-reterminate", "proxy-variable"),
        ("proxy-tls-reterminate", "cert-file"),
        ("proxy-per-name", "proxy-variable"),
        ("sni-allowlist", "no-proxy"),
        ("no-proxy-no-dns", "no-proxy"),
        ("no-proxy-no-dns", "no-resolver"),
        ("ws-upgrade-refused", "proxy-variable"),
        ("ipv6-only", "no-ipv4-route"),
        ("bind-netns-vm-denied", "netns-refused"),
        ("bind-netns-vm-denied", "vm-write-refused"),
    ];
    for (profile, name) in seen {
        let p = find(PROFILES, profile).expect("a profile");
        let part = p.parts.iter().find(|x| x.name == name).unwrap_or_else(|| panic!("{profile}/{name}"));
        for o in part.observe {
            assert!(!matches!(o, Observe::Lab(_) | Observe::Nothing { .. }), "{profile}/{name}: {o:?}");
        }
    }
}

/// A fact that a lab checks beside the rows is in a part that a row sees,
/// and a row of a fact names a key.
#[test]
fn a_lab_check_stands_beside_a_row() {
    for p in PROFILES {
        for part in p.parts {
            let rows = part.observe.iter().filter(|o| matches!(o, Observe::Row { .. })).count();
            if part.observe.iter().any(|o| matches!(o, Observe::LabCheck(_))) {
                assert!(rows > 0, "{}/{}", p.name, part.name);
            }
            for o in part.observe {
                if let Observe::Row { expect: Expect::Value { key, value } | Expect::Shows { key, value }, .. } = o {
                    assert!(!key.is_empty() && !value.is_empty() && !key.contains(';'), "{}/{}", p.name, part.name);
                }
            }
        }
    }
}
