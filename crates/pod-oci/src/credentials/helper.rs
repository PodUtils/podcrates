//! The credential helpers of docker: `docker-credential-NAME get`, `store`
//! and `erase`, with JSON on stdin and on stdout. A helper that is missing,
//! that fails or that prints nothing usable is a refusal that names the
//! helper, never a silent fall back. The stderr of a helper is discarded:
//! it can hold the secret.

use std::io::Write as _;
use std::process::{Command, Stdio};

use pod_core::{Class, Failure, Stage};

use super::Credential;
use crate::error::Result;

/// A refusal of a helper, at [`Stage::Host`].
fn refused(class: Class, text: String) -> Failure {
    Failure::new(Stage::Host, class, text)
}

/// Run `docker-credential-HELPER VERB` with `input` on stdin. Gives the
/// program name and its stdout when `capture`, or an empty stdout.
fn run(helper: &str, verb: &str, input: &[u8], capture: bool, what: &str) -> Result<(String, Vec<u8>)> {
    let bin = format!("docker-credential-{helper}");
    let mut child = Command::new(&bin)
        .arg(verb)
        .stdin(Stdio::piped())
        .stdout(if capture { Stdio::piped() } else { Stdio::null() })
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| refused(Class::Unavailable, format!("credential helper {bin} would not start: {e}")))?;
    let fed = match child.stdin.as_mut() {
        Some(stdin) => stdin
            .write_all(input)
            .map_err(|e| refused(Class::Unavailable, format!("credential helper {bin} would not take {what}: {e}"))),
        None => Err(refused(Class::Unavailable, format!("credential helper {bin} takes no stdin"))),
    };
    // The stdin closes here, so that the helper reads its end.
    drop(child.stdin.take());
    let out = child
        .wait_with_output()
        .map_err(|e| refused(Class::Unavailable, format!("credential helper {bin} has no exit to read: {e}")))?;
    fed?;
    if !out.status.success() {
        let code = out.status.code().map_or("signal".into(), |c| c.to_string());
        return Err(refused(Class::Refused, format!("credential helper {bin} refused {what}: exit {code}")));
    }
    Ok((bin, out.stdout))
}

/// The login from the helper `helper` for `server_url`.
pub fn helper_login(helper: &str, server_url: &str) -> Result<Credential> {
    let (bin, stdout) = run(helper, "get", server_url.as_bytes(), true, server_url)?;
    let unusable = |text: String| refused(Class::Protocol, text);
    let reply: serde_json::Value = serde_json::from_slice(&stdout)
        .map_err(|_| unusable(format!("credential helper {bin} printed nothing parseable for {server_url}")))?;
    let username = reply
        .get("Username")
        .and_then(|u| u.as_str())
        .filter(|u| !u.is_empty())
        .ok_or_else(|| unusable(format!("credential helper {bin} named no user for {server_url}")))?;
    let password = reply.get("Secret").and_then(|s| s.as_str()).unwrap_or("");
    Ok(Credential { username: username.into(), password: password.into() })
}

/// Store a login through the helper `helper`: `store` with
/// `{"ServerURL","Username","Secret"}` on stdin. No fall back to a file: the
/// file named the helper, so a file entry beside it does not win.
pub fn helper_store(helper: &str, server_url: &str, username: &str, password: &str) -> Result<()> {
    let body = serde_json::json!({ "ServerURL": server_url, "Username": username, "Secret": password });
    let what = format!("to store the login for {server_url}");
    run(helper, "store", body.to_string().as_bytes(), false, &what).map(|_| ())
}

/// Erase a login through the helper `helper`: `erase` with `{"ServerURL"}`
/// on stdin. No fall back to a file: a failed erase keeps each entry and
/// says so, as a logout that kept the credential with no word is the
/// defect.
pub fn helper_erase(helper: &str, server_url: &str) -> Result<()> {
    let body = serde_json::json!({ "ServerURL": server_url });
    let what = format!("to erase the login for {server_url}");
    run(helper, "erase", body.to_string().as_bytes(), false, &what).map(|_| ())
}
