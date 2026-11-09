use super::*;

fn tree(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("podvm-fuse-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("back/bin")).unwrap();
    d
}

/// The gate opens where the node opens: a regular file stands in
/// for the node, and the probe-gated staging tests below assert the
/// refusal arm with the absent real one.
#[test]
fn the_gate_opens_where_the_node_opens() {
    let d = tree("gate");
    std::fs::write(d.join("node"), b"x").unwrap();
    assert!(check_node_at(d.join("node").to_str().unwrap()).is_ok());
    let _ = std::fs::remove_dir_all(&d);
}

/// The gate refuses naming the node where nothing opens: the absent
/// path reads as the absent `/dev/fuse` does.
#[test]
fn the_gate_refuses_naming_the_node_where_nothing_opens() {
    let d = tree("gate-missing");
    let missing = d.join("no-such-node");
    let e = check_node_at(missing.to_str().unwrap()).unwrap_err();
    assert!(e.why.contains("/dev/fuse did not open"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A backing that is not a directory refuses before the store is
/// touched: no rung serves what nothing resolved, and no rung
/// litters the store refusing it.
#[test]
fn a_missing_backing_refuses_before_touching_the_store() {
    let d = tree("serve-missing");
    let store = d.join("store");
    let e = serve_tree(&store, &d.join("nope")).unwrap_err();
    assert!(e.why.contains("not a directory"), "{e}");
    assert!(!store.exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// Where the node is absent the serve refuses naming it and leaves
/// no mount directory: this lane's live arm of the same refusal.
#[test]
fn a_missing_node_refuses_naming_it_and_leaves_nothing() {
    let d = tree("serve-node");
    std::fs::write(d.join("back/bin/prog"), b"\x7fELF").unwrap();
    if check_node().is_ok() {
        let _ = std::fs::remove_dir_all(&d);
        return;
    }
    let store = d.join("store");
    let e = serve_tree(&store, &d.join("back")).unwrap_err();
    assert!(e.why.contains("/dev/fuse did not open"), "{e}");
    let staged: Vec<_> = std::fs::read_dir(store.join("fuse"))
        .map(|rd| rd.filter_map(|e| e.ok()).collect())
        .unwrap_or_default();
    assert!(staged.is_empty(), "a refused serve left staging behind");
    let _ = std::fs::remove_dir_all(&d);
}

/// Releasing a directory that was never mounted removes it: the
/// removal half of the release does not depend on the unmount half.
#[test]
fn releasing_an_unmounted_directory_removes_it() {
    let d = tree("release");
    let dir = d.join("mnt");
    std::fs::create_dir_all(&dir).unwrap();
    release_mount(&ActiveMount {
        pid: 1,
        dir: dir.clone(),
    });
    assert!(!dir.exists());
    let _ = std::fs::remove_dir_all(&d);
}

fn fixture_tagged(tag: &str) -> (PathBuf, Fs) {
    let d = tree(tag);
    std::fs::write(d.join("back/bin/prog"), b"\x7fELF-payload").unwrap();
    std::fs::write(d.join("back/note.txt"), b"hi").unwrap();
    std::os::unix::fs::symlink("prog", d.join("back/bin/sh")).unwrap();
    let fs = Fs::new(&d.join("back"));
    (d, fs)
}

/// A lookup finds the file, keeps its id while looked up, and
/// refuses what is not there, what carries a slash, and what is
/// empty: the kernel never sends those, and anything else did not
/// come from the kernel.
#[test]
fn a_lookup_finds_and_refuses() {
    let (d, mut fs) = fixture_tagged("a_lookup_finds_and_refuses");
    let bin = fs.lookup(1, b"bin").unwrap();
    let bin_id = get_u64(&bin, 0);
    assert!(bin_id != 1);
    let prog = fs.lookup(bin_id, b"prog").unwrap();
    let prog_id = get_u64(&prog, 0);
    // A second lookup of the same path keeps its id.
    let again = fs.lookup(bin_id, b"prog").unwrap();
    assert_eq!(get_u64(&again, 0), prog_id);
    assert!(fs.lookup(bin_id, b"absent").is_err());
    assert!(fs.lookup(bin_id, b"a/b").is_err());
    assert!(fs.lookup(bin_id, b"").is_err());
    let _ = std::fs::remove_dir_all(&d);
}

/// `..` answers the parent without touching the filesystem: no
/// lookup climbs past the backing root.
#[test]
fn a_dotdot_answers_the_parent() {
    let (d, mut fs) = fixture_tagged("a_dotdot_answers_the_parent");
    let bin = fs.lookup(1, b"bin").unwrap();
    let bin_id = get_u64(&bin, 0);
    let up = fs.lookup(bin_id, b"..").unwrap();
    assert_eq!(get_u64(&up, 0), 1);
    let top = fs.lookup(1, b"..").unwrap();
    assert_eq!(get_u64(&top, 0), 1);
    let _ = std::fs::remove_dir_all(&d);
}

/// A forget drops the node once its lookups stand at zero; the root
/// never drops.
#[test]
fn a_forget_drops_at_zero_and_never_the_root() {
    let (d, mut fs) = fixture_tagged("a_forget_drops_at_zero_and_never_the_root");
    let prog = fs.lookup(1, b"note.txt").unwrap();
    let id = get_u64(&prog, 0);
    fs.lookup(1, b"note.txt").unwrap();
    fs.forget(id, 1);
    assert!(fs.entry_bytes(id).is_some());
    fs.forget(id, 1);
    assert!(fs.entry_bytes(id).is_none());
    fs.forget(1, 99);
    assert!(fs.entry_bytes(1).is_some());
    let _ = std::fs::remove_dir_all(&d);
}

/// Attributes carry the file's own mode word: a directory reads as
/// one and a file as one.
#[test]
fn attributes_carry_the_mode_word() {
    let (d, mut fs) = fixture_tagged("attributes_carry_the_mode_word");
    let bin = fs.lookup(1, b"bin").unwrap();
    let attr = fs.getattr(get_u64(&bin, 0)).unwrap();
    let mode = get_u32(&attr, 16 + 60);
    assert_eq!(mode & 0o170000, 0o040000, "a directory reads as one");
    let prog = fs.lookup(get_u64(&bin, 0), b"prog").unwrap();
    let attr = fs.getattr(get_u64(&prog, 0)).unwrap();
    let mode = get_u32(&attr, 16 + 60);
    assert_eq!(mode & 0o170000, 0o100000, "a file reads as one");
    let _ = std::fs::remove_dir_all(&d);
}

/// A link reads as its target; reading a file as a link refuses.
#[test]
fn a_link_reads_as_its_target() {
    let (d, mut fs) = fixture_tagged("a_link_reads_as_its_target");
    let bin = get_u64(&fs.lookup(1, b"bin").unwrap(), 0);
    let sh = get_u64(&fs.lookup(bin, b"sh").unwrap(), 0);
    assert_eq!(fs.readlink(sh).unwrap(), b"prog");
    let prog = get_u64(&fs.lookup(bin, b"prog").unwrap(), 0);
    assert_eq!(fs.readlink(prog).unwrap_err(), errno(22));
    let _ = std::fs::remove_dir_all(&d);
}

/// Bytes are the file's, whole and from the offset: the payload
/// check the drive asserts.
#[test]
fn the_bytes_are_the_files_whole_and_from_the_offset() {
    let (d, mut fs) = fixture_tagged("the_bytes_are_the_files_whole_and_from_the_offset");
    let bin = get_u64(&fs.lookup(1, b"bin").unwrap(), 0);
    let prog = get_u64(&fs.lookup(bin, b"prog").unwrap(), 0);
    assert_eq!(fs.read(prog, 0, 64).unwrap(), b"\x7fELF-payload");
    assert_eq!(fs.read(prog, 5, 7).unwrap(), b"payload");
    assert!(fs.read(prog, 99, 64).unwrap().is_empty());
    assert_eq!(fs.read(1, 0, 64).unwrap_err(), errno(21));
    assert_eq!(fs.read(prog, 0, MAX_READ + 1).unwrap_err(), errno(22));
    let _ = std::fs::remove_dir_all(&d);
}

/// Only files and directories open: a link, which the kernel
/// resolves itself, never opens here.
#[test]
fn only_files_and_directories_open() {
    let (d, mut fs) = fixture_tagged("only_files_and_directories_open");
    let bin = get_u64(&fs.lookup(1, b"bin").unwrap(), 0);
    assert!(fs.open(bin, true).is_ok());
    let prog = get_u64(&fs.lookup(bin, b"prog").unwrap(), 0);
    assert!(fs.open(prog, false).is_ok());
    assert_eq!(fs.open(prog, true).unwrap_err(), errno(13));
    let sh = get_u64(&fs.lookup(bin, b"sh").unwrap(), 0);
    assert_eq!(fs.open(sh, false).unwrap_err(), errno(13));
    let _ = std::fs::remove_dir_all(&d);
}

/// A listing carries `.`, `..` and the children, resumes past a
/// cookie, and ends: the traversal the entry walks.
#[test]
fn a_listing_carries_dot_dotdot_children_and_an_end() {
    let (d, mut fs) = fixture_tagged("a_listing_carries_dot_dotdot_children_and_an_end");
    let all = fs.readdir(1, 0, 4096).unwrap();
    let names = dirent_names(&all);
    assert!(names.contains(&b".".to_vec()));
    assert!(names.contains(&b"..".to_vec()));
    assert!(names.contains(&b"bin".to_vec()));
    assert!(names.contains(&b"note.txt".to_vec()));
    // Resume past the first two cookies: `.` and `..` are gone.
    let rest = fs.readdir(1, 2, 4096).unwrap();
    let names = dirent_names(&rest);
    assert!(!names.contains(&b".".to_vec()));
    assert!(names.contains(&b"bin".to_vec()));
    // Past the end: empty, which is the kernel's stop.
    assert!(fs.readdir(1, 99, 4096).unwrap().is_empty());
    // A file is not a listing.
    let bin = get_u64(&fs.lookup(1, b"bin").unwrap(), 0);
    let prog = get_u64(&fs.lookup(bin, b"prog").unwrap(), 0);
    assert_eq!(fs.readdir(prog, 0, 4096).unwrap_err(), errno(20));
    let _ = std::fs::remove_dir_all(&d);
}

fn dirent_names(buf: &[u8]) -> Vec<Vec<u8>> {
    let mut names = Vec::new();
    let mut at = 0usize;
    while at + 24 <= buf.len() {
        let namelen = get_u32(buf, at + 16) as usize;
        let len = align8(24 + namelen);
        if at + len > buf.len() || namelen > len {
            break;
        }
        names.push(buf[at + 24..at + 24 + namelen].to_vec());
        at += len;
    }
    names
}

/// A write is `EROFS` and an unknown word is `ENOSYS`: the
/// read-only rung answers, never implements.
#[test]
fn a_write_is_rofs_and_an_unknown_word_is_nosys() {
    let (d, mut fs) = fixture_tagged("a_write_is_rofs_and_an_unknown_word_is_nosys");
    let prog = get_u64(&fs.lookup(1, b"note.txt").unwrap(), 0);
    let write_req = frame(16, 7, prog, &[]);
    let (answer, done) = dispatch(&mut fs, &write_req);
    assert_eq!(get_u32(&answer.unwrap(), 4) as i32, -30);
    assert!(!done);
    let bogus = frame(200, 8, prog, &[]);
    let (answer, _) = dispatch(&mut fs, &bogus);
    assert_eq!(get_u32(&answer.unwrap(), 4) as i32, -38);
    let _ = std::fs::remove_dir_all(&d);
}

/// A FORGET names its node in the header and carries the count in
/// the body: one forget of two lookups keeps the node, the second
/// drops it. The header-id half is what the first cut got wrong
/// (it read both out of the body).
#[test]
fn a_forget_names_its_node_in_the_header() {
    let (d, mut fs) = fixture_tagged("a_forget_names_its_node_in_the_header");
    let prog = get_u64(&fs.lookup(1, b"note.txt").unwrap(), 0);
    fs.lookup(1, b"note.txt").unwrap();
    let mut body = vec![0u8; 8];
    put_u64(&mut body, 0, 1);
    let forget = frame(2, 11, prog, &body);
    let (answer, done) = dispatch(&mut fs, &forget);
    assert!(answer.is_none());
    assert!(!done);
    assert!(fs.entry_bytes(prog).is_some(), "one of two lookups stands");
    let (answer, _) = dispatch(&mut fs, &forget);
    assert!(answer.is_none());
    assert!(fs.entry_bytes(prog).is_none(), "the second drops it");
    let _ = std::fs::remove_dir_all(&d);
}

/// Forgets and interrupts answer nothing: the kernel waits for
/// none of them.
#[test]
fn forgets_and_interrupts_answer_nothing() {
    let (d, mut fs) = fixture_tagged("forgets_and_interrupts_answer_nothing");
    let prog = get_u64(&fs.lookup(1, b"note.txt").unwrap(), 0);
    let mut body = vec![0u8; 8];
    put_u64(&mut body, 0, 1);
    let forget = frame(2, 7, prog, &body);
    assert!(dispatch(&mut fs, &forget).0.is_none());
    let interrupt = frame(36, 9, 0, &[]);
    assert!(dispatch(&mut fs, &interrupt).0.is_none());
    let _ = std::fs::remove_dir_all(&d);
}

/// INIT answers the offered version; DESTROY answers and exits.
#[test]
fn init_answers_the_version_and_destroy_exits() {
    let (d, mut fs) = fixture_tagged("init_answers_the_version_and_destroy_exits");
    let mut body = vec![0u8; 64];
    put_u32(&mut body, 0, 7);
    put_u32(&mut body, 4, 38);
    let init = frame(26, 3, 0, &body);
    let (answer, done) = dispatch(&mut fs, &init);
    let answer = answer.unwrap();
    assert!(!done);
    assert_eq!(get_u32(&answer, 16), 7);
    let destroy = frame(38, 5, 0, &[]);
    let (answer, done) = dispatch(&mut fs, &destroy);
    assert!(answer.is_some());
    assert!(done);
    let _ = std::fs::remove_dir_all(&d);
}

/// A reply is a 16-byte header ahead of the payload: the length
/// carries both, the error is zero, and the unique is echoed. The
/// socket drive proved this end to end; this pins it at dispatch.
#[test]
fn a_reply_is_a_header_ahead_of_the_payload() {
    let (d, mut fs) = fixture_tagged("a_reply_is_a_header_ahead_of_the_payload");
    let mut name = b"note.txt".to_vec();
    name.push(0);
    let (answer, done) = dispatch(&mut fs, &frame(1, 41, 1, &name));
    let answer = answer.unwrap();
    assert!(!done);
    assert_eq!(answer.len(), 16 + 128, "header plus entry");
    assert_eq!(get_u32(&answer, 0) as usize, answer.len());
    assert_eq!(get_u32(&answer, 4), 0);
    assert_eq!(get_u64(&answer, 8), 41);
    assert_eq!(get_u64(&answer, 16), 2, "the payload follows the header");
    let _ = std::fs::remove_dir_all(&d);
}

/// Garbage answers nothing and exits nothing: a short frame is
/// dropped rather than parsed.
#[test]
fn garbage_answers_nothing() {
    let (d, mut fs) = fixture_tagged("garbage_answers_nothing");
    assert_eq!(dispatch(&mut fs, &[1, 2, 3]).0, None);
    let _ = std::fs::remove_dir_all(&d);
}

fn frame(opcode: u32, unique: u64, nodeid: u64, body: &[u8]) -> Vec<u8> {
    let mut req = vec![0u8; 40 + body.len()];
    put_u32(&mut req, 0, 40 + body.len() as u32);
    put_u32(&mut req, 4, opcode);
    put_u64(&mut req, 8, unique);
    put_u64(&mut req, 16, nodeid);
    req[40..].copy_from_slice(body);
    req
}

/// The loop serves framed requests off a socket pair: INIT, then a
/// LOOKUP, a GETATTR and a READ whose bytes are the file's, then
/// DESTROY. The socket stands in for the node; the framing is what
/// is proved. Single-threaded: every frame is written up front (the
/// socket buffers hold them all), then the loop runs inline to
/// DESTROY, then the replies are read back. No thread: the T-0603
/// rule holds in tests too.
///
/// ⚠ The LOOKUP id is written as 2, the first allocation after the
/// root: this test performs no earlier lookup, so the counter
/// stands there. The reply asserts it back.
#[test]
fn the_loop_serves_off_a_socket_pair() {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    let (d, _) = fixture_tagged("the_loop_serves_off_a_socket_pair");
    let backing = d.join("back");
    let (mut driver, node) = UnixStream::pair().unwrap();
    let raw = {
        use std::os::unix::io::IntoRawFd;
        node.into_raw_fd() as i64
    };
    let mut body = vec![0u8; 64];
    put_u32(&mut body, 0, 7);
    put_u32(&mut body, 4, 38);
    driver.write_all(&frame(26, 1, 0, &body)).unwrap();
    let mut name = b"note.txt".to_vec();
    name.push(0);
    driver.write_all(&frame(1, 2, 1, &name)).unwrap();
    driver.write_all(&frame(3, 3, 2, &[])).unwrap();
    let mut read_body = vec![0u8; 40];
    put_u64(&mut read_body, 0, 77);
    put_u64(&mut read_body, 8, 0);
    put_u32(&mut read_body, 16, 16);
    driver.write_all(&frame(15, 4, 2, &read_body)).unwrap();
    driver.write_all(&frame(38, 5, 0, &[])).unwrap();
    assert_eq!(serve_until_destroy(raw, &backing, -1), 0);
    let init = read_frame(&mut driver, 1);
    assert_eq!(get_u32(&init, 16), 7, "INIT answers the version");
    let found = read_frame(&mut driver, 2);
    assert_eq!(get_u64(&found, 16), 2, "LOOKUP answers id 2");
    let attr = read_frame(&mut driver, 3);
    // 16 header, 16 attr-out head, 60 into the attributes.
    assert_eq!(get_u32(&attr, 16 + 16 + 60) & 0o170000, 0o100000);
    let data = read_frame(&mut driver, 4);
    assert_eq!(&data[16..], b"hi");
    let _ = read_frame(&mut driver, 5);
    let _ = std::fs::remove_dir_all(&d);
}

fn read_frame(driver: &mut std::os::unix::net::UnixStream, n: u32) -> Vec<u8> {
    use std::io::Read;
    let mut head = [0u8; 16];
    let mut got = 0usize;
    while got < 16 {
        let n = driver.read(&mut head[got..]).unwrap();
        assert!(n > 0, "the server closed mid-frame");
        got += n;
    }
    let len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
    assert!(
        (16..=1 << 20).contains(&len),
        "frame {n}: a sane frame length, got {len} in {head:?}"
    );
    let mut rest = vec![0u8; len - 16];
    let mut got = 0usize;
    while got < rest.len() {
        let n = driver.read(&mut rest[got..]).unwrap();
        assert!(n > 0, "the server closed mid-frame");
        got += n;
    }
    [head.to_vec(), rest].concat()
}
