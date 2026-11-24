use super::*;
use std::os::unix::fs::FileTypeExt;

/// Each socket directory of a test (at most 80 bytes, `pod_testkit`)
/// takes the stage under the 104 bytes of macOS.
#[test]
fn a_parent_of_80_bytes_takes_the_stage() {
    let parent = PathBuf::from(format!("/{}", "d".repeat(79)));
    assert!(stage_fits(&parent));
    let longer = PathBuf::from(format!("/{}", "d".repeat(80)));
    assert!(!stage_fits(&longer));
}

#[test]
fn the_peer_of_a_pair_is_this_process() {
    let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
    let ids = peer_ids(&a).unwrap();
    assert_eq!(ids.uid, super::super::euid());
    #[cfg(target_os = "linux")]
    assert_eq!(ids.pid, Some(std::process::id()));
}

/// A new directory of exactly `len` bytes under the temporary directory.
fn long_dir(name: &str, len: usize) -> PathBuf {
    let base = std::env::temp_dir().join(format!("pb-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let room = len.checked_sub(base.as_os_str().len() + 1).expect("a temporary directory shorter than the test");
    let dir = base.join("d".repeat(room));
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(dir.as_os_str().len(), len);
    dir
}

/// The directory of `long_dir` and each file in it go.
fn remove_long(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

/// The socket at `path` has `mode` and is a socket, and nothing else is
/// left beside it.
fn assert_socket(path: &Path, mode: u32) {
    let meta = std::fs::symlink_metadata(path).unwrap();
    assert!(meta.file_type().is_socket(), "{} is not a socket", path.display());
    assert_eq!(meta.permissions().mode() & 0o7777, mode);
    let names: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, [path.file_name().unwrap()], "only the socket is left");
}

#[test]
fn a_directory_of_120_bytes_takes_the_socket() {
    let dir = long_dir("120", 120);
    let path = dir.join("ctl.sock");
    let listener = bind_staged(&path, 0o600);
    remove_long_after(&dir, || {
        let _listener = listener.expect("a bind in a directory of 120 bytes");
        assert_socket(&path, 0o600);
    });
}

/// Run `f`, then remove `dir`, also when `f` panics.
fn remove_long_after(dir: &Path, f: impl FnOnce()) {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    remove_long(dir);
    if let Err(p) = r {
        std::panic::resume_unwind(p);
    }
}

/// A file name longer than `sun_path` alone binds: the bind takes the
/// leaf `s` of the stage, and `link(2)` takes a name of up to 255 bytes.
#[test]
fn a_file_name_longer_than_sun_path_binds() {
    let dir = long_dir("name", 40);
    let path = dir.join("n".repeat(110));
    let listener = bind_staged(&path, 0o600);
    remove_long_after(&dir, || {
        let _listener = listener.expect("a bind with a name of 110 bytes");
        assert_socket(&path, 0o600);
    });
}

#[test]
fn a_file_name_past_255_bytes_fails_with_one_clear_line() {
    let dir = long_dir("toolong", 40);
    let path = dir.join("n".repeat(256));
    let e = bind_staged(&path, 0o600).expect_err("a name of 256 bytes");
    let names = std::fs::read_dir(&dir).unwrap().count();
    remove_long(&dir);
    assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
    let text = e.to_string();
    assert!(text.contains("give a file name of 1 to 255 bytes, not 256"), "{text}");
    assert_eq!(names, 0, "no stage is left");
}

/// A new directory of mode 0700 for the links of a test: a route through
/// it alone is the route of a host with no `/proc`.
fn link_dir(name: &str) -> PathBuf {
    let links = std::env::temp_dir().join(format!("pb-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&links);
    std::fs::create_dir_all(&links).unwrap();
    std::fs::set_permissions(&links, std::fs::Permissions::from_mode(0o700)).unwrap();
    links
}

#[test]
fn the_link_goes_after_a_bind_and_after_a_failure() {
    let dir = long_dir("link", 120);
    let links = link_dir("links");
    let dirs = [links.as_path()];
    let route = alias::Route { proc_fd: false, link_dirs: &dirs };
    let path = dir.join("ctl.sock");
    let first = bind_by(&path, 0o640, &route);
    let after_bind = std::fs::read_dir(&links).unwrap().count();
    // The path is there now: the second bind fails at `link(2)`.
    let second = bind_by(&path, 0o640, &route);
    let after_failure = std::fs::read_dir(&links).unwrap().count();
    remove_long_after(&dir, || {
        let _listener = first.expect("a bind through a link");
        assert_eq!(after_bind, 0, "the link goes after the bind");
        assert_eq!(second.expect_err("the path is there").kind(), std::io::ErrorKind::AddrInUse);
        assert_eq!(after_failure, 0, "the link goes after a failure");
        assert_socket(&path, 0o640);
    });
    let _ = std::fs::remove_dir_all(&links);
}

/// A link directory that other users can write, with no sticky bit, takes
/// no link: the bind fails with a line that names the long path.
#[test]
fn a_directory_that_others_can_write_takes_no_link() {
    let dir = long_dir("open", 120);
    let links = link_dir("openlinks");
    std::fs::set_permissions(&links, std::fs::Permissions::from_mode(0o777)).unwrap();
    let dirs = [links.as_path()];
    let route = alias::Route { proc_fd: false, link_dirs: &dirs };
    let e = bind_by(&dir.join("ctl.sock"), 0o600, &route).expect_err("no safe directory");
    let left = std::fs::read_dir(&dir).unwrap().count() + std::fs::read_dir(&links).unwrap().count();
    remove_long(&dir);
    let _ = std::fs::remove_dir_all(&links);
    assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
    assert!(e.to_string().contains("no short path to it was made"), "{e}");
    assert_eq!(left, 0, "no stage and no link is left");
}

/// On Linux the bind takes `/proc/self/fd/N`, and a client reaches the
/// socket through the same way.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn a_client_reaches_a_socket_at_a_long_path() {
    use std::os::fd::AsRawFd;
    let dir = long_dir("reach", 120);
    let path = dir.join("ctl.sock");
    let listener = bind_staged(&path, 0o600);
    remove_long_after(&dir, || {
        let listener = listener.unwrap();
        let held = std::fs::File::open(&dir).unwrap();
        let via = format!("/proc/self/fd/{}/ctl.sock", held.as_raw_fd());
        let _client = std::os::unix::net::UnixStream::connect(via).unwrap();
        let (_server, _) = listener.accept().unwrap();
    });
}

/// A forked child keeps a copy of the listener until its exec: the closed
/// socket still takes a connect, but it does not answer. A live listener
/// answers; a path with no socket does not.
#[test]
fn a_socket_that_a_forked_child_holds_until_its_exec_is_stale() {
    let dir = long_dir("held", 40);
    remove_long_after(&dir, || {
        let path = dir.join("held.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let held = pod_testkit::held::hold_until_exec(&listener);
        drop(listener);
        assert!(!socket_answers(&path), "a closed socket that a child holds answers");
        assert!(held.join().success());
        std::fs::remove_file(&path).unwrap();
        let _live = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert!(socket_answers(&path));
        assert!(!socket_answers(&dir.join("none.sock")));
    });
}
