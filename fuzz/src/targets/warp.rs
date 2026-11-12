//! The bytes of WARP's front door (T-028).

/// The body of a CONNECT-IP stream (`pod_warp::capsule::CapsuleDecoder`;
/// podnet `src/warp/capsule.rs`), pushed in pieces whose sizes the first
/// byte sets. Each packet that comes out is a DATA capsule of 1 to
/// `MAX_CAPSULE_LEN` bytes that encodes back to bytes that decode to it;
/// after an error, nothing is kept and nothing more comes out. The
/// addresses of an ADDRESS_ASSIGN value read with no panic, each prefix
/// with an address of 4 or 16 bytes.
pub fn warp_capsule(data: &[u8]) {
    use pod_warp::capsule::{CAPSULE_DATA, CapsuleDecoder, MAX_CAPSULE_LEN, parse_address_assign, put_capsule};
    let Some((&first, bytes)) = data.split_first() else { return };
    let step = usize::from(first) % 64 + 1;
    let mut decoder = CapsuleDecoder::new();
    let mut taken = 0usize;
    for piece in bytes.chunks(step) {
        decoder.push(piece);
        taken += piece.len();
        while let Some(packet) = decoder.next_packet() {
            assert!(!packet.is_empty() && packet.len() as u64 <= MAX_CAPSULE_LEN, "{} bytes", packet.len());
            let mut again = bytes::BytesMut::new();
            put_capsule(&mut again, CAPSULE_DATA, &packet);
            let mut other = CapsuleDecoder::new();
            other.push(&again);
            assert_eq!(other.next_packet().as_deref(), Some(&packet[..]));
            assert_eq!(other.buffered(), 0);
        }
        assert!(decoder.buffered() <= taken);
        if decoder.error().is_some() {
            assert_eq!(decoder.buffered(), 0);
            decoder.push(&[0, 1, 0x45]);
            assert!(decoder.next_packet().is_none(), "nothing after an error");
            assert_eq!(decoder.buffered(), 0);
            return;
        }
    }
    let _ = decoder.take_assigned();
    if let Some(addresses) = parse_address_assign(bytes) {
        assert!(addresses.len() <= bytes.len() / 6 + 1);
    }
}
