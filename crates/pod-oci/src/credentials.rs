//! The logins of registries. No credential gets to a log, an error or a
//! result file.
//!
//! One read path: each credential that the client sends comes through
//! [`Logins::lookup`]. [`Files`] reads the two files of the machines of the
//! audience, docker's `config.json` and the `containers/auth.json` of podman,
//! at the paths that the caller gives. A `credsStore` or `credHelpers` helper
//! runs when a file names one, and a helper that fails is a refusal by name,
//! not a silent fall back to no login: a fall back changes a refused
//! permission into a 404 of a repository that exists.
//!
//! Nothing here prints. [`Credential`] has no debug view of the secret, and
//! the password never gets to a failure. A test checks it with a canary.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pod_core::{Class, Failure, Stage};

use crate::error::Result;

mod helper;
pub use helper::{helper_erase, helper_login, helper_store};

/// A registry login. In memory only: never logged, never in an error.
#[derive(Clone)]
pub struct Credential {
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for Credential {
    /// The username can print; the password never does. A failed test that
    /// calls `expect_err` prints this, where a derived `Debug` would show
    /// the secret.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Credential {{ username: {:?}, password: <redacted> }}", self.username)
    }
}

/// Where the client gets the login of a host.
pub trait Logins: Send + Sync {
    /// The login of `host`. `Ok(None)`: no login, and the request goes with
    /// no credential. `Err`: a named helper failed, and the failure names it.
    fn lookup(&self, host: &str) -> Result<Option<Credential>>;
}

/// No login for each host.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoLogins;

impl Logins for NoLogins {
    fn lookup(&self, _host: &str) -> Result<Option<Credential>> {
        Ok(None)
    }
}

/// The two credential files: docker's `config.json` and the `auth.json` of
/// the containers tools. The caller gives the paths, or the directories of
/// [`Files::new`]. The crate reads no variable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Files {
    pub docker: PathBuf,
    pub containers: PathBuf,
}

impl Files {
    /// The paths of docker and podman from the home directory and the
    /// runtime directory (`XDG_RUNTIME_DIR`) of the caller:
    /// `HOME/.docker/config.json`, and `RUNTIME/containers/auth.json` or,
    /// with no runtime directory, `HOME/.config/containers/auth.json`. A
    /// path that cannot be made is empty, and a lookup finds nothing there.
    pub fn new(home: Option<&Path>, runtime_dir: Option<&Path>) -> Files {
        let docker = home.map(|h| h.join(".docker/config.json")).unwrap_or_default();
        let containers = runtime_dir
            .map(|d| d.join("containers/auth.json"))
            .or_else(|| home.map(|h| h.join(".config/containers/auth.json")))
            .unwrap_or_default();
        Files { docker, containers }
    }

    /// [`store_login_in`] on these files.
    pub fn store_login(&self, server: &str, username: &str, password: &str) -> Result<()> {
        store_login_in(&self.docker, &self.containers, server, username, password)
    }

    /// [`remove_login_in`] on these files.
    pub fn remove_login(&self, server: &str) -> Result<bool> {
        remove_login_in(&self.docker, &self.containers, server)
    }
}

impl Logins for Files {
    fn lookup(&self, host: &str) -> Result<Option<Credential>> {
        lookup_in(&self.docker, &self.containers, host)
    }
}

/// The login of `host` from two files. The containers file wins where both
/// name the host: it is the file of the runtime audience. Absent files,
/// malformed files and unknown hosts all give `Ok(None)`.
pub fn lookup_in(docker_config: &Path, containers_auth: &Path, host: &str) -> Result<Option<Credential>> {
    // The helper comes before the files: a file that names a helper for
    // this host gives the trust to it, and the files beside it do not win.
    if let Some(name) = helper_for(docker_config, containers_auth, host) {
        let server = format!("https://{host}/v2/");
        return helper_login(&name, &server).map(Some);
    }
    if let Some(cred) = read_auth_file(containers_auth).get(host).cloned() {
        return Ok(Some(cred));
    }
    Ok(read_auth_file(docker_config).get(host).cloned())
}

/// The `Authorization` value for `cred`: `Basic` over `user:password`.
pub fn basic_header(cred: &Credential) -> String {
    use base64::Engine as _;
    let joined = format!("{}:{}", cred.username, cred.password);
    format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(joined))
}

/// The host logins of one `auths` document. An entry that does not decode
/// is skipped: a file with nothing usable for a host is no login for it,
/// not a failure of the lookup.
pub fn parse_auth_json(text: &str) -> HashMap<String, Credential> {
    let mut out = HashMap::new();
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(text) else {
        return out;
    };
    let Some(auths) = doc.get("auths").and_then(|a| a.as_object()) else {
        return out;
    };
    for (host, entry) in auths {
        if let Some(cred) = parse_entry(entry) {
            out.insert(host.clone(), cred);
        }
    }
    out
}

/// One `auths` entry: the `auth` field first, then the `username` and
/// `password` pair of podman's file. Each other entry is not a login.
fn parse_entry(entry: &serde_json::Value) -> Option<Credential> {
    use base64::Engine as _;
    if let Some(encoded) = entry.get("auth").and_then(|a| a.as_str()) {
        let raw = base64::engine::general_purpose::STANDARD.decode(encoded).ok()?;
        let text = String::from_utf8(raw).ok()?;
        let (username, password) = text.split_once(':')?;
        if username.is_empty() {
            return None;
        }
        return Some(Credential { username: username.into(), password: password.into() });
    }
    let username = entry.get("username").and_then(|u| u.as_str())?;
    let password = entry.get("password").and_then(|p| p.as_str())?;
    if username.is_empty() {
        return None;
    }
    Some(Credential { username: username.into(), password: password.into() })
}

/// The host logins of one `auths` file. An absent or unreadable file is
/// empty.
fn read_auth_file(path: &Path) -> HashMap<String, Credential> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    parse_auth_json(&text)
}

/// The helper that the files name for `host`: `credHelpers` of the host
/// first, then the global `credsStore`. Both of docker's names are read.
fn helper_for(docker_config: &Path, containers_auth: &Path, host: &str) -> Option<String> {
    for path in [docker_config, containers_auth] {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if let Some(name) = doc.get("credHelpers").and_then(|h| h.get(host)).and_then(|h| h.as_str()) {
            return Some(name.into());
        }
        if let Some(name) = doc.get("credsStore").and_then(|h| h.as_str()) {
            return Some(name.into());
        }
    }
    None
}

/// Store a login: through the helper that the files name for the host,
/// else in docker's file, or in the containers file where that file holds
/// the login of this host already, so that one host is in one file only.
pub fn store_login_in(
    docker_config: &Path,
    containers_auth: &Path,
    server: &str,
    username: &str,
    password: &str,
) -> Result<()> {
    let host = server_host(server);
    // The helper first, as for the read: a file that names a helper for this
    // host gets no file entry beside it. Nothing is written unless the
    // helper answers.
    if let Some(name) = helper_for(docker_config, containers_auth, host) {
        let server_url = format!("https://{host}/v2/");
        return helper_store(&name, &server_url, username, password);
    }
    // One home: the file that holds this host wins, else docker's.
    let target = if read_auth_file(containers_auth).contains_key(host) { containers_auth } else { docker_config };
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| local(format!("would not create {}", parent.display()), e))?;
    }
    let before = std::fs::read_to_string(target).unwrap_or_default();
    let after = upsert_auth_json(&before, host, username, password);
    write_owner_only(target, &after)
}

/// Remove the login of `server`: through the helper that the files name
/// for the host, else from the file that holds it. Missing files and
/// unknown hosts are not errors. True: an entry went. False is a logout
/// too, and the caller says so.
pub fn remove_login_in(docker_config: &Path, containers_auth: &Path, server: &str) -> Result<bool> {
    let host = server_host(server);
    // The helper first, as for the read and the store: a file entry beside
    // a named helper does not decide the logout.
    if let Some(name) = helper_for(docker_config, containers_auth, host) {
        let server_url = format!("https://{host}/v2/");
        helper_erase(&name, &server_url)?;
        return Ok(true);
    }
    // One home, in the order of the read: the containers file wins where
    // both name the host, and only the file that holds it is written.
    for path in [containers_auth, docker_config] {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let (after, removed) = delete_auth_json(&text, host);
        if removed {
            write_owner_only(path, &after)?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Delete the `auth` entry of `server`, and keep each other key and host.
/// Gives the new document and whether an entry went. An empty or broken
/// document holds nothing to remove.
pub fn delete_auth_json(text: &str, server: &str) -> (String, bool) {
    let Ok(mut doc) = serde_json::from_str::<serde_json::Value>(text) else {
        return (text.into(), false);
    };
    let Some(auths) = doc.get_mut("auths").and_then(|a| a.as_object_mut()) else {
        return (text.into(), false);
    };
    let removed = auths.remove(server).is_some();
    if !removed {
        return (text.into(), false);
    }
    (serde_json::to_string(&doc).unwrap_or_else(|_| text.into()), true)
}

/// Write `text` to `path`, readable by the owner only. A new file starts
/// with the mode 0600; an existing file keeps its owner and gets the mode
/// 0600. A credential file that a umask left readable is the defect.
fn write_owner_only(path: &Path, text: &str) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| local(format!("would not write {}", path.display()), e))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| local(format!("would not narrow {}", path.display()), e))?;
    file.write_all(text.as_bytes()).map_err(|e| local(format!("would not write {}", path.display()), e))
}

/// Insert or replace the `auth` entry of `server`, and keep each other key
/// and host. An empty or broken document starts again: a login does not
/// fail on it.
pub fn upsert_auth_json(text: &str, server: &str, username: &str, password: &str) -> String {
    use base64::Engine as _;
    let joined = format!("{username}:{password}");
    let encoded = base64::engine::general_purpose::STANDARD.encode(joined);
    let mut doc: serde_json::Value = serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!({}));
    if !doc.is_object() {
        doc = serde_json::json!({});
    }
    doc["auths"][server]["auth"] = serde_json::Value::String(encoded);
    serde_json::to_string(&doc).unwrap_or_default()
}

/// The key of a server as the user names it, with no scheme and no path:
/// `https://index.docker.io/v1/` and `index.docker.io` name one host.
pub fn server_host(server: &str) -> &str {
    let bare = server.strip_prefix("https://").or_else(|| server.strip_prefix("http://")).unwrap_or(server);
    bare.split('/').next().unwrap_or(bare)
}

/// A failure of a local file of logins, at [`Stage::Host`].
fn local(text: String, e: std::io::Error) -> Failure {
    let class = Class::of_io(e.kind());
    Failure::caused(Stage::Host, class, format!("{text}: {e}"), e)
}

#[cfg(test)]
mod tests;
