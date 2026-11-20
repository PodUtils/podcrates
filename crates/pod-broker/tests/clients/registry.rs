//! The far end of cargo (T-973): a sparse registry that needs a token
//! (`auth-required`), with one crate `foo` 1.0.0 in its index. Cargo asks
//! for `config.json` with no token first, and sends the token after the
//! 401.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::tls_common::{Answer, ok};

/// The manifest of the job: one dependency on `foo` of the registry `far`.
pub const MANIFEST: &str = "[package]\nname = \"job\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
                            [dependencies]\nfoo = { version = \"1\", registry = \"far\" }\n";

/// The variables of a run of cargo for the job in `dir`, with the token
/// `token`.
pub fn env(dir: &Path, token: &str) -> Vec<(String, String)> {
    let home = dir.join("cargo-home").to_string_lossy().into_owned();
    [
        ("CARGO_HOME", home.as_str()),
        ("CARGO_REGISTRIES_FAR_INDEX", "sparse+https://api.example.com/index/"),
        ("CARGO_REGISTRIES_FAR_TOKEN", token),
        ("CARGO_REGISTRY_GLOBAL_CREDENTIAL_PROVIDERS", "cargo:token"),
        ("CARGO_NET_RETRY", "0"),
    ]
    .iter()
    .map(|(n, v)| (n.to_string(), v.to_string()))
    .collect()
}

/// The program of cargo: `CARGO` (`cargo test` sets it), else the cargo
/// of the toolchain that `rustup which cargo` names in the directory of the
/// test, else `cargo`. The lab runs cargo with a clean environment in a
/// temporary directory: a proxy of rustup there finds no
/// `rust-toolchain.toml`, and a runner can have no default toolchain.
pub fn cargo() -> Option<String> {
    let set = std::env::var_os("CARGO").is_some_and(|v| !v.is_empty());
    if !set && pod_testkit::host::tool("rustup").is_ok() {
        let out = Command::new("rustup").args(["which", "cargo"]).output();
        if let Ok(out) = out
            && out.status.success()
        {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    super::need("CARGO", "cargo")
}

/// Write the job, a library with [`MANIFEST`], to `dir/job`.
pub fn job(dir: &Path) {
    std::fs::create_dir_all(dir.join("job/src")).unwrap();
    std::fs::write(dir.join("job/Cargo.toml"), MANIFEST).unwrap();
    std::fs::write(dir.join("job/src/lib.rs"), "").unwrap();
}

/// The far end: 401 to a request with no `Authorization`; the
/// configuration of the registry, the index line of `foo`, else 404.
pub fn answer(_dir: &Path) -> Answer {
    Arc::new(|head: &str, _body: &[u8]| {
        let auth = head.lines().skip(1).any(|l| l.to_ascii_lowercase().starts_with("authorization:"));
        if !auth {
            return b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n".to_vec();
        }
        if head.starts_with("GET /index/config.json ") {
            let config = r#"{"dl":"https://api.example.com/dl","api":"https://api.example.com","auth-required":true}"#;
            return ok("", config.as_bytes());
        }
        if head.starts_with("GET /index/3/f/foo ") {
            let cksum = "0".repeat(64);
            let line = format!(
                "{{\"name\":\"foo\",\"vers\":\"1.0.0\",\"deps\":[],\"cksum\":\"{cksum}\",\"features\":{{}},\"yanked\":false}}\n"
            );
            return ok("", line.as_bytes());
        }
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec()
    })
}
