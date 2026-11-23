//! The units of systemd: the golden texts, the hardening of a unit of the
//! system and of a user unit, the audit, and the checks of systemd itself
//! (`systemd-analyze verify` and `security`) where the host has them.

mod common;

use std::path::Path;
use std::process::Command;

use common::{golden, node, podrunner, section, tool};
use pod_unit::hardening::{LINES, Need};
use pod_unit::{Unit, systemd};

#[test]
fn a_user_unit_of_podrunner_is_its_golden_text() {
    golden("podrunner.service", &systemd::render(&podrunner()).expect("a unit"));
}

#[test]
fn a_unit_of_the_system_is_its_golden_text() {
    golden("podnet-relay-node.service", &systemd::render(&node()).expect("a unit"));
}

/// The lines that a unit of the system must hold, apart from the table of
/// the crate: a line that goes from the table fails here (podssh's list of
/// its system unit, and the lines of T-102).
const HARDENED: &[&str] = &[
    "NoNewPrivileges=yes",
    "CapabilityBoundingSet=",
    "ProtectSystem=strict",
    "ProtectHome=tmpfs",
    "PrivateTmp=yes",
    "PrivateDevices=yes",
    "ProtectKernelTunables=yes",
    "ProtectKernelModules=yes",
    "ProtectKernelLogs=yes",
    "ProtectControlGroups=yes",
    "RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX",
    "SystemCallFilter=@system-service",
    "LockPersonality=yes",
    "MemoryDenyWriteExecute=yes",
];

/// The plant `unit-hardening-line-missing` drops one line from the table
/// of the hardening; this test finds it.
#[test]
fn each_line_of_the_hardening_is_in_a_unit_of_the_system() {
    let mut unit = node();
    unit.hardening = Default::default();
    let text = systemd::render(&unit).expect("a unit");
    let service = section(&text, "Service");
    for want in HARDENED {
        assert!(service.iter().any(|l| l == want), "{want}: {text}");
    }
    for line in LINES {
        let want = format!("{}={}", line.key, line.value);
        assert!(service.contains(&want), "{want}: {text}");
    }
    assert_eq!(systemd::audit(&text, true), Vec::<String>::new(), "{text}");
    assert!(!text.contains("Relaxed"), "{text}");
}

#[test]
fn the_audit_names_a_line_that_is_gone_but_not_one_that_is_relaxed() {
    let text = systemd::render(&node()).expect("a unit");
    assert_eq!(systemd::audit(&text, true), Vec::<String>::new(), "{text}");
    assert!(text.contains("# Relaxed, as the engine of rules compiles at run time: MemoryDenyWriteExecute=yes\n"));
    let gone = text.replace("ProtectSystem=strict\n", "").replace("PrivateTmp=yes\n", "PrivateTmp=no\n");
    assert_eq!(systemd::audit(&gone, true), ["ProtectSystem=strict", "PrivateTmp=yes"]);
}

#[test]
fn a_user_unit_takes_the_lines_of_seccomp_alone() {
    let text = systemd::render(&podrunner()).expect("a unit");
    for line in LINES {
        let want = format!("{}={}", line.key, line.value);
        let there = text.lines().any(|l| l == want);
        assert_eq!(there, line.need == Need::Seccomp, "{want}: {text}");
    }
    assert_eq!(systemd::audit(&text, false), Vec::<String>::new());
    assert!(!text.contains("User=") && !text.contains("DynamicUser"), "{text}");
    assert!(section(&text, "Install").iter().any(|l| l == "WantedBy=default.target"), "{text}");
}

/// A-C-07: a revoked credential or a bad configuration (78) and a usage
/// (64) start no loop; a unit with no limit of starts restarts after each
/// other end.
#[test]
fn a_unit_restarts_after_each_end_with_no_limit_of_starts() {
    for unit in [podrunner(), node()] {
        let text = systemd::render(&unit).expect("a unit");
        let (head, service) = (section(&text, "Unit"), section(&text, "Service"));
        assert!(head.iter().any(|l| l == "StartLimitIntervalSec=0"), "{text}");
        // systemd older than 230 reads the old name, in [Service] alone.
        assert!(!head.iter().any(|l| l.starts_with("StartLimitInterval=")), "{text}");
        for line in ["Restart=always", "RestartSec=5", "RestartPreventExitStatus=64 78", "StartLimitInterval=0"] {
            assert!(service.iter().any(|l| l == line), "{line}: {text}");
        }
        // Each line that is not blank is a comment, a section or a key.
        for line in text.lines().filter(|l| !l.is_empty()) {
            assert!(line.starts_with('#') || line.starts_with('[') || line.contains('='), "{line:?}");
        }
    }
}

#[test]
fn a_secret_of_a_unit_of_the_system_is_a_credential_of_systemd() {
    let text = systemd::render(&node()).expect("a unit");
    assert!(text.contains("\nLoadCredential=tunnel:/etc/podnet/tunnel.token\n"), "{text}");
    assert!(text.contains("\nEnvironment=\"TUNNEL_TOKEN_FILE=%d/tunnel\"\n"), "{text}");
    assert!(text.contains("\nDynamicUser=yes\n"), "{text}");
    let user = systemd::render(&podrunner().secret_file("gh", Path::new("/home/ci/.gh"), "GH_FILE")).expect("a unit");
    assert!(user.contains("\nEnvironment=\"GH_FILE=/home/ci/.gh\"\n") && !user.contains("LoadCredential"), "{user}");
}

#[test]
fn a_path_that_a_list_of_systemd_cannot_hold_is_refused() {
    let mut unit = node();
    unit.hardening.bind.push("/home/a b".into());
    assert!(systemd::render(&unit).is_err());
    let bad = node().secret_file("x", Path::new("/etc/a:b"), "X_FILE");
    assert!(systemd::render(&bad).is_err());
}

/// The units of the golden files, each with a program that this host has,
/// as `systemd-analyze verify` checks that the program is there.
fn verifiable(mut unit: Unit, dir: &Path) -> std::path::PathBuf {
    unit.program = "/bin/sh".into();
    let path = dir.join(systemd::file_name(&unit));
    std::fs::write(&path, systemd::render(&unit).expect("a unit")).expect("a write");
    path
}

#[test]
fn systemd_analyze_verify_accepts_each_unit() {
    let Some(analyze) = tool("systemd-analyze") else { return };
    let dir = tempfile::tempdir().expect("a directory");
    for unit in [podrunner(), node()] {
        let path = verifiable(unit, dir.path());
        let out = Command::new(analyze).args(["verify", "--man=no"]).arg(&path).output().expect("a run");
        let said = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success() && said.trim().is_empty(), "{}: {said}", path.display());
    }
}

/// The exposure of a unit of the system, as `systemd-analyze security`
/// scores it with no manager (`--offline`, systemd 250 or later): 2.0 or
/// less, "OK".
#[test]
fn systemd_analyze_security_scores_a_unit_of_the_system_ok() {
    let Some(analyze) = tool("systemd-analyze") else { return };
    let dir = tempfile::tempdir().expect("a directory");
    let mut unit = node();
    unit.hardening = Default::default();
    let path = verifiable(unit, dir.path());
    let out = Command::new(analyze)
        .args(["security", "--offline=true", "--threshold=20", "--no-pager"])
        .arg(&path)
        .output()
        .expect("a run");
    let text = String::from_utf8_lossy(&out.stdout);
    let said = String::from_utf8_lossy(&out.stderr);
    if said.contains("--offline") || said.contains("unrecognized option") {
        eprintln!("skipped: this systemd-analyze has no --offline: {said}");
        return;
    }
    let score = text
        .lines()
        .find_map(|l| l.split("exposure level for ").nth(1))
        .and_then(|rest| rest.split_whitespace().nth(1))
        .and_then(|n| n.parse::<f64>().ok())
        .unwrap_or_else(|| panic!("no score: {text} {said}"));
    assert!(score <= 2.0, "the exposure is {score}: {text}");
}

#[test]
fn a_program_adds_a_line_past_the_table_but_not_one_of_the_table() {
    let mut unit = node();
    unit.hardening.add("PrivateUsers", "yes").expect("a line").add("ProcSubset", "pid").expect("a line");
    let text = systemd::render(&unit).expect("a unit");
    assert!(text.contains("\nUMask=0077\nPrivateUsers=yes\nProcSubset=pid\n"), "{text}");
    assert!(unit.hardening.add("ProtectSystem", "full").is_err(), "a key of the table");
    assert!(unit.hardening.add("Bad Key", "x").is_err(), "a key with a space");
}

/// A restart after a failure alone keeps the limit of starts of systemd:
/// neither name of the limit is in the unit.
#[test]
fn a_unit_that_restarts_after_a_failure_alone_keeps_the_limit_of_starts() {
    let mut unit = podrunner();
    unit.restart.always = false;
    let text = systemd::render(&unit).expect("a unit");
    assert!(!text.contains("StartLimitInterval"), "{text}");
}

/// PodUtils/podssh#342 (c): `RemoveIPC=yes` removes each IPC object of the
/// account at the stop of the unit. A named account can be a person's, so
/// the line is relaxed with its reason; an account of the service alone
/// (`DynamicUser=yes`, or a named one that the program says is its own)
/// keeps the line.
#[test]
fn a_named_account_keeps_its_ipc_and_an_account_of_the_service_loses_it() {
    let dynamic = systemd::render(&node()).expect("a unit");
    assert!(section(&dynamic, "Service").iter().any(|l| l == "RemoveIPC=yes"), "{dynamic}");
    let named = systemd::render(&node().system(Some("alice"))).expect("a unit");
    let service = section(&named, "Service");
    assert!(!service.iter().any(|l| l == "RemoveIPC=yes"), "{named}");
    assert!(
        service
            .iter()
            .any(|l| l.starts_with("# Relaxed, as ") && l.contains("alice") && l.ends_with(": RemoveIPC=yes")),
        "{named}"
    );
    assert_eq!(systemd::audit(&named, true), Vec::<String>::new(), "{named}");
    // The program says that the account is its own; a reason of the
    // program stays as it gave it.
    let mut own = node().system(Some("podrunner"));
    own.hardening.service_account();
    let text = systemd::render(&own).expect("a unit");
    assert!(section(&text, "Service").iter().any(|l| l == "RemoveIPC=yes"), "{text}");
    let mut relaxed = node().system(Some("alice"));
    relaxed.hardening.relax("RemoveIPC", "the program shares memory with its user").expect("a line");
    let text = systemd::render(&relaxed).expect("a unit");
    assert!(text.contains("\n# Relaxed, as the program shares memory with its user: RemoveIPC=yes\n"), "{text}");
}
