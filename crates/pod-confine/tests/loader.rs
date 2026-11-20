//! The program that a dynamic loader runs (`pod_confine::loader`): the
//! names of the loaders, and the arguments that a check sees for each
//! reading of the loaders' options. The table of podvm's copy
//! (PodUtils/podvm#613), with the names of musl's loader file.
#![cfg(target_os = "linux")]

use pod_confine::loader::{each_program, is_loader};

/// One table, one rule: the names, then (arguments, the names that the
/// check sees, the answer where the check lets each one run).
#[test]
fn the_program_of_the_loader_is_each_argument_that_a_loader_can_run() {
    let names: [(&[u8], bool); 13] = [
        (b"/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2", true),
        (b"/lib/ld-linux-aarch64.so.1", true),
        (b"/lib/x86_64-linux-gnu/ld-2.28.so", true),
        (b"/lib/ld-musl-x86_64.so.1", true),
        (b"/lib64/ld64.so.2", true),
        (b"/usr/lib/ld.so.1", true),
        (b"/usr/bin/ld", false),
        (b"/usr/bin/ldd", false),
        (b"/lib/x86_64-linux-gnu/libc.so.6", false),
        (b"ld-linux-x86-64.so.2", true),
        (b"/lib/libc.musl-x86_64.so.1", true),
        (b"/usr/bin/lddtree", false),
        (b"/opt/tool/libcrypto.so.3", false),
    ];
    for (n, want) in names {
        assert_eq!(is_loader(n), want, "{}", String::from_utf8_lossy(n));
    }
    let rows: [(&[&str], &[&str], bool); 14] = [
        (&["/tmp/p", "a"], &["/tmp/p"], true),
        (&["--library-path", "/tmp", "/tmp/p"], &["/tmp/p"], true),
        (&["--inhibit-cache", "--argv0", "x", "./p"], &["./p"], true),
        (&["--list", "/tmp/p"], &["/tmp/p"], true),
        (&["--library-path=/l", "/tmp/p"], &["--library-path=/l", "/tmp/p"], true),
        (&["--help"], &[], true),
        (&[], &[], true),
        (&["--audit", "/a.so", "--preload", "/b.so", "/tmp/p"], &["/tmp/p"], true),
        // A name with no `/`: glibc's loader searches it, so it is refused.
        (&["--library-path", "/tmp", "p"], &[], false),
        (&["--", "/tmp/p"], &[], false),
        (&["--unknown", "/tmp/p"], &[], false),
        (&["--argv0=x", "/tmp/p"], &[], false),
        (&["-x"], &[], false),
        (&["p", "/tmp/p"], &[], false),
    ];
    for (args, seen, want) in rows {
        let mut got: Vec<&str> = Vec::new();
        let ok = each_program(args.iter().map(|a| a.as_bytes()), |p| {
            got.push(core::str::from_utf8(p).unwrap());
            true
        });
        assert_eq!((ok, got.as_slice()), (want, seen), "{args:?}");
    }
    // The first program that the check refuses ends the walk.
    assert!(!each_program([b"--library-path=/l".as_slice(), b"/tmp/p"], |p| p != b"/tmp/p"));
    let mut seen = 0;
    assert!(!each_program([b"--library-path=/l".as_slice(), b"/tmp/p"], |_| {
        seen += 1;
        false
    }));
    assert_eq!(seen, 1, "the walk went on past a refusal");
}

/// The file takes no item of std and no item of the crate, so that an
/// object with no dependency keeps a copy of it as it is.
#[test]
fn the_loader_file_takes_no_item_of_std_or_of_the_crate() {
    let text = include_str!("../src/loader.rs");
    for word in ["std::", "crate::", "super::", "use ", "alloc::", "Vec<", "String", "Box<", "unsafe", "format!"] {
        assert!(!text.contains(word), "src/loader.rs holds {word:?}");
    }
}
