//! A payload over 128 MiB classifies and stages with bounded memory.
//!
//! The fixture is a sparse file of 200 MiB in the temporary directory: a
//! real ELF header at offset 0, the program headers after it, a section
//! table near the end, and a marker in the last bytes. It takes almost no
//! disk. A counting allocator measures the peak of the bytes that the
//! thread of each test holds: the bound is a few MiB, and a read of the
//! whole file is 200 MiB.
#![cfg(target_os = "linux")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::os::unix::fs::FileExt;
use std::path::PathBuf;

use pod_enter::Names;
use pod_enter::abi::{Elf, HEAD_BYTES, READ_CEILING, interp_of};
use pod_enter::ladder::payload_head;
use pod_enter::memfd::{self, MemfdRefusal, STAGE_CHUNK};

/// The size of the fixture: over the old ceiling of 128 MiB.
const SIZE: u64 = 200 * 1024 * 1024;

/// The most heap bytes that one step may hold over its start. A chunk of
/// the stage and the head are about 1 MiB together.
const BOUND: isize = 4 * 1024 * 1024;

const LOADER: &str = "/lib/ld-musl-x86_64.so.1";
const MARKER: &[u8; 8] = b"pod-end!";

// ------------------------------------------------------------ the counter

struct Counting;

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

/// Count on the thread that allocates. The tests run on parallel threads,
/// so a global count would add the bytes of the other tests.
fn note(d: isize) {
    let _ = LIVE.try_with(|l| {
        let v = l.get() + d;
        l.set(v);
        let _ = PEAK.try_with(|p| p.set(p.get().max(v)));
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            note(l.size() as isize);
        }
        p
    }

    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            note(l.size() as isize);
        }
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        note(-(l.size() as isize));
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            note(new as isize - l.size() as isize);
        }
        q
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// Run `f`, and give the peak of the heap bytes of this thread over the
/// bytes at the start.
fn peak<T>(f: impl FnOnce() -> T) -> (T, isize) {
    let start = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(start));
    let out = f();
    (out, PEAK.with(Cell::get) - start)
}

// ------------------------------------------------------------ the fixture

/// A root file system with one sparse payload of [`SIZE`] bytes at
/// `bin/big`, and the host path of the payload.
///
/// `interp` adds a `PT_INTERP` that names [`LOADER`]. `phoff` places the
/// program header table. `dynsym` adds a `.dynsym` section of that size.
fn fixture(tag: &str, interp: bool, phoff: u64, dynsym: Option<u64>) -> (PathBuf, String) {
    let d = std::env::temp_dir().join(format!("pod-enter-big-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("bin")).unwrap();
    let path = d.join("bin/big");
    let f = std::fs::File::create(&path).unwrap();
    f.set_len(SIZE).unwrap();
    let put = |at: u64, b: &[u8]| f.write_all_at(b, at).unwrap();

    // The ELF header: 64-bit, little-endian, ET_EXEC, this machine.
    let mut h = [0u8; 64];
    h[0..4].copy_from_slice(b"\x7fELF");
    h[4] = 2;
    h[5] = 1;
    h[6] = 1;
    h[16..18].copy_from_slice(&2u16.to_le_bytes());
    h[18..20].copy_from_slice(&pod_hostprobe::binfmt::SELF_MACHINE.to_le_bytes());
    h[32..40].copy_from_slice(&phoff.to_le_bytes());
    h[52..54].copy_from_slice(&64u16.to_le_bytes());
    h[54..56].copy_from_slice(&56u16.to_le_bytes());
    h[56..58].copy_from_slice(&(1 + u16::from(interp)).to_le_bytes());

    // The program headers: PT_LOAD, then PT_INTERP with its path at 4096.
    let mut ph = [0u8; 112];
    ph[0..4].copy_from_slice(&1u32.to_le_bytes());
    ph[56..60].copy_from_slice(&3u32.to_le_bytes());
    ph[64..72].copy_from_slice(&4096u64.to_le_bytes());
    put(phoff, &ph[..if interp { 112 } else { 56 }]);
    put(4096, format!("{LOADER}\0").as_bytes());

    // The section table near the end: null, .shstrtab, .note.go.buildid,
    // and .dynsym where asked.
    let names = b"\0.shstrtab\0.note.go.buildid\0.dynsym\0";
    let strtab = SIZE - 64 * 1024;
    let shoff = strtab + 4096;
    put(strtab, names);
    let mut sh = [0u8; 64 * 4];
    let mut entry = |i: usize, name: u32, kind: u32, off: u64, size: u64| {
        let e = &mut sh[i * 64..(i + 1) * 64];
        e[0..4].copy_from_slice(&name.to_le_bytes());
        e[4..8].copy_from_slice(&kind.to_le_bytes());
        e[24..32].copy_from_slice(&off.to_le_bytes());
        e[32..40].copy_from_slice(&size.to_le_bytes());
    };
    entry(1, 1, 3, strtab, names.len() as u64);
    entry(2, 11, 7, 0, 0);
    let count: u16 = if let Some(size) = dynsym {
        entry(3, 28, 11, 8192, size);
        4
    } else {
        3
    };
    put(shoff, &sh[..usize::from(count) * 64]);
    h[40..48].copy_from_slice(&shoff.to_le_bytes());
    h[58..60].copy_from_slice(&64u16.to_le_bytes());
    h[60..62].copy_from_slice(&count.to_le_bytes());
    h[62..64].copy_from_slice(&1u16.to_le_bytes());
    put(0, &h);
    put(SIZE - 8, MARKER);
    (d, path.to_string_lossy().into_owned())
}

fn root(d: &std::path::Path) -> String {
    d.to_string_lossy().into_owned()
}

// ------------------------------------------------------------ the tests

/// The decision of the entry reads the head only: the size of the file is
/// known, the head has [`HEAD_BYTES`] bytes, and the class is right.
#[test]
fn a_200_mib_payload_classifies_from_its_head() {
    for interp in [false, true] {
        let (d, _) = fixture(if interp { "dyn" } else { "static" }, interp, 64, None);
        let (p, held) = peak(|| payload_head(&root(&d), "big", &["/bin".to_string()]).unwrap());
        assert_eq!(p.len, SIZE);
        assert_eq!(p.head.len() as u64, HEAD_BYTES);
        assert!(held < BOUND, "the head of a {SIZE}-byte payload held {held} bytes");
        match memfd::eligible(&p.head) {
            Ok(()) => assert!(!interp, "a PT_INTERP payload was eligible"),
            Err(MemfdRefusal::HasInterp(i)) => assert!(interp && i == LOADER, "{i}"),
            Err(e) => panic!("{e}"),
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// The reader of the interposer tier reads the tables at the end of a
/// 200 MiB file with bounded memory, and finds the Go marker there.
#[test]
fn a_200_mib_elf_reads_its_tables_with_bounded_memory() {
    let (d, path) = fixture("read", true, 64, None);
    let (elf, held) = peak(|| Elf::read(&path).unwrap());
    assert!(held < BOUND, "Elf::read of a {SIZE}-byte file held {held} bytes");
    assert_eq!(elf.machine, pod_hostprobe::binfmt::SELF_MACHINE);
    assert_eq!(elf.interp.as_deref(), Some(LOADER));
    assert!(elf.sections.iter().any(|s| s == ".note.go.buildid"), "{:?}", elf.sections);
    let _ = std::fs::remove_dir_all(&d);
}

/// A section larger than [`READ_CEILING`] is a refusal that names the
/// bound, and the reader does not allocate it.
#[test]
fn a_section_over_the_read_ceiling_is_refused_unread() {
    let (d, path) = fixture("ceiling", false, 64, Some(READ_CEILING + 1));
    let (e, held) = peak(|| Elf::read(&path).unwrap_err());
    assert!(format!("{e}").contains(&READ_CEILING.to_string()), "{e}");
    assert!(held < BOUND, "the refusal held {held} bytes");
    let _ = std::fs::remove_dir_all(&d);
}

/// The program headers past the head: the head says "unknown", never
/// "static", and the bounded read of the file finds the loader.
#[test]
fn a_header_past_the_head_is_unknown() {
    let (d, path) = fixture("past", true, HEAD_BYTES + 4096, None);
    let p = payload_head(&root(&d), "big", &["/bin".to_string()]).unwrap();
    assert!(interp_of(&path, &p.head).is_err());
    assert!(matches!(memfd::eligible(&p.head), Err(MemfdRefusal::NotElf(_))));
    assert_eq!(Elf::read(&path).unwrap().interp.as_deref(), Some(LOADER));
    let _ = std::fs::remove_dir_all(&d);
}

/// The stage copies 200 MiB into a memfd through one chunk: the memfd has
/// each byte, the callback sees each byte once, in order, and the heap
/// holds about one chunk.
#[test]
fn a_200_mib_payload_stages_through_a_bounded_buffer() {
    // An emulator translates the calls, and its memory is the guest's: a
    // memfd of 200 MiB there tests the emulator, not the stage.
    let set = |v: &str| std::env::var_os(v).is_some();
    if pod_hostprobe::interp::detect(&set).is_emulated() {
        pod_testkit::host::skip_line("an emulator runs this test; the stage of 200 MiB runs on a kernel only");
        return;
    }
    let (d, _) = fixture("stage", false, 64, None);
    let p = payload_head(&root(&d), "big", &["/bin".to_string()]).unwrap();
    let (mut seen, mut biggest, mut tail) = (0u64, 0usize, Vec::new());
    let (fd, held) = peak(|| {
        memfd::stage_file_with(&Names::DEFAULT, p.file(), &mut |c| {
            seen += c.len() as u64;
            biggest = biggest.max(c.len());
            if seen == SIZE {
                tail = c[c.len() - 8..].to_vec();
            }
        })
        .unwrap()
    });
    assert!(held < BOUND, "the stage of a {SIZE}-byte payload held {held} bytes");
    assert_eq!(seen, SIZE);
    assert!(biggest <= STAGE_CHUNK, "a chunk had {biggest} bytes");
    assert_eq!(tail, MARKER);
    // SAFETY: the stage gave this descriptor to the test, which owns it.
    let staged = unsafe { <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(fd as i32) };
    assert_eq!(staged.metadata().unwrap().len(), SIZE);
    let mut b = [0u8; 8];
    staged.read_exact_at(&mut b, SIZE - 8).unwrap();
    assert_eq!(&b, MARKER);
    staged.read_exact_at(&mut b, 0).unwrap();
    assert_eq!(&b[..4], b"\x7fELF");
    let _ = std::fs::remove_dir_all(&d);
}
