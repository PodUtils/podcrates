//! A run of the tests of this crate with a carrier's feature off runs none
//! of that carrier's tests: their binaries are empty and pass (T-824). This
//! test fails and names each such feature, so that a run with fewer
//! features does not read as a pass. Run the tests with `--all-features`,
//! or one binary with its feature (`--features tcp --test tcp`).

/// Each feature, its test files, and whether it is on in this build.
const FEATURES: &[(&str, &str, bool)] = &[
    ("dns", "tests/dns.rs, dns_loss.rs, dns_resolver.rs", cfg!(feature = "dns")),
    ("h2", "tests/h2.rs", cfg!(feature = "h2")),
    ("icmp", "tests/icmp.rs, icmp_raw.rs", cfg!(feature = "icmp")),
    ("imap", "tests/mail.rs", cfg!(feature = "imap")),
    ("ntp", "tests/ntp.rs", cfg!(feature = "ntp")),
    ("poll", "tests/poll.rs, poll_lanes.rs", cfg!(feature = "poll")),
    ("smtp", "tests/mail.rs", cfg!(feature = "smtp")),
    ("quic", "tests/quic.rs", cfg!(feature = "quic")),
    ("tcp", "tests/tcp.rs, tcp_refused.rs", cfg!(feature = "tcp")),
    ("udp", "tests/udp.rs, udp_loss.rs", cfg!(feature = "udp")),
    ("ws", "tests/ws.rs", cfg!(feature = "ws")),
];

#[test]
fn each_carrier_feature_is_on_so_its_tests_ran() {
    let off: Vec<String> =
        FEATURES.iter().filter(|(_, _, on)| !on).map(|(name, files, _)| format!("{name} ({files})")).collect();
    assert!(
        off.is_empty(),
        "the tests of these features did not run, as the feature is off: {}; run with --all-features",
        off.join("; ")
    );
}
