//! The reader against images of the writer of `tests/writer.rs`: each
//! revision and form, the metadata partition, the extents, the faults and
//! the limits. `tests/faults.rs` changes bytes of an image.

mod faults;
mod writer;

use writer::{BS, DATA, Node, PART, Part, Writer, dir, file};

use super::*;

/// A tree as a Windows ISO has it: `/sources/boot.wim`, more files, and a
/// directory in a directory.
fn tree() -> Node {
    dir(vec![
        ("setup.exe", file(b"MZ setup")),
        (
            "sources",
            dir(vec![
                ("boot.wim", file(b"hello")),
                ("install.wim", file(&[7u8; 3 * BS + 100])),
                ("deep", dir(vec![("deeper", dir(vec![("leaf.txt", file(b"leaf"))]))])),
            ]),
        ),
        ("efi", dir(vec![])),
    ])
}

fn bytes_of(udf: &mut Udf<&[u8]>, path: &str) -> Vec<u8> {
    let extents = udf.find(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let bytes = udf.read(&extents, 1 << 20).unwrap();
    assert_eq!(bytes.len() as u64, extents.size);
    bytes
}

/// A file in a subdirectory: one part, at the bytes of the file in the
/// image.
#[test]
fn a_file_in_a_subdirectory_has_its_extent() {
    let image = Writer::default().image(&tree());
    let mut udf = Udf::open(image.as_slice()).unwrap();
    assert_eq!((udf.block_size(), udf.revision()), (2048, 0x0102));
    let extents = udf.find("/sources/boot.wim").unwrap();
    assert_eq!(extents.size, 5);
    let range = extents.range().expect("one part");
    assert_eq!(&image[range.start as usize..range.end as usize], b"hello");
    assert_eq!(range.start % BS as u64, 0);
    assert!(range.start >= u64::from(PART + DATA) * BS as u64, "{range:?}");
    assert_eq!(bytes_of(&mut udf, "sources/deep/deeper/leaf.txt"), b"leaf");
    assert_eq!(bytes_of(&mut udf, "\\sources\\install.wim"), vec![7u8; 3 * BS + 100]);
    let names: Vec<String> = udf.list("/").unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["setup.exe", "sources", "efi"]);
    assert!(udf.list("/efi").unwrap().is_empty());
    assert_eq!(find(image.as_slice(), "/setup.exe").unwrap().size, 8);
}

/// UDF 1.02 to 2.60, with `short_ad` and `long_ad`, with file entries and
/// extended file entries.
#[test]
fn each_revision_and_form_reads() {
    for revision in [0x0102, 0x0150, 0x0200, 0x0201, 0x0250, 0x0260] {
        for long_ads in [false, true] {
            let image = Writer { revision, long_ads, ..Writer::default() }.image(&tree());
            let mut udf = Udf::open(image.as_slice()).unwrap_or_else(|e| panic!("{revision:x}: {e}"));
            assert_eq!(udf.revision(), revision);
            assert_eq!(bytes_of(&mut udf, "/sources/deep/deeper/leaf.txt"), b"leaf", "{revision:x} {long_ads}");
            assert_eq!(bytes_of(&mut udf, "/sources/install.wim").len(), 3 * BS + 100);
        }
    }
}

/// A metadata partition (UDF 2.50, 2.60): the file entries and the
/// directories are blocks of the metadata file, whose two extents have a
/// gap between them; 40 files put entries in both extents.
#[test]
fn a_metadata_partition_maps_each_block() {
    let many: Vec<(String, Node)> =
        (0..40).map(|i| (format!("f{i:02}"), file(format!("file {i}").as_bytes()))).collect();
    let root = dir(vec![("many", Node::Dir(many)), ("sources", dir(vec![("boot.wim", file(b"hello"))]))]);
    for revision in [0x0250, 0x0260] {
        let image = Writer { revision, metadata: true, ..Writer::default() }.image(&root);
        let mut udf = Udf::open(image.as_slice()).unwrap();
        for i in 0..40 {
            assert_eq!(bytes_of(&mut udf, &format!("/many/f{i:02}")), format!("file {i}").as_bytes());
        }
        assert_eq!(bytes_of(&mut udf, "/sources/boot.wim"), b"hello");
        assert_eq!(udf.list("/many").unwrap().len(), 40);
    }
}

/// A missing name, a path through a file, a directory as a file, `..`.
#[test]
fn a_missing_path_is_not_found() {
    let image = Writer::default().image(&tree());
    let mut udf = Udf::open(image.as_slice()).unwrap();
    let err = udf.find("/sources/none.wim").unwrap_err();
    assert!(matches!(&err, Error::NotFound { path } if path == "/sources/none.wim"), "{err}");
    let err = udf.find("/nothing/boot.wim").unwrap_err();
    assert!(matches!(&err, Error::NotFound { path } if path == "/nothing"), "{err}");
    let err = udf.find("/setup.exe/boot.wim").unwrap_err();
    assert!(matches!(&err, Error::NotADirectory { path } if path == "/setup.exe"), "{err}");
    let err = udf.find("/sources").unwrap_err();
    assert!(matches!(&err, Error::NotAFile { path, file_type: 4 } if path == "/sources"), "{err}");
    assert!(matches!(udf.find("/").unwrap_err(), Error::NotAFile { .. }));
    assert!(matches!(udf.list("/setup.exe").unwrap_err(), Error::NotADirectory { .. }));
    assert!(matches!(udf.find("/sources/../setup.exe").unwrap_err(), Error::Path { .. }));
}

/// A file of two extents with a block between them: two parts, in order;
/// a hole between two extents reads as zeros.
#[test]
fn a_file_of_two_extents_gives_both_in_order() {
    let (one, two) = (vec![1u8; 2 * BS], vec![2u8; 300]);
    let root = dir(vec![
        ("two", Node::File(vec![Part::Bytes(one.clone()), Part::Bytes(two.clone())])),
        ("hole", Node::File(vec![Part::Bytes(one.clone()), Part::Hole(2), Part::Bytes(two.clone())])),
    ]);
    for w in [Writer::default(), Writer { revision: 0x0250, metadata: true, ..Writer::default() }] {
        let image = w.image(&root);
        let mut udf = Udf::open(image.as_slice()).unwrap();
        let extents = udf.find("/two").unwrap();
        assert_eq!(extents.size, (one.len() + two.len()) as u64);
        let [Extent::Data { offset: a, len: 4096 }, Extent::Data { offset: b, len: 300 }] = extents.parts[..] else {
            panic!("not two parts: {extents:?}")
        };
        assert_eq!(b, a + 3 * BS as u64, "the second extent follows the gap block");
        assert_eq!(extents.range(), None);
        assert_eq!(udf.read(&extents, 1 << 20).unwrap(), [one.clone(), two.clone()].concat());
        let extents = udf.find("/hole").unwrap();
        let lens: Vec<(bool, u64)> =
            extents.parts.iter().map(|p| (matches!(p, Extent::Zero { .. }), p.len())).collect();
        assert_eq!(lens, [(false, 4096), (true, 4096), (false, 300)]);
        assert_eq!(udf.read(&extents, 1 << 20).unwrap(), [one.clone(), vec![0; 2 * BS], two.clone()].concat());
    }
}

/// A file whose bytes are in its entry: one part inside the entry.
#[test]
fn an_embedded_file_is_inside_its_entry() {
    for revision in [0x0102, 0x0201] {
        let image =
            Writer { revision, ..Writer::default() }.image(&dir(vec![("small", Node::Embedded(b"tiny".to_vec()))]));
        let mut udf = Udf::open(image.as_slice()).unwrap();
        let extents = udf.find("/small").unwrap();
        let range = extents.range().unwrap();
        assert_eq!(&image[range.start as usize..range.end as usize], b"tiny");
        assert!(range.start < u64::from(PART + DATA) * BS as u64);
    }
}

/// The same name wins; else a name with no regard to case. A name in
/// UTF-16 reads.
#[test]
fn a_name_matches_with_no_regard_to_case() {
    let root = dir(vec![("a", file(b"small a")), ("A", file(b"big A")), ("Ünïcødé-文件", file(b"wide"))]);
    let image = Writer::default().image(&root);
    let mut udf = Udf::open(image.as_slice()).unwrap();
    assert_eq!(bytes_of(&mut udf, "A"), b"big A");
    assert_eq!(bytes_of(&mut udf, "a"), b"small a");
    assert_eq!(bytes_of(&mut udf, "Ünïcødé-文件"), b"wide");
    assert_eq!(bytes_of(&mut udf, "üNÏCØDÉ-文件"), b"wide");
    let image = Writer::default().image(&tree());
    let mut udf = Udf::open(image.as_slice()).unwrap();
    assert_eq!(bytes_of(&mut udf, "/SOURCES/Boot.WIM"), b"hello");
}

/// The bounds of depth, entries, directory bytes and allocation
/// descriptors.
#[test]
fn the_limits_hold() {
    let image = Writer::default().image(&tree());
    let open = |limits| Udf::open_with(image.as_slice(), limits).unwrap();
    let deep = "/a".repeat(65);
    let err = open(Limits::default()).find(&deep).unwrap_err();
    assert!(matches!(err, Error::Limit { what: "components of a path", limit: 64 }), "not the bound of depth: {err}");
    let err = open(Limits { entries: 2, ..Limits::default() }).list("/").unwrap_err();
    assert!(matches!(err, Error::Limit { what: "entries of a directory", .. }), "{err}");
    let err = open(Limits { directory_bytes: 64, ..Limits::default() }).find("/setup.exe").unwrap_err();
    assert!(matches!(err, Error::Limit { what: "bytes of a directory", .. }), "{err}");
    let parts = vec![Part::Bytes(vec![1; BS]), Part::Bytes(vec![2; BS]), Part::Bytes(vec![3; 10])];
    let image = Writer::default().image(&dir(vec![("three", Node::File(parts))]));
    let mut udf = Udf::open_with(image.as_slice(), Limits { extents: 2, ..Limits::default() }).unwrap();
    let err = udf.find("/three").unwrap_err();
    assert!(matches!(err, Error::Limit { what: "allocation descriptors of a file", limit: 2 }), "{err}");
    let mut udf = Udf::open(image.as_slice()).unwrap();
    let extents = udf.find("/three").unwrap();
    assert!(matches!(udf.read(&extents, 100).unwrap_err(), Error::Limit { what: "bytes of a read", .. }));
}

/// An image with no anchor, a short image, and a reader that fails.
#[test]
fn no_anchor_is_not_udf() {
    assert!(matches!(Udf::open(vec![0u8; 600 * 1024]).unwrap_err(), Error::NotUdf));
    assert!(matches!(Udf::open(&[0u8; 100][..]).unwrap_err(), Error::NotUdf));
    #[derive(Debug)]
    struct Broken;
    impl ReadAt for Broken {
        fn read_at(&mut self, _: u64, _: &mut [u8]) -> std::io::Result<()> {
            Err(std::io::Error::other("the line broke"))
        }
    }
    let err = Udf::open(Broken).unwrap_err();
    assert!(matches!(&err, Error::Read { offset: 524_288, len: 2048, .. }), "{err}");
    assert!(std::error::Error::source(&err).is_some());
}

/// A reader that seeks reads as a slice does.
#[test]
fn a_seeker_reads_as_a_slice() {
    let image = Writer { revision: 0x0250, metadata: true, ..Writer::default() }.image(&tree());
    let mut udf = Udf::open(Seeker(std::io::Cursor::new(image.clone()))).unwrap();
    let extents = udf.find("/sources/boot.wim").unwrap();
    assert_eq!(extents, find(image.as_slice(), "/sources/boot.wim").unwrap());
    assert_eq!(udf.read(&extents, 10).unwrap(), b"hello");
}

/// CRC-ITU-T with initial value 0 (XMODEM): the check value of
/// "123456789".
#[test]
fn the_crc_is_crc_itu_t() {
    assert_eq!(tag::crc16(b"123456789"), 0x31c3);
    assert_eq!(tag::crc16(b""), 0);
}

/// The image as the sparse records of the fuzz target `udf_image`:
/// `[u32 offset / 512][u16 length][bytes]` for each 512 bytes that are not
/// all zero, with no zeros at the end.
fn sparse(image: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, chunk) in image.chunks(512).enumerate() {
        let Some(last) = chunk.iter().rposition(|b| *b != 0) else { continue };
        out.extend((i as u32).to_le_bytes());
        out.extend(((last + 1) as u16).to_le_bytes());
        out.extend(&chunk[..=last]);
    }
    out
}

/// Write the seeds of the fuzz target `udf_image`:
/// `POD_UDF_SEEDS=fuzz/seeds/udf_image cargo test -p pod-udf --lib write_the_fuzz_seeds -- --ignored`.
#[test]
#[ignore = "writes the seeds of fuzz/ into POD_UDF_SEEDS"]
fn write_the_fuzz_seeds() {
    let out = std::path::PathBuf::from(std::env::var("POD_UDF_SEEDS").expect("POD_UDF_SEEDS"));
    let small = || {
        dir(vec![
            ("setup.exe", file(b"MZ")),
            ("sources", dir(vec![("boot.wim", file(b"hello")), ("deep", dir(vec![("leaf", file(b"leaf"))]))])),
        ])
    };
    let wide = dir(vec![("Ünïcødé-文件", file(b"wide")), ("sub", small())]);
    let parts = vec![Part::Bytes(vec![1; BS]), Part::Hole(1), Part::Bytes(vec![2; 9])];
    let mixed =
        dir(vec![("two", Node::File(parts)), ("small", Node::Embedded(b"tiny".to_vec())), ("sources", small())]);
    let seeds = [
        ("plain102", Writer::default().image(&small())),
        ("long201", Writer { revision: 0x0201, long_ads: true, ..Writer::default() }.image(&wide)),
        ("metadata250", Writer { revision: 0x0250, metadata: true, ..Writer::default() }.image(&mixed)),
    ];
    for (name, image) in seeds {
        std::fs::write(out.join(name), sparse(&image)).expect("a seed");
    }
}

/// The sparse records give back the image.
#[test]
fn the_sparse_records_give_back_the_image() {
    let image = Writer { revision: 0x0250, metadata: true, ..Writer::default() }.image(&tree());
    let records = sparse(&image);
    assert!(records.len() < image.len() / 10, "{} bytes", records.len());
    let mut back = vec![0u8; image.len()];
    let mut rest = &records[..];
    while let Some((head, tail)) = rest.split_first_chunk::<6>() {
        let at = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize * 512;
        let len = usize::from(u16::from_le_bytes([head[4], head[5]]));
        back[at..at + len].copy_from_slice(&tail[..len]);
        rest = &tail[len..];
    }
    assert_eq!(back, image);
}
