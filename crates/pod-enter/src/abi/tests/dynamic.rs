//! The reader of `.dynamic` (`Search`): `DT_NEEDED`, `DT_RUNPATH` and
//! `DT_RPATH` of a file, and the list that the loader searches.

use super::super::*;

/// A 64-bit little-endian shared object with a section table: `.dynstr`,
/// `.dynamic` (`DT_NEEDED` for each name of `needed`, then `DT_RUNPATH` and
/// `DT_RPATH` where given) and `.shstrtab`. It has no program header: the
/// reader of the search reads the sections only.
pub(crate) fn dyn_object(machine: u16, needed: &[&str], runpath: Option<&str>, rpath: Option<&str>) -> Vec<u8> {
    let mut strs = vec![0u8];
    let mut put = |s: &str| {
        let at = strs.len() as u64;
        strs.extend_from_slice(s.as_bytes());
        strs.push(0);
        at
    };
    let mut entries: Vec<(u64, u64)> = needed.iter().map(|n| (1, put(n))).collect();
    if let Some(r) = runpath {
        entries.push((29, put(r)));
    }
    if let Some(r) = rpath {
        entries.push((15, put(r)));
    }
    entries.push((0, 0));
    let names = b"\0.dynstr\0.dynamic\0.shstrtab\0";
    let mut b = super::tiny_elf(machine);
    b[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
    let str_at = b.len() as u64;
    b.extend_from_slice(&strs);
    b.resize(b.len().next_multiple_of(8), 0);
    let dyn_at = b.len() as u64;
    for (tag, val) in &entries {
        b.extend_from_slice(&tag.to_le_bytes());
        b.extend_from_slice(&val.to_le_bytes());
    }
    let names_at = b.len() as u64;
    b.extend_from_slice(names);
    b.resize(b.len().next_multiple_of(8), 0);
    let sh_at = b.len() as u64;
    // (name, type, offset, size, link) of the null section and the three.
    let headers = [
        (0u32, 0u32, 0u64, 0u64, 0u32),
        (1, 3, str_at, strs.len() as u64, 0),
        (9, 6, dyn_at, (entries.len() * 16) as u64, 1),
        (18, 3, names_at, names.len() as u64, 0),
    ];
    for (name, kind, offset, size, link) in headers {
        let mut h = [0u8; 64];
        h[0..4].copy_from_slice(&name.to_le_bytes());
        h[4..8].copy_from_slice(&kind.to_le_bytes());
        h[24..32].copy_from_slice(&offset.to_le_bytes());
        h[32..40].copy_from_slice(&size.to_le_bytes());
        h[40..44].copy_from_slice(&link.to_le_bytes());
        b.extend_from_slice(&h);
    }
    b[40..48].copy_from_slice(&sh_at.to_le_bytes());
    b[58..60].copy_from_slice(&64u16.to_le_bytes());
    b[60..62].copy_from_slice(&4u16.to_le_bytes());
    b[62..64].copy_from_slice(&3u16.to_le_bytes());
    b
}

/// The reader gives each `DT_NEEDED` in order, and the two lists; the
/// loader searches the `DT_RUNPATH` of an object that has both.
#[test]
fn the_runpath_and_the_rpath_of_a_file_are_read() {
    let b = dyn_object(62, &["libx.so.1", "libc.so.6"], Some("$ORIGIN/../lib:/usr/lib/systemd"), Some("/old"));
    let s = Search::parse("/o", &b).unwrap();
    assert_eq!(s.machine, 62);
    assert_eq!(s.needed, ["libx.so.1", "libc.so.6"]);
    assert_eq!(s.runpath.as_deref(), Some("$ORIGIN/../lib:/usr/lib/systemd"));
    assert_eq!(s.rpath.as_deref(), Some("/old"));
    assert_eq!(s.dirs(), ["$ORIGIN/../lib", "/usr/lib/systemd"]);
    let only_rpath = Search::parse("/o", &dyn_object(62, &[], None, Some("/a::/b"))).unwrap();
    assert_eq!(only_rpath.dirs(), ["/a", "/b"], "an empty entry is left out");
    let none = Search::parse("/o", &dyn_object(62, &["libc.so.6"], None, None)).unwrap();
    assert!(none.dirs().is_empty());
    // The reader of the symbols reads the same `.dynamic`.
    let elf = Elf::parse("/o", &b).unwrap();
    assert_eq!(elf.needed, ["libx.so.1", "libc.so.6"]);
}

/// A file with no section table is a named refusal, never an empty search.
#[test]
fn a_file_with_no_section_table_has_no_search() {
    let e = Search::parse("/o", &super::tiny_elf(62)).unwrap_err();
    assert!(e.0.contains("no section table"), "{e}");
}
