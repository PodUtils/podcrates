//! The units of the tests and the compare with a golden file.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use pod_unit::{Start, Stop, Unit};

/// The golden file `name` of `tests/golden`, compared with `text`. With
/// `POD_UNIT_BLESS=1` in the environment of the run, the test writes the
/// file from `text` first (a person reads the change in the diff).
pub fn golden(name: &str, text: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name);
    if std::env::var_os("POD_UNIT_BLESS").is_some() {
        std::fs::write(&path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    // A checkout on Windows can give the lines of a golden file a CR.
    let want = want.replace("\r\n", "\n");
    let text = text.replace("\r\n", "\n");
    assert!(want == text, "{name} differs from its golden file:\n--- golden\n{want}\n--- rendered\n{text}");
}

/// podrunner's `serve` as a user unit: it says READY, has a watchdog, and
/// drains at a stop with no limit of time.
pub fn podrunner() -> Unit {
    let mut unit = Unit::new("podrunner", Path::new("/home/ci/.local/bin/podrunner"), "podrunner print-unit").args([
        "serve",
        "--config",
        "/home/ci/.config/podrunner/podrunner.toml",
        "--state",
        "/home/ci/.local/state/pr",
    ]);
    unit.description = "podrunner: CI jobs in one fresh unit for each job".into();
    unit.start = Start::Notify { watchdog_s: Some(30) };
    unit.stop = Stop { drain: true, timeout_s: None };
    unit
}

/// A node of a relay as a hardened unit of the system, with an account of
/// its own, a credential, a placeholder, a state directory, netlink, and
/// one relaxed line.
pub fn node() -> Unit {
    let mut unit = Unit::new("podnet-relay-node", Path::new("/usr/local/bin/podnet"), "podnet unit print")
        .system(None)
        .args(["relay", "node", "--state", "/var/lib/podnet-relay-node", "--label", "a b"])
        .env("RUST_LOG", "info")
        .env("PODNET_NOTE", "100% $HOME")
        .secret_file("tunnel", Path::new("/etc/podnet/tunnel.token"), "TUNNEL_TOKEN_FILE")
        .placeholder("GITHUB_TOKEN", "PODCRED_GITHUB_TOKEN_0001")
        .expect("a placeholder");
    unit.description = "podnet relay node".into();
    unit.state_dir = Some("podnet-relay-node".into());
    unit.hardening.family("AF_NETLINK", "the kernel gives the addresses of the host by netlink").expect("a family");
    unit.hardening.relax("MemoryDenyWriteExecute", "the engine of rules compiles at run time").expect("a line");
    unit.hardening.write.push(PathBuf::from("/var/cache/podnet"));
    unit
}

/// The lines of the section `name` of a unit of systemd.
pub fn section(text: &str, name: &str) -> Vec<String> {
    let start = text.find(&format!("[{name}]\n")).unwrap_or_else(|| panic!("[{name}]: {text}"));
    let rest = &text[start + name.len() + 3..];
    rest[..rest.find("\n[").unwrap_or(rest.len())].lines().map(str::to_string).collect()
}

/// The program `name` when this host has it, else None after the reason of
/// the skip.
pub fn tool(name: &str) -> Option<&str> {
    match pod_testkit::host::tool(name) {
        Ok(()) => Some(name),
        Err(why) => {
            eprintln!("skipped: {why}");
            None
        }
    }
}
