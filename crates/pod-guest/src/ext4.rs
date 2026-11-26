//! An empty ext4 file system written into a sparse file, with no tool of
//! the host (TODO/machine.md T-1968).
//!
//! The layout is the plain one of `mke2fs -t ext4 -O ^has_journal,
//! ^flex_bg,^metadata_csum,^resize_inode -b 4096 -I 256 -m 0`: 4 KiB blocks,
//! 32,768 blocks for each group, one inode for each 16 KiB, a copy of the
//! superblock and the group descriptors in group 0, 1 and each power of 3,
//! 5 and 7 (`sparse_super`), then the block bitmap, the inode bitmap and
//! the inode table of each group. The root directory and `lost+found` have
//! one block each. The file system has no journal and no checksums: the
//! disk is disposable, and a guest that crashes boots on a new one.
//!
//! Only the metadata blocks are written. Each other block stays a hole of
//! the file, so a new disk takes some kilobytes of the host for each group,
//! and the guest's writes take host blocks as they come.

use std::os::unix::fs::FileExt;

pub const BLOCK: u64 = 4096;
const BLOCKS_PER_GROUP: u64 = 8 * BLOCK;
const INODE_SIZE: u64 = 256;
const BYTES_PER_INODE: u64 = 16 << 10;
const DESC_SIZE: u64 = 32;
const FIRST_INO: u32 = 11;
const ROOT_INO: u32 = 2;
const LOST_FOUND_INO: u32 = 11;
/// The fewest data blocks that a last group keeps; a smaller rest of the
/// size is left out of the file system, as `mke2fs` does.
const LAST_GROUP_SLACK: u64 = 50;

const COMPAT_EXT_ATTR: u32 = 0x8;
const COMPAT_DIR_INDEX: u32 = 0x20;
const INCOMPAT_FILETYPE: u32 = 0x2;
const INCOMPAT_EXTENTS: u32 = 0x40;
const RO_COMPAT_SPARSE_SUPER: u32 = 0x1;
const RO_COMPAT_LARGE_FILE: u32 = 0x2;
const RO_COMPAT_DIR_NLINK: u32 = 0x20;
const RO_COMPAT_EXTRA_ISIZE: u32 = 0x40;

/// The shape of one file system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub blocks: u64,
    pub groups: u64,
    pub inodes_per_group: u64,
    pub gdt_blocks: u64,
    pub itable_blocks: u64,
}

/// Whether group `g` has a copy of the superblock (`sparse_super`).
pub fn has_super(g: u64) -> bool {
    fn power_of(mut g: u64, b: u64) -> bool {
        while g > 1 && g.is_multiple_of(b) {
            g /= b;
        }
        g == 1
    }
    g <= 1 || power_of(g, 3) || power_of(g, 5) || power_of(g, 7)
}

impl Layout {
    /// The layout of a file system of `bytes`, or why it cannot hold one:
    /// fewer than 4 MiB, or more blocks than 32 bits count.
    pub fn of(bytes: u64) -> Result<Layout, String> {
        if bytes < 4 << 20 {
            return Err(format!("{bytes} bytes hold no ext4 file system"));
        }
        let mut blocks = bytes / BLOCK;
        if blocks > u64::from(u32::MAX) {
            return Err(format!(
                "{bytes} bytes are more than an ext4 file system of 32-bit block numbers holds \
                 (16 TiB)"
            ));
        }
        loop {
            let groups = blocks.div_ceil(BLOCKS_PER_GROUP);
            let per_block = BLOCK / INODE_SIZE;
            let inodes = (blocks * BLOCK / BYTES_PER_INODE).max(FIRST_INO.into());
            let ipg = inodes
                .div_ceil(groups)
                .div_ceil(per_block)
                .saturating_mul(per_block)
                .clamp(2 * per_block, 8 * BLOCK);
            let l = Layout {
                blocks,
                groups,
                inodes_per_group: ipg,
                gdt_blocks: (groups * DESC_SIZE).div_ceil(BLOCK),
                itable_blocks: ipg * INODE_SIZE / BLOCK,
            };
            let last = l.group_blocks(groups - 1);
            if last >= l.overhead(groups - 1) + LAST_GROUP_SLACK {
                return Ok(l);
            }
            if groups == 1 {
                return Err(format!("{bytes} bytes hold no ext4 file system"));
            }
            blocks = (groups - 1) * BLOCKS_PER_GROUP;
        }
    }

    fn group_start(&self, g: u64) -> u64 {
        g * BLOCKS_PER_GROUP
    }

    fn group_blocks(&self, g: u64) -> u64 {
        (self.blocks - self.group_start(g)).min(BLOCKS_PER_GROUP)
    }

    /// The blocks of the superblock copy and the descriptors of group `g`.
    fn super_blocks(&self, g: u64) -> u64 {
        if has_super(g) {
            1 + self.gdt_blocks
        } else {
            0
        }
    }

    /// The metadata blocks of group `g`.
    fn overhead(&self, g: u64) -> u64 {
        self.super_blocks(g) + 2 + self.itable_blocks
    }

    fn block_bitmap(&self, g: u64) -> u64 {
        self.group_start(g) + self.super_blocks(g)
    }

    fn inode_table(&self, g: u64) -> u64 {
        self.block_bitmap(g) + 2
    }

    /// The data blocks of the root directory and of `lost+found`.
    fn dir_blocks(&self) -> (u64, u64) {
        let first = self.inode_table(0) + self.itable_blocks;
        (first, first + 1)
    }

    /// The blocks in use in group `g`: its metadata, and in group 0 the two
    /// directory blocks.
    fn used_blocks(&self, g: u64) -> u64 {
        self.overhead(g) + if g == 0 { 2 } else { 0 }
    }

    fn used_inodes(&self, g: u64) -> u64 {
        if g == 0 {
            u64::from(FIRST_INO)
        } else {
            0
        }
    }

    pub fn free_blocks(&self) -> u64 {
        (0..self.groups)
            .map(|g| self.group_blocks(g) - self.used_blocks(g))
            .sum()
    }

    pub fn inodes(&self) -> u64 {
        self.groups * self.inodes_per_group
    }
}

fn put16(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 2].copy_from_slice(&(v as u16).to_le_bytes());
}

fn put32(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 4].copy_from_slice(&(v as u32).to_le_bytes());
}

/// The superblock of the copy in group `g` (1024 bytes).
fn superblock(l: &Layout, g: u64, uuid: &[u8; 16], seed: &[u8; 16], now: u64) -> Vec<u8> {
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
    put16(&mut s, 90, g);
    put32(&mut s, 92, (COMPAT_EXT_ATTR | COMPAT_DIR_INDEX).into());
    put32(&mut s, 96, (INCOMPAT_FILETYPE | INCOMPAT_EXTENTS).into());
    put32(
        &mut s,
        100,
        (RO_COMPAT_SPARSE_SUPER
            | RO_COMPAT_LARGE_FILE
            | RO_COMPAT_DIR_NLINK
            | RO_COMPAT_EXTRA_ISIZE)
            .into(),
    );
    s[104..120].copy_from_slice(uuid);
    s[120..120 + 10].copy_from_slice(b"podvm-disk");
    s[236..252].copy_from_slice(seed);
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
        let rec = if n + 1 == entries.len() {
            BLOCK as usize - at
        } else {
            len
        };
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
pub fn write(f: &std::fs::File, l: &Layout, uuid: [u8; 16], seed: [u8; 16]) -> std::io::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let at = |block: u64| block * BLOCK;
    let gdt = descriptors(l);
    for g in 0..l.groups {
        let start = l.group_start(g);
        if has_super(g) {
            let sb = superblock(l, g, &uuid, &seed, now);
            // Group 0 keeps its superblock 1024 bytes into the disk; each
            // copy starts its group's first block.
            f.write_all_at(&sb, if g == 0 { 1024 } else { at(start) })?;
            f.write_all_at(&gdt, at(start + 1))?;
        }
        let blocks = l.group_blocks(g);
        f.write_all_at(&bitmap(l.used_blocks(g), blocks), at(l.block_bitmap(g)))?;
        f.write_all_at(
            &bitmap(l.used_inodes(g), l.inodes_per_group),
            at(l.block_bitmap(g) + 1),
        )?;
    }
    let (root, lost) = l.dir_blocks();
    let inode_at = |ino: u32| at(l.inode_table(0)) + u64::from(ino - 1) * INODE_SIZE;
    f.write_all_at(&dir_inode(0o755, 3, root, now), inode_at(ROOT_INO))?;
    f.write_all_at(&dir_inode(0o700, 2, lost, now), inode_at(LOST_FOUND_INO))?;
    f.write_all_at(
        &dir_block(&[
            (ROOT_INO, "."),
            (ROOT_INO, ".."),
            (LOST_FOUND_INO, "lost+found"),
        ]),
        at(root),
    )?;
    f.write_all_at(
        &dir_block(&[(LOST_FOUND_INO, "."), (ROOT_INO, "..")]),
        at(lost),
    )?;
    Ok(())
}
