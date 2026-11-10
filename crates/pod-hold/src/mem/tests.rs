use super::*;

fn image(bytes: &[u8], writable: bool) -> Memory {
    Memory::of_fd(memfd(bytes).expect("a memfd"), writable)
}

#[test]
fn a_memfd_stands_in_for_the_memory_of_a_task() {
    let memory = image(b"0123456789", true);
    assert_eq!(memory.read(2, 3).unwrap(), b"234");
    memory.write(4, b"ab").unwrap();
    assert_eq!(memory.read(0, 10).unwrap(), b"0123ab6789");
    assert!(memory.writable());
}

// A read or a write out of the memory fails with EFAULT, with no panic,
// also at the highest addresses.
#[test]
fn a_read_past_the_memory_fails() {
    let memory = image(b"0123456789", true);
    let e = memory.read(8, 4).unwrap_err();
    assert_eq!(e.io_cause().and_then(std::io::Error::raw_os_error), Some(libc::EFAULT), "{e}");
    assert!(memory.read(u64::MAX, 1).is_err());
    assert!(memory.read(u64::MAX - 1, 4).is_err());
    assert!(memory.read(i64::MAX as u64, 2).is_err());
    assert!(memory.write(10, b"x").is_err(), "a sealed memfd does not grow");
    assert!(memory.write(u64::MAX, b"x").is_err());
}

#[test]
fn a_word_is_read_only_inside_the_bytes() {
    let word = size_of::<usize>();
    let mut bytes = vec![0u8; word + 1];
    bytes[1..1 + word].copy_from_slice(&0x1234usize.to_ne_bytes());
    assert_eq!(read_word(&bytes, 1), Some(0x1234));
    assert_eq!(read_word(&bytes, 2), None);
    assert_eq!(read_word(&bytes, usize::MAX), None);
}

// The memory of a live task: this process reads its own bytes, and the
// access of the caller decides the write.
#[test]
fn the_memory_of_a_task_reads_its_bytes() {
    let me = std::process::id();
    let data = *b"the bytes of the task";
    let addr = data.as_ptr() as u64;
    let memory = Memory::open(me, Access::default());
    assert!(memory.raw() >= 0);
    assert_eq!(memory.pid(), me);
    assert_eq!(memory.read(addr, data.len()).unwrap(), data);
    let read_only = Memory::open(me, Access::READ_ONLY);
    assert!(!read_only.writable());
    assert_eq!(read_only.read(addr, 3).unwrap(), b"the");
    assert!(read_only.write(addr, b"x").is_err(), "a read-only memory takes no write");
}
