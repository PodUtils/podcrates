//! The GNU sparse files of the PAX form through the real extraction. The
//! records are those of GNU tar 1.35 `-S --format=posix
//! --sparse-version=V` for a file of 1 MiB with `hello` at byte 499712 and
//! `tail` at byte 700000.

use std::io::Cursor;

use crate::drive::{Scratch, refusal_of, run};
use crate::safety::Refusal;

const REAL: u64 = 1 << 20;
const HELLO: usize = 499_712;
const TAIL: usize = 700_000;
/// The regions that GNU tar stores: two blocks of 4 KiB, and an empty
/// region at the end of the file.
const MAP: [(u64, u64); 3] = [(499_712, 4096), (696_320, 4096), (1_048_576, 0)];

/// The file on the disk of the writer.
fn whole() -> Vec<u8> {
    let mut f = vec![0; REAL as usize];
    f[HELLO..HELLO + 5].copy_from_slice(b"hello");
    f[TAIL..TAIL + 4].copy_from_slice(b"tail");
    f
}

/// The data of each region of `map`, in order: the data of the member.
fn condensed(map: &[(u64, u64)]) -> Vec<u8> {
    let f = whole();
    map.iter().flat_map(|(o, l)| f[*o as usize..(*o + *l) as usize].to_vec()).collect()
}

/// The map of version 1.0 at the start of the data, padded to a block.
fn map_text(map: &[(u64, u64)]) -> Vec<u8> {
    let mut t = format!("{}\n", map.len());
    for (o, l) in map {
        t.push_str(&format!("{o}\n{l}\n"));
    }
    padded(t.into_bytes())
}

fn padded(mut b: Vec<u8>) -> Vec<u8> {
    b.resize(b.len().div_ceil(512) * 512, 0);
    b
}

type Records = Vec<(String, String)>;

/// The records of GNU tar for version `v` and `map`, and the name of its
/// header.
fn records(v: &str, map: &[(u64, u64)]) -> (Records, &'static str) {
    let r = |k: &str, v: String| (k.to_string(), v);
    let n = map.len().to_string();
    match v {
        "0.0" => {
            let mut out = vec![r("GNU.sparse.size", REAL.to_string()), r("GNU.sparse.numblocks", n)];
            for (o, l) in map {
                out.push(r("GNU.sparse.offset", o.to_string()));
                out.push(r("GNU.sparse.numbytes", l.to_string()));
            }
            (out, "f")
        }
        "0.1" => {
            let list: Vec<String> = map.iter().map(|(o, l)| format!("{o},{l}")).collect();
            let out = vec![
                r("GNU.sparse.size", REAL.to_string()),
                r("GNU.sparse.numblocks", n),
                r("GNU.sparse.name", "f".into()),
                r("GNU.sparse.map", list.join(",")),
            ];
            (out, "./GNUSparseFile.6168/f")
        }
        _ => {
            let out = vec![
                r("GNU.sparse.major", "1".into()),
                r("GNU.sparse.minor", "0".into()),
                r("GNU.sparse.name", "f".into()),
                r("GNU.sparse.realsize", REAL.to_string()),
            ];
            (out, "./GNUSparseFile.6170/f")
        }
    }
}

/// The records of version `v` with `key` set to `value`, or removed when
/// `value` is `None`.
fn with(v: &str, key: &str, value: Option<&str>) -> Records {
    let mut r = records(v, &MAP).0;
    r.retain(|x| x.0 != key);
    if let Some(value) = value {
        r.push((key.to_string(), value.to_string()));
    }
    r
}

/// A layer: one member with `records`, `name`, `kind` and `data`, then the
/// file `after`.
fn layer(records: &Records, name: &str, kind: tar::EntryType, data: &[u8]) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    b.append_pax_extensions(records.iter().map(|(k, v)| (k.as_str(), v.as_bytes()))).unwrap();
    let mut h = tar::Header::new_ustar();
    h.set_entry_type(kind);
    h.set_mode(0o644);
    h.set_size(data.len() as u64);
    if kind.is_symlink() {
        b.append_link(&mut h, name, "target").unwrap();
    } else {
        b.append_data(&mut h, name, Cursor::new(data)).unwrap();
    }
    let mut h = tar::Header::new_ustar();
    h.set_entry_type(tar::EntryType::Regular);
    h.set_mode(0o644);
    h.set_size(4);
    b.append_data(&mut h, "after", Cursor::new(b"next")).unwrap();
    b.into_inner().unwrap()
}

/// The layer of GNU tar for version `v`.
fn sparse_layer(v: &str) -> Vec<u8> {
    let (recs, name) = records(v, &MAP);
    let mut data = if v == "1.0" { map_text(&MAP) } else { Vec::new() };
    data.extend(condensed(&MAP));
    layer(&recs, name, tar::EntryType::Regular, &data)
}

#[test]
fn each_version_of_a_gnu_sparse_file_of_pax_extracts_with_its_holes() {
    for v in ["0.0", "0.1", "1.0"] {
        let s = Scratch::new("sparse");
        run(s.path(), &[sparse_layer(v)]).unwrap_or_else(|f| panic!("version {v}: {}", f.chain()));
        let p = s.path().join("f");
        let got = std::fs::read(&p).unwrap_or_else(|e| panic!("version {v}: no file f: {e}"));
        assert!(got == whole(), "version {v}: the content differs, {} bytes", got.len());
        for fake in ["GNUSparseFile.6168", "GNUSparseFile.6170"] {
            assert!(!s.path().join(fake).exists(), "version {v}: the name of the header came out");
        }
        assert_eq!(std::fs::read(s.path().join("after")).unwrap(), b"next", "version {v}");
        let blocks = std::os::unix::fs::MetadataExt::blocks(&std::fs::metadata(&p).unwrap());
        assert!(blocks * 512 < REAL / 2, "version {v}: {blocks} blocks of 512 bytes: the holes were written");
    }
}

#[test]
fn a_gnu_sparse_map_of_pax_that_does_not_hold_is_refused() {
    let data = condensed(&MAP);
    let map01 = |map: &[(u64, u64)]| records("0.1", map).0;
    let v1 = |text: Vec<u8>| [text, condensed(&MAP)].concat();
    let mut swapped = records("0.0", &MAP).0;
    swapped.swap(2, 3);
    let lines = |count: usize, n: usize| padded(format!("{count}\n{}", "0\n".repeat(n)).into_bytes());
    // What is wrong, the records, the data, and a text of the refusal.
    let rows: Vec<(&str, Records, Vec<u8>, &str)> = vec![
        ("regions out of order", map01(&[(696_320, 4096), (499_712, 4096)]), data.clone(), "in order"),
        ("regions that overlap", map01(&[(499_712, 4096), (500_000, 4096)]), data.clone(), "in order"),
        ("a region past the real size", map01(&[(499_712, 4096), (1_048_000, 4096)]), data.clone(), "real size"),
        ("an end past u64", map01(&[(499_712, 4096), (u64::MAX, 4096)]), data.clone(), "real size"),
        ("a count that is not the map", with("0.1", "GNU.sparse.numblocks", Some("2")), data.clone(), "numblocks"),
        ("no count", with("0.1", "GNU.sparse.numblocks", None), data.clone(), "numblocks"),
        ("less data than the map", records("0.1", &MAP).0, data[..4096].to_vec(), "hold"),
        ("more data than the map", records("0.1", &MAP).0, [data.clone(), vec![0; 512]].concat(), "hold"),
        ("a size before its offset", swapped, data.clone(), "after"),
        ("a number that is not one", with("0.1", "GNU.sparse.map", Some("499712,4096,x,1")), data.clone(), "number"),
        ("an odd map", with("0.1", "GNU.sparse.map", Some("499712,4096,696320")), data.clone(), "numblocks"),
        ("an unknown version", with("1.0", "GNU.sparse.major", Some("2")), v1(map_text(&MAP)), "version"),
        ("no real size", with("0.1", "GNU.sparse.size", None), data.clone(), "real size"),
        ("an unknown key", with("0.1", "GNU.sparse.other", Some("1")), data.clone(), "GNU.sparse.other"),
        ("a byte that is no digit", records("1.0", &MAP).0, v1(padded(b"3\n49x712\n".to_vec())), "digit"),
        ("an empty line", records("1.0", &MAP).0, v1(padded(b"3\n\n".to_vec())), "digit"),
        ("too many regions", records("1.0", &MAP).0, v1(padded(b"300000\n".to_vec())), "more than 262144"),
        ("a map longer than the data", records("1.0", &MAP).0, lines(10_000, 253), "longer than"),
        ("a number past 20 digits", records("1.0", &MAP).0, v1(padded([b'1'; 21].to_vec())), "20 digits"),
    ];
    for (what, recs, data, why) in rows {
        let s = Scratch::new("sparse-bad");
        let name = if recs.iter().any(|r| r.0 == "GNU.sparse.name") { "./GNUSparseFile.1/f" } else { "f" };
        let r = refusal_of(run(s.path(), &[layer(&recs, name, tar::EntryType::Regular, &data)]));
        assert!(matches!(&r.refusal, Refusal::Member(w) if w.contains(why)), "{what}: {r:?}");
        assert!(!s.path().join("f").exists(), "{what}: the file was written");
    }
}

#[test]
fn a_gnu_sparse_map_of_pax_on_an_entry_that_is_not_a_file_is_refused() {
    let s = Scratch::new("sparse-link");
    let r = refusal_of(run(s.path(), &[layer(&records("0.1", &MAP).0, "f", tar::EntryType::Symlink, &[])]));
    assert!(matches!(&r.refusal, Refusal::Member(w) if w.contains("not a regular file")), "{r:?}");
}

/// The records of each version give the map, the real size and the name of
/// the member, and none goes to the `tar` reader.
#[test]
fn the_gnu_sparse_records_of_pax_give_the_map_and_the_name() {
    use crate::sparse::{Map, Region, Sparse};
    let regions: Vec<Region> = MAP.iter().map(|(offset, len)| Region { offset: *offset, len: *len }).collect();
    for (v, map) in [("0.0", Map::Records(regions.clone())), ("0.1", Map::Records(regions)), ("1.0", Map::Data)] {
        let side = crate::pax::Side::default();
        let framed = crate::pax::Framed::new(Cursor::new(sparse_layer(v)), side.clone());
        let mut ar = tar::Archive::new(framed);
        let mut e = ar.entries().unwrap().next().unwrap().unwrap();
        let m = side.next();
        assert_eq!(m.sparse, Some(Sparse { real_size: REAL, map }), "version {v}");
        assert_eq!(m.path.as_deref(), if v == "0.0" { None } else { Some(&b"f"[..]) }, "version {v}");
        let given = e.pax_extensions().unwrap().map_or(0, |x| x.count());
        assert_eq!(given, 0, "version {v}: the tar reader got a sparse record");
    }
}
