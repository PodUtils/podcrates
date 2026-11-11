//! The live test of the modloop reader (T-206): the modloop of Alpine's virt
//! kernel, which `live.yml` downloads. `POD_GUEST_MODLOOP` names the file.
//! The test walks the whole image (each directory, each file, the id and
//! export tables), so that each xz stream of the image decodes, and counts
//! the streams that the reader read, with those that carry the x86 BCJ
//! filter. Then it takes the module set of podvm's network lab
//! (PodUtils/podvm#260). The kernel release is the directory of
//! `modules/` that holds a `modules.dep`, so a new release of Alpine needs
//! no change.
#![cfg(target_os = "linux")]

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom};
use std::rc::Rc;

use pod_guest::squashfs::{Image, Kind};
use sha2::{Digest, Sha256};

/// The names of podvm's network lab.
const NAMES: [&str; 7] = ["nf_tables", "nft_nat", "sch_netem", "wireguard", "tun", "veth", "nf_nat"];
/// The magic of an xz stream.
const XZ_MAGIC: &[u8] = b"\xfd7zXZ\0";
/// The filter id of x86 BCJ in an xz block header.
const FILTER_X86: u64 = 0x04;

/// A source that records the place of each read.
struct Recording<R> {
    inner: R,
    at: u64,
    reads: Rc<RefCell<BTreeSet<u64>>>,
}

impl<R: Read> Read for Recording<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reads.borrow_mut().insert(self.at);
        let n = self.inner.read(buf)?;
        self.at += n as u64;
        Ok(n)
    }
}

impl<R: Seek> Seek for Recording<R> {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.at = self.inner.seek(to)?;
        Ok(self.at)
    }
}

/// A variable-length integer of xz at `at`, and the place after it.
fn vli(b: &[u8], mut at: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for shift in (0..63).step_by(7) {
        let byte = *b.get(at)?;
        at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some((value, at));
        }
    }
    None
}

/// The filter ids of the first block of the xz stream `s`.
fn filters(s: &[u8]) -> Option<Vec<u64>> {
    let flags = *s.get(13)?;
    let mut at = 14;
    for bit in [0x40, 0x80] {
        if flags & bit != 0 {
            at = vli(s, at)?.1;
        }
    }
    let mut ids = Vec::new();
    for _ in 0..=(flags & 3) {
        let (id, next) = vli(s, at)?;
        let (props, next) = vli(s, next)?;
        ids.push(id);
        at = next + props as usize;
    }
    Some(ids)
}

#[test]
#[ignore = "needs the modloop that live.yml downloads (POD_GUEST_MODLOOP)"]
fn the_modloop_of_alpine_reads_whole_and_gives_the_lab_set() {
    let path = std::env::var("POD_GUEST_MODLOOP").expect("POD_GUEST_MODLOOP names the modloop");
    let image_bytes = std::fs::read(&path).expect("the modloop");
    let reads = Rc::new(RefCell::new(BTreeSet::new()));
    let src = Recording { inner: std::fs::File::open(&path).expect("the modloop"), at: 0, reads: Rc::clone(&reads) };
    let mut image = Image::open(src).expect("the modloop opens");
    let sb = image.superblock().clone();
    println!("superblock: {sb:?}");

    let (mut files, mut bytes, mut dirs) = (0u64, 0u64, 0u64);
    let mut todo = vec![(String::new(), image.root().expect("the root"))];
    while let Some((at, inode)) = todo.pop() {
        match inode.kind {
            Kind::Directory(d) => {
                dirs += 1;
                for e in image.entries(&d).expect("a listing") {
                    let name = String::from_utf8(e.name).expect("a name");
                    todo.push((format!("{at}/{name}"), image.inode(e.inode).expect("an inode")));
                }
            }
            Kind::File(f) => {
                let data = image.read(&f).unwrap_or_else(|e| panic!("{at}: {e}"));
                files += 1;
                bytes += data.len() as u64;
            }
            _ => {}
        }
    }
    println!("ids: {:?}", image.ids().expect("the id table"));
    for number in 1..=sb.inodes {
        if let Some(reference) = image.export(number).expect("the export table") {
            assert_eq!(image.inode(reference).expect("an exported inode").number, number);
        }
    }
    let (mut streams, mut bcj) = (0, 0);
    for &at in reads.borrow().iter() {
        let s = &image_bytes[at as usize..];
        if s.starts_with(XZ_MAGIC) {
            streams += 1;
            if filters(s).expect("a block header").contains(&FILTER_X86) {
                bcj += 1;
            }
        }
    }
    println!("walk: {dirs} directories, {files} files, {bytes} bytes; {streams} xz streams, {bcj} with x86 BCJ");
    assert!(streams > 0 && bcj > 0, "the walk decoded no stream of the x86 filter");

    let modules = image.lookup("modules").expect("modules/");
    let Kind::Directory(d) = modules.kind else { panic!("modules/ is not a directory") };
    let mut releases = Vec::new();
    for e in image.entries(&d).expect("the listing of modules/") {
        let name = String::from_utf8(e.name).expect("a name");
        if image.file_at(&format!("modules/{name}/modules.dep")).is_ok() {
            releases.push(name);
        }
    }
    assert_eq!(releases.len(), 1, "{releases:?}");
    let release = &releases[0];
    let file = std::fs::File::open(&path).expect("the modloop");
    let set = pod_guest::modloop::modules(file, release, NAMES).expect("the module set");
    for m in &set.modules {
        let digest = Sha256::digest(&m.bytes);
        println!("{:>8} {} {}", m.bytes.len(), hex(&digest[..8]), m.path);
        assert!(m.bytes.starts_with(b"\x7fELF"), "{}", m.path);
        assert!(set.dep.lines().any(|l| l.starts_with(&format!("{}:", m.path))), "{}", m.path);
    }
    for name in NAMES {
        assert!(set.modules.iter().any(|m| m.name == name) || set.builtin.iter().any(|b| b == name), "{name}");
    }
    println!("release {release}: {} modules, {} bytes; built in: {:?}", set.modules.len(), set.size(), set.builtin);
    print!("{}", set.dep);
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
