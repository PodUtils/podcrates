//! The files of a node in another user's tree (`cache_owner/`), with the
//! places of podssh's app. The owners of a directory are a fact of Unix.

#![cfg(unix)]

mod cache_owner;
mod cleanup;

use std::path::PathBuf;

use pod_core::files::App;

const APP: App = App { name: "podssh", dir_env: "PODSSH_CACHE_DIR" };
/// A kept file with the name of a relay token, and a made-up body.
const TOKEN_FILE: &str = "relay-token-relay.example.json";
const TOKEN: &str = "ephm1.1900000000000.forward.TESTONLYNOTACREDENTIAL";

/// A fresh, empty scratch directory unique to one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pod-identity-owner-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    pod_core::files::private_dir(&dir).unwrap();
    cleanup::at_test_end(&dir);
    dir
}

/// An environment for the places: each pair is a variable.
fn env<'a>(pairs: &'a [(&'a str, PathBuf)]) -> impl Fn(&str) -> Option<std::ffi::OsString> + 'a {
    move |name| pairs.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone().into_os_string())
}
