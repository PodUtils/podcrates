use std::path::{Path, PathBuf};

use pod_core::Class;
use pod_core::entropy::{Entropy, NoRandom};

use super::{
    BLOCK, BLOCKS_PER_GROUP, Counts, Layout, MAX_BYTES, MIN_BYTES, Names, has_super, write_empty, write_heads,
};

/// Each size in and out of the bounds: the bytes of a disk, or the words
/// of its refusal.
#[test]
fn a_size_out_of_range_or_not_whole_blocks_is_refused_by_name() {
    let cases: [(u64, Result<u64, &str>); 7] = [
        (64 << 20, Ok(16384)),
        (1 << 30, Ok(262_144)),
        ((1 << 44) - 4096, Ok(u64::from(u32::MAX))),
        (1 << 44, Err("16 TiB")),
        (1 << 20, Err("fewer than 64 MiB")),
        ((64 << 20) - 4096, Err("fewer than 64 MiB")),
        ((64 << 20) + 4095, Err("not a whole number of 4096-byte blocks")),
    ];
    for (bytes, want) in cases {
        match (want, Layout::of(bytes)) {
            (Ok(w), Ok(l)) => assert_eq!(l.blocks, w, "{bytes}"),
            (Err(w), Err(f)) => {
                assert_eq!(f.class, Class::Usage, "{bytes}");
                let text = f.to_string();
                assert!(text.contains(w) && text.contains(&bytes.to_string()), "{bytes}: {w:?} is not in {text:?}");
            }
            (w, g) => panic!("{bytes}: want {w:?}, got {g:?}"),
        }
    }
    assert_eq!((MIN_BYTES, MAX_BYTES), (64 << 20, (1 << 44) - 4096));
}

/// The groups with a copy of the superblock: 0, 1 and the powers of 3, 5
/// and 7 (`sparse_super`).
#[test]
fn the_superblock_copies_are_in_the_sparse_groups() {
    let got: Vec<u64> = (0..130).filter(|g| has_super(*g)).collect();
    assert_eq!(got, [0, 1, 3, 5, 7, 9, 25, 27, 49, 81, 125]);
}

/// Each size gives whole groups and a last group that holds its metadata,
/// and the free blocks are the size less the metadata.
#[test]
fn a_layout_has_room_in_each_group() {
    for (bytes, groups, blocks) in [
        (64u64 << 20, 1u64, 16384u64),
        (1 << 30, 8, 262_144),
        // A rest of 10 blocks after 4 groups holds no group: it is left out.
        ((512 << 20) + 10 * 4096, 4, 131_072),
        (MAX_BYTES, 131_072, u64::from(u32::MAX)),
    ] {
        let l = Layout::of(bytes).unwrap();
        assert_eq!((l.groups, l.blocks), (groups, blocks), "{bytes}");
        assert!(l.free_blocks() < l.blocks, "{bytes}");
        assert!(l.free_blocks() > l.blocks * 9 / 10, "{bytes}: {l:?}");
        assert_eq!(l.inodes_per_group % (BLOCK / 256), 0, "{bytes}");
        assert!(l.inodes() <= u64::from(u32::MAX), "{bytes}");
    }
}

/// `s_block_group_nr` has 16 bits: a copy past group 65535 records 65535,
/// as mke2fs does, and not the low 16 bits of its number.
#[test]
fn a_superblock_copy_past_group_65535_records_65535() {
    let l = Layout::of(MAX_BYTES).unwrap();
    let names = Names { uuid: [1; 16], hash_seed: [2; 16], label: String::new() };
    for (g, want) in [(1u64, 1u16), (59_049, 59_049), (78_125, 0xffff), (117_649, 0xffff)] {
        assert!(has_super(g) && g < l.groups, "{g}");
        let sb = super::blocks::superblock(&l, g, &names, &[0; 16], &[], 0);
        assert_eq!(u16::from_le_bytes([sb[90], sb[91]]), want, "group {g}");
    }
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pod-guest-ext4-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A new disk of `bytes` in `dir`, as a caller makes it.
fn make(dir: &Path, bytes: u64) -> PathBuf {
    let path = dir.join(format!("disk-{bytes}.raw"));
    let _ = std::fs::remove_file(&path);
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).unwrap();
    write_empty(&file, bytes, &Names::random("pod-guest").unwrap()).unwrap();
    path
}

fn le32(b: &[u8], at: usize) -> u64 {
    u64::from(u32::from_le_bytes(b[at..at + 4].try_into().unwrap()))
}

fn le16(b: &[u8], at: usize) -> u64 {
    u64::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

/// The disk is a sparse file of the size: its metadata take some blocks of
/// the host, and a file that is not empty is refused.
#[test]
fn the_disk_is_a_sparse_file_of_the_size() {
    use std::os::unix::fs::MetadataExt;
    let d = scratch("sparse");
    let p = make(&d, 64 << 20);
    let m = std::fs::metadata(&p).unwrap();
    assert_eq!(m.len(), 64 << 20);
    assert!(m.blocks() * 512 < 1 << 20, "{} bytes on the host", m.blocks() * 512);
    let sb = std::fs::read(&p).unwrap();
    assert_eq!(&sb[1024 + 56..1024 + 58], &[0x53, 0xef], "the ext4 magic");
    let again = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
    let f = write_empty(&again, 64 << 20, &Names::random("").unwrap()).unwrap_err();
    assert_eq!(f.class, Class::Usage);
    assert!(f.to_string().contains("67108864 bytes"), "{f}");
    let _ = std::fs::remove_dir_all(&d);
}

/// The UUID, the hash seed and the label are in the superblock and in its
/// copy in group 1; a label of more than 16 bytes is refused by name.
#[test]
fn the_names_are_in_the_superblock() {
    let d = scratch("names");
    let path = d.join("disk.raw");
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).unwrap();
    let names = Names { uuid: [0x11; 16], hash_seed: [0x22; 16], label: "sixteen-bytes-ok".into() };
    write_empty(&file, 256 << 20, &names).unwrap();
    let disk = std::fs::read(&path).unwrap();
    for at in [1024usize, 32768 * 4096] {
        assert_eq!(&disk[at + 104..at + 120], &[0x11; 16], "the UUID at {at}");
        assert_eq!(&disk[at + 120..at + 136], b"sixteen-bytes-ok", "the label at {at}");
        assert_eq!(&disk[at + 236..at + 252], &[0x22; 16], "the hash seed at {at}");
    }
    for label in ["seventeen-bytes-x", "a\0b"] {
        let f = Names::random(label).unwrap_err();
        assert_eq!(f.class, Class::Usage, "{label:?}");
        assert!(f.to_string().contains(&format!("{label:?}")), "{f}");
        let bad = Names { label: label.into(), ..names.clone() };
        let empty = std::fs::OpenOptions::new().write(true).create_new(true).open(d.join(label.len().to_string()));
        assert_eq!(write_empty(&empty.unwrap(), 64 << 20, &bad).unwrap_err().class, Class::Usage);
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// A random UUID is of version 4; a source with no random bytes is a
/// failure, never a fixed UUID.
#[test]
fn a_source_with_no_random_bytes_is_a_failure() {
    struct Dry;
    impl Entropy for Dry {
        fn fill(&mut self, _: &mut [u8]) -> Result<(), NoRandom> {
            Err(NoRandom)
        }
    }
    assert_eq!(Names::from_entropy(&mut Dry, "x").unwrap_err().class, Class::Unavailable);
    let (a, b) = (Names::random("x").unwrap(), Names::random("x").unwrap());
    assert_ne!(a.uuid, b.uuid);
    assert_eq!((a.uuid[6] >> 4, a.uuid[8] >> 6), (4, 2));
}

/// The free counts of the superblock and of each group descriptor are the
/// clear bits of the bitmaps. It holds with no `e2fsck` on the host.
#[test]
fn the_free_counts_agree_with_the_bitmaps() {
    let d = scratch("counts");
    for bytes in [64u64 << 20, (1 << 30) + (100 << 20)] {
        let disk = std::fs::read(make(&d, bytes)).unwrap();
        let sb = &disk[1024..2048];
        let (blocks, per_group, ipg) = (le32(sb, 4), le32(sb, 32), le32(sb, 40));
        let groups = blocks.div_ceil(per_group);
        let (mut free_blocks, mut free_inodes) = (0, 0);
        for g in 0..groups {
            let e = &disk[(4096 + g * 32) as usize..][..32];
            let clear = |block: u64, valid: u64| {
                let b = &disk[(block * 4096) as usize..][..4096];
                (0..valid).filter(|bit| b[(bit / 8) as usize] & (1 << (bit % 8)) == 0).count() as u64
            };
            let valid = (blocks - g * per_group).min(per_group);
            assert_eq!(le16(e, 12), clear(le32(e, 0), valid), "{bytes}: the free blocks of group {g}");
            assert_eq!(le16(e, 14), clear(le32(e, 4), ipg), "{bytes}: the free inodes of group {g}");
            free_blocks += le16(e, 12);
            free_inodes += le16(e, 14);
        }
        assert_eq!(le32(sb, 12), free_blocks, "{bytes}: the free blocks of the superblock");
        assert_eq!(le32(sb, 16), free_inodes, "{bytes}: the free inodes of the superblock");
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// `Layout::fit` gives groups that hold the data blocks beside their
/// metadata and the inodes, with no block to spare past the metadata of the
/// last group; more than 32-bit block numbers hold is refused by name.
#[test]
fn a_fit_layout_holds_the_data_and_the_inodes_and_no_more() {
    for (data, inodes) in [(0u64, 11u64), (2, 11), (30_000, 100), (40_002, 20_000), (1_000_000, 300_000), (10, 70_000)]
    {
        let l = Layout::fit(data, inodes).unwrap();
        let meta: u64 = (0..l.groups).map(|g| l.overhead(g)).sum();
        let last = l.groups - 1;
        assert!(meta + data <= l.blocks, "{data} {inodes}: {l:?}");
        assert_eq!(l.blocks, (meta + data).max(l.group_start(last) + l.overhead(last) + 1), "{data} {inodes}: {l:?}");
        assert_eq!(l.groups, l.blocks.div_ceil(BLOCKS_PER_GROUP), "{data} {inodes}: {l:?}");
        assert!(l.inodes() >= inodes && l.inodes_per_group <= 8 * BLOCK, "{data} {inodes}: {l:?}");
        assert_eq!(l.inodes_per_group % (BLOCK / 256), 0, "{data} {inodes}");
    }
    let f = Layout::fit(u64::from(u32::MAX), 11).unwrap_err();
    assert_eq!(f.class, Class::Usage);
    assert!(f.to_string().contains("16 TiB") && f.to_string().contains("4294967295 blocks"), "{f}");
}

/// The heads carry the counts that the caller gives: each descriptor its
/// group's, and the superblock their sums. Counts that are not one for each
/// group, or that a group cannot hold, are refused.
#[test]
fn the_heads_carry_the_counts_of_the_caller() {
    let d = scratch("heads");
    let l = Layout::fit(40_002, 20_000).unwrap();
    assert_eq!(l.groups, 2, "{l:?}");
    let path = d.join("heads.raw");
    let file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path).unwrap();
    file.set_len(l.blocks * BLOCK).unwrap();
    let names = Names { uuid: [3; 16], hash_seed: [4; 16], label: "heads".into() };
    let counts =
        [Counts { free_blocks: 7, free_inodes: 9, dirs: 5 }, Counts { free_blocks: 3, free_inodes: 1, dirs: 0 }];
    write_heads(&file, &l, &names, &counts, 0).unwrap();
    let disk = std::fs::read(&path).unwrap();
    for (sb, gdt) in
        [(1024usize, 4096usize), ((BLOCKS_PER_GROUP * BLOCK) as usize, ((BLOCKS_PER_GROUP + 1) * BLOCK) as usize)]
    {
        assert_eq!((le32(&disk[sb..], 12), le32(&disk[sb..], 16)), (10, 10), "the sums at {sb}");
        for (g, c) in counts.iter().enumerate() {
            let e = &disk[gdt + g * 32..][..32];
            assert_eq!(
                (le16(e, 12), le16(e, 14), le16(e, 16)),
                (c.free_blocks, c.free_inodes, c.dirs),
                "group {g} at {gdt}"
            );
        }
    }
    let too_many = Counts { free_blocks: BLOCKS_PER_GROUP, ..counts[0] };
    for bad in
        [&counts[..1], &[too_many, counts[1]][..], &[counts[0], Counts { dirs: l.inodes_per_group, ..counts[1] }]]
    {
        let f = write_heads(&file, &l, &names, bad, 0).unwrap_err();
        assert_eq!(f.class, Class::Usage, "{bad:?}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// `e2fsck` of the host: in `PATH`, else in `/usr/sbin` or `/sbin`, which a
/// user that is not root often has not in `PATH`.
fn e2fsck() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(["/usr/sbin", "/sbin"].map(PathBuf::from))
        .map(|d| d.join("e2fsck"))
        .find(|p| p.is_file())
}

/// e2fsck of e2fsprogs finds no fault in a disk of each shape, with its
/// copies of the superblock (9 groups: copies in 1, 3, 5 and 7). The test
/// needs `e2fsck` of the host, which the runners of CI have; it skips with
/// its reason where the host has none, and the tests above still run.
#[test]
fn e2fsck_finds_no_fault_in_the_disk() {
    let Some(fsck) = e2fsck() else {
        pod_testkit::host::skip_line("this host has no e2fsck in PATH, /usr/sbin or /sbin");
        return;
    };
    let d = scratch("fsck");
    for bytes in [64u64 << 20, (1 << 30) + (100 << 20)] {
        let p = make(&d, bytes);
        let out = std::process::Command::new(&fsck).args(["-f", "-n"]).arg(&p).output().unwrap();
        assert!(
            out.status.success(),
            "{bytes}: e2fsck {}\n{}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // A fit layout with the two directory blocks as its data: no free
    // block in one group, and a short last group in two.
    for (data, inodes) in [(2u64, 11u64), (40_002, 20_000)] {
        let l = Layout::fit(data, inodes).unwrap();
        let p = d.join(format!("fit-{data}.raw"));
        let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&p).unwrap();
        file.set_len(l.blocks * BLOCK).unwrap();
        let names = Names::random("fit").unwrap();
        super::blocks::write(&file, &l, &names, &super::label_bytes("fit").unwrap(), 0).unwrap();
        let out = std::process::Command::new(&fsck).args(["-f", "-n"]).arg(&p).output().unwrap();
        assert!(
            out.status.success(),
            "fit {data} {inodes}: e2fsck {}\n{}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}
