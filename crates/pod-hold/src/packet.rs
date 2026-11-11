//! The packets of the netns hold of a SYN: the parse of a bare TCP SYN,
//! the RST|ACK answer to it, and the IPv4 header checksum. The consumer
//! holds each SYN on its TUN, dials first, and answers a failed dial with
//! [`rst_reply_for_syn`]: the `connect` of the payload then fails with
//! `ECONNREFUSED`.

use std::net::Ipv4Addr;

/// The one's-complement checksum of an IPv4 header (and of a TCP
/// pseudo-header with its segment). A correct header sums to 0.
pub fn ipv4_checksum(header: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    for pair in header.chunks(2) {
        let word = u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)]);
        sum += u32::from(word);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// The RST|ACK answer to the IPv4 TCP SYN `pkt`: the connect of the
/// program fails with `ECONNREFUSED`. `None` for each other packet.
pub fn rst_reply_for_syn(pkt: &[u8]) -> Option<Vec<u8>> {
    parse_tcp_syn(pkt)?;
    let ihl = usize::from(pkt[0] & 0x0f) * 4;
    let tcp = &pkt[ihl..];
    let seq = u32::from_be_bytes([tcp[4], tcp[5], tcp[6], tcp[7]]);
    let mut out = Vec::with_capacity(40);
    out.extend_from_slice(&[0x45, 0, 0, 40, 0, 0, 0x40, 0, 64, 6, 0, 0]);
    out.extend_from_slice(&pkt[16..20]); // source: the destination of the SYN
    out.extend_from_slice(&pkt[12..16]); // destination: the program
    let checksum = ipv4_checksum(&out[..20]);
    out[10..12].copy_from_slice(&checksum.to_be_bytes());
    out.extend_from_slice(&tcp[2..4]); // source port: the destination port
    out.extend_from_slice(&tcp[0..2]);
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&seq.wrapping_add(1).to_be_bytes());
    // Data offset 5 words; flags RST and ACK; window 0; checksum; urgent 0.
    out.extend_from_slice(&[0x50, 0x14, 0, 0, 0, 0, 0, 0]);
    // The TCP checksum covers a pseudo-header and the segment.
    let mut pseudo = Vec::with_capacity(32);
    pseudo.extend_from_slice(&out[12..20]);
    pseudo.extend_from_slice(&[0, 6, 0, 20]);
    pseudo.extend_from_slice(&out[20..40]);
    let checksum = ipv4_checksum(&pseudo);
    out[36..38].copy_from_slice(&checksum.to_be_bytes());
    Some(out)
}

/// Parse a bare IPv4 TCP SYN: (source address, source port, destination
/// address, destination port). `None` for each other packet: another
/// version or protocol, an ACK, a header shorter than 20 bytes, a fragment
/// past the first, or too few bytes for the TCP header.
pub fn parse_tcp_syn(pkt: &[u8]) -> Option<(Ipv4Addr, u16, Ipv4Addr, u16)> {
    if pkt.len() < 40 || (pkt[0] >> 4) != 4 || pkt[9] != 6 {
        return None;
    }
    let ihl = usize::from(pkt[0] & 0x0f) * 4;
    // An IHL under 5 puts the TCP header inside the IP header, and a later
    // fragment holds no TCP header: neither is a SYN to read.
    let offset = u16::from_be_bytes([pkt[6], pkt[7]]) & 0x1fff;
    if ihl < 20 || offset != 0 || pkt.len() < ihl + 20 {
        return None;
    }
    let flags = pkt[ihl + 13];
    if flags & 0x02 == 0 || flags & 0x10 != 0 {
        return None; // SYN set, ACK clear
    }
    let src = Ipv4Addr::new(pkt[12], pkt[13], pkt[14], pkt[15]);
    let dst = Ipv4Addr::new(pkt[16], pkt[17], pkt[18], pkt[19]);
    let sport = u16::from_be_bytes([pkt[ihl], pkt[ihl + 1]]);
    let dport = u16::from_be_bytes([pkt[ihl + 2], pkt[ihl + 3]]);
    Some((src, sport, dst, dport))
}

#[cfg(test)]
mod tests;
