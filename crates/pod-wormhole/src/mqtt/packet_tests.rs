//! The codec of MQTT 3.1.1: the bytes of the standard, a round trip of
//! each packet, and the refusal of each malformed form.

use super::*;

const MOST: usize = 1 << 20;

fn one(bytes: &[u8]) -> Result<Option<(Packet, usize)>, Malformed> {
    decode(bytes, MOST)
}

fn connect(login: Option<(&str, &[u8])>) -> Packet {
    let login = login.map(|(user, password)| (user.to_string(), password.to_vec()));
    Packet::Connect(Connect { client_id: "pw0123".into(), keep_alive: 60, clean: true, login })
}

#[test]
fn the_remaining_length_is_the_standards_in_one_to_four_bytes() {
    // The table of section 2.2.3.
    let table: [(usize, &[u8]); 8] = [
        (0, &[0x00]),
        (127, &[0x7f]),
        (128, &[0x80, 0x01]),
        (16_383, &[0xff, 0x7f]),
        (16_384, &[0x80, 0x80, 0x01]),
        (2_097_151, &[0xff, 0xff, 0x7f]),
        (2_097_152, &[0x80, 0x80, 0x80, 0x01]),
        (268_435_455, &[0xff, 0xff, 0xff, 0x7f]),
    ];
    for (length, bytes) in table {
        let mut out = Vec::new();
        encode_length(length, &mut out).unwrap();
        assert_eq!(out, bytes, "{length}");
        // A PINGRESP with that length is read up to its length, then waits.
        let mut head = vec![0xd0];
        head.extend_from_slice(bytes);
        assert_eq!(decode(&head, LENGTH_MOST).unwrap(), (length == 0).then_some((Packet::PingResp, 2)), "{length}");
    }
    assert!(encode_length(LENGTH_MOST + 1, &mut Vec::new()).is_err());
}

#[test]
fn each_packet_has_the_bytes_of_the_standard() {
    // A CONNECT: "MQTT", level 4, a clean session, keep-alive 60, the id.
    let mut id = vec![0x10, 0x12, 0x00, 0x04, b'M', b'Q', b'T', b'T', 0x04, 0x02, 0x00, 0x3c, 0x00, 0x06];
    id.extend_from_slice(b"pw0123");
    assert_eq!(encode(&connect(None)).unwrap(), id);
    let with_login = encode(&connect(Some(("public", b"public")))).unwrap();
    assert_eq!(with_login[9], 0xc2, "the user name, the password and a clean session");
    assert!(with_login.ends_with(b"\x00\x06public\x00\x06public"));
    let mut publish = vec![0x30, 0x05, 0x00, 0x01, b't'];
    publish.extend_from_slice(b"hi");
    assert_eq!(encode(&Packet::Publish { topic: "t".into(), payload: b"hi".to_vec() }).unwrap(), publish);
    let subscribe = [0x82, 0x06, 0x00, 0x01, 0x00, 0x01, b't', 0x00];
    assert_eq!(encode(&Packet::Subscribe { id: 1, filters: vec![("t".into(), 0)] }).unwrap(), subscribe);
    assert_eq!(encode(&Packet::ConnAck { present: false, code: 0 }).unwrap(), [0x20, 0x02, 0x00, 0x00]);
    assert_eq!(encode(&Packet::SubAck { id: 1, codes: vec![0] }).unwrap(), [0x90, 0x03, 0x00, 0x01, 0x00]);
    assert_eq!(encode(&Packet::PingReq).unwrap(), [0xc0, 0x00]);
    assert_eq!(encode(&Packet::PingResp).unwrap(), [0xd0, 0x00]);
    assert_eq!(encode(&Packet::Disconnect).unwrap(), [0xe0, 0x00]);
}

#[test]
fn each_packet_goes_both_ways() {
    let all = [
        connect(None),
        connect(Some(("public", b"public"))),
        Packet::ConnAck { present: false, code: 0 },
        Packet::ConnAck { present: true, code: 5 },
        Packet::Publish { topic: "pwabc0g".into(), payload: Vec::new() },
        Packet::Publish { topic: "pwabc0t".into(), payload: vec![7; 300] },
        Packet::Subscribe { id: 1, filters: vec![("pwabc0g".into(), 0), ("a/+/#".into(), 2)] },
        Packet::SubAck { id: 9, codes: vec![0, 1, 2, REFUSED] },
        Packet::PingReq,
        Packet::PingResp,
        Packet::Disconnect,
    ];
    let mut stream = Vec::new();
    for packet in &all {
        let bytes = encode(packet).unwrap();
        assert_eq!(one(&bytes).unwrap(), Some((packet.clone(), bytes.len())), "{}", packet.name());
        // Cut short at each byte, it waits for more.
        for cut in 0..bytes.len() {
            assert_eq!(one(&bytes[..cut]).unwrap(), None, "{} cut at {cut}", packet.name());
        }
        stream.extend_from_slice(&bytes);
    }
    // Packets one after another, as a WebSocket message may join them.
    let mut rest = &stream[..];
    for packet in &all {
        let (read, used) = one(rest).unwrap().expect("a whole packet");
        assert_eq!(&read, packet);
        rest = &rest[used..];
    }
    assert!(rest.is_empty());
}

#[test]
fn a_publish_at_qos_1_gives_its_topic_and_payload() {
    // QoS 1: the flags 0b0010, a packet identifier after the topic.
    let bytes = [0x32, 0x07, 0x00, 0x01, b't', 0x00, 0x2a, b'h', b'i'];
    let (packet, used) = one(&bytes).unwrap().unwrap();
    assert_eq!((packet, used), (Packet::Publish { topic: "t".into(), payload: b"hi".to_vec() }, 9));
    // QoS 0 with the retain and the duplicate flags: no identifier, and the
    // flags change nothing.
    let bytes = [0x39, 0x05, 0x00, 0x01, b't', 0x00, 0x2a];
    assert_eq!(one(&bytes).unwrap().unwrap().0, Packet::Publish { topic: "t".into(), payload: vec![0x00, 0x2a] });
}

#[test]
fn malformed_lengths_are_refused() {
    // Five bytes of length.
    let five = [0x30, 0xff, 0xff, 0xff, 0xff, 0x01];
    assert!(one(&five).unwrap_err().0.contains("more than four bytes"));
    // A length past the caller's bound, refused before its body comes.
    let big = [0x30, 0x80, 0x80, 0x80, 0x01];
    assert!(decode(&big, 1000).unwrap_err().0.contains("over the bound of 1000"));
    assert!(decode(&[0x30, 0xe9, 0x07], 1000).is_err(), "1001 bytes");
    assert_eq!(decode(&[0x30, 0xe8, 0x07], 1000).unwrap(), None, "1000 bytes: wait for them");
    // A string whose length runs past its packet.
    assert!(one(&[0x30, 0x03, 0x00, 0x05, b't']).unwrap_err().0.contains("ends inside a field"));
    // A packet of a fixed form with more or fewer bytes.
    assert!(one(&[0x20, 0x03, 0x00, 0x00, 0x00]).unwrap_err().0.contains("after its end"));
    assert!(one(&[0x20, 0x01, 0x00]).unwrap_err().0.contains("ends inside a field"));
    assert!(one(&[0xc0, 0x01, 0x00]).unwrap_err().0.contains("after its end"));
    assert!(one(&[0x90, 0x02, 0x00, 0x01]).is_err(), "a SUBACK with no code");
    assert!(one(&[0x82, 0x02, 0x00, 0x01]).is_err(), "a SUBSCRIBE with no filter");
}

#[test]
fn reserved_flags_types_and_strings_are_refused() {
    for bad in [
        &[0x31, 0x00][..],                                                         // no topic
        &[0x36, 0x03, 0x00, 0x01, b't'],                                           // QoS 3
        &[0x30, 0x03, 0x00, 0x01, b'#'],                                           // a wildcard in a topic
        &[0x30, 0x03, 0x00, 0x01, 0x00],                                           // U+0000
        &[0x30, 0x04, 0x00, 0x02, 0xc3, 0x28],                                     // no UTF-8
        &[0x80, 0x06, 0x00, 0x01, 0x00, 0x01, b't', 0x00],                         // SUBSCRIBE flags 0
        &[0x82, 0x06, 0x00, 0x01, 0x00, 0x01, b't', 0x03],                         // QoS 3 asked
        &[0x21, 0x02, 0x00, 0x00],                                                 // CONNACK flags 1
        &[0x20, 0x02, 0x02, 0x00],                                                 // CONNACK reserved bit
        &[0x90, 0x03, 0x00, 0x01, 0x03],                                           // SUBACK code 3
        &[0xc1, 0x00],                                                             // PINGREQ flags 1
        &[0x40, 0x02, 0x00, 0x01],                                                 // PUBACK
        &[0x00, 0x00],                                                             // reserved type 0
        &[0xf0, 0x00],                                                             // reserved type 15
        &[0x10, 0x0a, 0x00, 0x04, b'M', b'Q', b'I', b's', 0x03, 0x02, 0x00, 0x3c], // MQTT 3.1
    ] {
        assert!(one(bad).is_err(), "{bad:02x?}");
    }
    let mut will = encode(&connect(None)).unwrap();
    will[9] |= 0x04;
    assert!(one(&will).unwrap_err().0.contains("a will"));
    let mut password_alone = encode(&connect(None)).unwrap();
    password_alone[9] |= 0x40;
    assert!(one(&password_alone).is_err());
}

#[test]
fn no_debug_shows_the_password() {
    let Packet::Connect(c) = connect(Some(("public", b"SECRET-PASSWORD"))) else { unreachable!() };
    let shown = format!("{c:?}");
    assert!(shown.contains("public") && !shown.contains("SECRET"), "{shown}");
}

#[test]
fn a_string_over_65535_bytes_is_not_written() {
    let topic = "t".repeat(65_536);
    assert!(encode(&Packet::Publish { topic, payload: Vec::new() }).unwrap_err().0.contains("over 65535"));
}
