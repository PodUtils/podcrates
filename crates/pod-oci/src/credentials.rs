//! `TODO/image.md` T-0209: registry credentials, without one ever entering
//! this tree's logs, errors, or result files.
//!
//! One read path: every credential podvm sends comes through [`lookup`],
//! which reads the two files the audience's machines already have,
//! `~/.docker/config.json` and `$XDG_RUNTIME_DIR/containers/auth.json`.
//! A named `credsStore` or `credHelpers` helper is executed when the config
//! names one, and a helper that fails is a named refusal rather than a
//! silent fall back to anonymous: falling back turns a permission problem
//! into a 404 about a repository that exists.
//!
//! ⛔ Nothing here prints. [`Credential`] exposes no debug view of the
//! secret, and the password never reaches a [`crate::error::Error`]. A test
//! below pins that with a canary.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A registry login. Held in memory, never logged, never put in an error.
#[derive(Clone)]
pub struct Credential {
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for Credential {
    /// ⛔ The username may print; the password never does. Test failures
    /// calling `expect_err` render this, which is exactly where a derived
    /// `Debug` would leak the secret it was written to catch.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Credential {{ username: {:?}, password: <redacted> }}",
            self.username
        )
    }
}

/// Credentials for `host`.
///
/// `Ok(None)` means no file names any: the pull proceeds anonymously and a
/// private repository answers 401, which names the registry. Absent files,
/// malformed files, and unknown hosts all answer `Ok(None)`. `Err` means a
/// named helper failed, and the string names the helper.
pub fn lookup(host: &str) -> Result<Option<Credential>, String> {
    let (docker, containers) = default_paths();
    lookup_in(&docker, &containers, host)
}

/// Credentials for `host` from two explicit files. [`lookup`] resolves the
/// paths; tests point them at scratch. The containers file wins where both
/// name the host: it is the runtime audience's file.
pub fn lookup_in(
    docker_config: &Path,
    containers_auth: &Path,
    host: &str,
) -> Result<Option<Credential>, String> {
    // ⛔ The helper is consulted before the files: a config that names a
    // helper for this host delegates trust explicitly, and files beside it
    // must not silently win over that delegation.
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
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(joined)
    )
}

/// Parse one `auths` document into host logins. Pure, so tests own it.
/// Entries that do not decode are skipped: a file that names nothing usable
/// for a host is absence for that host, not a failure of the lookup.
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

/// One `auths` entry: the `auth` field first, then the `username` plus
/// `password` pair podman's file uses. Anything else is not a login.
fn parse_entry(entry: &serde_json::Value) -> Option<Credential> {
    use base64::Engine as _;
    if let Some(encoded) = entry.get("auth").and_then(|a| a.as_str()) {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .ok()?;
        let text = String::from_utf8(raw).ok()?;
        let (username, password) = text.split_once(':')?;
        if username.is_empty() {
            return None;
        }
        return Some(Credential {
            username: username.into(),
            password: password.into(),
        });
    }
    let username = entry.get("username").and_then(|u| u.as_str())?;
    let password = entry.get("password").and_then(|p| p.as_str())?;
    if username.is_empty() {
        return None;
    }
    Some(Credential {
        username: username.into(),
        password: password.into(),
    })
}

/// Read one `auths` file into host logins. Absent or unreadable is empty.
fn read_auth_file(path: &Path) -> HashMap<String, Credential> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    parse_auth_json(&text)
}

/// The helper the configs name for `host`, if any: per-host `credHelpers`
/// first, then the global `credsStore`. Docker's two spellings, both read.
fn helper_for(docker_config: &Path, containers_auth: &Path, host: &str) -> Option<String> {
    for path in [docker_config, containers_auth] {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if let Some(name) = doc
            .get("credHelpers")
            .and_then(|h| h.get(host))
            .and_then(|h| h.as_str())
        {
            return Some(name.into());
        }
        if let Some(name) = doc.get("credsStore").and_then(|h| h.as_str()) {
            return Some(name.into());
        }
    }
    None
}

/// The login from the helper the config names for `server_url`.
///
/// # Errors
///
/// Returns an error naming the helper where it is missing, fails, or prints
/// nothing parseable. A failed helper is never a silent fall back.
pub fn helper_login(helper: &str, server_url: &str) -> Result<Credential, String> {
    use std::io::Write;
    use std::process::Stdio;
    let bin = format!("docker-credential-{helper}");
    let mut child = std::process::Command::new(&bin)
        .arg("get")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("credential helper {bin} would not start: {e}"))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| format!("credential helper {bin} takes no stdin"))?
        .write_all(server_url.as_bytes())
        .map_err(|e| format!("credential helper {bin} would not take the server URL: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("credential helper {bin} has no exit to read: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "credential helper {bin} refused {server_url}: exit {}",
            out.status.code().map_or("signal".into(), |c| c.to_string())
        ));
    }
    let reply: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| {
        format!("credential helper {bin} printed nothing parseable for {server_url}")
    })?;
    let username = reply
        .get("Username")
        .and_then(|u| u.as_str())
        .filter(|u| !u.is_empty())
        .ok_or_else(|| format!("credential helper {bin} named no user for {server_url}"))?;
    let password = reply.get("Secret").and_then(|s| s.as_str()).unwrap_or("");
    Ok(Credential {
        username: username.into(),
        password: password.into(),
    })
}

/// Store a login through the named helper: `docker-credential-<helper>`
/// `store` with `{"ServerURL","Username","Secret"}` on stdin.
///
/// # Errors
///
/// Returns an error naming the helper where it would not start or where
/// storing fails. Never falls back to the file: the config named the
/// helper, so the file beside it must not silently win.
pub fn helper_store(
    helper: &str,
    server_url: &str,
    username: &str,
    password: &str,
) -> Result<(), String> {
    use std::io::Write;
    use std::process::Stdio;
    let bin = format!("docker-credential-{helper}");
    let mut child = std::process::Command::new(&bin)
        .arg("store")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("credential helper {bin} would not start: {e}"))?;
    let body = serde_json::json!({
        "ServerURL": server_url,
        "Username": username,
        "Secret": password,
    });
    let text = serde_json::to_string(&body)
        .map_err(|e| format!("credential helper {bin} takes no JSON: {e}"))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| format!("credential helper {bin} takes no stdin"))?
        .write_all(text.as_bytes())
        .map_err(|e| format!("credential helper {bin} would not take the login: {e}"))?;
    let status = child
        .wait()
        .map_err(|e| format!("credential helper {bin} has no exit to read: {e}"))?;
    if !status.success() {
        return Err(format!(
            "credential helper {bin} refused to store the login for {server_url}: exit {}",
            status.code().map_or("signal".into(), |c| c.to_string())
        ));
    }
    Ok(())
}

/// Store a login: through the named helper where one is configured for the
/// host, else in docker's file (or the containers file where that file
/// already holds this host's login, so one host never splits across two
/// files). [`store_login`] resolves the paths; tests point them at scratch.
pub fn store_login(server: &str, username: &str, password: &str) -> Result<(), String> {
    let (docker, containers) = default_paths();
    store_login_in(&docker, &containers, server, username, password)
}

/// [`store_login`] against explicit files.
pub fn store_login_in(
    docker_config: &Path,
    containers_auth: &Path,
    server: &str,
    username: &str,
    password: &str,
) -> Result<(), String> {
    let host = server_host(server);
    // ⛔ The helper first, like the read path: a config that names a helper
    // for this host must not gain a file entry beside it. Nothing is written
    // unless the helper answers.
    if let Some(name) = helper_for(docker_config, containers_auth, host) {
        let server_url = format!("https://{host}/v2/");
        return helper_store(&name, &server_url, username, password);
    }
    // One home: the file that already holds this host wins, else docker's.
    let target = if read_auth_file(containers_auth).contains_key(host) {
        containers_auth
    } else {
        docker_config
    };
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("would not create {}: {e}", parent.display()))?;
    }
    let before = std::fs::read_to_string(target).unwrap_or_default();
    let after = upsert_auth_json(&before, host, username, password);
    write_owner_only(target, &after)
}

/// `erase` with `{"ServerURL"}` on stdin (docker-credential protocol).
///
/// # Errors
///
/// Returns an error naming the helper where it would not start or where
/// erasing fails. Like [`helper_store`], never falls back to the file: a
/// failed erase keeps everything and says so, because a logout that
/// silently kept the credential is the defect this entry exists to stop.
pub fn helper_erase(helper: &str, server_url: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::Stdio;
    let bin = format!("docker-credential-{helper}");
    let mut child = std::process::Command::new(&bin)
        .arg("erase")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("credential helper {bin} would not start: {e}"))?;
    let body = serde_json::json!({
        "ServerURL": server_url,
    });
    let text = serde_json::to_string(&body)
        .map_err(|e| format!("credential helper {bin} takes no JSON: {e}"))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| format!("credential helper {bin} takes no stdin"))?
        .write_all(text.as_bytes())
        .map_err(|e| format!("credential helper {bin} would not take the erase: {e}"))?;
    let status = child
        .wait()
        .map_err(|e| format!("credential helper {bin} has no exit to read: {e}"))?;
    if !status.success() {
        return Err(format!(
            "credential helper {bin} refused to erase the login for {server_url}: exit {}",
            status.code().map_or("signal".into(), |c| c.to_string())
        ));
    }
    Ok(())
}

/// Remove the login for `server`: through the named helper where one is
/// configured for the host, else from whichever file holds it. Missing
/// files and unknown hosts are not errors. Returns true where an entry
/// went away; a `false` is still logged out, and the caller says so.
pub fn remove_login(server: &str) -> Result<bool, String> {
    let (docker, containers) = default_paths();
    remove_login_in(&docker, &containers, server)
}

/// [`remove_login`] against explicit files.
pub fn remove_login_in(
    docker_config: &Path,
    containers_auth: &Path,
    server: &str,
) -> Result<bool, String> {
    let host = server_host(server);
    // ⛔ The helper first, like both other paths: a config that names a
    // helper for this host delegates trust explicitly, and a file entry
    // beside it (or its absence) must not decide the logout.
    if let Some(name) = helper_for(docker_config, containers_auth, host) {
        let server_url = format!("https://{host}/v2/");
        helper_erase(&name, &server_url)?;
        return Ok(true);
    }
    // One home, the read path's order: the containers file wins where
    // both name the host, and only the file holding it is rewritten.
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

/// Delete the `auth` entry for `server`, keeping every other key and
/// host. Returns the rewritten document and whether an entry went away.
/// An empty or broken document holds nothing to remove.
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
    (
        serde_json::to_string(&doc).unwrap_or_else(|_| text.into()),
        true,
    )
}

/// Write `text` to `path` with owner-only permissions.
///
/// Creates the file where missing; an existing file keeps its ownership and
/// only its mode is narrowed. A credential file a umask left readable is
/// the defect this exists for.
fn write_owner_only(path: &Path, text: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).map_err(|e| format!("would not write {}: {e}", path.display()))?;
    let perm = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, perm)
        .map_err(|e| format!("would not narrow {}: {e}", path.display()))?;
    Ok(())
}

/// Where [`lookup`] reads: docker's file, then the containers file.
///
/// The containers path follows the runtime first: `$XDG_RUNTIME_DIR` where
/// set, else `~/.config/containers/auth.json`.
pub fn default_paths() -> (PathBuf, PathBuf) {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let docker = home
        .as_ref()
        .map(|h| h.join(".docker/config.json"))
        .unwrap_or_default();
    let containers = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|d| d.join("containers/auth.json"))
        .or_else(|| {
            home.as_ref()
                .map(|h| h.join(".config/containers/auth.json"))
        })
        .unwrap_or_default();
    (docker, containers)
}

/// Insert or replace the `auth` entry for `server`, keeping every other
/// key and host. `podvm login` writes through here; an empty or broken
/// document starts fresh rather than failing the login.
pub fn upsert_auth_json(text: &str, server: &str, username: &str, password: &str) -> String {
    use base64::Engine as _;
    let joined = format!("{username}:{password}");
    let encoded = base64::engine::general_purpose::STANDARD.encode(joined);
    let mut doc: serde_json::Value =
        serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!({}));
    if !doc.is_object() {
        doc = serde_json::json!({});
    }
    doc["auths"][server]["auth"] = serde_json::Value::String(encoded);
    serde_json::to_string(&doc).unwrap_or_default()
}

/// The lookup key for a server as the user names it: scheme and path
/// stripped, so `https://index.docker.io/v1/` and `index.docker.io` name
/// one host.
pub fn server_host(server: &str) -> &str {
    let bare = server
        .strip_prefix("https://")
        .or_else(|| server.strip_prefix("http://"))
        .unwrap_or(server);
    bare.split('/').next().unwrap_or(bare)
}

#[cfg(test)]
mod tests;
