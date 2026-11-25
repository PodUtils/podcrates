use super::*;

fn scratch(name: &str) -> PathBuf {
    pod_core::files::new_temp_dir(&std::env::temp_dir(), &format!("pl-unit-{name}")).unwrap()
}

fn mode(path: &Path) -> u32 {
    std::fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

/// The umask of this process, from `/proc/self/status`: `umask(2)` would
/// change it for each thread of the test.
fn umask() -> Option<u32> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find_map(|l| l.strip_prefix("Umask:"))?;
    u32::from_str_radix(line.trim(), 8).ok()
}

/// The mode that `fchmod` gives a socket before its bind is the mode of
/// its new file, less the umask, before any `chmod` of the path.
#[test]
fn the_mode_of_the_socket_is_the_mode_of_its_file_at_the_bind() {
    let Some(umask) = umask() else {
        println!("SKIP: no Umask line in /proc/self/status");
        return;
    };
    let dir = scratch("mode");
    for want in [0o600, 0o640, 0o700] {
        let path = dir.join(format!("{want:o}.sock"));
        let listener = bind_socket(&path, want).unwrap();
        assert_eq!(mode(&path), want & !umask, "fchmod {want:o}, umask {umask:03o}");
        std::os::unix::net::UnixStream::connect(&path).expect("the socket answers");
        drop(listener);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The bind with the mode gives 0600, and an existing path is `AddrInUse`.
#[tokio::test]
async fn a_bind_with_the_mode_refuses_an_existing_path() {
    let dir = scratch("again");
    let path = dir.join("s.sock");
    let _listener = bind_with_mode(&path).unwrap();
    assert_eq!(mode(&path), 0o600);
    let e = bind_with_mode(&path).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::AddrInUse, "{e}");
    assert!(path.exists(), "the socket of the first bind stays");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two binds in one directory each take a new stage and leave none, and a
/// stage of another run stays as it was.
#[tokio::test]
async fn each_bind_takes_a_new_stage_and_leaves_none() {
    let dir = scratch("stage");
    let other = dir.join(".pod-link-of-another-run");
    std::fs::create_dir(&other).unwrap();
    std::fs::write(other.join("s"), b"x").unwrap();
    let a = bind_private(&dir.join("a.sock")).unwrap();
    let b = bind_private(&dir.join("b.sock")).unwrap();
    for name in ["a.sock", "b.sock"] {
        assert_eq!(mode(&dir.join(name)), 0o600, "{name}");
    }
    let mut names: Vec<String> =
        std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, [".pod-link-of-another-run", "a.sock", "b.sock"]);
    assert_eq!(std::fs::read(other.join("s")).unwrap(), b"x");
    drop((a, b));
    let _ = std::fs::remove_dir_all(&dir);
}
