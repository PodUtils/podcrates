//! Images with changed bytes: a corrupt descriptor, a bad main sequence, a
//! bad metadata file, allocation extent descriptors.

use super::writer::{BS, DATA, Node, PART, Part, Writer, dir, file, seal};
use super::*;

/// The block of the image of block `lbn` of the partition (no metadata
/// partition).
fn at(lbn: u32) -> usize {
    (PART + lbn) as usize * BS
}

/// In the image of a root with one entry, the file entry of the root is at
/// block 2 of the partition, the entry of the file at 3.
const ROOT: u32 = 2;
const FIRST: u32 = 3;

/// A byte of the body, the checksum of the tag, a tag at another block:
/// each is a typed fault of the descriptor.
#[test]
fn a_corrupt_descriptor_is_refused() {
    let image = Writer::default().image(&dir(vec![("f", file(b"data"))]));
    let fault = |image: &[u8]| match Udf::open(image).and_then(|mut u| u.find("/f")) {
        Err(Error::Descriptor { what: "file entry", fault, .. }) => fault,
        other => panic!("not a fault of the file entry: {other:?}"),
    };
    let mut bad = image.clone();
    bad[at(FIRST) + 60] ^= 1;
    assert_eq!(fault(&bad), TagFault::Crc);
    let mut bad = image.clone();
    bad[at(ROOT) + 4] ^= 1;
    assert_eq!(fault(&bad), TagFault::Checksum);
    let mut bad = image.clone();
    let entry = image[at(FIRST)..at(FIRST) + BS].to_vec();
    bad[at(ROOT)..at(ROOT) + BS].copy_from_slice(&entry);
    assert_eq!(fault(&bad), TagFault::Location { want: ROOT, got: FIRST });
    let mut bad = image.clone();
    bad[at(FIRST) + 2] = 9;
    assert_eq!(fault(&bad), TagFault::Checksum, "a version change breaks the checksum first");
    // A file identifier descriptor with a bad CRC.
    let mut bad = image.clone();
    bad[at(4) + 40 + 38] ^= 1;
    let err = Udf::open(bad.as_slice()).unwrap().find("/f").unwrap_err();
    assert!(matches!(err, Error::Descriptor { what: "file identifier descriptor", fault: TagFault::Crc, .. }), "{err}");
}

/// A broken logical volume descriptor in the main sequence: the reserve
/// sequence stands in. Both broken: the fault of the main sequence.
#[test]
fn the_reserve_sequence_stands_in_for_a_bad_main() {
    let image = Writer::default().image(&dir(vec![("f", file(b"data"))]));
    let mut bad = image.clone();
    bad[34 * BS + 100] ^= 1;
    let mut udf = Udf::open(bad.as_slice()).expect("the reserve sequence stands in");
    let extents = udf.find("/f").unwrap();
    assert_eq!(udf.read(&extents, 10).unwrap(), b"data");
    bad[50 * BS + 100] ^= 1;
    let err = Udf::open(bad.as_slice()).unwrap_err();
    assert!(matches!(err, Error::Descriptor { offset, fault: TagFault::Crc, .. } if offset == 34 * BS as u64), "{err}");
}

/// A broken metadata file: its mirror stands in.
#[test]
fn the_metadata_mirror_stands_in_for_a_bad_metadata_file() {
    let image =
        Writer { revision: 0x0250, metadata: true, ..Writer::default() }.image(&dir(vec![("f", file(b"data"))]));
    let mut bad = image.clone();
    bad[at(0) + 20] ^= 1;
    let mut udf = Udf::open(bad.as_slice()).unwrap();
    let extents = udf.find("/f").unwrap();
    assert_eq!(udf.read(&extents, 10).unwrap(), b"data");
    bad[at(1) + 20] ^= 1;
    let err = Udf::open(bad.as_slice()).unwrap_err();
    assert!(matches!(err, Error::Descriptor { what: "file entry", fault: TagFault::Crc, .. }), "{err}");
}

/// Write the allocation descriptors `ads` (short) into the file entry at
/// `lbn` of a UDF 1.02 image, and an allocation extent descriptor at
/// block `aed` with `inner`.
fn patch(image: &mut [u8], lbn: u32, ads: &[(u32, u32, u32)], aed: u32, inner: &[(u32, u32, u32)]) {
    let list = |ads: &[(u32, u32, u32)]| -> Vec<u8> {
        ads.iter().flat_map(|&(kind, len, at)| [(kind << 30 | len).to_le_bytes(), at.to_le_bytes()].concat()).collect()
    };
    let e = &mut image[at(lbn)..at(lbn) + BS];
    let first = list(ads);
    e[172..176].copy_from_slice(&(first.len() as u32).to_le_bytes());
    e[176..176 + first.len()].copy_from_slice(&first);
    seal(e, 261, 2, lbn, 176 + first.len());
    let inner = list(inner);
    let d = &mut image[at(aed)..at(aed) + BS];
    d.fill(0);
    d[20..24].copy_from_slice(&(inner.len() as u32).to_le_bytes());
    d[24..24 + inner.len()].copy_from_slice(&inner);
    seal(d, 258, 2, aed, 24 + inner.len());
}

/// An allocation extent descriptor continues the list of a file; one that
/// continues to itself ends at the limit.
#[test]
fn allocation_extents_continue_the_list() {
    let parts = vec![Part::Bytes(vec![1; BS]), Part::Bytes(vec![2; 10])];
    let mut image = Writer::default().image(&dir(vec![("f", Node::File(parts))]));
    // The data: block DATA, the gap at DATA + 1, the second part at DATA + 2;
    // the gap holds the allocation extent descriptor.
    let gap = DATA + 1;
    patch(&mut image, FIRST, &[(0, BS as u32, DATA), (3, BS as u32, gap)], gap, &[(0, 10, DATA + 2)]);
    let mut udf = Udf::open(image.as_slice()).unwrap();
    let extents = udf.find("/f").unwrap();
    assert_eq!(extents.parts.len(), 2, "the allocation extent was not followed: {extents:?}");
    assert_eq!(udf.read(&extents, 1 << 20).unwrap(), [vec![1; BS], vec![2; 10]].concat());
    patch(&mut image, FIRST, &[(0, BS as u32, DATA), (3, BS as u32, gap)], gap, &[(3, BS as u32, gap)]);
    let mut udf = Udf::open_with(image.as_slice(), Limits { extents: 100, ..Limits::default() }).unwrap();
    let err = udf.find("/f").unwrap_err();
    assert!(matches!(err, Error::Limit { what: "allocation descriptors of a file", limit: 100 }), "{err}");
    // An extent past the end of the partition.
    patch(&mut image, FIRST, &[(0, BS as u32, 1 << 20)], gap, &[]);
    let err = Udf::open(image.as_slice()).unwrap().find("/f").unwrap_err();
    assert!(matches!(err, Error::Corrupt { what: "an extent past the end of its partition", .. }), "{err}");
    // Allocation descriptors that end before the size of the file.
    patch(&mut image, FIRST, &[(0, 100, DATA)], gap, &[]);
    let err = Udf::open(image.as_slice()).unwrap().find("/f").unwrap_err();
    assert!(matches!(err, Error::Corrupt { what: "the allocation descriptors end before the size of the file", .. }));
}

/// A sparable partition map and an ICB of strategy 4096 are valid UDF
/// that this reader does not read: each is `Unsupported`.
#[test]
fn a_structure_that_the_reader_does_not_read_is_unsupported() {
    let image =
        Writer { revision: 0x0250, metadata: true, ..Writer::default() }.image(&dir(vec![("f", file(b"data"))]));
    let mut bad = image.clone();
    for lvd in [34, 50] {
        let at = lvd * BS;
        let len = 440 + 6 + 64;
        bad[at + 440 + 6 + 5..at + 440 + 6 + 28].copy_from_slice(b"*UDF Sparable Partition");
        seal(&mut bad[at..at + len], 6, 3, lvd as u32, len);
    }
    let err = Udf::open(bad.as_slice()).unwrap_err();
    assert!(matches!(err, Error::Unsupported { what: "a partition map", .. }), "{err}");
    let image = Writer::default().image(&dir(vec![("f", file(b"data"))]));
    let mut bad = image.clone();
    let e = &mut bad[at(FIRST)..at(FIRST) + BS];
    e[20..22].copy_from_slice(&4096u16.to_le_bytes());
    let len = 176 + u32::from_le_bytes(e[172..176].try_into().unwrap()) as usize;
    seal(e, 261, 2, FIRST, len);
    let err = Udf::open(bad.as_slice()).unwrap().find("/f").unwrap_err();
    assert!(matches!(&err, Error::Unsupported { what: "the ICB strategy type", value } if value == "4096"), "{err}");
}
