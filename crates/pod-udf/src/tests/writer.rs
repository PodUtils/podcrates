//! A writer of small UDF images for the tests, from ECMA-167 and UDF 2.60:
//! the volume recognition sequence, the anchor, a main and a reserve volume
//! descriptor sequence, one partition, a file set, and a tree of
//! directories and files. With `metadata`, the descriptors of the files go
//! to a metadata partition whose file has two extents with a gap between.

use std::collections::BTreeMap;

use crate::tag::{TAG, checksum, crc16};

pub(crate) const BS: usize = 2048;
/// The first block of the partition.
pub(crate) const PART: u32 = 272;
/// The first block of the file data in the partition.
pub(crate) const DATA: u32 = 128;
/// The blocks of each of the two extents of the metadata file.
pub(crate) const META_HALF: u32 = 32;

/// A part of a file: recorded bytes (one extent each), or a hole of
/// blocks that the image does not record.
#[derive(Clone, Debug)]
pub(crate) enum Part {
    Bytes(Vec<u8>),
    Hole(u32),
}

#[derive(Clone, Debug)]
pub(crate) enum Node {
    Dir(Vec<(String, Node)>),
    File(Vec<Part>),
    /// A file whose bytes are in its file entry.
    Embedded(Vec<u8>),
}

pub(crate) fn dir(entries: Vec<(&str, Node)>) -> Node {
    Node::Dir(entries.into_iter().map(|(n, e)| (n.to_string(), e)).collect())
}

pub(crate) fn file(bytes: &[u8]) -> Node {
    Node::File(vec![Part::Bytes(bytes.to_vec())])
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Writer {
    /// The UDF revision as BCD: 0x0102 writes file entries and descriptor
    /// version 2; 0x0200 and later extended file entries and version 3.
    pub(crate) revision: u16,
    pub(crate) metadata: bool,
    /// `long_ad` for each file entry; a metadata partition needs them for
    /// the data.
    pub(crate) long_ads: bool,
}

impl Default for Writer {
    fn default() -> Self {
        Self { revision: 0x0102, metadata: false, long_ads: false }
    }
}

/// Fill the tag of the descriptor of `len` bytes at the head of `buf`.
pub(crate) fn seal(buf: &mut [u8], ident: u16, version: u16, location: u32, len: usize) {
    buf[0..2].copy_from_slice(&ident.to_le_bytes());
    buf[2..4].copy_from_slice(&version.to_le_bytes());
    let crc_len = (len - TAG) as u16;
    let crc = crc16(&buf[TAG..len]);
    buf[8..10].copy_from_slice(&crc.to_le_bytes());
    buf[10..12].copy_from_slice(&crc_len.to_le_bytes());
    buf[12..16].copy_from_slice(&location.to_le_bytes());
    let tag: [u8; TAG] = buf[..TAG].try_into().unwrap();
    buf[4] = checksum(&tag);
}

fn put16(buf: &mut [u8], at: usize, v: u16) {
    buf[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(buf: &mut [u8], at: usize, v: u32) {
    buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put64(buf: &mut [u8], at: usize, v: u64) {
    buf[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

/// A name in OSTA compressed unicode: 8 when each character fits a byte.
pub(crate) fn cs0(name: &str) -> Vec<u8> {
    if name.chars().all(|c| (c as u32) < 256) {
        std::iter::once(8).chain(name.chars().map(|c| c as u8)).collect()
    } else {
        std::iter::once(16).chain(name.encode_utf16().flat_map(u16::to_be_bytes)).collect()
    }
}

fn blocks(len: usize) -> u32 {
    len.div_ceil(BS) as u32
}

/// An allocation descriptor to write: type, bytes, block, partition ref.
type Ad = (u32, u32, u32, u16);

struct Image {
    w: Writer,
    /// The blocks of the descriptors of the files, by their number in the
    /// partition of `meta_ref`.
    meta: BTreeMap<u32, Vec<u8>>,
    /// The blocks of the data, by their number in the physical partition.
    data: BTreeMap<u32, Vec<u8>>,
    next_meta: u32,
    next_data: u32,
    meta_ref: u16,
}

impl Writer {
    fn version(&self) -> u16 {
        if self.revision >= 0x0200 { 3 } else { 2 }
    }

    /// The image of `root`.
    pub(crate) fn image(&self, root: &Node) -> Vec<u8> {
        let meta_ref = u16::from(self.metadata);
        let mut img =
            Image { w: *self, meta: BTreeMap::new(), data: BTreeMap::new(), next_meta: 2, next_data: DATA, meta_ref };
        img.file_set();
        let Node::Dir(entries) = root else { panic!("the root is a directory") };
        img.dir(entries, 2, 2);
        self.assemble(img)
    }

    fn assemble(&self, img: Image) -> Vec<u8> {
        let part_len = img.next_data;
        let mut out = vec![0u8; (PART + part_len) as usize * BS];
        // The volume recognition sequence at byte 32768.
        let nsr: &[u8] = if self.revision >= 0x0200 { b"NSR03" } else { b"NSR02" };
        for (i, ident) in [&b"BEA01"[..], nsr, b"TEA01"].into_iter().enumerate() {
            let at = 32768 + i * BS;
            out[at + 1..at + 6].copy_from_slice(ident);
            out[at + 6] = 1;
        }
        for start in [32u32, 48] {
            for (i, d) in self.sequence(start, part_len).into_iter().enumerate() {
                let at = (start as usize + i) * BS;
                out[at..at + d.len()].copy_from_slice(&d);
            }
        }
        let mut avdp = vec![0u8; 512];
        put32(&mut avdp, 16, 16 * BS as u32);
        put32(&mut avdp, 20, 32);
        put32(&mut avdp, 24, 16 * BS as u32);
        put32(&mut avdp, 28, 48);
        seal(&mut avdp, 2, self.version(), 256, 512);
        out[256 * BS..256 * BS + 512].copy_from_slice(&avdp);
        let mut put = |lbn: u32, block: &[u8]| {
            let at = (PART + lbn) as usize * BS;
            out[at..at + block.len()].copy_from_slice(block);
        };
        if self.metadata {
            put(META_HALF + 2, &[0xee; BS]);
            for (file_type, lbn) in [(250u8, 0u32), (251, 1)] {
                let ads = [(0, META_HALF * BS as u32, 2, 0), (0, META_HALF * BS as u32, META_HALF + 3, 0)];
                put(lbn, &self.entry(lbn, file_type, 2 * u64::from(META_HALF) * BS as u64, &ads, None, true));
            }
        }
        for (lbn, block) in &img.meta {
            assert!(*lbn < 2 * META_HALF, "the metadata area is full");
            let physical = match (self.metadata, *lbn < META_HALF) {
                (false, _) => *lbn,
                (true, true) => 2 + lbn,
                (true, false) => META_HALF + 3 + (lbn - META_HALF),
            };
            put(physical, block);
        }
        for (lbn, block) in &img.data {
            put(*lbn, block);
        }
        out
    }

    /// The volume descriptor sequence at block `start`: a primary volume
    /// descriptor, a partition descriptor, a logical volume descriptor, a
    /// terminating descriptor.
    fn sequence(&self, start: u32, part_len: u32) -> Vec<Vec<u8>> {
        let v = self.version();
        let mut pvd = vec![0u8; 512];
        put32(&mut pvd, 16, 1);
        seal(&mut pvd, 1, v, start, 512);
        let mut pd = vec![0u8; 512];
        put32(&mut pd, 16, 2);
        put16(&mut pd, 20, 1);
        pd[25..31].copy_from_slice(if v == 3 { b"+NSR03" } else { b"+NSR02" });
        put32(&mut pd, 184, 1);
        put32(&mut pd, 188, PART);
        put32(&mut pd, 192, part_len);
        seal(&mut pd, 5, v, start + 1, 512);
        let mut maps = vec![1u8, 6, 1, 0, 0, 0];
        if self.metadata {
            let mut m = vec![0u8; 64];
            m[0] = 2;
            m[1] = 64;
            m[5..28].copy_from_slice(b"*UDF Metadata Partition");
            put16(&mut m, 28, self.revision);
            put16(&mut m, 36, 1);
            put32(&mut m, 40, 0);
            put32(&mut m, 44, 1);
            put32(&mut m, 48, u32::MAX);
            put32(&mut m, 52, 32);
            put16(&mut m, 56, 1);
            maps.extend(m);
        }
        let len = 440 + maps.len();
        let mut lvd = vec![0u8; len];
        put32(&mut lvd, 16, 3);
        put32(&mut lvd, 212, BS as u32);
        lvd[217..236].copy_from_slice(b"*OSTA UDF Compliant");
        put16(&mut lvd, 240, self.revision);
        put32(&mut lvd, 248, BS as u32);
        put32(&mut lvd, 252, 0);
        put16(&mut lvd, 256, u16::from(self.metadata));
        put32(&mut lvd, 264, maps.len() as u32);
        put32(&mut lvd, 268, if self.metadata { 2 } else { 1 });
        lvd[440..].copy_from_slice(&maps);
        seal(&mut lvd, 6, v, start + 2, len);
        let mut td = vec![0u8; 512];
        seal(&mut td, 8, v, start + 3, 512);
        vec![pvd, pd, lvd, td]
    }

    /// A file entry (UDF 1.02) or an extended file entry at `lbn`, with
    /// `ads`, or with `embedded` bytes.
    fn entry(&self, lbn: u32, file_type: u8, size: u64, ads: &[Ad], embedded: Option<&[u8]>, short: bool) -> Vec<u8> {
        let mut e = vec![0u8; BS];
        let efe = self.revision >= 0x0200;
        let (lengths, base) = if efe { (208, 216) } else { (168, 176) };
        put16(&mut e, 20, 4);
        put16(&mut e, 24, 1);
        e[27] = file_type;
        put32(&mut e, 36, u32::MAX);
        put32(&mut e, 40, u32::MAX);
        put16(&mut e, 48, 1);
        put64(&mut e, 56, size);
        if efe {
            put64(&mut e, 64, size);
        }
        let mut list = Vec::new();
        let flags = match embedded {
            Some(bytes) => {
                list.extend_from_slice(bytes);
                3
            }
            None => {
                for &(kind, len, at, partition) in ads {
                    list.extend((kind << 30 | len).to_le_bytes());
                    list.extend(at.to_le_bytes());
                    if !short {
                        list.extend(partition.to_le_bytes());
                        list.extend([0u8; 6]);
                    }
                }
                u16::from(!short)
            }
        };
        put16(&mut e, 34, flags);
        put32(&mut e, lengths + 4, list.len() as u32);
        e[base..base + list.len()].copy_from_slice(&list);
        let len = base + list.len();
        seal(&mut e, if efe { 266 } else { 261 }, self.version(), lbn, len);
        e
    }
}

impl Image {
    fn alloc_meta(&mut self, n: u32) -> u32 {
        let at = self.next_meta;
        self.next_meta += n;
        at
    }

    fn file_set(&mut self) {
        let v = self.w.version();
        let mut fsd = vec![0u8; 512];
        put32(&mut fsd, 400, BS as u32);
        put32(&mut fsd, 404, 2);
        put16(&mut fsd, 408, self.meta_ref);
        seal(&mut fsd, 256, v, 0, 512);
        self.meta.insert(0, fsd);
        let mut td = vec![0u8; 512];
        seal(&mut td, 8, v, 1, 512);
        self.meta.insert(1, td);
        self.next_meta = 3;
    }

    /// The file identifier descriptors of a directory whose bytes start at
    /// block `at`: the parent first, then each entry.
    fn fids(&self, at: u32, parent: u32, entries: &[(String, Node)], icbs: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        let one = |out: &mut Vec<u8>, characteristics: u8, name: &[u8], icb: u32| {
            let len = (38 + name.len()).next_multiple_of(4);
            let mut d = vec![0u8; len];
            put16(&mut d, 16, 1);
            d[18] = characteristics;
            d[19] = name.len() as u8;
            put32(&mut d, 20, BS as u32);
            put32(&mut d, 24, icb);
            put16(&mut d, 28, self.meta_ref);
            d[38..38 + name.len()].copy_from_slice(name);
            seal(&mut d, 257, self.w.version(), at + (out.len() / BS) as u32, 38 + name.len());
            out.extend(d);
        };
        one(&mut out, 0x0a, &[], parent);
        for ((name, node), icb) in entries.iter().zip(icbs) {
            one(&mut out, if matches!(node, Node::Dir(_)) { 2 } else { 0 }, &cs0(name), *icb);
        }
        out
    }

    fn dir(&mut self, entries: &[(String, Node)], me: u32, parent: u32) {
        let icbs: Vec<u32> = entries.iter().map(|_| self.alloc_meta(1)).collect();
        let len = self.fids(0, parent, entries, &icbs).len();
        let at = self.alloc_meta(blocks(len));
        let content = self.fids(at, parent, entries, &icbs);
        for (i, chunk) in content.chunks(BS).enumerate() {
            self.meta.insert(at + i as u32, chunk.to_vec());
        }
        let short = !self.w.long_ads;
        let ad = (0, len as u32, at, self.meta_ref);
        self.meta.insert(me, self.w.entry(me, 4, len as u64, &[ad], None, short));
        for ((_, node), icb) in entries.iter().zip(&icbs) {
            match node {
                Node::Dir(children) => self.dir(children, *icb, me),
                Node::File(parts) => self.file(parts, *icb),
                Node::Embedded(bytes) => {
                    let e = self.w.entry(*icb, 5, bytes.len() as u64, &[], Some(bytes), true);
                    self.meta.insert(*icb, e);
                }
            }
        }
    }

    /// The data of a file: each part in its own extent, a block of 0xEE
    /// between two parts so that no two extents touch.
    fn file(&mut self, parts: &[Part], icb: u32) {
        let mut ads = Vec::new();
        let mut size = 0u64;
        for part in parts {
            match part {
                Part::Bytes(bytes) => {
                    if !ads.is_empty() {
                        self.data.insert(self.next_data, vec![0xee; BS]);
                        self.next_data += 1;
                    }
                    let at = self.next_data;
                    for (i, chunk) in bytes.chunks(BS).enumerate() {
                        self.data.insert(at + i as u32, chunk.to_vec());
                    }
                    self.next_data += blocks(bytes.len());
                    ads.push((0, bytes.len() as u32, at, 0));
                    size += bytes.len() as u64;
                }
                Part::Hole(n) => {
                    ads.push((2, n * BS as u32, 0, 0));
                    size += u64::from(*n) * BS as u64;
                }
            }
        }
        let short = !self.w.long_ads && !self.w.metadata;
        self.meta.insert(icb, self.w.entry(icb, 5, size, &ads, None, short));
    }
}
