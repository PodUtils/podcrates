//! RFC 7050 (`ipv4only.arpa`) and RFC 6052 PREF64 discovery and synthesis across all six
//! standard prefix lengths (96, 64, 56, 48, 40, 32).

use std::net::{Ipv4Addr, Ipv6Addr};

use crate::report::{Nat64Status, Pref64};

/// RFC 7050 discovery domain name.
pub const IPV4ONLY_ARPA: &str = "ipv4only.arpa";
/// RFC 7050 well-known IPv4 markers (`192.0.0.170`, `192.0.0.171`).
pub const MARKER_A: Ipv4Addr = Ipv4Addr::new(192, 0, 0, 170);
pub const MARKER_B: Ipv4Addr = Ipv4Addr::new(192, 0, 0, 171);
/// RFC 6052 Well-Known Prefix (`64:ff9b::/96`).
pub const WKP_PREFIX: Ipv6Addr = Ipv6Addr::new(0x0064, 0xff9b, 0, 0, 0, 0, 0, 0);
/// Well-known destination for local zero-packet WKP route check (`64:ff9b::808:808`).
pub const WKP_PROBE_TARGET: Ipv6Addr = Ipv6Addr::new(0x0064, 0xff9b, 0, 0, 0, 0, 0x0808, 0x0808);

const PREFIX_LENS: [u8; 6] = [96, 64, 56, 48, 40, 32];

/// Extract the embedded IPv4 address at RFC 6052 prefix length `len`.
/// For lengths 32..=64, checks that the reserved `u` octet (`a[8]`) and suffix octets are zero;
/// `/96` has no `u` octet (bytes `0..12` are the 96-bit prefix).
pub fn extract_embedded_v4(addr: Ipv6Addr, len: u8) -> Option<Ipv4Addr> {
    let a = addr.octets();
    if len != 96 && a[8] != 0 {
        return None;
    }
    let v4 = match len {
        32 => {
            if a[9..16].iter().any(|&b| b != 0) {
                return None;
            }
            [a[4], a[5], a[6], a[7]]
        }
        40 => {
            if a[10..16].iter().any(|&b| b != 0) {
                return None;
            }
            [a[5], a[6], a[7], a[9]]
        }
        48 => {
            if a[11..16].iter().any(|&b| b != 0) {
                return None;
            }
            [a[6], a[7], a[9], a[10]]
        }
        56 => {
            if a[12..16].iter().any(|&b| b != 0) {
                return None;
            }
            [a[7], a[9], a[10], a[11]]
        }
        64 => {
            if a[13..16].iter().any(|&b| b != 0) {
                return None;
            }
            [a[9], a[10], a[11], a[12]]
        }
        96 => [a[12], a[13], a[14], a[15]],
        _ => return None,
    };
    Some(Ipv4Addr::from(v4))
}

/// Synthesize an RFC 6052 IPv4-embedded IPv6 address from `pref` and `v4`.
pub fn synthesize_v6(pref: Pref64, v4: Ipv4Addr) -> Option<Ipv6Addr> {
    let mut a = pref.prefix.octets();
    let b = v4.octets();
    let keep = usize::from(pref.len / 8);
    if !PREFIX_LENS.contains(&pref.len) {
        return None;
    }
    for byte in &mut a[keep..] {
        *byte = 0;
    }
    match pref.len {
        32 => {
            a[4..8].copy_from_slice(&b);
        }
        40 => {
            a[5..8].copy_from_slice(&b[..3]);
            a[9] = b[3];
        }
        48 => {
            a[6..8].copy_from_slice(&b[..2]);
            a[9..11].copy_from_slice(&b[2..]);
        }
        56 => {
            a[7] = b[0];
            a[9..12].copy_from_slice(&b[1..]);
        }
        64 => {
            a[9..13].copy_from_slice(&b);
        }
        96 => {
            a[12..16].copy_from_slice(&b);
        }
        _ => return None,
    }
    if pref.len != 96 {
        a[8] = 0;
    }
    Some(Ipv6Addr::from(a))
}

/// Extract a [`Pref64`] prefix by zeroing bytes from `len / 8` onward.
fn mask_prefix(addr: Ipv6Addr, len: u8) -> Pref64 {
    let mut a = addr.octets();
    let keep = usize::from(len / 8);
    for byte in &mut a[keep..] {
        *byte = 0;
    }
    Pref64 { prefix: Ipv6Addr::from(a), len }
}

/// Inspect AAAA answers for `ipv4only.arpa.` and classify NAT64/DNS64 status per RFC 7050.
pub fn discover_pref64(aaaa_answers: &[Ipv6Addr], route_to_wkp: Option<bool>) -> Nat64Status {
    if aaaa_answers.is_empty() {
        return Nat64Status::NoDns64;
    }
    let mut prefixes: Vec<Pref64> = Vec::new();
    for &addr in aaaa_answers {
        let mut matches_for_addr = Vec::new();
        for &len in &PREFIX_LENS {
            if let Some(v4) = extract_embedded_v4(addr, len)
                && (v4 == MARKER_A || v4 == MARKER_B)
            {
                matches_for_addr.push((len, v4, mask_prefix(addr, len)));
            }
        }
        if matches_for_addr.is_empty() {
            return Nat64Status::DnsHijackSuspect;
        }
        // If one address matched at more than one offset, keep prefixes corroborated by a
        // second address carrying the other marker (RFC 7050 section 3.1.2).
        let chosen: Vec<Pref64> = if matches_for_addr.len() == 1 {
            vec![matches_for_addr[0].2]
        } else {
            let disambiguated: Vec<Pref64> = matches_for_addr
                .iter()
                .filter(|&&(len, marker, pref)| {
                    let other_marker = if marker == MARKER_A { MARKER_B } else { MARKER_A };
                    aaaa_answers.iter().any(|&other| {
                        extract_embedded_v4(other, len) == Some(other_marker) && mask_prefix(other, len) == pref
                    })
                })
                .map(|&(_, _, pref)| pref)
                .collect();
            if disambiguated.is_empty() { vec![matches_for_addr[0].2] } else { disambiguated }
        };
        for pref in chosen {
            if !prefixes.contains(&pref) {
                prefixes.push(pref);
            }
        }
    }
    Nat64Status::Pref64 { prefixes, route_to_wkp }
}
