//! The socket of a running WARP proxy of the caller (podnet's `warp up`,
//! PodUtils/podnet@T-070).
//!
//! A command that uses the running proxy (podnet's `warp exec`, `warp
//! shell`, `warp nc`) tries these sockets, in this order, and takes the
//! first that answers:
//!
//! 1. the socket that the user gave (when it is given, no other socket is
//!    tried);
//! 2. the socket of the caller's variable;
//! 3. `warp.sock` in the run directory of the caller;
//! 4. the socket of the pointer file that the proxy writes at a fixed place
//!    of the user: `$XDG_RUNTIME_DIR/NAME/warp-up.json`, else
//!    `NAME-<uid>/warp-up.json` in the temporary directory.
//!
//! The pointer file has mode 0600 in a directory of mode 0700, and is read
//! only when this user owns it. It names the socket and the state directory
//! of the proxy, so that a proxy with another state directory is found.
//! The caller gives each path and each value: the crate reads no variable.

use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use pod_core::{Class, Failure, Stage};
use serde::{Deserialize, Serialize};

/// The name of the pointer file in its directory.
pub const POINTER_FILE: &str = "warp-up.json";
/// The name of the socket in the run directory.
pub const SOCKET_FILE: &str = "warp.sock";

/// The pointer file of a running proxy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pointer {
    /// The Unix socket of the proxy.
    pub socket: PathBuf,
    /// The state directory of the proxy, when it has one.
    pub state_dir: Option<PathBuf>,
}

/// The directory of the pointer file of the program `name` (podnet:
/// `podnet`): `runtime/NAME` when `runtime` (the caller's
/// `XDG_RUNTIME_DIR`) is a directory, else `NAME-<uid>` in `temp`. It does
/// not depend on the state directory.
pub fn pointer_dir_of(runtime: Option<PathBuf>, temp: PathBuf, name: &str) -> PathBuf {
    match runtime.filter(|r| !r.as_os_str().is_empty() && r.is_dir()) {
        Some(runtime) => runtime.join(name),
        None => temp.join(format!("{name}-{}", crate::files::this_uid())),
    }
}

/// Write the pointer file `path` (mode 0600, a new directory mode 0700).
pub fn write_pointer(path: &Path, pointer: &Pointer) -> Result<(), Failure> {
    let bytes = serde_json::to_vec(pointer).expect("a pointer is JSON");
    crate::files::write_private(path, &bytes).map_err(|e| {
        let text = format!("the pointer file {}", path.display());
        Failure::caused(Stage::Host, Class::of_io(e.kind()), text, e)
    })
}

/// Read the pointer file `path`: none when it is missing, when another user
/// owns it, or when other users can read or write it.
pub fn read_pointer_at(path: &Path) -> Option<Pointer> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.uid() != crate::files::this_uid() || meta.mode() & 0o077 != 0 {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Remove the pointer file `path` when it names `socket`.
pub fn remove_pointer(path: &Path, socket: &Path) {
    if read_pointer_at(path).is_some_and(|p| p.socket == socket) {
        let _ = std::fs::remove_file(path);
    }
}

/// The sockets to try, in order, each with where it came from: `given` (the
/// user's socket, with the caller's words, as `--socket`) alone when it is
/// there; else `variable` (with the name of the caller's variable),
/// `warp.sock` of `run_dir`, and the socket of `pointer`. A path comes one
/// time, and an empty path is no path.
pub fn candidates_of(
    given: Option<(&'static str, PathBuf)>,
    variable: Option<(&'static str, PathBuf)>,
    run_dir: Option<PathBuf>,
    pointer: Option<Pointer>,
) -> Vec<(&'static str, PathBuf)> {
    if let Some(given) = given {
        return vec![given];
    }
    let mut out: Vec<(&'static str, PathBuf)> = Vec::new();
    let mut add = |from, path: PathBuf| {
        if !path.as_os_str().is_empty() && !out.iter().any(|(_, p)| *p == path) {
            out.push((from, path));
        }
    };
    if let Some((from, path)) = variable {
        add(from, path);
    }
    if let Some(dir) = run_dir {
        add("the run directory", dir.join(SOCKET_FILE));
    }
    if let Some(pointer) = pointer {
        add("the pointer file", pointer.socket);
    }
    out
}

/// Whether a stream connects to `socket`.
pub fn answers(socket: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(socket).is_ok()
}

/// The first socket of `tried` that answers. Else `Unavailable` (exit 69)
/// that names each socket tried, and the state directory of `pointer`.
pub fn running(tried: &[(&'static str, PathBuf)], pointer: Option<&Pointer>) -> Result<PathBuf, Failure> {
    if let Some((_, socket)) = tried.iter().find(|(_, s)| answers(s)) {
        return Ok(socket.clone());
    }
    Err(Failure::new(Stage::Warp, Class::Unavailable, not_running(tried, pointer)))
}

/// The text of the failure when no socket answers.
fn not_running(tried: &[(&'static str, PathBuf)], pointer: Option<&Pointer>) -> String {
    let mut text = String::from("the WARP proxy is not running (no answer on ");
    if tried.is_empty() {
        text.push_str("any socket: no run directory");
    }
    let list: Vec<String> = tried.iter().map(|(from, p)| format!("{} ({from})", p.display())).collect();
    text.push_str(&list.join(", "));
    text.push(')');
    if let Some(dir) = pointer.and_then(|p| p.state_dir.as_ref()) {
        text.push_str(&format!("; the last proxy had the state directory {}", dir.display()));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const VARIABLE: &str = "PODNET_WARP_SOCKET";

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn the_lookup_order_of_the_warp_socket() {
        let pointer = Pointer { socket: p("/other/run/warp.sock"), state_dir: Some(p("/other")) };
        let all = candidates_of(None, Some((VARIABLE, p("/v/warp.sock"))), Some(p("/run")), Some(pointer.clone()));
        assert_eq!(
            all,
            [
                (VARIABLE, p("/v/warp.sock")),
                ("the run directory", p("/run/warp.sock")),
                ("the pointer file", p("/other/run/warp.sock")),
            ]
        );
        // The given socket alone.
        let given = Some(("--socket", p("/s.sock")));
        let only = candidates_of(given, Some((VARIABLE, p("/v"))), Some(p("/run")), Some(pointer));
        assert_eq!(only, [("--socket", p("/s.sock"))]);
        // A path is tried one time; an empty variable is no path.
        let same = Pointer { socket: p("/run/warp.sock"), state_dir: None };
        let two = candidates_of(None, Some((VARIABLE, p(""))), Some(p("/run")), Some(same));
        assert_eq!(two, [("the run directory", p("/run/warp.sock"))]);
    }

    #[test]
    fn the_pointer_dir_does_not_depend_on_the_state_directory() {
        let tmp = crate::test_dir("find-dir");
        let rt = tmp.join("rt");
        std::fs::create_dir(&rt).unwrap();
        assert_eq!(pointer_dir_of(Some(rt.clone()), p("/tmp"), "podnet"), rt.join("podnet"));
        let uid = crate::files::this_uid();
        let want = p("/tmp").join(format!("podnet-{uid}"));
        assert_eq!(pointer_dir_of(None, p("/tmp"), "podnet"), want);
        // A runtime directory that does not exist is not used.
        assert_eq!(pointer_dir_of(Some(tmp.join("no")), p("/tmp"), "podnet"), want);
    }

    #[test]
    fn a_pointer_file_that_others_can_read_is_refused() {
        use std::os::unix::fs::PermissionsExt as _;
        let tmp = crate::test_dir("find-mode");
        let path = tmp.join(POINTER_FILE);
        let pointer = Pointer { socket: p("/r/warp.sock"), state_dir: Some(p("/s")) };
        write_pointer(&path, &pointer).unwrap();
        assert_eq!(read_pointer_at(&path), Some(pointer));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(read_pointer_at(&path), None);
        // A pointer of another socket stays; the pointer of this one goes.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        remove_pointer(&path, &p("/r/other.sock"));
        assert!(path.exists());
        remove_pointer(&path, &p("/r/warp.sock"));
        assert!(!path.exists());
    }

    #[test]
    fn warp_exec_finds_warp_up_of_another_state_directory() {
        // A listener stands in for the socket of a proxy with another state
        // directory: only the pointer file names it.
        let tmp = crate::test_dir("find-other");
        let other = tmp.join("other-run/warp.sock");
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(&other).unwrap();
        let pointer = Pointer { socket: other.clone(), state_dir: Some(tmp.join("other")) };
        let tried = candidates_of(None, None, Some(tmp.join("own-run")), Some(pointer.clone()));
        assert_eq!(running(&tried, Some(&pointer)).unwrap(), other);
        // With no answer, the failure names each socket and the state
        // directory of the pointer file.
        drop(listener);
        std::fs::remove_file(&other).unwrap();
        let failure = running(&tried, Some(&pointer)).unwrap_err();
        assert_eq!(failure.class, Class::Unavailable);
        let text = failure.text();
        assert!(text.contains("own-run/warp.sock (the run directory)"), "{text}");
        assert!(text.contains("other-run/warp.sock (the pointer file)"), "{text}");
        let state = format!("the state directory {}", tmp.join("other").display());
        assert!(text.ends_with(&state), "{text}");
    }
}
