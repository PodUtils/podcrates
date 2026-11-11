use super::*;

/// A SYN from 10.0.0.2:40000 to 93.184.216.34:443, with a correct header
/// checksum.
fn syn() -> Vec<u8> {
    let mut syn = vec![0x45, 0, 0, 40, 0, 0, 0x40, 0, 64, 6, 0, 0];
    syn.extend_from_slice(&[10, 0, 0, 2, 93, 184, 216, 34]);
    let c = ipv4_checksum(&syn[..20]);
    syn[10..12].copy_from_slice(&c.to_be_bytes());
    syn.extend_from_slice(&40000u16.to_be_bytes());
    syn.extend_from_slice(&443u16.to_be_bytes());
    syn.extend_from_slice(&1000u32.to_be_bytes());
    syn.extend_from_slice(&[0, 0, 0, 0, 0x50, 0x02, 0xff, 0xff, 0, 0, 0, 0]);
    syn
}

// The test of podnet's `lib.rs`, with its name.
#[test]
fn parses_a_tcp_syn() {
    let mut pkt = vec![0u8; 40];
    pkt[0] = 0x45; // IPv4, IHL 5
    pkt[9] = 6; // TCP
    pkt[12..16].copy_from_slice(&[10, 0, 0, 2]);
    pkt[16..20].copy_from_slice(&[93, 184, 216, 34]);
    pkt[20..22].copy_from_slice(&40000u16.to_be_bytes());
    pkt[22..24].copy_from_slice(&443u16.to_be_bytes());
    pkt[33] = 0x02; // SYN
    assert_eq!(parse_tcp_syn(&pkt), Some((Ipv4Addr::new(10, 0, 0, 2), 40000, Ipv4Addr::new(93, 184, 216, 34), 443)));

    // An ACK is not a SYN.
    pkt[33] = 0x10;
    assert_eq!(parse_tcp_syn(&pkt), None);
    // UDP is not TCP.
    pkt[9] = 17;
    pkt[33] = 0x02;
    assert_eq!(parse_tcp_syn(&pkt), None);
    // Too short.
    assert_eq!(parse_tcp_syn(&pkt[..20]), None);
}

// The test of podnet's `lib.rs` (podnet's issue 190), with its name: the
// answer to a failed dial is a RST|ACK of the SYN.
#[test]
fn the_rst_reply_answers_the_syn() {
    let rst = rst_reply_for_syn(&syn()).unwrap();
    assert_eq!(rst.len(), 40);
    assert_eq!(&rst[12..16], &[93, 184, 216, 34]);
    assert_eq!(&rst[16..20], &[10, 0, 0, 2]);
    assert_eq!(u16::from_be_bytes([rst[20], rst[21]]), 443);
    assert_eq!(u16::from_be_bytes([rst[22], rst[23]]), 40000);
    assert_eq!(u32::from_be_bytes([rst[28], rst[29], rst[30], rst[31]]), 1001);
    assert_eq!(rst[33], 0x14, "RST and ACK");
    // A correct header sums to zero.
    assert_eq!(ipv4_checksum(&rst[..20]), 0);
    let mut pseudo = rst[12..20].to_vec();
    pseudo.extend_from_slice(&[0, 6, 0, 20]);
    pseudo.extend_from_slice(&rst[20..40]);
    assert_eq!(ipv4_checksum(&pseudo), 0);
    // Another packet gets no answer.
    assert!(rst_reply_for_syn(&rst).is_none());
}

/// podnet's parse took an IHL under 5: the "TCP header" then starts inside
/// the IP header, and the ports and flags are bytes of the addresses.
#[test]
fn a_header_shorter_than_20_bytes_is_no_syn() {
    for ihl in 0u8..5 {
        let mut pkt = syn();
        pkt[0] = 0x40 | ihl;
        // Make the byte that the short header reads as the flags a SYN.
        let at = usize::from(ihl) * 4 + 13;
        pkt[at] = 0x02;
        assert_eq!(parse_tcp_syn(&pkt), None, "IHL {ihl}");
        assert_eq!(rst_reply_for_syn(&pkt), None, "IHL {ihl}");
    }
}

#[test]
fn a_fragment_past_the_first_is_no_syn() {
    let mut pkt = syn();
    // Fragment offset 1 (8 bytes), with no flag.
    pkt[6..8].copy_from_slice(&1u16.to_be_bytes());
    assert_eq!(parse_tcp_syn(&pkt), None);
    // The first fragment of a larger datagram (the flag MF, offset 0) holds
    // the TCP header.
    pkt[6..8].copy_from_slice(&0x2000u16.to_be_bytes());
    assert!(parse_tcp_syn(&pkt).is_some());
}

#[test]
fn a_syn_with_options_in_the_ip_header_is_read_past_them() {
    let mut pkt = syn();
    // IHL 6: four bytes of options after the fixed header.
    pkt[0] = 0x46;
    pkt.splice(20..20, [1, 1, 1, 0]);
    let parsed = parse_tcp_syn(&pkt).expect("a SYN with IP options");
    assert_eq!(parsed.1, 40000);
    assert_eq!(parsed.3, 443);
    let rst = rst_reply_for_syn(&pkt).unwrap();
    assert_eq!(u32::from_be_bytes([rst[28], rst[29], rst[30], rst[31]]), 1001);
}

#[test]
fn the_checksum_of_an_odd_length_pads_with_zero() {
    assert_eq!(ipv4_checksum(&[0x01]), !0x0100);
    assert_eq!(ipv4_checksum(&[]), 0xffff);
}
