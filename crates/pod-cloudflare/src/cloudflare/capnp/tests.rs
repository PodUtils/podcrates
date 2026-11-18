use super::*;

/// One segment of `words`, framed.
fn framed(words: &[u64]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(words.len() as u32).to_le_bytes());
    for w in words {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out
}

/// A struct pointer at `at` to a struct at `target`.
fn struct_ptr(at: usize, target: usize, data: u16, ptrs: u16) -> u64 {
    let offset = (target as i64 - at as i64 - 1) as u32 & 0x3fff_ffff;
    ((offset as u64) << 2) | ((data as u64) << 32) | ((ptrs as u64) << 48)
}

/// A struct whose one pointer points to the struct itself: a reader
/// that follows it again and again stops with an error (the depth
/// limit or the read budget) and does not hang.
#[test]
fn a_pointer_loop_ends_with_an_error() {
    // Word 0: the root pointer to the struct at word 1 (0 data, 1 ptr).
    // Word 1: the pointer of that struct, to word 1 again.
    let bytes = framed(&[struct_ptr(0, 1, 0, 1), struct_ptr(1, 1, 0, 1)]);
    let (message, _) = Message::parse(&bytes).unwrap().unwrap();
    let mut s = message.root().unwrap();
    let mut steps = 0;
    let end = loop {
        steps += 1;
        assert!(steps < 10_000, "the loop does not end");
        match s.get_struct(0) {
            Ok(Some(child)) => s = child,
            Ok(None) => panic!("the loop has a null pointer"),
            Err(e) => break e,
        }
    };
    assert!(steps <= MAX_DEPTH as usize + 1, "{steps} steps");
    let text = format!("{end:#}");
    assert!(text.contains("deep") || text.contains("limit") || text.contains("depth"), "{text}");
}

/// The reader refuses a list pointer with a count far above the size of the
/// message, before an allocation of that size.
#[test]
fn a_huge_list_count_is_refused() {
    // A byte list (element size 2) of 2^29 - 1 elements in a 2-word
    // message, and the same count of 8-byte elements (size 5).
    for elem in [2u64, 5] {
        let list = 1 | (elem << 32) | (((1u64 << 29) - 1) << 35);
        let bytes = framed(&[struct_ptr(0, 1, 0, 1), list]);
        let (message, _) = Message::parse(&bytes).unwrap().unwrap();
        let root = message.root().unwrap();
        assert!(root.get_data(0).is_err());
        assert!(root.get_text_list(0).is_err());
        assert!(root.get_struct_list(0).is_err());
    }
    // The framing refuses a segment table that claims 4 GiB.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(Message::parse(&bytes).is_err());
}

/// A case of the fuzz target capnp_message: a composite list of zero-size
/// structs with a very large count. Each element reads no word, so the range
/// check passes. With no charge, the reader makes one reader for each of the
/// 2^24 elements (an allocation of 80 MiB for 96 bytes).
#[test]
fn fuzz_a_zero_size_struct_list_is_charged() {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode("AAAAAA0AAAAAAAAAAQACAAAAAAAAAAAABQAAAJIAAAANAAAAFwAAAGh0dHA6Ly9sb2NhbGhvc3QvAAAAAAAAAAADAAQAAAAAAAAAAFoAAAAJAAAAIgAAAEh0dHBNZXRob2QAAAAAAABHRVQAAAAAAA==")
        .unwrap();
    crate::fuzzing::capnp_message(&bytes);
    // The same shape, made by hand: a tag of 2^29 - 1 elements of 0 words.
    let list = 1 | (7u64 << 32); // composite, 0 words
    let tag = ((1u64 << 29) - 1) << 2;
    let bytes = framed(&[struct_ptr(0, 1, 0, 1), list, tag]);
    let (message, _) = Message::parse(&bytes).unwrap().unwrap();
    let err = message.root().unwrap().get_struct_list(0).unwrap_err();
    assert!(format!("{err:#}").contains("read limit"), "{err:#}");
}

/// A composite list whose tag gives 2^29 elements of 8 words: the product
/// is 2^32 words, 0 in a 32-bit usize. The reader refuses it on each target.
fn overflowing_struct_list() -> Vec<u8> {
    let list = 1 | (7u64 << 32) | (1u64 << 35); // composite, 1 word
    let tag = (1u64 << 29) << 2 | (8u64 << 32);
    framed(&[struct_ptr(0, 1, 0, 1), list, tag, 0])
}

#[test]
fn a_struct_list_whose_size_overflows_is_an_error() {
    let (message, _) = Message::parse(&overflowing_struct_list()).unwrap().unwrap();
    let err = message.root().unwrap().get_struct_list(0).unwrap_err();
    assert!(format!("{err:#}").contains("overrun"), "{err:#}");
    crate::fuzzing::capnp_message(&overflowing_struct_list());
}

#[test]
fn a_struct_round_trips() {
    let mut b = Builder::new();
    let root = b.init_root(3, 4);
    b.set_u8(root, 0, 7);
    b.set_u16(root, 1, 0xbeef);
    b.set_u32(root, 1, 0x1234_5678);
    b.set_u64(root, 1, u64::MAX - 1);
    b.set_bool(root, 130, true);
    b.set_text(root, 0, "hello");
    b.set_data(root, 1, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
    let child = b.init_struct(root, 2, 1, 1);
    b.set_u32(child, 0, 42);
    b.set_text_list(child, 0, &["a".into(), "bc".into()]);
    let items = b.init_struct_list(root, 3, 3, 0, 1);
    for (i, item) in items.iter().enumerate() {
        b.set_text(*item, 0, &format!("item{i}"));
    }
    let bytes = b.to_bytes();
    let (msg, used) = Message::parse(&bytes).unwrap().unwrap();
    assert_eq!(used, bytes.len());
    let r = msg.root().unwrap();
    assert_eq!(r.get_u8(0), 7);
    assert_eq!(r.get_u16(1), 0xbeef);
    assert_eq!(r.get_u32(1), 0x1234_5678);
    assert_eq!(r.get_u64(1), u64::MAX - 1);
    assert!(r.get_bool(130));
    assert!(!r.get_bool(131));
    assert_eq!(r.get_text(0).unwrap(), "hello");
    assert_eq!(r.get_data(1).unwrap(), vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
    let c = r.get_struct(2).unwrap().unwrap();
    assert_eq!(c.get_u32(0), 42);
    assert_eq!(c.get_text_list(0).unwrap(), vec!["a", "bc"]);
    let list = r.get_struct_list(3).unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[2].get_text(0).unwrap(), "item2");
    // Fields outside the sections read as defaults.
    assert_eq!(r.get_u64(9), 0);
    assert!(r.get_struct(9).unwrap().is_none());
}

#[test]
fn a_capability_pointer_round_trips() {
    let mut b = Builder::new();
    let root = b.init_root(0, 1);
    b.set_capability(root, 0, 5);
    let (msg, _) = Message::parse(&b.to_bytes()).unwrap().unwrap();
    assert_eq!(msg.root().unwrap().get_capability(0).unwrap(), Some(5));
}

/// A message from the Cap'n Proto reference encoder (`capnp encode`):
/// a struct with data `0x0102030405060708` and the text "hi".
#[test]
fn reads_a_reference_encoding() {
    let bytes: Vec<u8> = [
        0u32.to_le_bytes().to_vec(),
        4u32.to_le_bytes().to_vec(),
        // root: struct, offset 0, 1 data word, 1 pointer
        0x0001_0001_0000_0000u64.to_le_bytes().to_vec(),
        0x0102_0304_0506_0708u64.to_le_bytes().to_vec(),
        // list pointer: offset 0, byte elements, 3 bytes ("hi\0")
        (1u64 | (2u64 << 32) | (3u64 << 35)).to_le_bytes().to_vec(),
        [b'h', b'i', 0, 0, 0, 0, 0, 0].to_vec(),
    ]
    .concat();
    let (msg, _) = Message::parse(&bytes).unwrap().unwrap();
    let root = msg.root().unwrap();
    assert_eq!(root.get_u64(0), 0x0102_0304_0506_0708);
    assert_eq!(root.get_text(0).unwrap(), "hi");
}

#[test]
fn follows_far_pointers() {
    // Segment 0: a far pointer to word 0 of segment 1, which is a struct
    // pointer to word 1 (offset 0) with one data word.
    let far = 2u64 | (1u64 << 32);
    let landing = 0x0000_0001_0000_0000u64;
    let bytes: Vec<u8> = [
        1u32.to_le_bytes().to_vec(),
        1u32.to_le_bytes().to_vec(),
        2u32.to_le_bytes().to_vec(),
        0u32.to_le_bytes().to_vec(),
        far.to_le_bytes().to_vec(),
        landing.to_le_bytes().to_vec(),
        99u64.to_le_bytes().to_vec(),
    ]
    .concat();
    let (msg, _) = Message::parse(&bytes).unwrap().unwrap();
    assert_eq!(msg.root().unwrap().get_u64(0), 99);
}

#[test]
fn refuses_hostile_messages() {
    // A pointer outside the segment.
    let bytes: Vec<u8> = [
        0u32.to_le_bytes().to_vec(),
        1u32.to_le_bytes().to_vec(),
        (0x0001_0000_0000_0000u64 | (100u64 << 2)).to_le_bytes().to_vec(),
    ]
    .concat();
    let (msg, _) = Message::parse(&bytes).unwrap().unwrap();
    assert!(msg.root().is_err());
    // Too many segments.
    let bytes = 10_000u32.to_le_bytes();
    assert!(Message::parse(&bytes).is_err());
    // A partial message.
    let mut b = Builder::new();
    b.init_root(1, 0);
    let full = b.to_bytes();
    assert!(Message::parse(&full[..full.len() - 1]).unwrap().is_none());
}

#[tokio::test]
async fn reads_framed_messages_from_a_stream() {
    let mut b = Builder::new();
    let root = b.init_root(1, 1);
    b.set_u32(root, 0, 3);
    b.set_text(root, 0, "x");
    let mut stream = b.to_bytes();
    stream.extend_from_slice(&b.to_bytes());
    let mut reader = &stream[..];
    for _ in 0..2 {
        let msg = Message::read(&mut reader).await.unwrap().unwrap();
        assert_eq!(msg.root().unwrap().get_u32(0), 3);
    }
    assert!(Message::read(&mut reader).await.unwrap().is_none());
}
