use super::*;

/// A new directory of one test.
fn dir(name: &str) -> PathBuf {
    new_temp_dir(&std::env::temp_dir(), &format!("pod-core-temp-{name}")).expect("a directory of the test")
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    names
}

/// Random parts that a test gives: each in turn, then the last again.
struct Parts {
    list: Vec<String>,
    calls: usize,
}

impl Parts {
    fn new(list: &[&str]) -> Parts {
        Parts { list: list.iter().map(|p| p.to_string()).collect(), calls: 0 }
    }

    fn next(&mut self) -> String {
        let part = self.list[self.calls.min(self.list.len() - 1)].clone();
        self.calls += 1;
        part
    }
}

const A: &str = "000000000000000a";
const B: &str = "000000000000000b";
const C: &str = "000000000000000c";

fn is_random(part: &str) -> bool {
    part.len() == RANDOM_DIGITS && part.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// (a) Two calls with one prefix in one directory give two paths.
#[test]
fn two_calls_with_one_prefix_in_one_directory_give_two_paths() {
    let dir = dir("two");
    let dirs: Vec<PathBuf> = (0..32).map(|_| new_temp_dir(&dir, "same").unwrap()).collect();
    let files: Vec<PathBuf> = (0..32).map(|_| new_temp_file(&dir, "same", ".tmp").unwrap().0).collect();
    let mut all: Vec<&PathBuf> = dirs.iter().chain(&files).collect();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 64, "64 calls give 64 paths");
    assert_eq!(names(&dir).len(), 64);
    for d in &dirs {
        assert!(std::fs::symlink_metadata(d).unwrap().is_dir());
        let name = d.file_name().unwrap().to_str().unwrap();
        assert!(name.strip_prefix("same-").is_some_and(is_random), "{name}");
    }
    for f in &files {
        assert!(std::fs::symlink_metadata(f).unwrap().is_file());
        let name = f.file_name().unwrap().to_str().unwrap();
        let part = name.strip_prefix("same-").and_then(|n| n.strip_suffix(".tmp"));
        assert!(part.is_some_and(is_random), "{name}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// (b) A path that is there is never opened, truncated or removed: with
/// the same random part two times, the second call takes the next name.
#[test]
fn a_path_that_is_there_is_never_opened_truncated_or_removed() {
    let dir = dir("there");
    let mut parts = Parts::new(&[A, A, B]);
    let (first, mut file) = new_named(&dir, "f", ".tmp", &mut || parts.next(), &mut create_file).unwrap();
    std::io::Write::write_all(&mut file, b"first").unwrap();
    drop(file);
    let (second, _) = new_named(&dir, "f", ".tmp", &mut || parts.next(), &mut create_file).unwrap();
    assert_eq!((first.clone(), second.clone()), (dir.join(format!("f-{A}.tmp")), dir.join(format!("f-{B}.tmp"))));
    assert_eq!(parts.calls, 3);
    assert_eq!(std::fs::read(&first).unwrap(), b"first", "the first file keeps its bytes");

    let mut parts = Parts::new(&[A, A, B]);
    let one = new_named(&dir, "d", "", &mut || parts.next(), &mut create_dir).unwrap().0;
    std::fs::write(one.join("kept"), b"kept").unwrap();
    let two = new_named(&dir, "d", "", &mut || parts.next(), &mut create_dir).unwrap().0;
    assert_eq!((one.clone(), two), (dir.join(format!("d-{A}")), dir.join(format!("d-{B}"))));
    assert_eq!(std::fs::read(one.join("kept")).unwrap(), b"kept", "the first directory keeps its file");

    // A file is there at the name of a directory, and a directory at the
    // name of a file.
    let mut parts = Parts::new(&[A, C]);
    let (beside, _) = new_named(&dir, "d", "", &mut || parts.next(), &mut create_file).unwrap();
    assert_eq!(beside, dir.join(format!("d-{C}")));
    let mut parts = Parts::new(&[A, B]);
    let e = new_named(&dir, "f", ".tmp", &mut || parts.next(), &mut create_dir).map(|(p, ())| p);
    assert_eq!(e.unwrap_err().kind(), io::ErrorKind::AlreadyExists, "f-A.tmp and f-B.tmp are both there");
    assert_eq!(std::fs::read(&first).unwrap(), b"first");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// (b) A symbolic link at the name, to a file or to nothing, is not
/// followed: its target keeps its bytes, and a missing target is not made.
#[cfg(unix)]
#[test]
fn a_link_at_the_name_is_never_followed() {
    let dir = dir("link");
    let (victim, missing) = (dir.join("victim"), dir.join("missing"));
    std::fs::write(&victim, b"victim").unwrap();
    std::os::unix::fs::symlink(&victim, dir.join(format!("l-{A}"))).unwrap();
    std::os::unix::fs::symlink(&missing, dir.join(format!("m-{A}"))).unwrap();
    for prefix in ["l", "m"] {
        let mut parts = Parts::new(&[A, B]);
        let (path, file) = new_named(&dir, prefix, "", &mut || parts.next(), &mut create_file).unwrap();
        drop(file);
        assert_eq!(path, dir.join(format!("{prefix}-{B}")));
        let mut parts = Parts::new(&[A, B]);
        let e = new_named(&dir, prefix, "", &mut || parts.next(), &mut create_dir).map(|(p, ())| p);
        assert_eq!(e.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        let link = std::fs::symlink_metadata(dir.join(format!("{prefix}-{A}"))).unwrap();
        assert!(link.file_type().is_symlink(), "the link stays");
    }
    assert_eq!(std::fs::read(&victim).unwrap(), b"victim", "the target of the link keeps its bytes");
    assert!(std::fs::symlink_metadata(&missing).is_err(), "the missing target is not made");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// (c) When each name is there, the call fails with `AlreadyExists` after
/// the bound of tries, and no path is made.
#[test]
fn only_names_that_are_there_fail_after_the_bound() {
    let dir = dir("bound");
    std::fs::write(dir.join(format!("p-{A}")), b"there").unwrap();
    let before = names(&dir);
    let mut parts = Parts::new(&[A]);
    let e = new_named(&dir, "p", "", &mut || parts.next(), &mut create_file).map(|(p, _)| p).unwrap_err();
    assert_eq!((e.kind(), parts.calls), (io::ErrorKind::AlreadyExists, NAMES));
    assert!(e.to_string().contains("each of 8 names p-RANDOM was there"), "{e}");
    let mut parts = Parts::new(&[A]);
    let e = new_named(&dir, "p", "", &mut || parts.next(), &mut create_dir).map(|(p, ())| p).unwrap_err();
    assert_eq!((e.kind(), parts.calls), (io::ErrorKind::AlreadyExists, NAMES));
    assert_eq!(names(&dir), before, "no path is made");
    assert_eq!(std::fs::read(dir.join(format!("p-{A}"))).unwrap(), b"there");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The create of a caller: an `AlreadyExists` takes the next random name,
/// another error ends the tries at once, and a bad prefix is refused before
/// the first create.
#[test]
fn the_create_of_a_caller_takes_the_same_rule() {
    let mut seen = Vec::new();
    let (path, n) = new_temp_with(Path::new(""), ".node", ".x", |p| {
        seen.push(p.to_path_buf());
        if seen.len() < 3 { Err(io::ErrorKind::AlreadyExists.into()) } else { Ok(seen.len()) }
    })
    .unwrap();
    assert_eq!((n, path.clone()), (3, seen[2].clone()));
    let name = path.to_str().unwrap();
    let part = name.strip_prefix(".node-").and_then(|r| r.strip_suffix(".x")).unwrap();
    assert!(is_random(part), "{name}");
    assert_ne!(seen[0], seen[1]);
    let mut calls = 0;
    let e = new_temp_with(Path::new(""), "p", "", |_| -> io::Result<()> {
        calls += 1;
        Err(io::ErrorKind::PermissionDenied.into())
    })
    .unwrap_err();
    assert_eq!((e.kind(), calls), (io::ErrorKind::PermissionDenied, 1));
    let e = new_temp_with(Path::new(""), "a/b", "", |_| -> io::Result<()> { panic!("no create") }).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
}

/// (e) A prefix or a suffix that can leave the directory or cut the name
/// is refused before a name is made.
#[test]
fn a_prefix_or_a_suffix_that_leaves_the_name_is_refused() {
    let dir = dir("refused");
    let bad = ["a/b", "/a", "..", "a..b", "a\0b", if cfg!(windows) { "a\\b" } else { "../a" }];
    for part in bad {
        let mut parts = Parts::new(&[A]);
        let e = new_checked(&dir, part, "", &mut || parts.next(), &mut create_dir).map(|(p, ())| p).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput, "prefix {part:?}");
        let e = new_checked(&dir, "p", part, &mut || parts.next(), &mut create_file).map(|(p, _)| p).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput, "suffix {part:?}");
        assert_eq!(parts.calls, 0, "no name is made for {part:?}");
    }
    assert_eq!(new_temp_dir(&dir, "a/b").unwrap_err().kind(), io::ErrorKind::InvalidInput);
    assert_eq!(new_temp_file(&dir, "a", "/b").unwrap_err().kind(), io::ErrorKind::InvalidInput);
    assert!(names(&dir).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// With no random bytes from the system, two names still differ.
#[test]
fn with_no_random_bytes_two_names_differ() {
    struct Dry;
    impl Entropy for Dry {
        fn fill(&mut self, _: &mut [u8]) -> Result<(), crate::entropy::NoRandom> {
            Err(crate::entropy::NoRandom)
        }
    }
    let (one, two) = (random_from(&mut Dry), random_from(&mut Dry));
    assert_ne!(one, two);
    assert!(is_random(&one) && is_random(&two), "{one} {two}");
    assert!(is_random(&random_part()));
}

/// The variable that makes a test below a child of its parent test: it
/// names the directory of the parent.
#[cfg(unix)]
const CHILD: &str = "POD_CORE_TEMP_CHILD";

/// The test `name` of this binary in a new process, with `pre` run in the
/// child before its exec.
#[cfg(unix)]
fn rerun<F>(name: &str, dir: &Path, pre: F) -> io::Result<std::process::Output>
where
    F: FnMut() -> io::Result<()> + Send + Sync + 'static,
{
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command.args(["--exact", name, "--nocapture", "--test-threads=1"]).env(CHILD, dir);
    // SAFETY: `pre` makes system calls only, with no allocation.
    unsafe { command.pre_exec(pre) };
    command.output()
}

/// The line of a child from `tag` on, with the output in the failure.
#[cfg(unix)]
fn line_of(out: &std::process::Output, tag: &str) -> String {
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "the child failed: {}\n{stdout}\n{stderr}", out.status);
    // The harness writes the name of the test before it on the same line.
    let line = stdout.lines().find_map(|l| l.find(tag).map(|at| l[at..].to_string()));
    line.unwrap_or_else(|| panic!("no line {tag:?} from the child:\n{stdout}\n{stderr}"))
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

/// The child of the test of the modes: the modes of a new directory, a new
/// file, and of a control directory that asks for 0777.
#[cfg(unix)]
#[test]
fn child_of_the_modes() {
    use std::os::unix::fs::DirBuilderExt;
    let Some(dir) = std::env::var_os(CHILD) else { return };
    let dir = PathBuf::from(dir);
    let made = new_temp_dir(&dir, "mode").unwrap();
    let (file, _) = new_temp_file(&dir, "mode", ".tmp").unwrap();
    let control = dir.join(format!("control-{}", random_part()));
    std::fs::DirBuilder::new().mode(0o777).create(&control).unwrap();
    println!("temp-child modes {:o} {:o} control {:o}", mode_of(&made), mode_of(&file), mode_of(&control));
}

/// (d) The directory has mode 0700 and the file 0600 with a umask of 0,
/// and with a umask that takes bits of the owner away.
#[cfg(unix)]
#[test]
fn the_modes_are_0700_and_0600_whatever_the_umask() {
    for (umask, control) in [(0o000, 0o777), (0o277, 0o500)] {
        let dir = dir("modes");
        let out = rerun("files::temp::tests::child_of_the_modes", &dir, move || {
            // SAFETY: umask has no preconditions and cannot fail.
            unsafe { libc::umask(umask) };
            Ok(())
        })
        .unwrap();
        let line = line_of(&out, "temp-child modes ");
        assert_eq!(line, format!("temp-child modes 700 600 control {control:o}"), "umask {umask:03o}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// The child of the test of the PID namespaces: pid 1 of a new namespace.
#[cfg(target_os = "linux")]
#[test]
fn child_in_a_pid_namespace() {
    let Some(dir) = std::env::var_os(CHILD) else { return };
    let made = new_temp_dir(Path::new(&dir), "same").expect("a new directory");
    println!("temp-child pid {} made {}", std::process::id(), made.display());
}

/// In the child before its exec: a new user and PID namespace, then a
/// fork. The grandchild is pid 1 of the namespace and goes on to the exec;
/// this process waits for it and exits with its code.
#[cfg(target_os = "linux")]
fn pid_namespace() -> io::Result<()> {
    // SAFETY: the calls of this function make system calls only.
    if unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWPID) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => return Err(io::Error::last_os_error()),
        0 => return Ok(()),
        _ => {}
    }
    let mut status = 0;
    while unsafe { libc::waitpid(pid, &mut status, 0) } == -1 {
        if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            unsafe { libc::_exit(127) }
        }
    }
    let code = if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) } else { 128 + libc::WTERMSIG(status) };
    unsafe { libc::_exit(code) }
}

/// (f) Two processes with the same pid, each pid 1 of a PID namespace of
/// its own, make two paths with one prefix in one directory.
#[cfg(target_os = "linux")]
#[test]
fn two_processes_of_one_pid_in_pid_namespaces_make_two_paths() {
    let dir = dir("pidns");
    let mut lines = Vec::new();
    for _ in 0..2 {
        let out = match rerun("files::temp::tests::child_in_a_pid_namespace", &dir, pid_namespace) {
            Ok(out) => out,
            Err(e) if matches!(e.raw_os_error(), Some(libc::EPERM | libc::EINVAL | libc::ENOSPC | libc::ENOSYS)) => {
                pod_testkit::host::skip_line(&format!("this host gives no PID namespace of a user namespace: {e}"));
                std::fs::remove_dir_all(&dir).unwrap();
                return;
            }
            Err(e) => panic!("the child: {e}"),
        };
        lines.push(line_of(&out, "temp-child pid "));
    }
    assert!(
        lines.iter().all(|l| l.starts_with("temp-child pid 1 made ")),
        "the control: each child is pid 1: {lines:?}"
    );
    assert_ne!(lines[0], lines[1], "two paths");
    assert_eq!(names(&dir).len(), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}
