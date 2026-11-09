//! T-1644: the link set, read from a fixture rootfs with a fixture reader.
//! The files exist so that `resolve_in` walks real links; the reader maps
//! each resolved path to a fake ELF, so no ELF bytes are crafted.

use super::super::tests::{fake, tiny_elf};
use super::*;
use std::cell::RefCell;
use std::path::PathBuf;

const ME: u16 = podvm_probe::binfmt::SELF_MACHINE;
const LOADER: &str = "/lib64/ld-linux-x86-64.so.2";
const TRIPLET: &str = "lib/x86_64-linux-gnu";

/// A versioned import that names its provider file, as `.gnu.version_r` does.
fn imp(name: &str, version: &str, file: &str) -> Versioned {
    Versioned {
        name: name.to_string(),
        version: Some(version.to_string()),
        weak: false,
        file: Some(file.to_string()),
    }
}

/// The glibc object's shape: four needs, and one import from each.
fn object() -> Elf {
    let mut o = fake(
        "/.podvm/interpose.so",
        None,
        &[
            "libdl.so.2",
            "libgcc_s.so.1",
            "libc.so.6",
            "ld-linux-x86-64.so.2",
        ],
        &[],
        &[],
    );
    o.imported = vec![
        imp("__tls_get_addr", "GLIBC_2.3", "ld-linux-x86-64.so.2"),
        imp("dlsym", "GLIBC_2.2.5", "libdl.so.2"),
        imp("_Unwind_Resume", "GCC_3.0", "libgcc_s.so.1"),
        imp("open", "GLIBC_2.2.5", "libc.so.6"),
    ];
    o
}

/// The libraries of a glibc 2.34+ image (bookworm): the loader defines
/// `__tls_get_addr`, libc defines `dlsym`, and `libdl.so.2` is a stub.
fn bookworm() -> Vec<(&'static str, Elf)> {
    let g = ["GLIBC_2.2.5", "GLIBC_2.3", "GLIBC_2.34"];
    vec![
        (
            "ld-linux-x86-64.so.2",
            fake(
                "",
                Some("ld-linux-x86-64.so.2"),
                &[],
                &[("__tls_get_addr", Some("GLIBC_2.3"))],
                &g,
            ),
        ),
        (
            "libc.so.6",
            fake(
                "",
                Some("libc.so.6"),
                &["ld-linux-x86-64.so.2"],
                &[
                    ("open", Some("GLIBC_2.2.5")),
                    ("dlsym", Some("GLIBC_2.2.5")),
                ],
                &g,
            ),
        ),
        (
            "libdl.so.2",
            fake(
                "",
                Some("libdl.so.2"),
                &["libc.so.6"],
                &[],
                &["GLIBC_2.2.5"],
            ),
        ),
        (
            "libgcc_s.so.1",
            fake(
                "",
                Some("libgcc_s.so.1"),
                &["libc.so.6"],
                &[("_Unwind_Resume", Some("GCC_3.0"))],
                &["GCC_3.0"],
            ),
        ),
    ]
}

/// A rootfs that holds `libs` under the triplet directory, with Debian's
/// absolute `/lib64` loader link.
struct Fixture {
    root: PathBuf,
    libs: Vec<(&'static str, Elf)>,
    read: RefCell<Vec<String>>,
}

impl Fixture {
    fn new(tag: &str, libs: Vec<(&'static str, Elf)>) -> Fixture {
        let root = std::env::temp_dir().join(format!("podvm-t1644-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(TRIPLET)).unwrap();
        std::fs::create_dir_all(root.join("lib64")).unwrap();
        for (name, _) in &libs {
            std::fs::write(root.join(TRIPLET).join(name), tiny_elf(ME)).unwrap();
        }
        std::os::unix::fs::symlink(
            format!("/{TRIPLET}/ld-linux-x86-64.so.2"),
            root.join("lib64/ld-linux-x86-64.so.2"),
        )
        .unwrap();
        Fixture {
            root,
            libs,
            read: RefCell::new(Vec::new()),
        }
    }

    /// The fixture reader: a resolved path under the root answers its fake
    /// ELF; each call is recorded.
    fn reader(&self, path: &str) -> Result<Elf, Unreadable> {
        self.read.borrow_mut().push(path.to_string());
        let base = path.rsplit('/').next().unwrap_or(path);
        match self.libs.iter().find(|(n, _)| *n == base) {
            Some((_, e)) if Path::new(path).starts_with(&self.root) => {
                let mut e = e.clone();
                e.path = path.to_string();
                Ok(e)
            }
            _ => Err(Unreadable(format!("{path}: not a fixture"))),
        }
    }

    fn set(&self, obj: &Elf) -> Result<LinkSet, String> {
        let rootfs = self.root.display().to_string();
        let dirs = guest_lib_dirs(&rootfs, LOADER);
        link_set(&self.root, obj, LOADER, &dirs, &|p| self.reader(p))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn refused(v: Verdict) -> String {
    match v {
        Verdict::Refused(why) => why,
        Verdict::Admitted => panic!("admitted where a refusal was expected"),
    }
}

/// T-1644: the union read from the closure admits an object that imports
/// `__tls_get_addr`, `dlsym` and `_Unwind_Resume`, each from its own
/// provider. Against libc alone the same object refuses, which was the
/// beta.12 decline on bookworm.
#[test]
fn a_closure_read_inside_the_image_admits_the_three_imports() {
    let fx = Fixture::new("bookworm", bookworm());
    let set = fx.set(&object()).expect("the closure resolves");
    let names: Vec<&str> = set.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, [LOADER, "libdl.so.2", "libgcc_s.so.1", "libc.so.6"]);
    assert_eq!(admits_set(&object(), &set), Verdict::Admitted);
    let libc = set.libc().expect("a libc").clone();
    let why = refused(admits(&object(), &libc));
    assert!(
        why.contains("__tls_get_addr") || why.contains("_Unwind_Resume"),
        "{why}"
    );
}

/// The bullseye shape (glibc 2.31): libc defines no `dlsym`, and
/// `libdl.so.2` does. The closure holds it, so the object admits.
#[test]
fn a_pre_234_image_finds_dlsym_in_libdl() {
    let mut libs = bookworm();
    libs[1].1.defined.retain(|d| d.name != "dlsym");
    libs[2].1.defined.push(Versioned {
        name: "dlsym".into(),
        version: Some("GLIBC_2.2.5".into()),
        weak: false,
        file: None,
    });
    let fx = Fixture::new("bullseye", libs);
    let set = fx.set(&object()).expect("the closure resolves");
    assert_eq!(admits_set(&object(), &set), Verdict::Admitted);
}

/// A needed library that the image does not hold is named, with the
/// library that needs it: the loader would refuse the preload there.
#[test]
fn a_missing_needed_library_is_named() {
    let mut libs = bookworm();
    libs.retain(|(n, _)| *n != "libdl.so.2");
    let fx = Fixture::new("nolibdl", libs);
    let why = fx.set(&object()).expect_err("libdl.so.2 is absent");
    assert!(why.contains("libdl.so.2"), "{why}");
    assert!(why.contains("/.podvm/interpose.so"), "{why}");
}

/// The version is checked in the namespace of the file the reference
/// names: `GLIBC_2.2.5` from `libdl.so.2` refuses where that file does
/// not declare it, even though libc does.
#[test]
fn a_version_is_checked_against_its_provider_file() {
    let mut libs = bookworm();
    libs[2].1.declares = vec!["GLIBC_2.34".into()];
    let fx = Fixture::new("ns", libs);
    let set = fx.set(&object()).expect("the closure resolves");
    let why = refused(admits_set(&object(), &set));
    assert!(
        why.contains("`dlsym`") && why.contains("from libdl.so.2"),
        "{why}"
    );
    assert!(why.contains("GLIBC_2.34"), "{why}");
}

/// An import no member defines names its provider in the decline.
#[test]
fn a_missing_definition_names_the_provider() {
    let mut libs = bookworm();
    libs[3].1.defined.clear();
    let fx = Fixture::new("nodef", libs);
    let set = fx.set(&object()).expect("the closure resolves");
    let why = refused(admits_set(&object(), &set));
    assert!(
        why.contains("`_Unwind_Resume`") && why.contains("from libgcc_s.so.1"),
        "{why}"
    );
}

/// A loader link that resolves only on the host is not read. The image's
/// `/lib64` link names an absolute path that exists outside the rootfs and
/// not inside it. A host join would read that file; the set refuses and
/// the reader never sees a path outside the root.
#[test]
fn a_loader_link_that_resolves_only_on_the_host_is_not_read() {
    let fx = Fixture::new("hostonly", bookworm());
    let host_dir = fx.root.with_extension("host");
    std::fs::create_dir_all(&host_dir).unwrap();
    let host_loader = host_dir.join("ld-linux-x86-64.so.2");
    std::fs::write(&host_loader, tiny_elf(ME)).unwrap();
    let link = fx.root.join("lib64/ld-linux-x86-64.so.2");
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&host_loader, &link).unwrap();
    let why = fx
        .set(&object())
        .expect_err("the loader resolves only on the host");
    let _ = std::fs::remove_dir_all(&host_dir);
    assert!(why.contains("interpreter"), "{why}");
    for p in fx.read.borrow().iter() {
        assert!(
            Path::new(p).starts_with(&fx.root),
            "read {p} outside the rootfs"
        );
    }
}

/// musl: the loader is the C library, and it answers `libc.so` itself.
#[test]
fn a_musl_set_is_its_loader() {
    let root = std::env::temp_dir().join(format!("podvm-t1644-musl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::write(root.join("lib/ld-musl-x86_64.so.1"), tiny_elf(ME)).unwrap();
    let ld = fake(
        "",
        None,
        &[],
        &[("open", None), ("__libc_start_main", None)],
        &[],
    );
    let read = |p: &str| -> Result<Elf, Unreadable> {
        let mut e = ld.clone();
        e.path = p.to_string();
        Ok(e)
    };
    let mut obj = fake("/.podvm/interpose.so", None, &["libc.so"], &[], &[]);
    obj.imported = vec![Versioned {
        name: "open".into(),
        version: None,
        weak: false,
        file: None,
    }];
    let rootfs = root.display().to_string();
    let dirs = guest_lib_dirs(&rootfs, "/lib/ld-musl-x86_64.so.1");
    let set = link_set(&root, &obj, "/lib/ld-musl-x86_64.so.1", &dirs, &read).expect("musl set");
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(set.members.len(), 1);
    assert_eq!(admits_set(&obj, &set), Verdict::Admitted);
}
