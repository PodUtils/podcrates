use std::collections::BTreeMap;
use std::io::Cursor;

use pod_core::Class;

use super::*;
use crate::squashfs::tests::{TAIL, TAIL_XZ, X86, X86_BCJ_XZ};
use crate::squashfs::writer::{Block, Node, Writer};

const RELEASE: &str = "6.1.0-0-test";

/// The `modules.dep` of the tests: `a` needs `b` and `c`, `b` needs `c`,
/// `wire-guard` needs `b` and `c`, `code` (an xz block with the x86 filter)
/// needs nothing.
const DEP: &str = "kernel/net/a.ko: kernel/lib/b.ko kernel/lib/c.ko
kernel/lib/b.ko: kernel/lib/c.ko
kernel/lib/c.ko:

kernel/drivers/net/wire-guard.ko: kernel/lib/b.ko kernel/lib/c.ko
kernel/arch/x86/code.ko:
";

/// A tree of nested directories with `files` at their paths.
fn tree(files: Vec<(String, Node)>) -> Node {
    let mut dirs: BTreeMap<String, Vec<(String, Node)>> = BTreeMap::new();
    let mut entries = Vec::new();
    for (path, node) in files {
        match path.split_once('/') {
            Some((head, rest)) => dirs.entry(head.to_string()).or_default().push((rest.to_string(), node)),
            None => entries.push((path.into_bytes(), node)),
        }
    }
    entries.extend(dirs.into_iter().map(|(name, files)| (name.into_bytes(), tree(files))));
    Node::Dir { entries, ext: false }
}

/// A modloop with `dep` and the modules of [`DEP`], under `prefix`.
fn modloop_with(prefix: &str, dep: &str) -> Vec<u8> {
    let w = Writer::default();
    let base = format!("{prefix}/{RELEASE}");
    let code = Node::File {
        size: (X86.len() + TAIL.len()) as u64,
        blocks: vec![Block::Xz(X86_BCJ_XZ.to_vec()), Block::Xz(TAIL_XZ.to_vec())],
        fragment: None,
        ext: false,
    };
    let mut files = vec![
        (format!("{base}/modules.dep"), w.file(dep.as_bytes())),
        (format!("{base}/modules.builtin"), w.file(b"kernel/net/unix.ko\nkernel/fs/ext-4.ko\n")),
        (format!("{base}/kernel/arch/x86/code.ko"), code),
        (format!("{prefix}/firmware/x.bin"), w.file(b"firmware")),
    ];
    for path in ["kernel/net/a.ko", "kernel/lib/b.ko", "kernel/lib/c.ko", "kernel/drivers/net/wire-guard.ko"] {
        files.push((format!("{base}/{path}"), w.file(format!("module {path}").as_bytes())));
    }
    w.image(&tree(files))
}

fn modloop() -> Vec<u8> {
    modloop_with("modules", DEP)
}

fn set(img: &[u8], names: &[&str]) -> Result<ModuleSet> {
    modules(Cursor::new(img), RELEASE, names)
}

fn paths(set: &ModuleSet) -> Vec<&str> {
    set.modules.iter().map(|m| m.path.as_str()).collect()
}

fn fault_of(r: Result<ModuleSet>) -> Fault {
    let e = r.expect_err("a fault");
    super::fault(&e).cloned().unwrap_or_else(|| panic!("no fault in {e}"))
}

#[test]
fn the_set_holds_each_dependency_before_its_dependent() {
    let s = set(&modloop(), &["a"]).unwrap();
    assert_eq!(paths(&s), ["kernel/lib/c.ko", "kernel/lib/b.ko", "kernel/net/a.ko"]);
    assert_eq!(
        s.dep,
        "kernel/lib/c.ko:\nkernel/lib/b.ko: kernel/lib/c.ko\nkernel/net/a.ko: kernel/lib/b.ko kernel/lib/c.ko\n"
    );
    assert_eq!(s.modules[2].bytes, b"module kernel/net/a.ko");
    assert_eq!(s.modules[2].name, "a");
    assert_eq!(s.release, RELEASE);
    assert!(s.builtin.is_empty());
    assert_eq!(s.size(), s.modules.iter().map(|m| m.bytes.len() as u64).sum::<u64>());
}

#[test]
fn each_module_comes_once_and_a_dash_is_an_underscore() {
    let s = set(&modloop(), &["c", "wire_guard", "a", "wire-guard", "b"]).unwrap();
    assert_eq!(
        paths(&s),
        ["kernel/lib/c.ko", "kernel/lib/b.ko", "kernel/drivers/net/wire-guard.ko", "kernel/net/a.ko"]
    );
    assert_eq!(s.modules[2].name, "wire_guard");
    assert_eq!(s.dep.lines().count(), 4);
}

#[test]
fn a_module_in_an_x86_block_reads_through_the_filter() {
    let s = set(&modloop(), &["code"]).unwrap();
    assert_eq!(paths(&s), ["kernel/arch/x86/code.ko"]);
    assert_eq!(s.modules[0].bytes, [X86, TAIL].concat());
}

#[test]
fn a_name_that_modules_dep_does_not_list_is_an_error_that_names_it() {
    let e = set(&modloop(), &["a", "nf-tables"]).unwrap_err();
    assert_eq!(super::fault(&e), Some(&Fault::Unknown("nf-tables".into())));
    assert_eq!(e.class, Class::NotFound);
    assert!(e.to_string().contains("nf-tables"), "{e}");
}

#[test]
fn a_built_in_name_needs_no_file() {
    let s = set(&modloop(), &["unix", "ext_4", "ext-4", "c"]).unwrap();
    assert_eq!(s.builtin, ["unix", "ext_4"]);
    assert_eq!(paths(&s), ["kernel/lib/c.ko"]);
}

#[test]
fn a_root_file_system_holds_lib_modules() {
    let s = set(&modloop_with("lib/modules", DEP), &["b"]).unwrap();
    assert_eq!(paths(&s), ["kernel/lib/c.ko", "kernel/lib/b.ko"]);
}

#[test]
fn a_release_that_the_image_does_not_hold_is_refused() {
    let e = modules(Cursor::new(modloop()), "6.1.0-0-other", ["a"]).unwrap_err();
    assert_eq!(super::fault(&e), Some(&Fault::Release("6.1.0-0-other".into())));
    assert_eq!(e.class, Class::NotFound);
    for bad in ["", ".", "..", "a/b"] {
        let e = modules(Cursor::new(modloop()), bad, ["a"]).unwrap_err();
        assert_eq!(super::fault(&e), Some(&Fault::Release(bad.into())));
        assert_eq!(e.class, Class::Usage, "{bad:?}");
    }
    // `firmware` is a directory with no modules.dep.
    let e = modules(Cursor::new(modloop()), "firmware", ["a"]).unwrap_err();
    assert_eq!(super::fault(&e), Some(&Fault::Release("firmware".into())));
}

#[test]
fn a_modules_dep_that_does_not_parse_is_refused() {
    let dep = |text: &str| fault_of(set(&modloop_with("modules", text), &["a"]));
    assert_eq!(dep("kernel/net/a.ko kernel/lib/b.ko\n"), Fault::Dep { line: 1, why: "no colon" });
    assert_eq!(
        dep("kernel/net/a.ko:\n\nkernel/net/a.ko:\n"),
        Fault::Dep { line: 3, why: "a second line of one module" }
    );
    assert_eq!(dep("kernel/net/a.ko: ../b.ko\n"), Fault::Path("../b.ko".into()));
    assert_eq!(dep("/lib/modules/x/kernel/net/a.ko:\n"), Fault::Path("/lib/modules/x/kernel/net/a.ko".into()));
    assert_eq!(dep("kernel//a.ko:\n"), Fault::Path("kernel//a.ko".into()));
    let e = set(&modloop_with("modules", "kernel/net/a.ko: kernel/lib/b.ko\n"), &["a"]).unwrap();
    assert_eq!(paths(&e), ["kernel/lib/b.ko", "kernel/net/a.ko"], "a dependency with no line of its own");
    let e = set(&modloop_with("modules", "kernel/net/a.ko: kernel/lib/gone.ko\n"), &["a"]).unwrap_err();
    let missing = format!("modules/{RELEASE}/kernel/lib/gone.ko");
    assert_eq!(crate::squashfs::fault(&e), Some(&crate::squashfs::Fault::Missing(missing)));
}

#[test]
fn a_dependency_loop_is_refused() {
    let looped =
        "kernel/net/a.ko: kernel/lib/b.ko\nkernel/lib/b.ko: kernel/lib/c.ko\nkernel/lib/c.ko: kernel/net/a.ko\n";
    assert_eq!(fault_of(set(&modloop_with("modules", looped), &["b"])), Fault::Loop("kernel/lib/b.ko".into()));
    let own = "kernel/net/a.ko: kernel/net/a.ko\n";
    assert_eq!(fault_of(set(&modloop_with("modules", own), &["a"])), Fault::Loop("kernel/net/a.ko".into()));
}

/// The walk keeps its own stack: a chain of 200,000 modules does not
/// exhaust the stack of a test thread.
#[test]
fn a_long_chain_walks_with_no_recursion() {
    let n = 200_000;
    let text: String = (0..n).map(|i| format!("k/m{i}.ko: k/m{}.ko\n", i + 1)).collect();
    let table = Table::parse(&text).unwrap();
    let order = table.order(&[table.by_name["m0"]]).unwrap();
    assert_eq!(order.len(), n + 1);
    assert_eq!(table.paths[order[0]], format!("k/m{n}.ko"));
    assert_eq!(table.paths[order[n]], "k/m0.ko");
}

#[test]
fn a_set_over_its_ceiling_is_refused_before_a_read() {
    let w = Writer::default();
    let big = |n| Node::File { size: 3 << 30, blocks: vec![Block::Sparse; n], fragment: None, ext: true };
    let files = vec![
        (format!("modules/{RELEASE}/modules.dep"), w.file(b"kernel/a.ko: kernel/b.ko\nkernel/b.ko:\n")),
        (format!("modules/{RELEASE}/kernel/a.ko"), big(1)),
        (format!("modules/{RELEASE}/kernel/b.ko"), w.file(b"b")),
    ];
    let img = w.image(&tree(files));
    let e = modules_within(Cursor::new(&img), RELEASE, ["a"], 1 << 20).unwrap_err();
    assert_eq!(super::fault(&e), Some(&Fault::TooLarge { size: (3 << 30) + 1, limit: 1 << 20 }));
    assert_eq!(e.class, Class::Policy);
    assert_eq!(fault_of(set(&img, &["a"])), Fault::TooLarge { size: (3 << 30) + 1, limit: MAX_SET });
}

/// The seeds of the fuzz target `squashfs_image`, each cut at the end of
/// its image: `modloop` (the modloop of these tests), `tree` (each kind of
/// inode, small metadata blocks, an xz fragment, an export table) and
/// `options` (the compressor options and an x86 block). The directory
/// comes from `POD_GUEST_SEEDS`:
/// `POD_GUEST_SEEDS=fuzz/seeds/squashfs_image cargo test -p pod-guest --lib write_the_fuzz_seeds -- --ignored`.
#[test]
#[ignore = "writes the seeds of fuzz/ into POD_GUEST_SEEDS"]
fn write_the_fuzz_seeds() {
    let dir = std::path::PathBuf::from(std::env::var("POD_GUEST_SEEDS").expect("POD_GUEST_SEEDS"));
    let mut w = Writer { meta_chunk: 64, export: true, ..Writer::default() };
    w.fragments.push(Block::Xz(TAIL_XZ.to_vec()));
    let tree = crate::squashfs::tests::tree(&mut w);
    let code = || Node::File { size: 4096, blocks: vec![Block::Xz(X86_BCJ_XZ.to_vec())], fragment: None, ext: false };
    let options = Writer { options: Some((8192, 1)), ..Writer::default() };
    let options = options.image(&crate::squashfs::writer::dir(vec![("code.ko", code())]));
    for (name, img) in [("modloop", modloop()), ("tree", w.image(&tree)), ("options", options)] {
        let used = u64::from_le_bytes(img[40..48].try_into().unwrap()) as usize;
        std::fs::write(dir.join(name), &img[..used]).expect("a seed");
    }
}
