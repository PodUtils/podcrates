//! The metadata blocks of a new file system: the superblock and its
//! copies, the group descriptors, the bitmaps, the two directory inodes
//! and their blocks.

use std::fs::File;
use std::os::unix::fs::FileExt;

use super::{
    BLOCK, BLOCKS_PER_GROUP, DESC_SIZE, FIRST_INO, INODE_SIZE, LOST_FOUND_INO, Layout, Names, ROOT_INO, has_super,
};

const COMPAT_EXT_ATTR: u32 = 0x8;
const COMPAT_DIR_INDEX: u32 = 0x20;
const INCOMPAT_FILETYPE: u32 = 0x2;
const INCOMPAT_EXTENTS: u32 = 0x40;
const RO_COMPAT_SPARSE_SUPER: u32 = 0x1;
const RO_COMPAT_LARGE_FILE: u32 = 0x2;
const RO_COMPAT_DIR_NLINK: u32 = 0x20;
const RO_COMPAT_EXTRA_ISIZE: u32 = 0x40;

fn put16(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 2].copy_from_slice(&(v as u16).to_le_bytes());
}

fn put32(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 4].copy_from_slice(&(v as u32).to_le_bytes());
}

/// The superblock of the copy in group `g` (1024 bytes).
pub(super) fn superblock(l: &Layout, g: u64, names: &Names, label: &[u8; 16], now: u64) -> Vec<u8> {
    let mut s = vec![0u8; 1024];
    put32(&mut s, 0, l.inodes());
    put32(&mut s, 4, l.blocks);
    put32(&mut s, 12, l.free_blocks());
    put32(&mut s, 16, l.inodes() - u64::from(FIRST_INO));
    put32(&mut s, 24, 2); // 1024 << 2
    put32(&mut s, 28, 2);
    put32(&mut s, 32, BLOCKS_PER_GROUP);
    put32(&mut s, 36, BLOCKS_PER_GROUP);
    put32(&mut s, 40, l.inodes_per_group);
    put32(&mut s, 48, now);
    put16(&mut s, 54, 0xffff); // no count of mounts before a check
    put16(&mut s, 56, 0xef53);
    put16(&mut s, 58, 1); // clean
    put16(&mut s, 60, 1); // continue on errors
    put32(&mut s, 64, now);
    put32(&mut s, 76, 1); // dynamic revision
    put32(&mut s, 84, FIRST_INO.into());
    put16(&mut s, 88, INODE_SIZE);
    // A 16-bit field: a copy past group 65535 records 65535, as mke2fs does.
    put16(&mut s, 90, g.min(0xffff));
    put32(&mut s, 92, (COMPAT_EXT_ATTR | COMPAT_DIR_INDEX).into());
    put32(&mut s, 96, (INCOMPAT_FILETYPE | INCOMPAT_EXTENTS).into());
    put32(
        &mut s,
        100,
        (RO_COMPAT_SPARSE_SUPER | RO_COMPAT_LARGE_FILE | RO_COMPAT_DIR_NLINK | RO_COMPAT_EXTRA_ISIZE).into(),
    );
    s[104..120].copy_from_slice(&names.uuid);
    s[120..136].copy_from_slice(label);
    s[236..252].copy_from_slice(&names.hash_seed);
    s[252] = 1; // half MD4, the hash of an indexed directory
    put32(&mut s, 264, now);
    put16(&mut s, 348, 32);
    put16(&mut s, 350, 32);
    put32(&mut s, 352, 2); // the hash of names is unsigned
    s
}

/// The group descriptor table (the first copy and each backup).
fn descriptors(l: &Layout) -> Vec<u8> {
    let mut d = vec![0u8; (l.gdt_blocks * BLOCK) as usize];
    for g in 0..l.groups {
        let at = (g * DESC_SIZE) as usize;
        let e = &mut d[at..at + DESC_SIZE as usize];
        put32(e, 0, l.block_bitmap(g));
        put32(e, 4, l.block_bitmap(g) + 1);
        put32(e, 8, l.inode_table(g));
        put16(e, 12, l.group_blocks(g) - l.used_blocks(g));
        put16(e, 14, l.inodes_per_group - l.used_inodes(g));
        put16(e, 16, if g == 0 { 2 } else { 0 });
    }
    d
}

/// A bitmap block with the first `used` bits set, and each bit from
/// `valid` to the end of the block set as padding.
fn bitmap(used: u64, valid: u64) -> Vec<u8> {
    let mut b = vec![0u8; BLOCK as usize];
    for bit in (0..used).chain(valid..8 * BLOCK) {
        b[(bit / 8) as usize] |= 1 << (bit % 8);
    }
    b
}

/// One directory inode with one data block.
fn dir_inode(mode: u64, links: u64, block: u64, now: u64) -> Vec<u8> {
    let mut i = vec![0u8; INODE_SIZE as usize];
    put16(&mut i, 0, 0o040000 | mode);
    put32(&mut i, 4, BLOCK);
    for at in [8, 12, 16] {
        put32(&mut i, at, now);
    }
    put16(&mut i, 26, links);
    put32(&mut i, 28, BLOCK / 512);
    put32(&mut i, 40, block);
    put16(&mut i, 128, 32); // i_extra_isize
    i
}

/// A directory block of `entries` (inode, name); the last entry takes the
/// rest of the block.
fn dir_block(entries: &[(u32, &str)]) -> Vec<u8> {
    let mut b = vec![0u8; BLOCK as usize];
    let mut at = 0usize;
    for (n, (ino, name)) in entries.iter().enumerate() {
        let len = (8 + name.len()).div_ceil(4) * 4;
        let rec = if n + 1 == entries.len() { BLOCK as usize - at } else { len };
        put32(&mut b, at, u64::from(*ino));
        put16(&mut b, at + 4, rec as u64);
        b[at + 6] = name.len() as u8;
        b[at + 7] = 2; // a directory
        b[at + 8..at + 8 + name.len()].copy_from_slice(name.as_bytes());
        at += rec;
    }
    b
}

/// Write an empty ext4 file system of `l` into `f`, a file of at least
/// `l.blocks` blocks whose bytes are zero (a new sparse file).
pub(super) fn write(f: &File, l: &Layout, names: &Names, label: &[u8; 16], now: u64) -> std::io::Result<()> {
    let at = |block: u64| block * BLOCK;
    let gdt = descriptors(l);
    for g in 0..l.groups {
        let start = l.group_start(g);
        if has_super(g) {
            let sb = superblock(l, g, names, label, now);
            // Group 0 keeps its superblock 1024 bytes into the disk; each
            // copy starts the first block of its group.
            f.write_all_at(&sb, if g == 0 { 1024 } else { at(start) })?;
            f.write_all_at(&gdt, at(start + 1))?;
        }
        let blocks = l.group_blocks(g);
        f.write_all_at(&bitmap(l.used_blocks(g), blocks), at(l.block_bitmap(g)))?;
        f.write_all_at(&bitmap(l.used_inodes(g), l.inodes_per_group), at(l.block_bitmap(g) + 1))?;
    }
    let (root, lost) = l.dir_blocks();
    let inode_at = |ino: u32| at(l.inode_table(0)) + u64::from(ino - 1) * INODE_SIZE;
    f.write_all_at(&dir_inode(0o755, 3, root, now), inode_at(ROOT_INO))?;
    f.write_all_at(&dir_inode(0o700, 2, lost, now), inode_at(LOST_FOUND_INO))?;
    f.write_all_at(&dir_block(&[(ROOT_INO, "."), (ROOT_INO, ".."), (LOST_FOUND_INO, "lost+found")]), at(root))?;
    f.write_all_at(&dir_block(&[(LOST_FOUND_INO, "."), (ROOT_INO, "..")]), at(lost))?;
    Ok(())
}
