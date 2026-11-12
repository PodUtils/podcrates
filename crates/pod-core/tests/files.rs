//! The private files of a program, in temporary directories: where they
//! go, what is trusted, and what each place of the environment gives. The
//! app of these tests is podssh's (`PODSSH_CACHE_DIR`, `podssh`).

mod cleanup;

use std::path::PathBuf;

use pod_core::files::{
    App, Chosen, candidate_dirs_from, chosen_from, chosen_note, load_file_from, store_file_in_first, store_noting,
};

const APP: App = App { name: "podssh", dir_env: "PODSSH_CACHE_DIR" };

/// A fresh, empty scratch directory unique to one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pod-core-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    pod_core::files::private_dir(&dir).unwrap();
    cleanup::at_test_end(&dir);
    dir
}

#[test]
fn file_names_cannot_escape_the_directory() {
    assert_eq!(pod_core::files::safe_name("Relay.Example"), "relay.example");
    assert_eq!(pod_core::files::safe_name("../../etc/passwd"), ".._.._etc_passwd");
}

#[cfg(unix)]
#[test]
fn files_others_could_read_or_symlinks_are_not_trusted() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("unix-perms");
    let path = store_file_in_first(std::slice::from_ref(&dir), "kept.json", b"kept").unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(load_file_from(std::slice::from_ref(&dir), "kept.json").as_deref(), Some("kept"));

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(load_file_from(std::slice::from_ref(&dir), "kept.json").is_none(), "a world-readable file is ignored");

    let other = scratch("unix-link");
    let real = store_file_in_first(std::slice::from_ref(&other), "kept.json", b"kept").unwrap();
    let linked_dir = scratch("unix-linked");
    std::os::unix::fs::symlink(&real, linked_dir.join("kept.json")).unwrap();
    assert!(load_file_from(&[linked_dir], "kept.json").is_none(), "a symlink is ignored");
}

#[test]
fn the_first_usable_directory_wins_and_unusable_ones_are_skipped() {
    let root = scratch("first");
    let blocked = root.join("blocked");
    std::fs::write(&blocked, b"a file where the directory should be").unwrap();
    let good = root.join("good");
    let path = store_file_in_first(&[blocked.join("cache"), good.clone()], "kept.json", b"kept").unwrap();
    assert!(path.starts_with(&good), "{path:?}");
    assert_eq!(load_file_from(&[blocked.join("cache"), good], "kept.json").as_deref(), Some("kept"));
}

#[test]
fn when_no_directory_works_the_error_names_each_one() {
    let root = scratch("none-works");
    let (a, b) = (root.join("a"), root.join("b"));
    std::fs::write(&a, b"x").unwrap();
    std::fs::write(&b, b"x").unwrap();
    let err = store_file_in_first(&[a.join("c"), b.join("c")], "kept.json", b"kept").unwrap_err();
    assert!(err.contains(&a.display().to_string()) && err.contains(&b.display().to_string()), "{err}");
}

/// An environment for [`candidate_dirs_from`]: each pair is a variable.
fn env<'a>(pairs: &'a [(&'a str, PathBuf)]) -> impl Fn(&str) -> Option<std::ffi::OsString> + 'a {
    move |name| pairs.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone().into_os_string())
}

/// The order: the directory of the app's variable, the user's cache
/// directory, the runtime directory, the temporary directory with the
/// user's tag, the working directory.
#[test]
fn the_order_follows_the_environment() {
    let root = scratch("order");
    let user_cache = if cfg!(windows) { "LOCALAPPDATA" } else { "XDG_CACHE_HOME" };
    let pairs = [
        ("PODSSH_CACHE_DIR", root.join("chosen")),
        (user_cache, root.join("cache")),
        ("XDG_RUNTIME_DIR", root.join("run")),
        ("TMPDIR", root.join("tmp")),
    ];
    let dirs = candidate_dirs_from(&APP, &env(&pairs), Some(root.join("work")));
    assert_eq!(dirs.len(), 5, "{dirs:?}");
    assert_eq!(dirs[0], root.join("chosen"));
    assert_eq!(dirs[1], root.join("cache").join("podssh"));
    assert_eq!(dirs[2], root.join("run").join("podssh"));
    assert!(dirs[3].starts_with(root.join("tmp")), "{dirs:?}");
    assert!(dirs[3].file_name().unwrap().to_string_lossy().starts_with("podssh-"), "{dirs:?}");
    assert_eq!(dirs[4], root.join("work").join(".podssh"));
    // With no variable, the user's places are missing, never guessed.
    let bare = candidate_dirs_from(&APP, &env(&[]), None);
    assert!(!bare.iter().any(|d| d.ends_with("podssh") && !d.to_string_lossy().contains("podssh-")), "{bare:?}");
}

/// PODSSH_CACHE_DIR=none gives no directory, and a store says why; a
/// relative path is not used.
#[test]
fn none_keeps_no_file_and_a_relative_path_is_not_used() {
    let root = scratch("none");
    for spelling in ["none", "NONE"] {
        let pairs = [("PODSSH_CACHE_DIR", PathBuf::from(spelling)), ("TMPDIR", root.clone())];
        assert_eq!(chosen_from(&APP, &env(&pairs)), Chosen::Nothing);
        assert!(candidate_dirs_from(&APP, &env(&pairs), Some(root.clone())).is_empty());
    }
    let err = store_noting(&[], "relay.example", b"x").unwrap_err();
    assert!(err.contains("the cache is off (none)"), "{err}");
    let pairs = [("PODSSH_CACHE_DIR", PathBuf::from("relative/dir")), ("TMPDIR", root.clone())];
    assert_eq!(chosen_from(&APP, &env(&pairs)), Chosen::Relative("relative/dir".into()));
    let dirs = candidate_dirs_from(&APP, &env(&pairs), None);
    assert!(!dirs.iter().any(|d| d.ends_with("dir")), "{dirs:?}");
    assert_eq!(chosen_from(&APP, &env(&[("PODSSH_CACHE_DIR", PathBuf::new())])), Chosen::Unset);
}

/// A named directory that refuses a write is skipped, the next one takes
/// the file, and the note names the refusal.
#[test]
fn a_refused_cache_dir_is_named_and_the_next_one_takes_the_file() {
    let root = scratch("refused");
    let blocked = root.join("blocked");
    std::fs::write(&blocked, b"a file where the directory should be").unwrap();
    let chosen = blocked.join("cache");
    let next = root.join("next");
    let stored = store_noting(&[chosen.clone(), next.clone()], "relay.example", b"body").unwrap();
    assert!(stored.path.starts_with(&next), "{stored:?}");
    let note = chosen_note(APP.dir_env, &Chosen::Dir(chosen.clone()), &stored).expect("a note");
    assert!(note.contains("PODSSH_CACHE_DIR could not be used") && note.contains("blocked"), "{note}");
    // A choice that was followed says nothing.
    let followed = store_noting(std::slice::from_ref(&next), "relay.example", b"body").unwrap();
    assert_eq!(chosen_note(APP.dir_env, &Chosen::Dir(next), &followed), None);
}

/// With no variable that names the temporary directory, the fixed places
/// of Unix come last, after each place that the environment gives.
#[cfg(unix)]
#[test]
fn with_no_temporary_variable_the_fixed_places_come_last() {
    let root = scratch("fixed");
    let pairs = [("XDG_CACHE_HOME", root.join("cache"))];
    let dirs = candidate_dirs_from(&APP, &env(&pairs), Some(root.join("work")));
    let n = dirs.len();
    assert!(dirs[n - 3].starts_with("/tmp") && dirs[n - 2].starts_with("/dev/shm"), "{dirs:?}");
    assert_eq!(dirs[0], root.join("cache").join("podssh"));
}

/// No absolute path stays in the code of the files module but the temporary
/// directory's two fixed places, in their one list, and the file of the user
/// database, whose home comes in place of another user's.
#[test]
fn the_scan_finds_no_other_absolute_path_in_the_cache() {
    let code = [
        include_str!("../src/files.rs"),
        include_str!("../src/files/dirs.rs"),
        include_str!("../src/files/own.rs"),
        include_str!("../src/files/owner.rs"),
    ]
    .concat();
    let mut found = Vec::new();
    for (i, _) in code.match_indices('"') {
        let rest = &code[i + 1..];
        let Some(end) = rest.find('"') else { continue };
        let word = &rest[..end];
        let windows = word.len() > 2 && word.as_bytes()[1] == b':' && word.as_bytes()[0].is_ascii_alphabetic();
        if word.starts_with('/') && word.len() > 1 || windows {
            found.push(word.to_string());
        }
    }
    found.sort();
    found.dedup();
    assert_eq!(found, ["/dev/shm", "/etc/passwd", "/tmp"], "{found:?}");
    let fixed = code.lines().filter(|l| l.contains("\"/tmp\"") || l.contains("\"/dev/shm\"")).count();
    assert_eq!(fixed, 1, "the fixed places are in one list");
}

/// `icacls FILE ARGS`, as a user would open a file to others.
#[cfg(windows)]
fn icacls(file: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("icacls").arg(file).args(args).output().expect("icacls runs");
    assert!(out.status.success(), "icacls {args:?}: {}", String::from_utf8_lossy(&out.stdout));
}

/// On Windows the owner and the access list say whether a file is closed,
/// as OpenSSH for Windows reads them. A kept file is read back, and ignored
/// once the group Users (S-1-5-32-545) may read it. A list
/// that the group may read is still read as a file that others may read,
/// and refused as a secret; one that it may write, append to, or whose access
/// list it may change is refused.
#[cfg(windows)]
#[test]
fn on_windows_a_file_that_others_may_open_is_not_trusted() {
    use pod_core::files::{Others, read_own};
    let dir = scratch("win-acl");
    let path = store_file_in_first(std::slice::from_ref(&dir), "kept.json", b"kept").unwrap();
    assert!(load_file_from(std::slice::from_ref(&dir), "kept.json").is_some(), "a kept file is read");
    icacls(&path, &["/grant", "*S-1-5-32-545:(R)"]);
    assert!(load_file_from(std::slice::from_ref(&dir), "kept.json").is_none(), "a file that others may read");

    let list = dir.join("list");
    std::fs::write(&list, "a list\n").unwrap();
    assert_eq!(read_own(&list, Others::Read).unwrap().as_deref(), Some("a list\n"));
    icacls(&list, &["/grant", "*S-1-5-32-545:(R)"]);
    assert_eq!(read_own(&list, Others::Read).unwrap().as_deref(), Some("a list\n"), "others may read a list");
    let why = read_own(&list, Others::NoAccess).unwrap_err();
    assert!(why.contains("others can read it") && why.contains("S-1-5-32-545"), "{why}");
    for right in ["W", "AD", "WDAC"] {
        let file = dir.join(format!("open-{right}"));
        std::fs::write(&file, "x").unwrap();
        icacls(&file, &["/grant", &format!("*S-1-5-32-545:({right})")]);
        let why = read_own(&file, Others::Read).unwrap_err();
        assert!(why.contains("others can change it") && why.contains("the group Users"), "{right}: {why}");
    }
}

/// A directory that others may open is made private: it gets an access list
/// of its own, and the file in it, which others could change, is closed.
#[cfg(windows)]
#[test]
fn on_windows_a_directory_open_to_others_is_made_private() {
    use pod_core::files::{Others, read_own};
    let dir = scratch("win-dir").join("open");
    std::fs::create_dir_all(&dir).unwrap();
    icacls(&dir, &["/grant", "*S-1-5-32-545:(OI)(CI)(M)"]);
    let file = dir.join("list");
    std::fs::write(&file, "x").unwrap();
    assert!(read_own(&file, Others::Read).unwrap_err().contains("others can change it"));
    pod_core::files::private_dir(&dir).unwrap();
    assert_eq!(read_own(&file, Others::NoAccess).unwrap().as_deref(), Some("x"), "closed now");
}

/// Threads that store one name at once each succeed, and the file is one
/// whole body: the temporary name is of the call, not of the process
/// (T-195).
#[test]
fn threads_that_store_one_name_at_once_leave_one_whole_body() {
    let dir = scratch("threads");
    let dirs = vec![dir.clone()];
    let bodies: Vec<String> = (0..8).map(|n| format!("{n}").repeat(64 * 1024)).collect();
    std::thread::scope(|s| {
        for body in &bodies {
            let dirs = dirs.clone();
            s.spawn(move || {
                for _ in 0..16 {
                    pod_core::files::store_file_in_first(&dirs, "shared", body.as_bytes()).unwrap();
                }
            });
        }
    });
    let text = std::fs::read_to_string(dir.join("shared")).unwrap();
    assert!(bodies.contains(&text), "a torn file of {} bytes", text.len());
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
    assert_eq!(left, ["shared"], "a temporary file stayed");
}

/// One name for each host: an IPv6 address and a name with `_` do not share
/// a cache file (T-246).
#[test]
fn exact_name_is_one_to_one() {
    use pod_core::files::exact_name;
    assert_eq!(exact_name("Relay.Example.org"), "relay.example.org");
    assert_eq!(exact_name("::1"), "_3a_3a1");
    assert_eq!(exact_name("__1"), "_5f_5f1");
    assert_ne!(exact_name("::1"), exact_name("__1"));
    assert_ne!(exact_name("[::1]:8443"), exact_name("___1__8443"));
    assert_eq!(exact_name("../x"), ".._2fx", "no path separator");
}
