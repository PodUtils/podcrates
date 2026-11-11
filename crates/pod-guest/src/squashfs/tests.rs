use std::io::Cursor;

use pod_core::Class;

use super::writer::{Block, Node, STORED, Writer, dir, inode_at};
use super::*;

pub(crate) const X86: &[u8] = include_bytes!("../../tests/fixtures/squashfs/x86.bin");
pub(crate) const X86_BCJ_XZ: &[u8] = include_bytes!("../../tests/fixtures/squashfs/x86.bcj.xz");
pub(crate) const TAIL: &[u8] = include_bytes!("../../tests/fixtures/squashfs/tail.bin");
pub(crate) const TAIL_XZ: &[u8] = include_bytes!("../../tests/fixtures/squashfs/tail.xz");

fn open(img: &[u8]) -> Image<Cursor<&[u8]>> {
    Image::open(Cursor::new(img)).expect("the image opens")
}

/// The fault of a failed call, or a panic with what came.
fn fault_of<T: std::fmt::Debug>(r: Result<T>) -> Fault {
    let e = r.expect_err("a fault");
    super::fault(&e).cloned().unwrap_or_else(|| panic!("no fault in {e}"))
}

/// Bytes of a pattern of `len`, other for each `seed`.
fn bytes(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

/// A tree of each kind of inode: stored blocks, a stored fragment, a sparse
/// block, an extended file and directory, a link and a FIFO.
pub(crate) fn tree(w: &mut Writer) -> Node {
    w.fragments.push(Block::Stored([bytes(100, 7), bytes(50, 9)].concat()));
    dir(vec![
        ("one", w.file(&bytes(9000, 1))),
        ("empty", w.file(&[])),
        (
            "tailed",
            Node::File {
                size: 4096 + 50,
                blocks: vec![Block::Stored(bytes(4096, 1))],
                fragment: Some((0, 100)),
                ext: false,
            },
        ),
        (
            "sparse",
            Node::File {
                size: 4106,
                blocks: vec![Block::Sparse, Block::Stored(bytes(10, 3))],
                fragment: None,
                ext: true,
            },
        ),
        ("link", Node::Symlink(b"one".to_vec())),
        ("fifo", Node::Fifo),
        ("sub", Node::Dir { entries: vec![(b"small".to_vec(), w.file(b"small file"))], ext: true }),
        ("void", dir(vec![])),
    ])
}

fn read(img: &[u8], path: &str) -> Result<Vec<u8>> {
    let mut image = open(img);
    let file = image.file_at(path)?;
    image.read(&file)
}

#[test]
fn a_written_image_reads_back() {
    let mut w = Writer { export: true, ..Writer::default() };
    let root = tree(&mut w);
    let img = w.image(&root);
    assert_eq!(read(&img, "one").unwrap(), bytes(9000, 1));
    assert_eq!(read(&img, "/empty").unwrap(), b"");
    assert_eq!(read(&img, "tailed").unwrap(), [bytes(4096, 1), bytes(50, 9)].concat());
    assert_eq!(read(&img, "sparse").unwrap(), [vec![0; 4096], bytes(10, 3)].concat());
    assert_eq!(read(&img, "sub//small").unwrap(), b"small file");
    let mut image = open(&img);
    assert_eq!(image.lookup("link").unwrap().kind, Kind::Symlink(b"one".to_vec()));
    assert_eq!(image.lookup("fifo").unwrap().kind, Kind::Other(6));
    let Kind::Directory(root) = image.root().unwrap().kind else { panic!("the root is a directory") };
    let names: Vec<_> = image.entries(&root).unwrap().into_iter().map(|e| String::from_utf8(e.name).unwrap()).collect();
    assert_eq!(names, ["empty", "fifo", "link", "one", "sparse", "sub", "tailed", "void"]);
    let Kind::Directory(void) = image.lookup("void").unwrap().kind else { panic!("void is a directory") };
    assert!(image.entries(&void).unwrap().is_empty());
    assert_eq!(image.ids().unwrap(), [0]);
    let sb = image.superblock().clone();
    assert_eq!(sb.compressor, Compressor::Xz { dictionary: 8192, filters: 0 });
    for number in 1..=sb.inodes {
        let reference = image.export(number).unwrap().expect("an export table");
        assert_eq!(image.inode(reference).unwrap().number, number);
    }
    assert_eq!(fault_of(image.export(sb.inodes + 1)), Fault::Index { table: "export", index: u64::from(sb.inodes) });
}

#[test]
fn an_inode_and_a_listing_that_cross_blocks_read() {
    for chunk in [1, 7, 37, 100] {
        let mut w = Writer { meta_chunk: chunk, ..Writer::default() };
        let root = tree(&mut w);
        let img = w.image(&root);
        assert_eq!(read(&img, "one").unwrap(), bytes(9000, 1), "chunk {chunk}");
        assert_eq!(read(&img, "tailed").unwrap(), [bytes(4096, 1), bytes(50, 9)].concat(), "chunk {chunk}");
        assert_eq!(read(&img, "sub/small").unwrap(), b"small file", "chunk {chunk}");
    }
}

#[test]
fn an_x86_block_decodes_through_its_bcj_filter() {
    let w = Writer::default();
    let file = Node::File {
        size: (X86.len() + TAIL.len()) as u64,
        blocks: vec![Block::Xz(X86_BCJ_XZ.to_vec()), Block::Xz(TAIL_XZ.to_vec())],
        fragment: None,
        ext: false,
    };
    let img = w.image(&dir(vec![("code.ko", file)]));
    assert_eq!(read(&img, "code.ko").unwrap(), [X86, TAIL].concat());
}

#[test]
fn a_tail_in_an_xz_fragment_reads() {
    let w = Writer { fragments: vec![Block::Xz(TAIL_XZ.to_vec())], ..Writer::default() };
    let file = Node::File {
        size: 4096 + 600,
        blocks: vec![Block::Xz(X86_BCJ_XZ.to_vec())],
        fragment: Some((0, 300)),
        ext: true,
    };
    let other = Node::File { size: 10, blocks: vec![], fragment: Some((0, 0)), ext: false };
    let img = w.image(&dir(vec![("code.ko", file), ("other", other)]));
    assert_eq!(read(&img, "code.ko").unwrap(), [X86, &TAIL[300..900]].concat());
    assert_eq!(read(&img, "other").unwrap(), &TAIL[..10]);
}

#[test]
fn the_compressor_options_bound_the_dictionary() {
    let code = || Node::File { size: 4096, blocks: vec![Block::Xz(X86_BCJ_XZ.to_vec())], fragment: None, ext: false };
    let with = |options| Writer { options: Some(options), ..Writer::default() }.image(&dir(vec![("c", code())]));
    assert_eq!(read(&with((8192, 1)), "c").unwrap(), X86);
    assert_eq!(open(&with((8192, 1))).superblock().compressor, Compressor::Xz { dictionary: 8192, filters: 1 });
    // The stream asks for 8 KiB: a dictionary of 4 KiB refuses it.
    assert!(matches!(fault_of(read(&with((4096, 1)), "c")), Fault::Stream { .. }));
    for bad in [0, 5000, 3 << 13, 1 << 20] {
        let img = with((bad, 1));
        assert_eq!(fault_of(Image::open(Cursor::new(&img[..]))), Fault::Dictionary(bad), "{bad}");
    }
    let mut img = with((8192, 1));
    img[96] = 8;
    img[97] = 0;
    assert_eq!(fault_of(Image::open(Cursor::new(&img[..]))), Fault::Options("the block is compressed"));
}

/// A valid image with a file of `blocks` and `size`, and its root.
fn one_file(blocks: Vec<Block>, size: u64, fragment: Option<(u32, u32)>, ext: bool) -> Vec<u8> {
    let w = Writer::default();
    w.image(&dir(vec![("f", Node::File { size, blocks, fragment, ext })]))
}

#[test]
fn a_malformed_superblock_is_a_fault() {
    let img = one_file(vec![Block::Stored(b"x".to_vec())], 1, None, false);
    let at = |i: usize, v: &[u8]| {
        let mut m = img.clone();
        m[i..i + v.len()].copy_from_slice(v);
        fault_of(Image::open(Cursor::new(m)))
    };
    assert_eq!(at(0, b"hsqt"), Fault::Magic(u32::from_le_bytes(*b"hsqt")));
    assert_eq!(at(28, &[3, 0]), Fault::Version { major: 3, minor: 0 });
    assert_eq!(at(12, &8192u32.to_le_bytes()), Fault::BlockSize { size: 8192, log: 12 });
    assert_eq!(at(22, &[21, 0]), Fault::BlockSize { size: 4096, log: 21 });
    assert_eq!(at(20, &[9, 0]), Fault::Compressor(9));
    assert!(matches!(at(40, &u64::MAX.to_le_bytes()), Fault::Past { .. }));
    assert_eq!(fault_of(Image::open(Cursor::new(&img[..95]))), Fault::Past { what: "the superblock", at: 0 });
    let used = u64::from_le_bytes(img[40..48].try_into().unwrap()) as usize;
    let e = Image::open(Cursor::new(&img[..used - 1])).expect_err("a cut image");
    assert_eq!(e.class, Class::Protocol);
}

#[test]
fn a_malformed_table_is_a_fault() {
    let img = one_file(vec![Block::Stored(b"x".to_vec())], 1, None, false);
    let root = u64::from_le_bytes(img[32..40].try_into().unwrap());
    let inode_table = u64::from_le_bytes(img[64..72].try_into().unwrap()) as usize;
    let dir_table = u64::from_le_bytes(img[72..80].try_into().unwrap()) as usize;
    let patched = |at: usize, v: &[u8]| {
        let mut m = img.clone();
        m[at..at + v.len()].copy_from_slice(v);
        fault_of(read(&m, "f"))
    };
    // The header of the first inode block: no bytes, then more than 8 KiB.
    assert_eq!(patched(inode_table, &0x8000u16.to_le_bytes()), Fault::MetadataSize { at: inode_table as u64, size: 0 });
    assert!(matches!(patched(inode_table, &0x2001u16.to_le_bytes()), Fault::MetadataSize { size: 0x2001, .. }));
    assert_eq!(patched(inode_table + 2, &15u16.to_le_bytes()), Fault::InodeType(15));
    assert_eq!(patched(dir_table + 2, &300u32.to_le_bytes()), Fault::Listing("more than 256 entries under one header"));
    assert_eq!(patched(dir_table + 2 + 12 + 8, b"/"), Fault::Listing("a name that is not one part of a path"));
    // The root's listing starts past the data of its block.
    let root_at = inode_at(&img, root);
    assert!(matches!(patched(root_at + 16 + 10, &5000u16.to_le_bytes()), Fault::Offset { offset: 5000, .. }));
    // A reference in the directory table, and one with a large offset.
    let mut m = img.clone();
    m[32..40].copy_from_slice(&(((dir_table - inode_table) as u64) << 16).to_le_bytes());
    assert!(matches!(fault_of(open(&m).root()), Fault::Reference(_)));
    m[32..40].copy_from_slice(&0x2000u64.to_le_bytes());
    assert_eq!(fault_of(open(&m).root()), Fault::Reference(0x2000));
    let mut image = open(&img);
    assert_eq!(fault_of(image.file_at("g")), Fault::Missing("g".into()));
    assert_eq!(fault_of(image.lookup("f/g")), Fault::NotADirectory("f".into()));
    assert_eq!(fault_of(image.file_at("")), Fault::NotAFile(String::new()));
    assert_eq!(image.file_at("g").unwrap_err().class, Class::NotFound);
}

#[test]
fn a_malformed_block_is_a_fault() {
    let file = |blocks, size| fault_of(read(&one_file(blocks, size, None, false), "f"));
    assert!(matches!(file(vec![Block::Raw(4097, vec![0; 4097])], 5000), Fault::BlockLength { size: 4097, .. }));
    assert_eq!(file(vec![Block::Stored(vec![1; 10])], 11), Fault::Length { at: 96, want: 11, got: 10 });
    assert_eq!(file(vec![Block::Xz(TAIL_XZ.to_vec())], 1001), Fault::Length { at: 96, want: 1001, got: 1000 });
    let stream = |raw: Vec<u8>, size| match file(vec![Block::Xz(raw)], size) {
        Fault::Stream { at: 96, why } => why,
        other => panic!("{other:?}"),
    };
    assert_eq!(stream(TAIL_XZ.to_vec(), 999), "it holds more than 999 bytes");
    assert_eq!(stream(TAIL_XZ[..TAIL_XZ.len() - 20].to_vec(), 1000), "it ends early");
    assert_eq!(stream([TAIL_XZ, &[0; 4]].concat(), 1000), "4 bytes after its end");
    let mut flipped = TAIL_XZ.to_vec();
    flipped[40] ^= 0x55;
    assert!(!stream(flipped, 1000).is_empty());
    // A block that starts 5 bytes before the end of the image and has 10.
    let img = one_file(vec![Block::Raw(10 | STORED, vec![1; 10])], 10, None, false);
    let used = u64::from_le_bytes(img[40..48].try_into().unwrap());
    let start = u64::from_le_bytes(img[64..72].try_into().unwrap()) as usize + 2 + 16;
    let mut m = img.clone();
    m[start..start + 4].copy_from_slice(&(used as u32 - 5).to_le_bytes());
    assert_eq!(fault_of(read(&m, "f")), Fault::Past { what: "a data block", at: used - 5 });
    let gzip = |img: &mut Vec<u8>| img[20..22].copy_from_slice(&1u16.to_le_bytes());
    let mut img = one_file(vec![Block::Xz(TAIL_XZ.to_vec())], 1000, None, false);
    gzip(&mut img);
    assert_eq!(fault_of(read(&img, "f")), Fault::Compressor(1));
    let mut img = one_file(vec![Block::Stored(TAIL.to_vec())], 1000, None, false);
    gzip(&mut img);
    assert_eq!(read(&img, "f").unwrap(), TAIL);
}

#[test]
fn a_fragment_that_does_not_hold_its_tail_is_a_fault() {
    let w = Writer { fragments: vec![Block::Stored(vec![1; 100])], ..Writer::default() };
    let file = |fragment| Node::File { size: 50, blocks: vec![], fragment: Some(fragment), ext: false };
    let img = w.image(&dir(vec![("past", file((0, 60))), ("index", file((1, 0))), ("fits", file((0, 50)))]));
    assert_eq!(read(&img, "fits").unwrap(), vec![1; 50]);
    assert_eq!(fault_of(read(&img, "past")), Fault::Fragment { index: 0 });
    assert_eq!(fault_of(read(&img, "index")), Fault::Index { table: "fragment", index: 1 });
}

#[test]
fn a_ceiling_holds_before_an_allocation() {
    let huge = MAX_FILE + 1;
    let blocks = vec![Block::Sparse; 3];
    let img = one_file(blocks, huge, None, true);
    let e = read(&img, "f").unwrap_err();
    assert_eq!(super::fault(&e), Some(&Fault::TooLarge { what: "a file", size: huge, limit: MAX_FILE }));
    assert_eq!(e.class, Class::Policy);
    let w = Writer::default();
    let img = w.image(&Node::Dir { entries: vec![(b"f".to_vec(), w.file(b"x"))], ext: true });
    let mut m = img.clone();
    let root = u64::from_le_bytes(img[32..40].try_into().unwrap());
    let size = u32::MAX.to_le_bytes();
    let at = inode_at(&img, root) + 16 + 4;
    m[at..at + 4].copy_from_slice(&size);
    let Kind::Directory(d) = open(&m).root().unwrap().kind else { panic!("the root is a directory") };
    let limit = MAX_LISTING;
    assert_eq!(
        fault_of(open(&m).entries(&d)),
        Fault::TooLarge { what: "a listing", size: u64::from(u32::MAX) - 3, limit }
    );
    let img = w.image(&dir(vec![("l", Node::Symlink(vec![b'a'; 4097]))]));
    assert_eq!(fault_of(open(&img).lookup("l")), Fault::TooLarge { what: "a link", size: 4097, limit: MAX_LINK });
}

/// Each mutation of a valid image reads to a value or a fault, with no
/// panic (the fuzz target `squashfs_image` does the same with libFuzzer).
#[test]
fn a_mutated_image_reads_or_faults() {
    let mut w = Writer { meta_chunk: 64, export: true, ..Writer::default() };
    w.fragments.push(Block::Xz(TAIL_XZ.to_vec()));
    let mut root = tree(&mut w);
    if let Node::Dir { entries, .. } = &mut root {
        let code = Node::File {
            size: 4096 + 20,
            blocks: vec![Block::Xz(X86_BCJ_XZ.to_vec())],
            fragment: Some((1, 5)),
            ext: false,
        };
        entries.push((b"code".to_vec(), code));
    }
    let img = w.image(&root);
    let mut state = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..3000 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let mut m = img.clone();
        let i = (state >> 8) as usize % m.len();
        m[i] ^= (state >> 32) as u8 | 1;
        if state.is_multiple_of(3) {
            m.truncate((state >> 40) as usize % m.len());
        }
        walk(&m);
    }
}

/// Read each file and each table of `img` that opens, with a bound on the
/// work: the stable twin of the fuzz target.
fn walk(img: &[u8]) {
    let Ok(mut image) = Image::open(Cursor::new(img)) else { return };
    let _ = image.ids();
    let mut todo = match image.root() {
        Ok(root) => vec![root],
        Err(_) => return,
    };
    let mut budget = 64;
    while let Some(inode) = todo.pop() {
        budget -= 1;
        if budget == 0 {
            return;
        }
        match inode.kind {
            Kind::Directory(d) => {
                for e in image.entries(&d).unwrap_or_default() {
                    if let Ok(i) = image.inode(e.inode) {
                        todo.push(i);
                    }
                }
            }
            Kind::File(f) => {
                if let Ok(bytes) = image.read(&f) {
                    assert_eq!(bytes.len() as u64, f.size());
                }
            }
            _ => {}
        }
    }
}
