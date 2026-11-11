//! The metadata blocks (a 2-byte header, then at most 8 KiB, stored or
//! compressed), the lookup tables of fragments, ids and exports, and the
//! decoder of a compressed block.

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::rc::Rc;

use xz4rust::{XzDecoder, XzError, XzNextBlockResult};

use super::{Compressor, Fault, Image, METADATA_SIZE, NO_TABLE};
use crate::Result;

/// The ceiling of the cache of decoded metadata blocks. The cache empties
/// when it is full: it is a cache, and a walk of a large image can touch
/// each block of its tables.
const CACHE_BYTES: usize = 8 << 20;

/// One decoded metadata block, and the place of the block after it.
pub(super) struct Block {
    data: Vec<u8>,
    next: u64,
}

#[derive(Default)]
pub(super) struct Cache {
    blocks: HashMap<u64, Rc<Block>>,
    bytes: usize,
}

/// A place in a metadata table: the block at `block` (an offset of the
/// image) and an offset in its decoded data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub block: u64,
    pub offset: usize,
}

/// The width of a fragment entry: its start, its size word, a word that is
/// not used.
pub(super) const FRAGMENT_ENTRY: usize = 16;

impl<R: Read + Seek> Image<R> {
    /// The metadata block at `at`, from the cache or decoded now.
    fn block(&mut self, at: u64) -> Result<Rc<Block>> {
        if let Some(b) = self.meta.blocks.get(&at) {
            return Ok(Rc::clone(b));
        }
        let mut head = [0u8; 2];
        self.read_at("a metadata block", at, &mut head)?;
        let word = u16::from_le_bytes(head);
        let size = word & 0x7fff;
        if size == 0 || usize::from(size) > METADATA_SIZE {
            return Err(Fault::MetadataSize { at, size }.into());
        }
        let raw = self.bytes_at("a metadata block", at + 2, usize::from(size))?;
        let data = if word & 0x8000 != 0 { raw } else { self.decode(at + 2, &raw, METADATA_SIZE)? };
        let block = Rc::new(Block { data, next: at + 2 + u64::from(size) });
        if self.meta.bytes + block.data.len() > CACHE_BYTES {
            self.meta = Cache::default();
        }
        self.meta.bytes += block.data.len();
        self.meta.blocks.insert(at, Rc::clone(&block));
        Ok(block)
    }

    /// Fill `out` from the metadata at `place`, across blocks, and move
    /// `place` past the bytes. As in the kernel, an offset at or past the end
    /// of its block is a fault, and the end of a block moves to the next.
    pub(super) fn read_meta(&mut self, place: &mut Place, out: &mut [u8]) -> Result<()> {
        let mut done = 0;
        while done < out.len() {
            let block = self.block(place.block)?;
            if place.offset >= block.data.len() {
                return Err(Fault::Offset { at: place.block, offset: place.offset }.into());
            }
            let n = (out.len() - done).min(block.data.len() - place.offset);
            out[done..done + n].copy_from_slice(&block.data[place.offset..place.offset + n]);
            done += n;
            place.offset += n;
            if place.offset == block.data.len() {
                *place = Place { block: block.next, offset: 0 };
            }
        }
        Ok(())
    }

    /// `N` bytes of metadata at `place`.
    pub(super) fn meta_array<const N: usize>(&mut self, place: &mut Place) -> Result<[u8; N]> {
        let mut out = [0u8; N];
        self.read_meta(place, &mut out)?;
        Ok(out)
    }

    /// `len` bytes of metadata at `place`.
    pub(super) fn meta_vec(&mut self, place: &mut Place, len: usize) -> Result<Vec<u8>> {
        let mut out = vec![0u8; len];
        self.read_meta(place, &mut out)?;
        Ok(out)
    }

    /// Entry `index` of a lookup table: the table at `start` holds the
    /// places of the metadata blocks of `count` entries of `width` bytes.
    /// An entry does not cross a block, as 8 KiB is a multiple of each
    /// width.
    fn entry(&mut self, table: &'static str, start: u64, count: u64, width: usize, index: u64) -> Result<Vec<u8>> {
        if index >= count || start == NO_TABLE {
            return Err(Fault::Index { table, index }.into());
        }
        let byte = index * width as u64;
        let (n, offset) = (byte / METADATA_SIZE as u64, (byte % METADATA_SIZE as u64) as usize);
        let mut pointer = [0u8; 8];
        let at = start.checked_add(n * 8).ok_or(Fault::Past { what: table, at: start })?;
        self.read_at(table, at, &mut pointer)?;
        let mut place = Place { block: u64::from_le_bytes(pointer), offset };
        self.meta_vec(&mut place, width)
    }

    /// The fragment entry `index`: the start of its block and its size word.
    pub(super) fn fragment_entry(&mut self, index: u32) -> Result<(u64, u32)> {
        let (start, count) = (self.sb.fragment_table, u64::from(self.sb.fragments));
        let e = self.entry("fragment", start, count, FRAGMENT_ENTRY, u64::from(index))?;
        Ok((super::u64_at(&e, 0), super::u32_at(&e, 8)))
    }

    /// The uids and gids of the image, by their index.
    pub fn ids(&mut self) -> Result<Vec<u32>> {
        let (start, count) = (self.sb.id_table, u64::from(self.sb.ids));
        (0..count).map(|i| Ok(super::u32_at(&self.entry("id", start, count, 4, i)?, 0))).collect()
    }

    /// The reference of inode `number` (from 1) by the export table, or
    /// `None` when the image has no export table.
    pub fn export(&mut self, number: u32) -> Result<Option<u64>> {
        if self.sb.export_table == NO_TABLE {
            return Ok(None);
        }
        let (start, count) = (self.sb.export_table, u64::from(self.sb.inodes));
        let index = u64::from(number).checked_sub(1).ok_or(Fault::Index { table: "export", index: 0 })?;
        Ok(Some(super::u64_at(&self.entry("export", start, count, 8, index)?, 0)))
    }

    /// Decode the compressed block `raw` that starts at `at`: at most `max`
    /// bytes. A stream that holds more, that ends early, or that has bytes
    /// after its end is a fault, as in the kernel.
    pub(super) fn decode(&mut self, at: u64, raw: &[u8], max: usize) -> Result<Vec<u8>> {
        let dictionary = match self.sb.compressor {
            Compressor::Xz { dictionary, .. } => dictionary,
            Compressor::Other(id) => return Err(Fault::Compressor(id).into()),
        };
        // The dictionary grows to what a stream asks, up to the dictionary
        // of the image: a larger ask is a fault, not an allocation.
        let decoder =
            self.xz.get_or_insert_with(|| Box::new(XzDecoder::with_alloc_dict(Vec::new(), dictionary as usize)));
        decoder.reset();
        let mut out = vec![0u8; max];
        let (mut i, mut o) = (0, 0);
        let stream = |why: String| Fault::Stream { at, why };
        loop {
            match decoder.decode(&raw[i..], &mut out[o..]) {
                Ok(XzNextBlockResult::EndOfStream(a, b)) => {
                    i += a;
                    o += b;
                    break;
                }
                Ok(XzNextBlockResult::NeedMoreData(a, b)) => {
                    i += a;
                    o += b;
                }
                // No progress: the input is spent, or the output is full.
                Err(XzError::NeedsLargerInputBuffer) if i == raw.len() => {
                    return Err(stream("it ends early".to_string()).into());
                }
                Err(XzError::NeedsLargerInputBuffer) if o == max => {
                    return Err(stream(format!("it holds more than {max} bytes")).into());
                }
                Err(e) => return Err(stream(e.to_string()).into()),
            }
        }
        if i != raw.len() {
            return Err(stream(format!("{} bytes after its end", raw.len() - i)).into());
        }
        out.truncate(o);
        Ok(out)
    }
}
