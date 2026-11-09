//! The mailbox volume: an MBR-partitioned FAT16 image built and read in
//! process. `TODO/milestones.md` T-1112.
//!
//! ⛔ **Why a filesystem writer lives in podvm at all.** The guest agent is
//! `cmd.exe`, and `cmd.exe` can only be handed a file through a volume the
//! Windows driver mounts. The mailbox is therefore the driver's only
//! interface, and building it by shelling out to `mkfs.fat`/`mtools` would
//! make one of podvm's two guest OSes depend on host packages the rest of
//! the tool refuses to depend on. The format is small and frozen; this is
//! that format and nothing more.
//!
//! ⚠ **The geometry is the tested one, not a preferred one.** The volume is
//! 16 MiB behind an MBR partition at LBA 2048, which is what was measured
//! against Validation OS: a bare "superfloppy" with no partition table is
//! not reliably mounted as a *fixed* NVMe disk, and a partition is what the
//! reference implementation presents too. The cluster is 4 KiB because the
//! volume is small and FAT16's cluster count has to stay under 65525.
//!
//! ⚠ **8.3 names only, and that is a property rather than a limitation.**
//! Every file the protocol uses, `WQMARK.TXT`, `WQGO.TXT`, `WQCMD.CMD`,
//! `WQOUT.TXT`, `WQERR.TXT`, `WQCODE.TXT`, `WQAGENT.CMD`, `WA.CMD`,
//! `SETUP.TXT`, is 8.3. A name that is not is refused by [`Fat16::put`]
//! naming it, rather than written under a mangled name the guest would then
//! never find.

/// The sector, which is the volume's and the kernel's unit here.
pub const SECTOR: usize = 512;
/// 4 KiB clusters.
pub const SECTORS_PER_CLUSTER: usize = 8;
/// In the volume, as the BPB records it.
pub const RESERVED_SECTORS: usize = 1;
/// Two FATs, which is what a reader that trusts the second copy expects.
pub const NUM_FATS: usize = 2;
/// 512 root entries, so 32 sectors, which is also FAT16's fixed-root shape.
pub const ROOT_ENTRIES: usize = 512;
/// The volume itself, 16 MiB.
pub const VOLUME_SECTORS: usize = 32768;
/// Sectors per FAT. Pinned rather than computed: the arithmetic below is
/// checked against the cluster count by [`format`], so a wrong pin refuses
/// rather than producing a volume that under- or over-describes itself.
pub const FAT_SECTORS: usize = 16;
/// Where the partition starts. The reference geometry.
pub const PART_LBA: usize = 2048;
/// The MBR partition type for a FAT16 volume of this size.
pub const PART_TYPE: u8 = 0x06;
/// `ROOT_ENTRIES * 32 / SECTOR`.
pub const ROOT_SECTORS: usize = ROOT_ENTRIES * 32 / SECTOR;
/// First data-sector of the volume, in volume-relative sectors.
pub const DATA_SECTOR: usize = RESERVED_SECTORS + NUM_FATS * FAT_SECTORS + ROOT_SECTORS;
/// Clusters the geometry yields.
pub const CLUSTERS: usize = (VOLUME_SECTORS - DATA_SECTOR) / SECTORS_PER_CLUSTER;
/// The whole image, MBR included.
pub const IMAGE_SECTORS: usize = PART_LBA + VOLUME_SECTORS;
/// The whole image in bytes.
pub const IMAGE_LEN: usize = IMAGE_SECTORS * SECTOR;
/// The label the guest agent scans for, written into the BPB.
pub const LABEL: &str = "WQMAILBOX";

/// A compile-time check that the pinned FAT is big enough for the clusters
/// the geometry yields. `(CLUSTERS + 2) * 2` is the FAT in bytes.
const _: () = assert!(CLUSTERS + 2 <= FAT_SECTORS * SECTOR / 2);
const _: () = assert!(CLUSTERS + 2 < 0xFFF0, "FAT16, not FAT32");
// The volume is not an exact multiple of the cluster: the data area keeps
// whatever whole clusters fit and the remainder is slack no file can reach.
const _: () = assert!(DATA_SECTOR + CLUSTERS * SECTORS_PER_CLUSTER <= VOLUME_SECTORS);
const _: () =
    assert!(VOLUME_SECTORS - (DATA_SECTOR + CLUSTERS * SECTORS_PER_CLUSTER) < SECTORS_PER_CLUSTER);

/// One volume in memory: the whole image, MBR and all.
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
    /// A formatted, empty volume.
    pub fn new() -> Self {
        Fat16 { image: format() }
    }

    /// Adopt an image a previous run wrote, so results can be read back
    /// without rebuilding the volume.
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

    /// Write `data` as `name`, replacing any file already there. The name is
    /// uppercased and must be 8.3; anything else is a `String` naming it.
    pub fn put(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
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

    /// Every file name in the root, in directory order.
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

    /// True where the volume declares itself under [`LABEL`], which is the
    /// cheapest way for a reader to be sure it is looking at a mailbox.
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

    fn free_slot(&self) -> Result<usize, String> {
        for i in 0..ROOT_ENTRIES {
            if self.entry(i)[0] == 0x00 || self.entry(i)[0] == 0xE5 {
                return Ok(i);
            }
        }
        Err("the mailbox root is full".to_string())
    }

    /// The first cluster with a free FAT entry, marking it used.
    fn take_cluster(&mut self) -> Result<u16, String> {
        for n in 2..(CLUSTERS as u16 + 2) {
            if self.fat(n) == 0 {
                self.set_fat(n, 0xFFFF);
                return Ok(n);
            }
        }
        Err("the mailbox volume is full".to_string())
    }

    fn write_entry(
        &mut self,
        slot: usize,
        bare: &[u8; 11],
        case: u8,
        data: &[u8],
    ) -> Result<(), String> {
        let mut e = [0u8; 32];
        e[0..11].copy_from_slice(bare);
        e[11] = 0x20; // archive
        e[12] = case;
        // The FAT timestamp is 1980-01-01: a fixed value rather than the
        // host's clock, so the image is reproducible byte for byte.
        e[14] = 0x00;
        e[15] = 0x00;
        e[16] = 0x21;
        e[22] = 0x00;
        e[23] = 0x00;
        e[24] = 0x21;
        let size = data.len() as u32;
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

    fn read_chain(&self, first: u16, size: u32) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(size as usize);
        let mut n = first;
        while (2..0xFFF8).contains(&n) && out.len() < size as usize {
            let at = self.cluster_at(n);
            let take = core::cmp::min(SECTORS_PER_CLUSTER * SECTOR, size as usize - out.len());
            out.extend_from_slice(&self.image[at..at + take]);
            n = self.fat(n);
        }
        if out.len() != size as usize {
            return None;
        }
        Some(out)
    }

    fn free_chain(&mut self, mut n: u16) {
        while (2..0xFFF8).contains(&n) {
            let next = self.fat(n);
            self.set_fat(n, 0);
            n = next;
        }
    }
}

/// A formatted, empty mailbox image.
pub fn format() -> Vec<u8> {
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
    img[b + 3..b + 11].copy_from_slice(b"PODVM   ");
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

/// `(11-byte uppercase name, case byte)` for an 8.3 name.
fn parse_name(name: &str) -> Result<([u8; 11], u8), String> {
    let bad = |why: &str| Err(format!("{name:?} is not an 8.3 name: {why}"));
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
    // The "NT case" byte: Windows displays the base lowercase when bit 3 is
    // set and the extension lowercase when bit 4 is. Storing it means a name
    // round-trips through the volume as the caller wrote it.
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
    if ext.is_empty() {
        base
    } else {
        format!("{base}.{ext}")
    }
}

#[cfg(test)]
mod tests;
