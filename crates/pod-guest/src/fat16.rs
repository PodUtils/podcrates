//! The mailbox volume: a FAT16 image behind an MBR partition, built and
//! read in process.
//!
//! The agent of the UEFI flavour is `cmd.exe`, and `cmd.exe` gets a file
//! only through a volume that the guest mounts. `mkfs.fat` and `mtools`
//! are packages of the host; this module writes the small, fixed format
//! itself.
//!
//! The geometry is the tested one: a volume of 16 MiB behind an MBR
//! partition at LBA 2048, as measured against Validation OS. Windows does
//! not mount a volume with no partition table as a fixed NVMe disk every
//! time. The cluster is 4 KiB, so that the count of clusters stays below
//! 65525, the limit of FAT16.
//!
//! Names are 8.3 only. Each file of the protocol (`WQMARK.TXT`, `WQGO.TXT`,
//! `WQCMD.CMD`, `WQOUT.TXT`, `WQERR.TXT`, `WQCODE.TXT`, `WQAGENT.CMD`,
//! `WA.CMD`, `SETUP.TXT`) is 8.3. [`Fat16::put`] refuses each other name
//! and names it: a changed name is a file that the guest does not find.

use pod_core::Class;

use crate::{Result, failure};

/// The sector: the unit of the volume and of the kernel.
pub const SECTOR: usize = 512;
/// 4 KiB clusters.
pub const SECTORS_PER_CLUSTER: usize = 8;
/// The reserved sectors of the volume, as the BPB records them.
pub const RESERVED_SECTORS: usize = 1;
/// Two FATs: a reader that trusts the second copy expects it.
pub const NUM_FATS: usize = 2;
/// 512 root entries, so 32 sectors: the fixed root of FAT16.
pub const ROOT_ENTRIES: usize = 512;
/// The volume itself, 16 MiB.
pub const VOLUME_SECTORS: usize = 32768;
/// Sectors for each FAT. A fixed value, not a computed one: the checks at
/// compile time below compare it with the count of clusters, so a wrong
/// value does not build.
pub const FAT_SECTORS: usize = 16;
/// The first sector of the partition.
pub const PART_LBA: usize = 2048;
/// The MBR partition type for a FAT16 volume of this size.
pub const PART_TYPE: u8 = 0x06;
/// `ROOT_ENTRIES * 32 / SECTOR`.
pub const ROOT_SECTORS: usize = ROOT_ENTRIES * 32 / SECTOR;
/// The first data sector, in sectors from the start of the volume.
pub const DATA_SECTOR: usize = RESERVED_SECTORS + NUM_FATS * FAT_SECTORS + ROOT_SECTORS;
/// The count of clusters of the geometry.
pub const CLUSTERS: usize = (VOLUME_SECTORS - DATA_SECTOR) / SECTORS_PER_CLUSTER;
/// The whole image, MBR included.
pub const IMAGE_SECTORS: usize = PART_LBA + VOLUME_SECTORS;
/// The whole image in bytes.
pub const IMAGE_LEN: usize = IMAGE_SECTORS * SECTOR;
/// The label of the volume in the BPB. [`Fat16::is_mailbox`] reads it.
pub const LABEL: &str = "WQMAILBOX";

/// A check at compile time: the FAT holds each cluster of the geometry.
/// `(CLUSTERS + 2) * 2` is the size of the FAT in bytes.
const _: () = assert!(CLUSTERS + 2 <= FAT_SECTORS * SECTOR / 2);
const _: () = assert!(CLUSTERS + 2 < 0xFFF0, "FAT16, not FAT32");
// The volume is not a multiple of the cluster: the data area keeps the
// whole clusters that fit, and no file uses the rest.
const _: () = assert!(DATA_SECTOR + CLUSTERS * SECTORS_PER_CLUSTER <= VOLUME_SECTORS);
const _: () = assert!(VOLUME_SECTORS - (DATA_SECTOR + CLUSTERS * SECTORS_PER_CLUSTER) < SECTORS_PER_CLUSTER);

/// One volume in memory: the whole image, with the MBR.
#[derive(Clone)]
pub struct Fat16 {
    image: Vec<u8>,
}

impl Default for Fat16 {
    fn default() -> Self {
        Self::new()
    }
}

impl Fat16 {
    /// A formatted, empty volume with the OEM name [`OEM`].
    pub fn new() -> Self {
        Fat16 { image: format() }
    }

    /// A formatted, empty volume with the OEM name `oem`: at most 8
    /// printable ASCII characters.
    pub fn with_oem(oem: &str) -> Result<Self> {
        Ok(Fat16 { image: format_with(oem_bytes(oem)?) })
    }

    /// Take an image that a run wrote, so that the host reads the results
    /// with no new volume. `None`: the image is too short.
    pub fn from_image(image: Vec<u8>) -> Option<Self> {
        if image.len() < IMAGE_LEN {
            return None;
        }
        Some(Fat16 { image })
    }

    pub fn image(&self) -> &[u8] {
        &self.image
    }

    pub fn into_image(self) -> Vec<u8> {
        self.image
    }

    /// Write `data` as `name`, in place of a file of that name. The name
    /// becomes upper case and must be 8.3; each other name is a
    /// [`Class::Usage`] failure that names it. A full volume or root is
    /// [`Class::Unavailable`].
    pub fn put(&mut self, name: &str, data: &[u8]) -> Result<()> {
        let (bare, case) = parse_name(name)?;
        if let Some(slot) = self.find(&bare) {
            self.free_chain(slot.1);
            self.write_entry(slot.0, &bare, case, data)?;
            return Ok(());
        }
        let slot = self.free_slot()?;
        self.write_entry(slot, &bare, case, data)
    }

    /// The bytes of `name`, or `None` where it is not in the root.
    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        let (bare, _) = parse_name(name).ok()?;
        let (_, first, size) = self.find(&bare)?;
        self.read_chain(first, size)
    }

    /// Each file name in the root, in the order of the directory.
    pub fn list(&self) -> Vec<String> {
        let mut out = Vec::new();
        for i in 0..ROOT_ENTRIES {
            let e = self.entry(i);
            if e[0] == 0x00 {
                break;
            }
            if e[0] == 0xE5 || e[11] & 0x18 != 0 {
                continue;
            }
            out.push(render_name(&e));
        }
        out
    }

    /// True when the volume has the label [`LABEL`]: a reader that is sure
    /// to look at a mailbox.
    pub fn is_mailbox(&self) -> bool {
        let base = PART_LBA * SECTOR;
        self.image[base + 0x2B..base + 0x36] == label_bytes()
    }

    // ---------------------------------------------------------------- layout

    fn fat(&self, n: u16) -> u16 {
        let base = PART_LBA * SECTOR + RESERVED_SECTORS * SECTOR;
        let at = base + usize::from(n) * 2;
        u16::from_le_bytes([self.image[at], self.image[at + 1]])
    }

    fn set_fat(&mut self, n: u16, v: u16) {
        let base = PART_LBA * SECTOR + RESERVED_SECTORS * SECTOR;
        for fat in 0..NUM_FATS {
            let at = base + fat * FAT_SECTORS * SECTOR + usize::from(n) * 2;
            self.image[at..at + 2].copy_from_slice(&v.to_le_bytes());
        }
    }

    fn cluster_at(&self, n: u16) -> usize {
        let sector = DATA_SECTOR + (usize::from(n) - 2) * SECTORS_PER_CLUSTER;
        (PART_LBA + sector) * SECTOR
    }

    fn entry(&self, i: usize) -> [u8; 32] {
        let at = PART_LBA * SECTOR + DATA_SECTOR * SECTOR - ROOT_SECTORS * SECTOR + i * 32;
        let mut e = [0u8; 32];
        e.copy_from_slice(&self.image[at..at + 32]);
        e
    }

    fn put_entry(&mut self, i: usize, e: &[u8; 32]) {
        let at = PART_LBA * SECTOR + DATA_SECTOR * SECTOR - ROOT_SECTORS * SECTOR + i * 32;
        self.image[at..at + 32].copy_from_slice(e);
    }

    /// `(slot, first cluster, size)` for an 8.3 name already uppercased.
    fn find(&self, bare: &[u8; 11]) -> Option<(usize, u16, u32)> {
        for i in 0..ROOT_ENTRIES {
            let e = self.entry(i);
            if e[0] == 0x00 {
                return None;
            }
            if e[0] == 0xE5 || e[11] & 0x18 != 0 {
                continue;
            }
            if &e[0..11] == bare {
                let first = u16::from_le_bytes([e[26], e[27]]);
                let size = u32::from_le_bytes([e[28], e[29], e[30], e[31]]);
                return Some((i, first, size));
            }
        }
        None
    }

    fn free_slot(&self) -> Result<usize> {
        for i in 0..ROOT_ENTRIES {
            if self.entry(i)[0] == 0x00 || self.entry(i)[0] == 0xE5 {
                return Ok(i);
            }
        }
        Err(failure(Class::Unavailable, "the mailbox root is full"))
    }

    /// The first cluster with a free FAT entry, marking it used.
    fn take_cluster(&mut self) -> Result<u16> {
        for n in 2..(CLUSTERS as u16 + 2) {
            if self.fat(n) == 0 {
                self.set_fat(n, 0xFFFF);
                return Ok(n);
            }
        }
        Err(failure(Class::Unavailable, "the mailbox volume is full"))
    }

    fn write_entry(&mut self, slot: usize, bare: &[u8; 11], case: u8, data: &[u8]) -> Result<()> {
        let mut e = [0u8; 32];
        e[0..11].copy_from_slice(bare);
        e[11] = 0x20; // archive
        e[12] = case;
        // The FAT time is 1980-01-01: a fixed value, not the clock of the
        // host, so that the image is the same byte for byte.
        e[14] = 0x00;
        e[15] = 0x00;
        e[16] = 0x21;
        e[22] = 0x00;
        e[23] = 0x00;
        e[24] = 0x21;
        // A size of more than 4 GiB does not fit in the entry; the volume
        // is 16 MiB, so it is full first. No truncation on a 64-bit host.
        let size = u32::try_from(data.len()).map_err(|_| failure(Class::Unavailable, "the mailbox volume is full"))?;
        if size == 0 {
            e[26] = 0;
            e[27] = 0;
            self.put_entry(slot, &e);
            return Ok(());
        }
        let first = self.take_cluster()?;
        e[26] = (first & 0xFF) as u8;
        e[27] = (first >> 8) as u8;
        e[28..32].copy_from_slice(&size.to_le_bytes());
        let mut n = first;
        let mut off = 0usize;
        loop {
            let at = self.cluster_at(n);
            let take = core::cmp::min(SECTORS_PER_CLUSTER * SECTOR, data.len() - off);
            self.image[at..at + take].copy_from_slice(&data[off..off + take]);
            off += take;
            if off >= data.len() {
                self.set_fat(n, 0xFFFF);
                break;
            }
            let next = self.take_cluster()?;
            self.set_fat(n, next);
            n = next;
        }
        self.put_entry(slot, &e);
        Ok(())
    }

    /// The bytes of a chain. The guest wrote the volume, so the chain is
    /// not trusted: a cluster outside the volume, a loop, or a size that
    /// the chain does not hold reads as `None`, never as a panic or as an
    /// allocation of the size in the entry.
    fn read_chain(&self, first: u16, size: u32) -> Option<Vec<u8>> {
        let size = usize::try_from(size).ok()?;
        if size > CLUSTERS * SECTORS_PER_CLUSTER * SECTOR {
            return None;
        }
        let mut out = Vec::with_capacity(size);
        let mut seen = vec![false; CLUSTERS + 2];
        let mut n = first;
        while in_volume(n) && out.len() < size {
            if std::mem::replace(&mut seen[usize::from(n)], true) {
                return None;
            }
            let at = self.cluster_at(n);
            let take = core::cmp::min(SECTORS_PER_CLUSTER * SECTOR, size - out.len());
            out.extend_from_slice(&self.image[at..at + take]);
            n = self.fat(n);
        }
        if out.len() != size {
            return None;
        }
        Some(out)
    }

    fn free_chain(&mut self, mut n: u16) {
        // A freed entry becomes 0, so a loop in the chain ends at the
        // first cluster that it meets again.
        while in_volume(n) {
            let next = self.fat(n);
            self.set_fat(n, 0);
            n = next;
        }
    }
}

/// True when `n` is a data cluster of the volume.
fn in_volume(n: u16) -> bool {
    (2..CLUSTERS as u16 + 2).contains(&n)
}

/// The OEM name of [`Fat16::new`] and [`format()`].
pub const OEM: &str = "PODGUEST";

/// The 8 bytes of an OEM name, padded with spaces. More than 8 bytes, or a
/// byte that is not printable ASCII, is a [`Class::Usage`] failure.
pub fn oem_bytes(oem: &str) -> Result<[u8; 8]> {
    if oem.len() > 8 || !oem.bytes().all(|c| c.is_ascii_graphic() || c == b' ') {
        return Err(failure(Class::Usage, format!("{oem:?} is not an OEM name: 8 printable ASCII characters at most")));
    }
    let mut out = [b' '; 8];
    out[..oem.len()].copy_from_slice(oem.as_bytes());
    Ok(out)
}

/// A formatted, empty mailbox image with the OEM name [`OEM`].
pub fn format() -> Vec<u8> {
    let mut oem = [b' '; 8];
    oem.copy_from_slice(OEM.as_bytes());
    format_with(oem)
}

/// A formatted, empty mailbox image with the OEM name `oem`.
pub fn format_with(oem: [u8; 8]) -> Vec<u8> {
    let mut img = vec![0u8; IMAGE_LEN];
    // ------------------------------------------------------------------ MBR
    img[0x1BE] = 0x00; // not bootable
    img[0x1BF] = 0x00; // start CHS, head
    img[0x1C0] = 0x02; // start CHS, sector 1 (1-based) of cylinder 0
    img[0x1C1] = 0x00; // start CHS, cylinder
    img[0x1C2] = PART_TYPE;
    img[0x1C3] = 0xFE; // end CHS, head
    img[0x1C4] = 0xFF; // end CHS, sector
    img[0x1C5] = 0xFF; // end CHS, cylinder
    img[0x1C6..0x1CA].copy_from_slice(&(PART_LBA as u32).to_le_bytes());
    img[0x1CA..0x1CE].copy_from_slice(&(VOLUME_SECTORS as u32).to_le_bytes());
    img[0x1FE] = 0x55;
    img[0x1FF] = 0xAA;

    // ------------------------------------------------------------------ BPB
    let b = PART_LBA * SECTOR;
    img[b..b + 3].copy_from_slice(&[0xEB, 0x3C, 0x90]);
    img[b + 3..b + 11].copy_from_slice(&oem);
    img[b + 11..b + 13].copy_from_slice(&(SECTOR as u16).to_le_bytes());
    img[b + 13] = SECTORS_PER_CLUSTER as u8;
    img[b + 14..b + 16].copy_from_slice(&(RESERVED_SECTORS as u16).to_le_bytes());
    img[b + 16] = NUM_FATS as u8;
    img[b + 17..b + 19].copy_from_slice(&(ROOT_ENTRIES as u16).to_le_bytes());
    img[b + 19..b + 21].copy_from_slice(&(VOLUME_SECTORS as u16).to_le_bytes());
    img[b + 21] = 0xF8; // fixed disk
    img[b + 22..b + 24].copy_from_slice(&(FAT_SECTORS as u16).to_le_bytes());
    img[b + 24..b + 26].copy_from_slice(&32u16.to_le_bytes()); // sectors/track
    img[b + 26..b + 28].copy_from_slice(&64u16.to_le_bytes()); // heads
    img[b + 28..b + 32].copy_from_slice(&(PART_LBA as u32).to_le_bytes()); // hidden
    img[b + 32..b + 36].copy_from_slice(&(VOLUME_SECTORS as u32).to_le_bytes());
    img[b + 36] = 0x80; // drive number
    img[b + 38] = 0x29; // extended boot signature
    img[b + 39..b + 43].copy_from_slice(&0x504F_4442u32.to_le_bytes()); // volume id
    img[b + 43..b + 54].copy_from_slice(&label_bytes());
    img[b + 54..b + 62].copy_from_slice(b"FAT16   ");
    img[b + SECTOR - 2] = 0x55;
    img[b + SECTOR - 1] = 0xAA;

    // ------------------------------------------------------------------ FAT
    let fat = b + RESERVED_SECTORS * SECTOR;
    for f in 0..NUM_FATS {
        let at = fat + f * FAT_SECTORS * SECTOR;
        img[at..at + 2].copy_from_slice(&0xFFF8u16.to_le_bytes());
        img[at + 2..at + 4].copy_from_slice(&0xFFFFu16.to_le_bytes());
    }
    img
}

fn label_bytes() -> [u8; 11] {
    let mut out = [b' '; 11];
    let l = LABEL.as_bytes();
    out[..l.len()].copy_from_slice(l);
    out
}

/// `(the name in 11 upper-case bytes, the case byte)` of an 8.3 name.
fn parse_name(name: &str) -> Result<([u8; 11], u8)> {
    let bad = |why: &str| Err(failure(Class::Usage, format!("{name:?} is not an 8.3 name: {why}")));
    let (base, ext) = match name.split_once('.') {
        Some((b, e)) => (b, e),
        None => (name, ""),
    };
    if ext.contains('.') {
        return bad("more than one dot");
    }
    if base.is_empty() || base.len() > 8 || ext.len() > 3 {
        return bad("8 characters then 3, at most");
    }
    let ok = |s: &str| {
        s.bytes().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(
                    c,
                    b'$' | b'%'
                        | b'\''
                        | b'-'
                        | b'_'
                        | b'@'
                        | b'~'
                        | b'!'
                        | b'#'
                        | b'&'
                        | b'('
                        | b')'
                        | b'{'
                        | b'}'
                        | b'^'
                )
        })
    };
    if !ok(base) || !ok(ext) {
        return bad("a character outside the 8.3 set");
    }
    let mut out = [b' '; 11];
    for (i, c) in base.bytes().enumerate() {
        out[i] = c.to_ascii_uppercase();
    }
    for (i, c) in ext.bytes().enumerate() {
        out[8 + i] = c.to_ascii_uppercase();
    }
    // The "NT case" byte: Windows shows the base in lower case when bit 3 is
    // set, and the extension when bit 4 is set. With it, a name comes back
    // from the volume as the caller wrote it.
    let has_alpha_base = base.bytes().any(|c| c.is_ascii_lowercase());
    let has_alpha_ext = ext.bytes().any(|c| c.is_ascii_lowercase());
    let mut case = 0u8;
    if has_alpha_base && !base.bytes().any(|c| c.is_ascii_uppercase()) {
        case |= 0x08;
    }
    if has_alpha_ext && !ext.bytes().any(|c| c.is_ascii_uppercase()) {
        case |= 0x10;
    }
    Ok((out, case))
}

fn render_name(e: &[u8; 32]) -> String {
    let base = String::from_utf8_lossy(&e[0..8]).trim_end().to_string();
    let ext = String::from_utf8_lossy(&e[8..11]).trim_end().to_string();
    let mut base = base;
    let mut ext = ext;
    if e[12] & 0x08 != 0 {
        base = base.to_ascii_lowercase();
    }
    if e[12] & 0x10 != 0 {
        ext = ext.to_ascii_lowercase();
    }
    if ext.is_empty() { base } else { format!("{base}.{ext}") }
}

#[cfg(test)]
mod tests;
