//! A writer of small squashfs images for the tests. The metadata blocks are
//! stored, in pieces of a chosen size, so that an inode or a listing
//! crosses blocks. A data block is stored, sparse, an xz stream of a
//! fixture, or raw bytes with any size word (for a malformed image).

use super::{FLAG_COMPRESSOR_OPTIONS, MAGIC, METADATA_SIZE, NO_TABLE, XZ};

/// The bit of a size word: the block is stored.
pub(crate) const STORED: u32 = 1 << 24;
/// The fragment index of a file with no fragment.
pub(crate) const NO_FRAGMENT: u32 = u32::MAX;
/// The flag of an image with an export table.
const FLAG_EXPORTABLE: u16 = 0x0080;

/// One block of a file or one fragment block.
#[derive(Clone)]
pub(crate) enum Block {
    Stored(Vec<u8>),
    /// An xz stream, which decodes to the block.
    Xz(Vec<u8>),
    /// Size word 0: a block of zeros.
    Sparse,
    /// Bytes on disk with this size word.
    Raw(u32, Vec<u8>),
}

impl Block {
    fn word(&self) -> (u32, &[u8]) {
        match self {
            Block::Stored(b) => (b.len() as u32 | STORED, b),
            Block::Xz(b) => (b.len() as u32, b),
            Block::Sparse => (0, &[]),
            Block::Raw(word, b) => (*word, b),
        }
    }
}

/// One node of the tree of an image. A name is bytes, so that a test can
/// write a name that the reader refuses.
#[derive(Clone)]
pub(crate) enum Node {
    Dir {
        entries: Vec<(Vec<u8>, Node)>,
        ext: bool,
    },
    /// The size, the blocks, and the fragment (index and offset) of the
    /// tail. `ext` writes the extended inode.
    File {
        size: u64,
        blocks: Vec<Block>,
        fragment: Option<(u32, u32)>,
        ext: bool,
    },
    Symlink(Vec<u8>),
    Fifo,
}

/// A directory of basic inodes.
pub(crate) fn dir(entries: Vec<(&str, Node)>) -> Node {
    Node::Dir { entries: entries.into_iter().map(|(n, e)| (n.as_bytes().to_vec(), e)).collect(), ext: false }
}

/// The settings of an image.
#[derive(Clone)]
pub(crate) struct Writer {
    pub block_size: u32,
    pub compressor: u16,
    /// The xz options: the dictionary and the filters.
    pub options: Option<(u32, u32)>,
    /// The size of each stored metadata block of the inode and directory
    /// tables.
    pub meta_chunk: usize,
    pub fragments: Vec<Block>,
    pub export: bool,
}

impl Default for Writer {
    fn default() -> Writer {
        Writer {
            block_size: 4096,
            compressor: XZ,
            options: None,
            meta_chunk: METADATA_SIZE,
            fragments: Vec::new(),
            export: false,
        }
    }
}

/// The inodes and the listings while the tree is written, and each inode
/// reference by its number.
#[derive(Default)]
struct Tables {
    inodes: Vec<u8>,
    dirs: Vec<u8>,
    refs: Vec<u64>,
}

impl Writer {
    /// A file of `bytes` in stored blocks, with no fragment.
    pub fn file(&self, bytes: &[u8]) -> Node {
        let blocks = bytes.chunks(self.block_size as usize).map(|c| Block::Stored(c.to_vec())).collect();
        Node::File { size: bytes.len() as u64, blocks, fragment: None, ext: false }
    }

    /// The place in a table of the byte `at` of its content: the offset of
    /// its block from the table start, and the offset in the block.
    fn place(&self, at: usize) -> (u64, u16) {
        let n = at / self.meta_chunk;
        ((n * (self.meta_chunk + 2)) as u64, (at % self.meta_chunk) as u16)
    }

    /// The image of the tree `root`.
    pub fn image(&self, root: &Node) -> Vec<u8> {
        let mut img = vec![0u8; 96];
        let mut flags = 0u16;
        if let Some((dictionary, filters)) = self.options {
            flags |= FLAG_COMPRESSOR_OPTIONS;
            img.extend((0x8000u16 | 8).to_le_bytes());
            img.extend(dictionary.to_le_bytes());
            img.extend(filters.to_le_bytes());
        }
        let mut t = Tables::default();
        let (root_ref, _, _) = self.node(root, &mut img, &mut t);
        let mut fragment_entries = Vec::new();
        for f in &self.fragments {
            let (word, bytes) = f.word();
            fragment_entries.extend((img.len() as u64).to_le_bytes());
            fragment_entries.extend(word.to_le_bytes());
            fragment_entries.extend(0u32.to_le_bytes());
            img.extend(bytes);
        }
        let inode_table = img.len() as u64;
        meta(&mut img, &t.inodes, self.meta_chunk);
        let directory_table = img.len() as u64;
        meta(&mut img, &t.dirs, self.meta_chunk);
        let fragment_table = lookup(&mut img, &fragment_entries);
        let export_table = if self.export {
            flags |= FLAG_EXPORTABLE;
            lookup(&mut img, &t.refs.iter().flat_map(|r| r.to_le_bytes()).collect::<Vec<_>>())
        } else {
            NO_TABLE
        };
        let id_table = lookup(&mut img, &0u32.to_le_bytes());
        let bytes_used = img.len() as u64;
        img.resize(img.len().next_multiple_of(4096), 0);
        let mut sb = Vec::with_capacity(96);
        sb.extend(MAGIC.to_le_bytes());
        sb.extend((t.refs.len() as u32).to_le_bytes());
        sb.extend(0u32.to_le_bytes());
        sb.extend(self.block_size.to_le_bytes());
        sb.extend((self.fragments.len() as u32).to_le_bytes());
        sb.extend(self.compressor.to_le_bytes());
        sb.extend((self.block_size.trailing_zeros() as u16).to_le_bytes());
        sb.extend(flags.to_le_bytes());
        sb.extend(1u16.to_le_bytes());
        sb.extend(4u16.to_le_bytes());
        sb.extend(0u16.to_le_bytes());
        for v in [root_ref, bytes_used, id_table, NO_TABLE, inode_table, directory_table, fragment_table, export_table]
        {
            sb.extend(v.to_le_bytes());
        }
        img[..96].copy_from_slice(&sb);
        img
    }

    /// Write `n` and what it holds, children first: the blocks of a file go
    /// into the image, the inode and the listing into the tables. The
    /// reference, the basic type and the number of the inode.
    fn node(&self, n: &Node, img: &mut Vec<u8>, t: &mut Tables) -> (u64, u16, u32) {
        let mut body = Vec::new();
        let kind: u16 = match n {
            Node::Dir { entries, ext } => {
                let mut children: Vec<(Vec<u8>, (u64, u16, u32))> =
                    entries.iter().map(|(name, child)| (name.clone(), self.node(child, img, t))).collect();
                children.sort_by(|a, b| a.0.cmp(&b.0));
                let start = t.dirs.len();
                for group in children.chunk_by(|a, b| a.1.0 >> 16 == b.1.0 >> 16).flat_map(|g| g.chunks(256)) {
                    let base = group[0].1.2;
                    t.dirs.extend((group.len() as u32 - 1).to_le_bytes());
                    t.dirs.extend(((group[0].1.0 >> 16) as u32).to_le_bytes());
                    t.dirs.extend(base.to_le_bytes());
                    for (name, (r, kind, number)) in group {
                        t.dirs.extend(((r & 0xffff) as u16).to_le_bytes());
                        t.dirs.extend(((*number as i64 - base as i64) as i16).to_le_bytes());
                        t.dirs.extend(kind.to_le_bytes());
                        t.dirs.extend((name.len() as u16 - 1).to_le_bytes());
                        t.dirs.extend(name);
                    }
                }
                let size = (t.dirs.len() - start + 3) as u32;
                let (block, offset) = self.place(start);
                if *ext {
                    for v in [2u32, size, block as u32, 0] {
                        body.extend(v.to_le_bytes());
                    }
                    body.extend(0u16.to_le_bytes());
                    body.extend(offset.to_le_bytes());
                    body.extend(u32::MAX.to_le_bytes());
                    8
                } else {
                    body.extend((block as u32).to_le_bytes());
                    body.extend(2u32.to_le_bytes());
                    body.extend((size as u16).to_le_bytes());
                    body.extend(offset.to_le_bytes());
                    body.extend(0u32.to_le_bytes());
                    1
                }
            }
            Node::File { size, blocks, fragment, ext } => {
                let start = img.len() as u64;
                let mut words = Vec::new();
                for b in blocks {
                    let (word, bytes) = b.word();
                    words.extend(word.to_le_bytes());
                    img.extend(bytes);
                }
                let (index, offset) = fragment.unwrap_or((NO_FRAGMENT, 0));
                if *ext {
                    for v in [start, *size, 0] {
                        body.extend(v.to_le_bytes());
                    }
                    for v in [1u32, index, offset, u32::MAX] {
                        body.extend(v.to_le_bytes());
                    }
                } else {
                    for v in [start as u32, index, offset, *size as u32] {
                        body.extend(v.to_le_bytes());
                    }
                }
                body.extend(words);
                if *ext { 9 } else { 2 }
            }
            Node::Symlink(target) => {
                body.extend(1u32.to_le_bytes());
                body.extend((target.len() as u32).to_le_bytes());
                body.extend(target);
                3
            }
            Node::Fifo => {
                body.extend(1u32.to_le_bytes());
                6
            }
        };
        let number = t.refs.len() as u32 + 1;
        let (block, offset) = self.place(t.inodes.len());
        let reference = (block << 16) | u64::from(offset);
        t.refs.push(reference);
        t.inodes.extend(kind.to_le_bytes());
        t.inodes.extend(0o755u16.to_le_bytes());
        t.inodes.extend([0u8; 4]);
        t.inodes.extend(0u32.to_le_bytes());
        t.inodes.extend(number.to_le_bytes());
        t.inodes.extend(body);
        let basic = match kind {
            8 => 1,
            9 => 2,
            k => k,
        };
        (reference, basic, number)
    }
}

/// Write `bytes` as stored metadata blocks of `chunk` bytes.
fn meta(img: &mut Vec<u8>, bytes: &[u8], chunk: usize) {
    for piece in bytes.chunks(chunk) {
        img.extend((0x8000u16 | piece.len() as u16).to_le_bytes());
        img.extend(piece);
    }
}

/// Write the entries of a lookup table as metadata blocks of 8 KiB, then
/// the place of each block. The place of the lookup table.
fn lookup(img: &mut Vec<u8>, entries: &[u8]) -> u64 {
    let mut places = Vec::new();
    for piece in entries.chunks(METADATA_SIZE) {
        places.push(img.len() as u64);
        meta(img, piece, METADATA_SIZE);
    }
    let at = img.len() as u64;
    for p in places {
        img.extend(p.to_le_bytes());
    }
    at
}

/// The place in `img` of the first byte of the inode `reference`, for a
/// test that changes it. The inode must not cross a block.
pub(crate) fn inode_at(img: &[u8], reference: u64) -> usize {
    let table = u64::from_le_bytes(img[64..72].try_into().unwrap());
    (table + (reference >> 16) + 2 + (reference & 0xffff)) as usize
}
