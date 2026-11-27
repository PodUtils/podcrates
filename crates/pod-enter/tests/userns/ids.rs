//! The ids inside the user namespace of the namespace rung (T-786,
//! PodUtils/podvm#447), as a caller with no capabilities (uid 65534 where
//! the test runs as root). With `Plan::userns_ids` 1001:1001 the payload
//! is uid and gid 1001, a file of the caller shows as owned by 1001, and
//! the payload holds no capability; the mounts and the host name of the
//! rung stay. An exec that joins that run is 1001 too: the map of the run
//! holds. With no ids the payload is 0, as before.

use std::os::unix::fs::MetadataExt;

use super::*;

/// The file of the caller in the image.
pub const OF_THE_CALLER: &str = "of-the-caller";

/// The ids inside of the test.
const IDS: (u32, u32) = (1001, 1001);

/// The parent adds a file of the caller (of uid 65534 where the parent is
/// root) and the `stat` applet.
pub fn of_the_image(root: &Path) {
    fs::write(root.join(OF_THE_CALLER), "x").unwrap();
    if nobody::is_root() {
        std::os::unix::fs::chown(root.join(OF_THE_CALLER), Some(nobody::NOBODY), Some(nobody::NOBODY)).unwrap();
    }
    std::os::unix::fs::symlink("busybox", root.join("bin/stat")).unwrap();
}

fn with_ids(script: &str, ids: Option<(u32, u32)>) -> Plan {
    let mut p = plan(script, Some(NAME));
    p.userns_ids = ids;
    p
}

/// The test as the caller with no capabilities.
pub fn inside(root: &Path) {
    if !no_capabilities() {
        return;
    }
    let meta = fs::metadata(root.join(OF_THE_CALLER)).unwrap();
    let (uid, gid) = (meta.uid(), meta.gid());
    let dir = RootDir::open(root.to_str().unwrap()).unwrap();
    let (u, g) = IDS;
    let script = format!(
        "test \"$(id -u)\" = {u} || exit 21; test \"$(id -g)\" = {g} || exit 22; \
         test \"$(stat -c %u:%g /{OF_THE_CALLER})\" = {u}:{g} || exit 23; \
         grep -Eq '^CapEff:[[:space:]]+0+$' /proc/self/status || exit 24; \
         grep -Eq '^CapPrm:[[:space:]]+0+$' /proc/self/status || exit 25; \
         read a b c < /proc/self/uid_map; test \"$a $b $c\" = \"{u} {uid} 1\" || exit 26; \
         read a b c < /proc/self/gid_map; test \"$a $b $c\" = \"{g} {gid} 1\" || exit 27; \
         touch /tmp/new && test \"$(stat -c %u /tmp/new)\" = {u} || exit 28; \
         test \"$(uname -n)\" = {NAME} || exit 29"
    );
    let mut err = Vec::new();
    let code = run_namespace(&dir, &with_ids(&script, Some(IDS)), &mut err);
    let err = String::from_utf8_lossy(&err);
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the payload with the ids {u}:{g}: {err}");
    assert!(!err.contains("entered chroot instead"), "{err}");
    assert!(!err.contains("runs with no /proc"), "{err}");

    // No ids: the payload is 0 and holds its capabilities, in the nested
    // user namespace of the lock (T-1560), whose map is `0 0 1`.
    let script = format!(
        "test \"$(id -u)\" = 0 || exit 31; test \"$(id -g)\" = 0 || exit 32; \
         test \"$(stat -c %u:%g /{OF_THE_CALLER})\" = 0:0 || exit 33; \
         grep -Eq '^CapEff:[[:space:]]+0+$' /proc/self/status && exit 34; \
         read a b c < /proc/self/uid_map; test \"$a $b $c\" = \"0 0 1\" || exit 35"
    );
    let mut err = Vec::new();
    let code = run_namespace(&dir, &with_ids(&script, None), &mut err);
    let err = String::from_utf8_lossy(&err);
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the payload with no ids: {err}");

    // A join of the run with the ids: the exec is 1001 with no capability,
    // also with a plan that names no ids.
    let mut err = Vec::new();
    let run = spawn_namespace(&dir, &with_ids("exec sleep 60", Some(IDS)), &mut err).unwrap();
    let comm = format!("/proc/{}/comm", run.pid);
    for _ in 0..200 {
        if fs::read_to_string(&comm).is_ok_and(|c| c.trim_end() == "sleep") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let pidfd = sys::pidfd_open(run.pid).unwrap();
    let script =
        format!("test \"$(id -u)\" = {u} || exit 41; grep -Eq '^CapEff:[[:space:]]+0+$' /proc/self/status || exit 42");
    let mut joined = Vec::new();
    // A join takes no host name: the UTS namespace of the run holds.
    let code = spawn_joined(&dir, &plan(&script, None), pidfd, &mut joined).and_then(|c| c.wait());
    let _ = sys::close(pidfd);
    let _ = sys::kill(run.pid, 9);
    let _ = run.wait();
    let (err, joined) = (String::from_utf8_lossy(&err), String::from_utf8_lossy(&joined));
    assert!(!err.contains("entered chroot instead"), "the run fell back: {err}");
    assert_eq!(code.map_err(|e| e.to_string()), Ok(0), "the joined exec: {joined}");
    assert!(!joined.contains("does not share the mounts"), "{joined}");
}
