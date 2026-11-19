use std::io::{Read, Write};

use super::*;

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn zstd(data: &[u8]) -> Vec<u8> {
    ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest)
}

fn all(kind: Compression, blob: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    Members::new(kind, blob)?.read_to_end(&mut out)?;
    Ok(out)
}

/// Each member of a layer, in order, and the bytes after the last member
/// stay unread (zeros of a padded blob), for each compression.
#[test]
fn each_member_of_a_layer_is_read_and_the_bytes_after_them_stay() {
    let parts: [&[u8]; 3] = [b"the first member, ", &[7u8; 70_000], b" and the last"];
    let whole: Vec<u8> = parts.concat();
    for (kind, encode) in [(Compression::Gzip, gzip as fn(&[u8]) -> Vec<u8>), (Compression::Zstd, zstd)] {
        let mut blob: Vec<u8> = parts.iter().flat_map(|p| encode(p)).collect();
        assert_eq!(all(kind, &blob).unwrap(), whole, "{kind:?}");
        blob.extend_from_slice(&[0u8; 512]);
        let mut r = Members::new(kind, &blob[..]).unwrap();
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(out, whole, "{kind:?} with zeros after it");
    }
}

/// A zstd skippable frame before and between frames is no data.
#[test]
fn a_zstd_skippable_frame_is_skipped() {
    let skippable = |len: u32| {
        let mut f = vec![0x5a, 0x2a, 0x4d, 0x18];
        f.extend_from_slice(&len.to_le_bytes());
        f.extend(std::iter::repeat_n(9u8, len as usize));
        f
    };
    let blob = [skippable(5), zstd(b"one "), skippable(0), zstd(b"two")].concat();
    assert_eq!(all(Compression::Zstd, &blob).unwrap(), b"one two");
}

/// The magic of a next member across the end of the read-ahead: a reader
/// that gives one byte at a time.
#[test]
fn a_member_that_starts_across_a_short_read_is_found() {
    struct OneByte<'a>(&'a [u8]);
    impl Read for OneByte<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let Some((b, rest)) = self.0.split_first() else { return Ok(0) };
            out[0] = *b;
            self.0 = rest;
            Ok(1)
        }
    }
    for (kind, blob) in [
        (Compression::Gzip, [gzip(b"ab"), gzip(b"cd")].concat()),
        (Compression::Zstd, [zstd(b"ab"), zstd(b"cd")].concat()),
    ] {
        let mut out = Vec::new();
        Members::new(kind, OneByte(&blob)).unwrap().read_to_end(&mut out).unwrap();
        assert_eq!(out, b"abcd", "{kind:?}");
    }
}

/// A blob that is not of its compression fails, and a member cut short
/// fails: no part is taken as the whole.
#[test]
fn a_blob_of_another_kind_or_cut_short_fails() {
    assert!(all(Compression::Gzip, b"not gzip at all").is_err());
    assert!(Members::new(Compression::Zstd, &b"not zstd at all"[..]).is_err());
    let g = [gzip(b"whole"), gzip(&[3u8; 4096])].concat();
    assert!(all(Compression::Gzip, &g[..g.len() - 10]).is_err());
}
