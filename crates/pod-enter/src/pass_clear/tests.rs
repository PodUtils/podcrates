use pod_hostprobe::sys::{self, CBuf};

use super::*;
use crate::{Fds, Names};

fn plan(pass: Vec<(i64, i64)>) -> Plan {
    Plan {
        argv: vec!["/bin/true".to_string()],
        env: Vec::new(),
        working_dir: "/".to_string(),
        fds: Fds { pass },
        banner: String::new(),
        path_dirs: vec!["/bin".to_string()],
        names: Names::DEFAULT,
        limits: Vec::new(),
        no_new_privs: false,
        die_with_parent: false,
        confine: None,
        hostname: None,
        volumes: Vec::new(),
        userns_ids: None,
        pid_namespace: false,
        tmpfs: Vec::new(),
        relay_title: None,
        read_only_root: false,
    }
}

fn open_dir() -> i64 {
    let tmp = CBuf::new(std::env::temp_dir().to_str().unwrap()).unwrap();
    sys::open(&tmp, sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC, 0).unwrap()
}

/// The device and inode that `fd` names, where it is open.
fn ident(fd: i64) -> Option<(u64, u64)> {
    sys::fstatat(fd, &sys::cempty(), sys::AT_EMPTY_PATH).ok().map(|s| (s.st_dev, s.st_ino))
}

/// A new directory of this test alone, opened: no other thread of the test
/// binary opens it, so its identity tells the copy from a reused number.
fn open_own_dir(name: &str) -> (i64, std::path::PathBuf) {
    let d = std::env::temp_dir().join(format!("pod-enter-pass-clear-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let c = CBuf::new(d.to_str().unwrap()).unwrap();
    (sys::open(&c, sys::O_RDONLY | sys::O_DIRECTORY | sys::O_CLOEXEC, 0).unwrap(), d)
}

/// A root descriptor on a passed number moves above each passed number;
/// the caller keeps its own; the copy closes with `Moved`.
#[test]
fn a_root_on_a_passed_number_moves_above_each_passed_number() {
    let (root, dir) = open_own_dir("moves");
    let p = plan(vec![(root, 0), (root + 3, 0)]);
    let mut moved = Moved::default();
    let copy = keep(&p, root, "the root file system", &mut moved).unwrap();
    assert!(copy > root + 3, "{copy} is not above {}", root + 3);
    let own = ident(root);
    assert!(own.is_some() && ident(copy) == own);
    drop(moved);
    // Another thread can take the number of the copy once it is closed; it
    // cannot open the directory of this test.
    assert_ne!(ident(copy), own, "the copy stayed open");
    assert_eq!(ident(root), own, "the caller's descriptor closed");
    let _ = sys::close(root);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A descriptor on no passed number stays, and no copy is made.
#[test]
fn a_descriptor_on_no_passed_number_stays() {
    let root = open_dir();
    let p = plan(vec![(root + 1, 0)]);
    let mut moved = Moved::default();
    assert_eq!(keep(&p, root, "the root file system", &mut moved).unwrap(), root);
    assert!(moved.0.is_empty());
    let _ = sys::close(root);
}

/// The kind of the file names what holds the number in a refusal.
#[test]
fn the_kind_names_a_directory_and_a_pipe() {
    let root = open_dir();
    assert_eq!(kind(root), "a directory");
    let mut pipe = [0i32; 2];
    sys::pipe2(&mut pipe, sys::O_CLOEXEC).unwrap();
    assert_eq!(kind(i64::from(pipe[0])), "a pipe");
    // A closed number far over the low numbers: another test thread that
    // opens a file takes the lowest free number, never this one.
    let high = sys::fcntl(root, sys::F_DUPFD_CLOEXEC, 900).unwrap();
    for fd in [root, high, i64::from(pipe[0]), i64::from(pipe[1])] {
        let _ = sys::close(fd);
    }
    assert_eq!(kind(high), "a descriptor that fstat cannot read");
}
