use super::*;

#[test]
fn a_formatted_volume_declares_its_geometry_and_its_label() {
    let img = format();
    assert_eq!(img.len(), IMAGE_LEN);
    assert_eq!(u16::from_le_bytes([img[0x1FE], img[0x1FF]]), 0xAA55);
    assert_eq!(img[0x1C2], PART_TYPE);
    assert_eq!(
        u32::from_le_bytes([img[0x1C6], img[0x1C7], img[0x1C8], img[0x1C9]]),
        PART_LBA as u32
    );
    let b = PART_LBA * SECTOR;
    assert_eq!(&img[b + 54..b + 62], b"FAT16   ");
    assert_eq!(img[b + 13], SECTORS_PER_CLUSTER as u8);
    assert_eq!(img[b + 16], NUM_FATS as u8);
    assert!(Fat16::from_image(img).unwrap().is_mailbox());
}

#[test]
fn the_built_volume_declares_the_geometry_it_was_pinned_to() {
    // The const block above already pins the arithmetic at compile
    // time; what this checks is the artifact: the bytes a guest
    // actually mounts carry the same geometry.
    let m = Fat16::new();
    let img = m.image();
    let base = PART_LBA * SECTOR;
    let bpb = |o: usize| u16::from_le_bytes([img[base + o], img[base + o + 1]]);
    assert_eq!(bpb(11), SECTOR as u16, "bytes per sector");
    assert_eq!(
        img[base + 13] as usize,
        SECTORS_PER_CLUSTER,
        "sectors per cluster"
    );
    assert_eq!(bpb(14) as usize, RESERVED_SECTORS, "reserved sectors");
    assert_eq!(img[base + 16] as usize, NUM_FATS, "two FATs");
    assert_eq!(bpb(17) as usize, ROOT_ENTRIES, "root entries");
    assert_eq!(bpb(22) as usize, FAT_SECTORS, "sectors per FAT");
    let mut padded = [b' '; 11];
    padded[..LABEL.len()].copy_from_slice(LABEL.as_bytes());
    assert_eq!(&img[base + 43..base + 54], &padded, "volume label");
    assert_eq!([img[510], img[511]], [0x55, 0xAA], "MBR signature");
    assert_eq!(img[446 + 4], PART_TYPE, "partition type");
}

#[test]
fn a_file_round_trips_through_the_volume() {
    let mut v = Fat16::new();
    v.put("WQMARK.TXT", b"podvm").unwrap();
    assert_eq!(v.get("WQMARK.TXT").as_deref(), Some(&b"podvm"[..]));
    assert!(v.is_mailbox());
}

#[test]
fn a_file_that_spans_clusters_comes_back_whole() {
    let mut v = Fat16::new();
    let big: Vec<u8> = (0..(SECTORS_PER_CLUSTER * SECTOR * 3 + 7))
        .map(|i| (i % 251) as u8)
        .collect();
    v.put("BIG.BIN", &big).unwrap();
    assert_eq!(v.get("BIG.BIN").as_deref(), Some(&big[..]));
}

#[test]
fn an_empty_file_has_no_cluster_and_reads_back_empty() {
    let mut v = Fat16::new();
    v.put("EMPTY.TXT", b"").unwrap();
    assert_eq!(v.get("EMPTY.TXT").as_deref(), Some(&b""[..]));
}

#[test]
fn rewriting_a_name_replaces_the_bytes_and_frees_the_old_chain() {
    let mut v = Fat16::new();
    v.put("WQOUT.TXT", &vec![b'a'; 20_000]).unwrap();
    // Five clusters, so cluster 2 is the head of a chain, not an end.
    assert_eq!(v.fat(2), 3);
    assert_ne!(v.fat(3), 0);
    v.put("WQOUT.TXT", b"short").unwrap();
    assert_eq!(v.get("WQOUT.TXT").as_deref(), Some(&b"short"[..]));
    // The long file's clusters past the first were released and cluster
    // 2 is the whole file again, so its entry is the end of a chain.
    assert_eq!(v.fat(2), 0xFFFF);
    assert_eq!(v.fat(3), 0);
}

#[test]
fn the_root_lists_what_was_written() {
    let mut v = Fat16::new();
    v.put("WQMARK.TXT", b"x").unwrap();
    v.put("WQGO.TXT", b"token").unwrap();
    let mut l = v.list();
    l.sort();
    assert_eq!(l, vec!["WQGO.TXT", "WQMARK.TXT"]);
}

#[test]
fn names_are_8_3_or_they_are_refused_naming_themselves() {
    let mut v = Fat16::new();
    assert!(v.put("WQCODE.TXT", b"0 t").is_ok());
    assert!(v.put("ADIR", b"x").is_ok());
    // 8 then 3 at most; a base over 8 or an extension over 3 is refused.
    assert!(v.put("toolongname.txt", b"x").is_err());
    assert!(v.put("name.long", b"x").is_err());
    assert!(v.put("a.b.c", b"x").is_err());
    assert!(v.put("bad name.txt", b"x").is_err());
    assert!(v.put("", b"x").is_err());
    let e = v.put("toolongname.txt", b"x").unwrap_err();
    assert!(e.contains("toolongname.txt"), "{e}");
}

#[test]
fn a_lowercase_name_round_trips_as_written() {
    let mut v = Fat16::new();
    v.put("wqcmd.cmd", b"x").unwrap();
    assert!(v.list()[0].contains("wqcmd"), "{:?}", v.list());
    assert!(v.get("wqcmd.cmd").is_some());
    // and it is the same file as the uppercase spelling: FAT is
    // case-insensitive, which the reader must be too.
    assert!(v.get("WQCMD.CMD").is_some());
}

#[test]
fn a_missing_name_reads_as_none_and_a_built_image_adopts() {
    assert!(Fat16::from_image(vec![0u8; IMAGE_LEN]).is_some());
    let mut v = Fat16::new();
    v.put("WQMARK.TXT", b"x").unwrap();
    assert!(v.get("NOTHERE.TXT").is_none());
}
