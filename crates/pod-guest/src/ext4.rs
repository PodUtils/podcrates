//! An empty ext4 file system written into a sparse file, with no tool of
//! the host.
//!
//! A static program cannot expect `mke2fs` on the host, so this module
//! writes the file system itself. The layout is the plain one of
//! `mke2fs -t ext4 -O ^has_journal,^flex_bg,^metadata_csum,^resize_inode
//! -b 4096 -I 256 -m 0`: 4 KiB blocks, 32,768 blocks for each group, one
//! inode for each 16 KiB, a copy of the superblock and the group
//! descriptors in group 0, 1 and each power of 3, 5 and 7 (`sparse_super`),
//! then the block bitmap, the inode bitmap and the inode table of each
//! group. The root directory and `lost+found` have one block each. The
//! file system has no journal and no checksums: the disk is disposable,
//! and a guest that crashes boots on a new one.
//!
//! Only the metadata blocks are written. Each other block stays a hole of
//! the file, so a new disk takes some kilobytes of the host for each group,
//! and the writes of the guest take host blocks as they come.

use std::fs::File;

use pod_core::entropy::{Entropy, OsEntropy};
use pod_core::{Class, Failure, Stage};

use crate::{Result, failure};

mod blocks;

/// The block of the file system, 4 KiB.
pub const BLOCK: u64 = 4096;
/// The smallest disk: 64 MiB.
pub const MIN_BYTES: u64 = 64 << 20;
/// The largest disk: each block number fits 32 bits, so 16 TiB less one
/// block.
pub const MAX_BYTES: u64 = u32::MAX as u64 * BLOCK;
/// The most bytes of a label (`s_volume_name`).
pub const LABEL_LEN: usize = 16;

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

/// The names of one file system: its UUID, the seed of the hashes of its
/// indexed directories, and its label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Names {
    pub uuid: [u8; 16],
    pub hash_seed: [u8; 16],
    /// At most [`LABEL_LEN`] bytes, with no NUL byte.
    pub label: String,
}

impl Names {
    /// A random version 4 UUID and a random hash seed from the system, and
    /// `label`.
    pub fn random(label: &str) -> Result<Names> {
        Names::from_entropy(&mut OsEntropy, label)
    }

    /// [`Names::random`] from the source `e`. A source that gives no bytes
    /// is a [`Class::Unavailable`] failure, never a fixed UUID. A label
    /// that [`write_empty`] refuses is refused here too.
    pub fn from_entropy(e: &mut impl Entropy, label: &str) -> Result<Names> {
        label_bytes(label)?;
        let mut b = [0u8; 32];
        e.fill(&mut b).map_err(|why| Failure::caused(Stage::Host, Class::Unavailable, why.to_string(), why))?;
        let (mut uuid, mut hash_seed) = ([0u8; 16], [0u8; 16]);
        uuid.copy_from_slice(&b[..16]);
        hash_seed.copy_from_slice(&b[16..]);
        uuid[6] = (uuid[6] & 0x0f) | 0x40; // version 4
        uuid[8] = (uuid[8] & 0x3f) | 0x80; // the variant of RFC 9562
        Ok(Names { uuid, hash_seed, label: label.to_string() })
    }
}

/// The 16 bytes of `s_volume_name`, padded with NUL bytes. More than
/// [`LABEL_LEN`] bytes, or a NUL byte, is a [`Class::Usage`] failure.
fn label_bytes(label: &str) -> Result<[u8; 16]> {
    if label.len() > LABEL_LEN || label.contains('\0') {
        return Err(failure(
            Class::Usage,
            format!("{label:?} is not an ext4 label: {LABEL_LEN} bytes at most, with no NUL byte"),
        ));
    }
    let mut out = [0u8; 16];
    out[..label.len()].copy_from_slice(label.as_bytes());
    Ok(out)
}

/// Write an empty ext4 file system of `bytes` into `file`, an empty file
/// open for write: the file gets the length `bytes`, and only the metadata
/// blocks are written. The refusals, each a [`Class::Usage`] failure that
/// names the value: a size that [`Layout::of`] refuses, a label that
/// [`Names`] refuses, a file that is not empty (the inode tables must be
/// zero bytes, and they stay holes). An I/O error has the class of its
/// kind; the caller removes the file.
pub fn write_empty(file: &File, bytes: u64, names: &Names) -> Result<()> {
    let layout = Layout::of(bytes)?;
    let label = label_bytes(&names.label)?;
    let len = file.metadata().map_err(|e| io("read the length of the disk", e))?.len();
    if len != 0 {
        return Err(failure(
            Class::Usage,
            format!(
                "the disk has {len} bytes: an ext4 file system goes into an empty file, as its inode tables must be zero bytes"
            ),
        ));
    }
    file.set_len(bytes).map_err(|e| io(&format!("set the length of the disk to {bytes} bytes"), e))?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    blocks::write(file, &layout, names, &label, now).map_err(|e| io("write the ext4 metadata", e))
}

fn io(what: &str, e: std::io::Error) -> Failure {
    Failure::caused(Stage::Host, Class::of_io(e.kind()), format!("{what}: {e}"), e)
}

/// The shape of one file system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The blocks of the file system: the size in blocks, less a last rest
    /// that holds no group.
    pub blocks: u64,
    pub groups: u64,
    pub inodes_per_group: u64,
    /// The blocks of the group descriptor table.
    pub gdt_blocks: u64,
    /// The blocks of the inode table of each group.
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
    /// The layout of a file system of `bytes`. Each refusal is a
    /// [`Class::Usage`] failure that names `bytes`: fewer than
    /// [`MIN_BYTES`], more than [`MAX_BYTES`], or not a whole number of
    /// [`BLOCK`]s.
    pub fn of(bytes: u64) -> Result<Layout> {
        let refuse = |why: &str| Err(failure(Class::Usage, format!("{bytes} bytes {why}")));
        if bytes < MIN_BYTES {
            return refuse("are fewer than 64 MiB, the smallest ext4 disk");
        }
        if bytes > MAX_BYTES {
            return refuse("are more than an ext4 file system of 32-bit block numbers holds (16 TiB less 4096 bytes)");
        }
        if !bytes.is_multiple_of(BLOCK) {
            return refuse("are not a whole number of 4096-byte blocks");
        }
        let mut blocks = bytes / BLOCK;
        loop {
            let groups = blocks.div_ceil(BLOCKS_PER_GROUP);
            let per_block = BLOCK / INODE_SIZE;
            let inodes = (blocks * BLOCK / BYTES_PER_INODE).max(FIRST_INO.into());
            let ipg =
                inodes.div_ceil(groups).div_ceil(per_block).saturating_mul(per_block).clamp(2 * per_block, 8 * BLOCK);
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
                return refuse("hold no ext4 file system");
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
        if has_super(g) { 1 + self.gdt_blocks } else { 0 }
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
        if g == 0 { u64::from(FIRST_INO) } else { 0 }
    }

    /// The free blocks of a new file system.
    pub fn free_blocks(&self) -> u64 {
        (0..self.groups).map(|g| self.group_blocks(g) - self.used_blocks(g)).sum()
    }

    pub fn inodes(&self) -> u64 {
        self.groups * self.inodes_per_group
    }
}

#[cfg(test)]
mod tests;
