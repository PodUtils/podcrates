//! The changes of the host that a check before the call cannot see: a link
//! made between the walk and the call, a volume replaced by a link, a `..`,
//! a link planted at an absent name, and a kernel with no `openat2`.
#![cfg(target_os = "linux")]

mod common;

use common::*;
use pod_beneath::{
    AT_FDCWD, Calls, EAGAIN_TRIES, Libc, O_CLOEXEC, O_CREAT, O_DIRECTORY, O_NOFOLLOW, O_PATH, O_WRONLY, OpenHow,
    Refusal, open_beneath, open_parent_beneath, open_under_base,
};

const BUF: usize = 4096;

/// A process of the host replaces a directory of the path by a link after
/// the check and before the walk: the walk refuses the link.
#[test]
fn a_link_made_between_the_walk_and_the_call_is_refused() {
    let t = Tree::new("race");
    let mut buf = [0u8; BUF];
    // The check passes on the tree as it is.
    let fd = open_under_base(&Libc, &t.b(), &t.at("w/sub/f"), O_PATH, &mut buf).unwrap();
    assert_eq!(text_of(fd), "inside");
    Libc.close(fd);
    // Then the host wins the race: `w/sub` is a link to OUT at the call.
    let (base, out) = (t.base.clone(), t.out.clone());
    let race = Race::new(move || {
        std::fs::remove_dir_all(base.join("w/sub")).unwrap();
        std::os::unix::fs::symlink(&out, base.join("w/sub")).unwrap();
    });
    assert_eq!(open_under_base(&race, &t.b(), &t.at("w/sub/f"), O_PATH, &mut buf), Err(Refusal::Link));
    assert!(race.change.borrow().is_none(), "the change ran");
    // The same race under a descriptor of the volume that the caller holds.
    let vol = open_under_base(&Libc, &t.b(), &t.at("w"), O_PATH | O_DIRECTORY, &mut buf).unwrap();
    let (base, out) = (t.base.clone(), t.out.clone());
    let race = Race::new(move || std::os::unix::fs::symlink(out.join("f"), base.join("w/late")).unwrap());
    assert_eq!(open_beneath(&race, vol, b"late", O_PATH, 0, &mut buf), Err(Refusal::Link));
    Libc.close(vol);
}

/// A volume that the host replaces by a link after the start: the volume
/// and each path under it are refused, a link out of the base and a link
/// to a directory in the base the same.
#[test]
fn a_volume_replaced_by_a_link_is_refused() {
    let t = Tree::new("replaced");
    let mut buf = [0u8; BUF];
    let fd = open_under_base(&Libc, &t.b(), &t.at("w2"), O_PATH | O_DIRECTORY, &mut buf).unwrap();
    Libc.close(fd);
    t.link_over("w2", &t.out);
    for rel in ["w2", "w2/", "w2/f", "w2/sub/f"] {
        assert_eq!(open_under_base(&Libc, &t.b(), &t.at(rel), O_PATH, &mut buf), Err(Refusal::Link), "{rel}");
    }
    t.link_over("work", &t.base.join("w"));
    assert_eq!(open_under_base(&Libc, &t.b(), &t.at("work/file"), O_PATH, &mut buf), Err(Refusal::Link));
}

/// A `..` that goes above the base or the volume is refused; a `..` that
/// stays under it opens, as the kernel decides.
#[test]
fn a_dotdot_above_the_directory_is_refused() {
    let t = Tree::new("dotdot");
    let mut buf = [0u8; BUF];
    for rel in ["..", "../out", "../out/f", "w/../../out/f", "w/sub/../../../out"] {
        assert_eq!(open_under_base(&Libc, &t.b(), &t.at(rel), O_PATH, &mut buf), Err(Refusal::DotDot), "{rel}");
    }
    let fd = open_under_base(&Libc, &t.b(), &t.at("w/../w2/f"), O_PATH, &mut buf).unwrap();
    assert_eq!(text_of(fd), "inside", "a `..` under the base");
    Libc.close(fd);
    let vol = open_under_base(&Libc, &t.b(), &t.at("w"), O_PATH | O_DIRECTORY, &mut buf).unwrap();
    for rel in [&b"../w2/f"[..], b"..", b"sub/../../w2", b"/etc", b"/"] {
        let shown = String::from_utf8_lossy(rel);
        assert_eq!(open_beneath(&Libc, vol, rel, O_PATH, 0, &mut buf), Err(Refusal::DotDot), "{shown}");
    }
    assert_eq!(open_parent_beneath(&Libc, vol, b"/f", &mut buf), Err(Refusal::DotDot), "an absolute path");
    Libc.close(vol);
}

/// An absent name: the directory and the name go to the call, and a link
/// that the host plants at the name before the call fails the call; a
/// create through the crate refuses the link too. No file out of the base.
#[test]
fn an_absent_name_with_a_planted_link_makes_no_file_out_of_the_base() {
    let t = Tree::new("planted");
    let mut buf = [0u8; BUF];
    let vol = open_under_base(&Libc, &t.b(), &t.at("w"), O_PATH | O_DIRECTORY, &mut buf).unwrap();
    assert_eq!(open_beneath(&Libc, vol, b"late", O_PATH, 0, &mut buf), Err(Refusal::Open(ENOENT)));
    let (dir, name) = open_parent_beneath(&Libc, vol, b"late", &mut buf).unwrap();
    assert_eq!(name, b"late");
    std::os::unix::fs::symlink(t.out.join("late"), t.base.join("w/late")).unwrap();
    // The caller's own call: its directory, its name and O_NOFOLLOW.
    let how = OpenHow { flags: O_WRONLY | O_CREAT | O_NOFOLLOW | O_CLOEXEC, mode: 0o644, resolve: 0 };
    assert_eq!(Libc.openat2(dir, c"late", &how), Err(ELOOP));
    Libc.close(dir);
    // A create through the crate.
    assert_eq!(open_beneath(&Libc, vol, b"late", O_WRONLY | O_CREAT, 0o644, &mut buf), Err(Refusal::Link));
    std::os::unix::fs::symlink(t.out.join("late2"), t.base.join("w/sub/late2")).unwrap();
    assert_eq!(open_beneath(&Libc, vol, b"sub/late2", O_WRONLY | O_CREAT, 0o644, &mut buf), Err(Refusal::Link));
    assert!(!t.out.join("late").exists() && !t.out.join("late2").exists(), "no file out of the base");
    // With no link, the create makes the file in the volume.
    let fd = open_beneath(&Libc, vol, b"made", O_WRONLY | O_CREAT, 0o644, &mut buf).unwrap();
    Libc.close(fd);
    assert!(t.base.join("w/made").is_file());
    Libc.close(vol);
}

/// A kernel with no `openat2` (or a filter that hides it) is its own
/// refusal, at the base and at the walk; the base is closed.
#[test]
fn a_kernel_with_no_openat2_gives_its_own_refusal() {
    let t = Tree::new("enosys");
    let mut buf = [0u8; BUF];
    let none = Answer::new(ENOSYS, false);
    assert_eq!(open_under_base(&none, &t.b(), &t.at("w"), O_PATH, &mut buf), Err(Refusal::NoOpenat2));
    assert_eq!(open_beneath(&none, AT_FDCWD, b"w", O_PATH, 0, &mut buf), Err(Refusal::NoOpenat2));
    assert_eq!(none.closes.get(), 0);
    let walk = Answer::new(ENOSYS, true);
    assert_eq!(open_under_base(&walk, &t.b(), &t.at("w"), O_PATH, &mut buf), Err(Refusal::NoOpenat2));
    assert_eq!(walk.closes.get(), 1, "the base is closed");
    assert_eq!(Refusal::NoOpenat2.errno(), ENOSYS);
    // Another errno of the base is the base's own refusal.
    let denied = Answer::new(13, false);
    assert_eq!(open_under_base(&denied, &t.b(), &t.at("w"), O_PATH, &mut buf), Err(Refusal::Base(13)));
}

/// EAGAIN (a rename that raced a `..`) is tried again, up to
/// `EAGAIN_TRIES` calls, and then given.
#[test]
fn eagain_is_tried_again_and_then_given() {
    struct Flaky(std::cell::Cell<usize>);
    impl Calls for Flaky {
        fn openat2(&self, dirfd: i32, path: &std::ffi::CStr, how: &OpenHow) -> Result<i32, i32> {
            let n = self.0.get();
            self.0.set(n + 1);
            if n < 3 { Err(EAGAIN) } else { Libc.openat2(dirfd, path, how) }
        }
        fn close(&self, fd: i32) {
            Libc.close(fd)
        }
    }
    let t = Tree::new("eagain");
    let mut buf = [0u8; BUF];
    let base = std::fs::File::open(&t.base).unwrap();
    let dir = std::os::fd::AsRawFd::as_raw_fd(&base);
    let flaky = Flaky(std::cell::Cell::new(0));
    let fd = open_beneath(&flaky, dir, b"w/file", O_PATH, 0, &mut buf).unwrap();
    assert_eq!((text_of(fd), flaky.0.get()), ("inside".to_string(), 4));
    Libc.close(fd);
    let busy = Answer::new(EAGAIN, false);
    assert_eq!(open_beneath(&busy, dir, b"w/file", O_PATH, 0, &mut buf), Err(Refusal::Open(EAGAIN)));
    assert_eq!(busy.calls.get(), EAGAIN_TRIES);
}
