//! The tables of the copies that take the crate: the check of podvm at the
//! start of a run of the loader family and the volumes of its syscall tier
//! (a volume opens under its base as a directory), and the check of its
//! interposer at each path call (a path under a volume under the base).
#![cfg(target_os = "linux")]

mod common;

use common::*;
use pod_beneath::{
    Calls, Libc, O_DIRECTORY, O_PATH, Refusal, open_beneath, open_parent_beneath, open_under_base, split,
};
use std::os::raw::c_int;

const BUF: usize = 4096;

/// The check of a volume of the loader family: `host` opens under `base`
/// as a directory with no link. Each case: the path under the base, and
/// the answer.
#[test]
fn a_volume_opens_under_its_base_as_a_directory_with_no_link() {
    let t = Tree::new("dir");
    let out = t.out.as_os_str().as_encoded_bytes().to_vec();
    let cases: &[(Vec<u8>, Result<(), Refusal>)] = &[
        (t.at("work"), Ok(())),
        (t.at("work/sub"), Ok(())),
        (t.b(), Ok(())),
        (t.at(""), Ok(())),
        (t.at("work//sub/"), Ok(())),
        (out, Err(Refusal::NotUnderBase)),
        (t.at("away"), Err(Refusal::Link)),
        (t.at("near"), Err(Refusal::Link)),
        (t.at("near/sub"), Err(Refusal::Link)),
        (t.at("file"), Err(Refusal::NotDirectory)),
        (t.at("absent"), Err(Refusal::Open(ENOENT))),
    ];
    for (host, want) in cases {
        let c = Count::default();
        let mut buf = [0u8; BUF];
        let r = open_under_base(&c, &t.b(), host, O_PATH | O_DIRECTORY, &mut buf);
        let shown = String::from_utf8_lossy(host);
        assert_eq!(r.map(|_| ()), *want, "{shown}");
        match r {
            Ok(fd) => {
                assert_eq!(c.held(), 1, "{shown}: the descriptor of the volume alone stays open");
                Libc.close(fd);
            }
            Err(_) => assert_eq!(c.held(), 0, "{shown}: a refusal leaves no descriptor"),
        }
    }
}

/// The interposer's walk of one path call, as podvm takes the crate: the
/// volume under the base, then the rest under the volume. `follow`: the
/// call follows a link at its last name; `create`: the call can make the
/// file of an absent last name. The descriptor and the last name that the
/// call takes next; an empty name: the descriptor is the place.
fn pin(base: &[u8], vol: &[u8], host: &[u8], follow: bool, create: bool) -> Result<(c_int, Vec<u8>), Refusal> {
    let mut buf = [0u8; BUF];
    let vfd = open_under_base(&Libc, base, vol, O_PATH | O_DIRECTORY, &mut buf)?;
    let rest = split(vol, host)?;
    let (_, name) = pod_beneath::last_name(rest);
    let dots = name == b"." || name == b"..";
    if rest == b"." {
        return Ok((vfd, Vec::new()));
    }
    let r = if follow || dots {
        match open_beneath(&Libc, vfd, rest, O_PATH, 0, &mut buf) {
            Err(Refusal::Open(ENOENT)) if create && !dots => open_parent_beneath(&Libc, vfd, rest, &mut buf),
            r => r.map(|fd| (fd, &b""[..])),
        }
    } else {
        open_parent_beneath(&Libc, vfd, rest, &mut buf)
    };
    Libc.close(vfd);
    r.map(|(fd, name)| (fd, name.to_vec()))
}

/// The class of the interposer's test
/// `a_volume_under_the_base_reaches_no_file_out_of_the_base`: each link
/// that the host makes under the base after the start (also between the
/// walk and the call) is a refusal, never the file out of the base.
#[test]
fn a_path_of_a_volume_under_the_base_reaches_no_file_out_of_the_base() {
    let t = Tree::new("pin");
    let b = t.b();
    let (w, w2) = (t.at("w"), t.at("w2"));
    // A plain path, before the host makes a link.
    let (fd, name) = pin(&b, &w, &t.at("w/file"), true, false).unwrap();
    assert_eq!((text_of(fd), name), ("inside".to_string(), Vec::new()), "a plain file");
    Libc.close(fd);
    // A link inside the volume: the walk of the caller reads it first in
    // guest names, so a link that reaches the crate is a change of the host.
    assert_eq!(pin(&b, &w, &t.at("w/rel"), true, false).map(|_| ()), Err(Refusal::Link), "w/rel");

    std::os::unix::fs::symlink(&t.out, t.base.join("w/away")).unwrap();
    t.link_over("w2", &t.out);
    // (host path, its volume, follow, create, the last name, or the refusal)
    type Case<'a> = (&'a str, &'a [u8], bool, bool, Result<&'a str, Refusal>);
    let cases: &[Case] = &[
        ("w/file", &w, true, false, Ok("")),
        ("w/sub/f", &w, false, false, Ok("f")),
        ("w", &w, false, false, Ok("")),
        ("w/absent", &w, true, true, Ok("absent")),
        ("w/absent", &w, true, false, Err(Refusal::Open(ENOENT))),
        ("w/sub/", &w, false, false, Ok("")),
        ("w/sub/..", &w, false, false, Ok("")),
        ("w/away/f", &w, true, false, Err(Refusal::Link)),
        ("w/away/f", &w, false, false, Err(Refusal::Link)),
        ("w/away", &w, true, false, Err(Refusal::Link)),
        ("w2/f", &w2, true, false, Err(Refusal::Link)),
        ("w2", &w2, false, false, Err(Refusal::Link)),
        ("w/../w2/f", &w, true, false, Err(Refusal::DotDot)),
        ("w/..", &w, false, false, Err(Refusal::DotDot)),
    ];
    for (rel, vol, follow, create, want) in cases {
        let r = pin(&b, vol, &t.at(rel), *follow, *create);
        let got = r.as_ref().map(|(_, n)| String::from_utf8_lossy(n).into_owned()).map_err(|e| *e);
        assert_eq!(got, want.map(str::to_string), "{rel} follow={follow} create={create}");
        if let Ok((fd, _)) = r {
            assert!(!text_of(fd).contains("OUTSIDE"), "{rel}");
            Libc.close(fd);
        }
    }
    // A path out of the base: the base does not apply.
    let out = t.out.join("f");
    let out = out.as_os_str().as_encoded_bytes();
    assert_eq!(open_under_base(&Libc, &b, out, O_PATH, &mut [0u8; BUF]), Err(Refusal::NotUnderBase));
}

/// The split of the volumes of the syscall tier and of pod-enter: the base
/// itself is `.`, a path beside the base is not under it, and the root as
/// the base takes each absolute path.
#[test]
fn a_host_path_splits_at_its_base() {
    let ok = |base: &str, host: &str| {
        split(base.as_bytes(), host.as_bytes()).map(|r| String::from_utf8_lossy(r).into_owned())
    };
    assert_eq!(ok("/b", "/b"), Ok(".".into()));
    assert_eq!(ok("/b/", "/b"), Ok(".".into()));
    assert_eq!(ok("/b", "/b/"), Ok(".".into()));
    assert_eq!(ok("/b", "/b/w/x"), Ok("w/x".into()));
    assert_eq!(ok("/b//", "/b//w/"), Ok("w/".into()));
    assert_eq!(ok("/b", "/b/../out"), Ok("../out".into()), "the kernel refuses the `..`");
    assert_eq!(ok("/", "/etc"), Ok("etc".into()));
    assert_eq!(ok("/", "/"), Ok(".".into()));
    assert_eq!(ok("/b", "/b2/w"), Err(Refusal::NotUnderBase));
    assert_eq!(ok("/b", "/elsewhere"), Err(Refusal::NotUnderBase));
    assert_eq!(ok("/b", "b/w"), Err(Refusal::NotUnderBase));
    assert_eq!(ok("b", "b/w"), Err(Refusal::BaseNotAbsolute));
    assert_eq!(ok("", "/b"), Err(Refusal::BaseNotAbsolute));
    let last = |rel: &str| {
        let (d, n) = pod_beneath::last_name(rel.as_bytes());
        (String::from_utf8_lossy(d).into_owned(), String::from_utf8_lossy(n).into_owned())
    };
    assert_eq!(last("a/b/c"), ("a/b".into(), "c".into()));
    assert_eq!(last("a//c"), ("a".into(), "c".into()));
    assert_eq!(last("c"), (".".into(), "c".into()));
    assert_eq!(last("a/"), ("a".into(), "".into()));
    assert_eq!(last("/c"), ("/".into(), "c".into()));
}

/// A path that does not fit the buffer, or that holds a NUL, opens nothing.
#[test]
fn a_path_that_does_not_fit_the_buffer_opens_nothing() {
    let t = Tree::new("buf");
    let c = Count::default();
    let host = t.at("w");
    let mut small = vec![0u8; t.b().len()];
    assert_eq!(open_under_base(&c, &t.b(), &host, O_PATH, &mut small), Err(Refusal::TooLong), "the base");
    let long = t.at(&"w".repeat(BUF));
    let mut buf = [0u8; BUF];
    assert_eq!(open_under_base(&c, &t.b(), &long, O_PATH, &mut buf), Err(Refusal::TooLong), "the path under it");
    assert_eq!(open_under_base(&c, &t.b(), &t.at("w\0x"), O_PATH, &mut buf), Err(Refusal::Nul));
    let mut tiny = [0u8; 2];
    assert_eq!(open_beneath(&c, pod_beneath::AT_FDCWD, b"ab", O_PATH, 0, &mut tiny), Err(Refusal::TooLong));
    assert_eq!(c.opens.get(), 0);
}

/// Each refusal gives its errno and one line of plain text.
#[test]
fn each_refusal_has_its_errno_and_a_line() {
    let all = [
        (Refusal::NotUnderBase, EXDEV),
        (Refusal::BaseNotAbsolute, 22),
        (Refusal::Base(ENOTDIR), ENOTDIR),
        (Refusal::Link, ELOOP),
        (Refusal::DotDot, EXDEV),
        (Refusal::NotDirectory, ENOTDIR),
        (Refusal::NoOpenat2, ENOSYS),
        (Refusal::TooLong, 36),
        (Refusal::Nul, 22),
        (Refusal::Open(ENOENT), ENOENT),
    ];
    for (r, errno) in all {
        assert_eq!(r.errno(), errno, "{r:?}");
        let line = r.to_string();
        assert!(line.starts_with(r.text()) && !line.is_empty(), "{r:?}: {line}");
        assert!(line.bytes().all(|c| (0x20..0x7f).contains(&c)), "{r:?}: {line}");
    }
    assert_eq!(Refusal::Open(ENOENT).to_string(), "the path does not open (errno 2)");
}
