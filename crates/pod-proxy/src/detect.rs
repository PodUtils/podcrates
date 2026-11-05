//! The proxy that the configuration of a tool names, for a host whose
//! programs find their proxy there and not in a variable: git
//! (`http.proxy`), npm (`https-proxy`), pip (`proxy`), and the variables of
//! `/etc/environment`. [`crate::Environment::or_detected`] takes it when no
//! variable names a proxy.
//!
//! The crate reads no variable and opens no file here: the caller gives the
//! home directory and the reader of a file ([`read_file`] reads it as the
//! system has it).

use std::path::{Path, PathBuf};

/// `http.proxy` (or `https.proxy`) of a git config file.
pub fn proxy_in_gitconfig(text: &str) -> Option<String> {
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') || line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').trim().to_ascii_lowercase();
            continue;
        }
        if (section == "http" || section == "https")
            && let Some((k, v)) = line.split_once('=')
            && k.trim().eq_ignore_ascii_case("proxy")
        {
            let v = v.trim().trim_matches('"').to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// `https-proxy` (else `proxy`) of an npmrc file.
pub fn proxy_in_npmrc(text: &str) -> Option<String> {
    let mut plain = None;
    for line in text.lines() {
        let line = line.trim();
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().trim_matches('"').to_string();
        if v.is_empty() || v == "null" {
            continue;
        }
        match k.trim().to_ascii_lowercase().as_str() {
            "https-proxy" => return Some(v),
            "proxy" => plain = Some(v),
            _ => {}
        }
    }
    plain
}

/// `proxy` of a pip config file (any section).
pub fn proxy_in_pipconf(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once(['=', ':'])?;
        (k.trim().eq_ignore_ascii_case("proxy") && !v.trim().is_empty()).then(|| v.trim().to_string())
    })
}

/// A proxy variable of `/etc/environment`.
pub fn proxy_in_environment_file(text: &str) -> Option<String> {
    for key in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"] {
        for line in text.lines() {
            let line = line.trim().trim_start_matches("export ").trim();
            if let Some(v) = line.strip_prefix(key).and_then(|r| r.strip_prefix('=')) {
                let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// A parser of a tool's file: the proxy that it names.
pub type Parser = fn(&str) -> Option<String>;

/// The files that the detection reads, in order, with their parser: those
/// of the user under `home`, then those of the system.
pub fn sources(home: Option<&Path>) -> Vec<(PathBuf, Parser)> {
    let mut out: Vec<(PathBuf, Parser)> = Vec::new();
    if let Some(home) = home {
        out.push((home.join(".gitconfig"), proxy_in_gitconfig));
        out.push((home.join(".config/git/config"), proxy_in_gitconfig));
        out.push((home.join(".npmrc"), proxy_in_npmrc));
        out.push((home.join(".config/pip/pip.conf"), proxy_in_pipconf));
        out.push((home.join(".pip/pip.conf"), proxy_in_pipconf));
    }
    out.push((PathBuf::from("/etc/gitconfig"), proxy_in_gitconfig));
    out.push((PathBuf::from("/etc/pip.conf"), proxy_in_pipconf));
    out.push((PathBuf::from("/etc/environment"), proxy_in_environment_file));
    out
}

/// The proxy of the first file of [`sources`] that names one, and the file;
/// `read` gives the text of a file, or none.
pub fn detect(home: Option<&Path>, read: &dyn Fn(&Path) -> Option<String>) -> Option<(String, PathBuf)> {
    sources(home).into_iter().find_map(|(path, parse)| {
        let text = read(&path)?;
        parse(&text).map(|proxy| (proxy, path))
    })
}

/// The text of the file at `path`, as the system has it, or none.
pub fn read_file(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_proxy_of_each_tool_config() {
        let git = "[core]\n\tbare = false\n[http]\n\tproxy = http://gw:3128\n";
        assert_eq!(proxy_in_gitconfig(git).as_deref(), Some("http://gw:3128"));
        assert_eq!(proxy_in_gitconfig("[user]\nproxy = x\n"), None);
        let npm = "registry=https://r\nproxy=http://a:1\nhttps-proxy=http://b:2\n";
        assert_eq!(proxy_in_npmrc(npm).as_deref(), Some("http://b:2"));
        assert_eq!(proxy_in_npmrc("proxy=null\n"), None);
        let pip = "[global]\nproxy = http://p:8080\n";
        assert_eq!(proxy_in_pipconf(pip).as_deref(), Some("http://p:8080"));
        let env = "PATH=/usr/bin\nhttps_proxy=\"http://e:3128\"\n";
        assert_eq!(proxy_in_environment_file(env).as_deref(), Some("http://e:3128"));
    }

    #[test]
    fn detection_takes_the_first_file() {
        let home = std::env::temp_dir().join(format!("pod-proxy-detect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(".npmrc"), "proxy=http://npm:1\n").unwrap();
        let (proxy, path) = detect(Some(&home), &read_file).unwrap();
        assert_eq!(proxy, "http://npm:1");
        assert!(path.ends_with(".npmrc"));
        std::fs::write(home.join(".gitconfig"), "[http]\nproxy = http://git:2\n").unwrap();
        assert_eq!(detect(Some(&home), &read_file).unwrap().0, "http://git:2");
        let _ = std::fs::remove_dir_all(&home);
    }
}
