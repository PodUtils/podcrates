//! The CA bundle that the environment names: one order of the variables
//! that the tools of a host read, in place of the lists of the consumers
//! (podrunner, podvm; the owner's ruling "Shared code"). The caller decides
//! what the file is to it: the CA file used alone ([`crate::Settings::ca_file`])
//! or a bundle that adds ([`crate::Settings::extra_bundle`]).

use std::path::PathBuf;

/// The variables, in the order that they are read: OpenSSL's first, then
/// those of curl, Python's requests, Node.js, git and the AWS tools.
pub const BUNDLE_VARS: [&str; 6] =
    ["SSL_CERT_FILE", "CURL_CA_BUNDLE", "REQUESTS_CA_BUNDLE", "NODE_EXTRA_CA_CERTS", "GIT_SSL_CAINFO", "AWS_CA_BUNDLE"];

/// The bundle that a variable named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvBundle {
    pub var: &'static str,
    pub path: PathBuf,
}

/// A variable that was set and passed over, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passed {
    pub var: &'static str,
    pub why: String,
}

/// The first variable of [`BUNDLE_VARS`] that `get` gives and that names a
/// file which opens for read, and each set variable before it that was
/// passed over (empty, not a file, or not readable), with why. A variable
/// after the one taken is not read.
pub fn env_bundle(get: impl Fn(&str) -> Option<String>) -> (Option<EnvBundle>, Vec<Passed>) {
    let mut passed = Vec::new();
    for var in BUNDLE_VARS {
        let Some(value) = get(var) else { continue };
        if value.trim().is_empty() {
            passed.push(Passed { var, why: "it is empty".into() });
            continue;
        }
        let path = PathBuf::from(&value);
        let why = match std::fs::metadata(&path) {
            Ok(m) if !m.is_file() => Some(format!("{value} is not a file")),
            Ok(_) => std::fs::File::open(&path).err().map(|e| format!("{value} cannot be read: {e}")),
            Err(e) => Some(format!("{value} cannot be read: {e}")),
        };
        match why {
            Some(why) => passed.push(Passed { var, why }),
            None => return (Some(EnvBundle { var, path }), passed),
        }
    }
    (None, passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first readable file wins, the variables before it that were set
    /// say why they were passed over, and the ones after it are not read.
    #[test]
    fn the_first_readable_bundle_of_the_order_is_taken() {
        let dir = std::env::temp_dir().join(format!("pod-tls-env-bundle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("corp.pem");
        std::fs::write(&file, b"-----BEGIN CERTIFICATE-----\n").unwrap();
        let (f, d) = (file.to_str().unwrap().to_string(), dir.to_str().unwrap().to_string());
        let env = |pairs: Vec<(&'static str, String)>| {
            move |k: &str| pairs.iter().find(|(v, _)| *v == k).map(|(_, x)| x.clone())
        };
        let (got, passed) = env_bundle(env(vec![
            ("SSL_CERT_FILE", String::new()),
            ("CURL_CA_BUNDLE", d.clone()),
            ("REQUESTS_CA_BUNDLE", format!("{d}/missing.pem")),
            ("NODE_EXTRA_CA_CERTS", f.clone()),
            ("GIT_SSL_CAINFO", "/never/read".into()),
        ]));
        assert_eq!(got, Some(EnvBundle { var: "NODE_EXTRA_CA_CERTS", path: file.clone() }));
        let vars: Vec<&str> = passed.iter().map(|p| p.var).collect();
        assert_eq!(vars, ["SSL_CERT_FILE", "CURL_CA_BUNDLE", "REQUESTS_CA_BUNDLE"]);
        assert!(passed[1].why.contains("is not a file"), "{passed:?}");
        assert!(passed[2].why.contains("cannot be read"), "{passed:?}");
        assert_eq!(env_bundle(|_| None), (None, Vec::new()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
