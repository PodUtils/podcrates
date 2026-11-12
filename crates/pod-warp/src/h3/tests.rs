use super::wire::{get_prefixed, get_varint, put_prefixed, put_string};
use super::*;

#[test]
fn varints_and_frames() {
    for v in [0u64, 63, 64, 16383, 16384, (1 << 30) - 1, 1 << 30, (1 << 62) - 1] {
        let mut b = Vec::new();
        put_varint(&mut b, v);
        assert_eq!(get_varint(&b), Some((v, b.len())), "{v}");
    }
    // RFC 9000, appendix A.1.
    assert_eq!(get_varint(&[0x7b, 0xbd]), Some((15293, 2)));
    assert_eq!(get_varint(&[0x9d, 0x7f, 0x3e, 0x7d]), Some((494878333, 4)));
    let mut frames = Frames::default();
    let one = frame(FRAME_HEADERS, b"abc");
    let two = frame(FRAME_DATA, &[7; 300]);
    frames.push(&one[..2]);
    assert!(frames.next_frame().unwrap().is_none());
    frames.push(&one[2..]);
    frames.push(&two);
    assert_eq!(frames.next_frame().unwrap(), Some((FRAME_HEADERS, b"abc".to_vec())));
    assert_eq!(frames.next_frame().unwrap(), Some((FRAME_DATA, vec![7; 300])));
    assert!(frames.next_frame().unwrap().is_none());
}

#[test]
fn a_frame_larger_than_the_limit_is_refused() {
    let mut head = Vec::new();
    put_varint(&mut head, FRAME_DATA);
    put_varint(&mut head, (wire::MAX_FRAME + 1) as u64);
    let mut frames = Frames::default();
    frames.push(&head);
    let err = frames.next_frame().expect_err("a frame over the limit fails");
    assert_eq!(err.class, Class::Protocol);
}

#[test]
fn prefixed_integers() {
    // RFC 7541, C.1: 10 with a 5-bit prefix; 1337 with a 5-bit prefix.
    let mut b = Vec::new();
    put_prefixed(&mut b, 0, 5, 10);
    assert_eq!(b, [10]);
    b.clear();
    put_prefixed(&mut b, 0, 5, 1337);
    assert_eq!(b, [31, 154, 10]);
    assert_eq!(get_prefixed(&b, 5), Some((1337, 3)));
}

#[test]
fn the_request_and_the_status() {
    let section = connect_ip_headers("cloudflareaccess.com", "cf-connect-ip");
    assert_eq!(&section[..5], &[0, 0, 0xcf, 0xd7, 0xc1]);
    assert!(section.windows(13).any(|w| w == b"cf-connect-ip"));
    assert!(section.windows(9).any(|w| w == b":protocol"));
    // :status 200 (static 25), then a literal header.
    let mut answer = vec![0, 0, 0xc0 | 25];
    put_string(&mut answer, 0x20, 3, "server");
    put_string(&mut answer, 0, 7, "cloudflare");
    assert_eq!(response_status(&answer).unwrap(), 200);
    // 403 is static 68: more than the 6-bit prefix holds.
    let mut refused = vec![0, 0];
    put_prefixed(&mut refused, 0xc0, 6, 68);
    assert_eq!(refused.len(), 4);
    assert_eq!(response_status(&refused).unwrap(), 403);
    // A literal value with a name reference to :status.
    let mut literal = vec![0, 0];
    put_prefixed(&mut literal, 0x50, 4, 24);
    put_string(&mut literal, 0, 7, "429");
    assert_eq!(response_status(&literal).unwrap(), 429);
    // The dynamic table is refused.
    assert!(response_status(&[1, 0, 0x80]).is_err());
    assert!(response_status(&[0, 0, 0x80 | 2]).is_err());
    assert!(response_status(&[0, 0]).is_err());
}

/// A string literal whose length runs past the end of the section is a
/// failure, not a panic.
#[test]
fn a_literal_past_the_end_is_refused() {
    let mut section = vec![0, 0, 0x20 | 7];
    section.push(0x7f);
    section.extend_from_slice(&[0xff; 9]);
    assert!(response_status(&section).is_err());
}
