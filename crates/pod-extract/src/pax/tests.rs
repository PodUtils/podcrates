use std::io::{Cursor, Read};

use super::*;
use crate::safety::Refusal;

/// The members of `data` through [`Framed`]: the path that the `tar` reader
/// gives, the records of the side, and the content.
type Read3 = (Vec<u8>, Member, Vec<u8>);

fn read_all(data: &[u8]) -> io::Result<Vec<Read3>> {
    let side = Side::default();
    let mut ar = tar::Archive::new(Framed::new(Cursor::new(data.to_vec()), side.clone()));
    let mut out = Vec::new();
    for e in ar.entries()? {
        let mut e = e?;
        let m = side.next();
        let mut body = Vec::new();
        e.read_to_end(&mut body)?;
        out.push((e.path_bytes().into_owned(), m, body));
    }
    Ok(out)
}

fn file(b: &mut tar::Builder<Vec<u8>>, path: &str, body: &[u8]) {
    let mut h = tar::Header::new_ustar();
    h.set_entry_type(tar::EntryType::Regular);
    h.set_mode(0o644);
    h.set_size(body.len() as u64);
    b.append_data(&mut h, path, Cursor::new(body.to_vec())).unwrap();
}

/// A header of `kind` that gives `size` bytes, with no data after it.
fn bare(kind: tar::EntryType, size: u64) -> Vec<u8> {
    let mut h = tar::Header::new_ustar();
    h.set_entry_type(kind);
    h.set_path("././@PaxHeader").unwrap();
    h.set_size(size);
    h.set_cksum();
    h.as_bytes().to_vec()
}

#[test]
fn pax_records_parse_by_their_length() {
    // A value with a newline byte, and bytes after it that read as a
    // record when the records are cut at each newline.
    let mut data = Vec::new();
    push_record(&mut data, b"SCHILY.xattr.user.bin", b"a\n20 path=etc/passwd\n");
    push_record(&mut data, b"path", b"f");
    let got = records(&data).unwrap();
    assert_eq!(got, vec![(&b"SCHILY.xattr.user.bin"[..], &b"a\n20 path=etc/passwd\n"[..]), (&b"path"[..], &b"f"[..])]);
    for bad in [&b"5 a=b\n"[..], b"9 a=b\n", b"x a=b\n", b"6 ab\n\n", b"7 a=bc ", b"5 =b\n"] {
        assert!(records(bad).is_err(), "{:?} parsed", String::from_utf8_lossy(bad));
    }
}

#[test]
fn a_record_counts_the_digits_of_its_length() {
    // Bodies that put the length at 9, 10, 99, 100 and more.
    for n in 0..120 {
        let mut out = Vec::new();
        push_record(&mut out, b"k", &vec![b'v'; n]);
        let digits = out.iter().position(|b| *b == b' ').unwrap();
        let len: usize = std::str::from_utf8(&out[..digits]).unwrap().parse().unwrap();
        assert_eq!(len, out.len(), "a value of {n} bytes");
        assert_eq!(records(&out).unwrap(), vec![(&b"k"[..], &vec![b'v'; n][..])]);
    }
}

#[test]
fn a_value_with_a_newline_byte_round_trips() {
    let value = b"\x01\x00\x00\x02\x0a\x20\n20 path=etc/passwd\n";
    let mut b = tar::Builder::new(Vec::new());
    b.append_pax_extensions([("SCHILY.xattr.security.capability", &value[..]), ("SCHILY.xattr.user.a", b"1")]).unwrap();
    file(&mut b, "bin/ping", b"ping");
    file(&mut b, "after", b"next");
    let got = read_all(&b.into_inner().unwrap()).unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].0, b"bin/ping");
    let want = vec![
        (b"SCHILY.xattr.security.capability".to_vec(), value.to_vec()),
        (b"SCHILY.xattr.user.a".to_vec(), b"1".to_vec()),
    ];
    assert_eq!(got[0].1.xattrs, want);
    assert_eq!(got[0].2, b"ping");
    assert_eq!(
        (got[1].0.as_slice(), got[1].1.clone(), got[1].2.as_slice()),
        (&b"after"[..], Member::default(), &b"next"[..])
    );
    let w = crate::xattr::read(&got[0].1.xattrs, "bin/ping");
    assert_eq!((w.attrs.len(), w.refused.len()), (2, 0), "{:?}", w.refused);
    assert_eq!(w.attrs[0].value, value);
}

#[test]
fn a_path_with_a_newline_byte_comes_from_the_side() {
    let mut b = tar::Builder::new(Vec::new());
    b.append_pax_extensions([("path", &b"dir/a\nb"[..]), ("linkpath", b"t\nu")]).unwrap();
    file(&mut b, "short", b"x");
    let got = read_all(&b.into_inner().unwrap()).unwrap();
    assert_eq!(got[0].1.path.as_deref(), Some(&b"dir/a\nb"[..]));
    assert_eq!(got[0].1.linkpath.as_deref(), Some(&b"t\nu"[..]));
    // The record was not given to the `tar` reader: it gives the header.
    assert_eq!(got[0].0, b"short");
}

#[test]
fn the_size_record_frames_the_next_member() {
    // The header gives 0 bytes; the last PAX size gives 5, and both readers
    // must skip 5 bytes, not 0 or 9, to find the next header.
    let mut b = tar::Builder::new(Vec::new());
    b.append_pax_extensions([("size", &b"9"[..]), ("size", b"5"), ("path", b"big")]).unwrap();
    let mut h = tar::Header::new_ustar();
    h.set_entry_type(tar::EntryType::Regular);
    h.set_mode(0o644);
    h.set_size(0);
    b.append_data(&mut h, "x", Cursor::new(b"12345".to_vec())).unwrap();
    file(&mut b, "after", b"next");
    let got = read_all(&b.into_inner().unwrap()).unwrap();
    let names: Vec<&[u8]> = got.iter().map(|g| g.0.as_slice()).collect();
    assert_eq!(names, vec![&b"big"[..], b"after"]);
    assert_eq!((got[0].2.as_slice(), got[1].2.as_slice()), (&b"12345"[..], &b"next"[..]));
}

/// A reader that fails a test when more than `left` bytes are read.
struct Capped(Cursor<Vec<u8>>, usize);

impl Read for Capped {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.0.read(buf)?;
        self.1 = self.1.checked_sub(n).expect("the reader read the data of a refused member");
        Ok(n)
    }
}

#[test]
fn a_member_past_its_bound_is_refused_before_its_data() {
    let cases = [
        (tar::EntryType::XHeader, 2 * PAX_MAX, "more than 1048576"),
        (tar::EntryType::GNULongName, 8192, "more than 4096"),
        (tar::EntryType::GNULongLink, LONG_MAX + 1, "more than 4096"),
    ];
    for (kind, size, why) in cases {
        let mut data = bare(kind, size);
        data.extend(vec![b'z'; 4096]);
        let side = Side::default();
        let mut ar = tar::Archive::new(Framed::new(Capped(Cursor::new(data.clone()), BLOCK), side.clone()));
        let e = ar.entries().unwrap().next().unwrap().err().expect("refused");
        let (what, said) = side.refused().expect("named");
        assert!(said.contains(why) && what.contains("at byte 0"), "{what}: {said}");
        let f = failed(&side, "sha256:layer", e);
        let r = crate::error::refused(&f).expect("a refusal");
        assert_eq!(r.refusal, Refusal::Member(said.clone()));
        // The pass of the whiteouts refuses the layer the same way.
        let f = crate::remove::collect("sha256:layer", &mut Cursor::new(data)).unwrap_err();
        assert!(crate::error::refused(&f).is_some_and(|r| r.refusal == Refusal::Member(said.clone())), "{f}");
    }
}

#[test]
fn a_pax_header_that_does_not_parse_is_refused() {
    let mut data = bare(tar::EntryType::XHeader, 6);
    data.extend(b"9 a=b\n");
    data.resize(BLOCK * 2, 0);
    let mut b = tar::Builder::new(data);
    file(&mut b, "f", b"x");
    let f = crate::remove::collect("sha256:layer", &mut Cursor::new(b.into_inner().unwrap())).unwrap_err();
    let r = crate::error::refused(&f).expect("a refusal");
    assert!(matches!(&r.refusal, Refusal::Member(w) if w.contains("does not parse")), "{r:?}");
}

#[test]
fn a_global_header_is_an_entry_and_takes_the_records_before_it() {
    let mut b = tar::Builder::new(Vec::new());
    b.append_pax_extensions([("SCHILY.xattr.user.a", &b"1"[..])]).unwrap();
    let mut h = tar::Header::new_ustar();
    h.set_entry_type(tar::EntryType::XGlobalHeader);
    h.set_size(0);
    b.append_data(&mut h, "global", std::io::empty()).unwrap();
    file(&mut b, "f", b"x");
    let got = read_all(&b.into_inner().unwrap()).unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].1.xattrs.len(), 1);
    assert_eq!((got[1].0.as_slice(), got[1].1.clone()), (&b"f"[..], Member::default()));
}
