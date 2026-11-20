use std::ffi::{OsStr, OsString};

use super::*;

/// A directory of the test with `tool.EXE`, a file that can run.
fn dir_with_tool(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pod-manual-program-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("tool.EXE");
    std::fs::write(&file, b"").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir
}

#[test]
fn windows_takes_pathext_or_its_default_and_another_system_none() {
    assert_eq!(extensions(false, Some(".EXE".into())), None);
    assert_eq!(extensions(true, None), Some(OsString::from(PATHEXT_DEFAULT)));
    assert_eq!(extensions(true, Some(OsString::new())), Some(OsString::from(PATHEXT_DEFAULT)));
    assert_eq!(extensions(true, Some(".FAKE;.EXE".into())), Some(OsString::from(".FAKE;.EXE")));
}

#[test]
fn a_name_with_no_extension_tries_each_extension_of_pathext() {
    let exts = OsStr::new(".FAKE;;.EXE");
    assert_eq!(names("git", Some(exts)), ["git", "git.FAKE", "git.EXE"]);
    assert_eq!(names("git.exe", Some(exts)), ["git.exe"], "a name with an extension is used as it is");
    assert_eq!(names("git", None), ["git"]);
    assert!(is_path("C:\\Git\\bin\\git.exe", Some(exts)));
    assert!(!is_path("C:\\Git\\bin\\git.exe", None), "a backslash is a byte of a name on Unix");
    assert!(is_path("/bin/sh", None));
    assert!(!is_path("git", Some(exts)));
}

#[test]
fn the_lookup_of_windows_finds_a_name_with_its_pathext_extension() {
    let dir = dir_with_tool("pathext");
    let path = Some(OsString::from(dir.as_os_str()));
    let exts = extensions(true, Some(".FAKE;.EXE".into()));
    assert_eq!(find("tool", path.clone(), exts.as_deref()), Some(dir.join("tool.EXE")));
    assert_eq!(find("tool", path.clone(), None), None, "no extension is tried on Unix");
    let other = extensions(true, Some(".FAKE;.BAT".into()));
    assert_eq!(find("tool", path.clone(), other.as_deref()), None, "an extension that PATHEXT does not name");
    assert_eq!(find("tool.EXE", path, None), Some(dir.join("tool.EXE")));
    let _ = std::fs::remove_dir_all(&dir);
}
