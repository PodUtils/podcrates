//! TODO/machine.md T-1968: the sized disk of a machine guest.

use super::ext::{has_super, Layout, BLOCK};
use super::*;

/// One table for the kept value of `--storage-opt size=` and each refusal
/// of a guest that cannot have its disk: the value, a guest of the pinned
/// x86_64 kernel or not, the pin or `PODVM_MACHINE_KERNEL`, and the bytes
/// or the words of the refusal.
#[test]
fn the_size_of_a_disk_holds_or_refuses_by_name() {
    type Want = Result<u64, &'static str>;
    let cases: [(&str, bool, bool, Want); 9] = [
        ("67108864B", true, true, Ok(64 << 20)),
        ("1073741824B", true, true, Ok(1 << 30)),
        ("17592186040320B", true, true, Ok((1 << 44) - 4096)),
        ("17592186044416B", true, true, Err("16 TiB")),
        ("67108864B", false, true, Err("linux/arm64")),
        ("67108864B", true, false, Err("PODVM_MACHINE_KERNEL")),
        ("1048576B", true, true, Err("hold no ext4 file system")),
        ("64m", true, true, Err("not a count of bytes")),
        ("B", true, true, Err("not a count of bytes")),
    ];
    for (value, amd64, pinned, want) in cases {
        let got = match bytes_of(value) {
            None => Err(format!("--storage-opt size={value}: not a count of bytes")),
            Some(b) => refusal(amd64, pinned, b).map_or(Ok(b), Err),
        };
        match (want, got) {
            (Ok(w), Ok(g)) => assert_eq!(w, g, "{value}"),
            (Err(w), Err(g)) => {
                assert!(g.contains(w), "{value}: {w:?} is not in {g:?}");
                assert!(g.starts_with("--storage-opt size="), "{value}: {g}");
            }
            (w, g) => panic!("{value} amd64={amd64} pinned={pinned}: want {w:?}, got {g:?}"),
        }
    }
    assert_eq!(of_value(None, false), Ok(None), "no flag is no disk");
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
        ((64 << 20) + 4095, 1, 16384),
        (1 << 30, 8, 262_144),
        // A rest of 10 blocks after 4 groups holds no group: it is left out.
        ((512 << 20) + 10 * 4096, 4, 131_072),
    ] {
        let l = Layout::of(bytes).unwrap();
        assert_eq!((l.groups, l.blocks), (groups, blocks), "{bytes}");
        assert!(l.free_blocks() < l.blocks, "{bytes}");
        assert!(l.free_blocks() > l.blocks * 9 / 10, "{bytes}: {l:?}");
        assert_eq!(l.inodes_per_group % (BLOCK / 256), 0, "{bytes}");
    }
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("podvm-t1968-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The disk is a sparse file of the size: its metadata take some blocks of
/// the host, and a second make replaces it.
#[test]
fn the_disk_is_a_sparse_file_of_the_size() {
    use std::os::unix::fs::MetadataExt;
    let d = scratch("sparse");
    let p = make(&d, 64 << 20).unwrap();
    let m = std::fs::metadata(&p).unwrap();
    assert_eq!(m.len(), 64 << 20);
    assert!(
        m.blocks() * 512 < 1 << 20,
        "{} bytes on the host",
        m.blocks() * 512
    );
    let sb = std::fs::read(&p).unwrap();
    assert_eq!(&sb[1024 + 56..1024 + 58], &[0x53, 0xef], "the ext4 magic");
    assert_eq!(
        make(&d, 64 << 20).unwrap(),
        p,
        "a second make replaces the disk"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// The plant: e2fsck of e2fsprogs finds no fault in a disk of each shape,
/// with its copies of the superblock (9 groups: copies in 1, 3, 5 and 7).
/// The test needs `e2fsck` of the host, which the gate's runner has.
#[test]
fn e2fsck_finds_no_fault_in_the_disk() {
    let Some(fsck) = ["/usr/sbin/e2fsck", "/sbin/e2fsck"]
        .into_iter()
        .find(|p| Path::new(p).exists())
    else {
        eprintln!("SKIP: no e2fsck on this host; the layout tests above still run");
        return;
    };
    let d = scratch("fsck");
    for bytes in [64u64 << 20, (1 << 30) + (100 << 20)] {
        let p = make(&d, bytes).unwrap();
        let out = std::process::Command::new(fsck)
            .args(["-f", "-n"])
            .arg(&p)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{bytes}: e2fsck {}\n{}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// The init of a run with a disk: its mounts, then the exec of `podvm
/// guest-disk`; the rest of the script is the script of the root on the
/// disk, and the failure ends the run with 125 and the reason.
#[test]
fn the_init_of_a_disk_execs_guest_disk_after_its_mounts() {
    let head = super::super::init::head();
    let script = format!("{head}echo body\n");
    let load: Vec<String> = ["crc16", "jbd2", "ext4"].map(String::from).to_vec();
    let (first, rest) = split(&script, &head, &load).unwrap();
    assert!(first.starts_with(&head));
    assert!(first.ends_with(&format!(
        "exec /podvm/podvm guest-disk --load crc16,jbd2,ext4 {NEXT} {FAIL}\n"
    )));
    assert_eq!(rest, "#!/bin/sh\necho body\n");
    assert!(split("#!/bin/sh\necho other\n", &head, &load).is_err());
    let bad = ["ext4 ; reboot".to_string()];
    assert!(split(&script, &head, &bad)
        .unwrap_err()
        .contains("not the name of a module"));
    let fail = super::super::init::disk_fail("N1");
    for want in ["PODVM-READY-N1", "PODVM-EXIT-N1 125", ERR, "T-1968"] {
        assert!(fail.contains(want), "{want} is not in {fail}");
    }
    let (same, none) = init(script.clone(), &[], "N1").unwrap();
    assert_eq!((same.as_str(), none.len()), (script.as_str(), 0));
    let a = argv(Path::new("/s,t/disk.raw"));
    assert!(a[1].contains("file=/s,,t/disk.raw,"), "{}", a[1]);
    assert!(a[3].ends_with(&format!("serial={SERIAL}")), "{}", a[3]);
}
