use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use super::*;
use crate::stun::{HEADER, decode, message};

const TID: TransactionId = [0xb7, 0xe7, 0xa7, 0x01, 0xbc, 0x34, 0xd6, 0x86, 0xfa, 0x87, 0xdf, 0xae];

#[test]
fn builder_encodes_change_request_and_software() {
    let bytes = NatRequestBuilder::binding(TID)
        .change_request(CHANGE_IP | CHANGE_PORT)
        .software("probe")
        .build()
        .expect("valid request");
    let msg = decode(&bytes).expect("valid STUN");
    assert_eq!(msg.kind, BINDING_REQUEST);
    assert_eq!(msg.id, TID);
    assert_eq!(msg.change_request(), Some(Ok(0x06)));
    assert_eq!(msg.attribute(SOFTWARE), Some(&b"probe"[..]));
}

#[test]
fn builder_refuses_response_port_with_padding() {
    let err = NatRequestBuilder::binding(TID).response_port(5000).padding(64).build().unwrap_err();
    assert_eq!(err, NatBuildError::ResponsePortWithPadding);
}

#[test]
fn builder_refuses_bad_flags_and_software_len() {
    assert_eq!(
        NatRequestBuilder::binding(TID).change_request(0x01).build().unwrap_err(),
        NatBuildError::InvalidChangeFlags(0x01)
    );
    assert_eq!(NatRequestBuilder::binding(TID).software("").build().unwrap_err(), NatBuildError::InvalidSoftwareLen(0));
    assert_eq!(
        NatRequestBuilder::binding(TID).software("a".repeat(33)).build().unwrap_err(),
        NatBuildError::InvalidSoftwareLen(33)
    );
    assert_eq!(
        NatRequestBuilder::binding(TID).padding(65_484).build().unwrap_err(),
        NatBuildError::PaddingTooLarge(65_484)
    );
    // The largest PADDING alone: its 4-byte header and 65,480 bytes make the largest body.
    let max_ok = NatRequestBuilder::binding(TID).padding(65_480).build().expect("max pad fits");
    assert!(decode(&max_ok).is_ok());
    assert_eq!(max_ok.len(), HEADER + MAX_NAT_BODY);
    // Adding SOFTWARE shrinks available padding space.
    assert_eq!(
        NatRequestBuilder::binding(TID).software("probe").padding(65_480).build().unwrap_err(),
        NatBuildError::PaddingTooLarge(65_480)
    );
}

#[test]
fn the_largest_padded_message_fits_one_udp_datagram_over_ipv4() {
    // 65,535 bytes of IPv4, less the IPv4 header (20) and the UDP header (8).
    const UDP_PAYLOAD_V4: usize = 65_507;
    let request = NatRequestBuilder::binding(TID).padding(65_480).build().expect("max pad fits");
    assert!(request.len() <= UDP_PAYLOAD_V4, "{} bytes", request.len());
    let a = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 32853);
    let response = binding_success_nat(&TID, Some(a), Some(a), Some(a), Some(a), Some(usize::MAX));
    assert!(response.len() <= UDP_PAYLOAD_V4, "{} bytes", response.len());
    assert!(decode(&response).is_ok());
}

#[test]
fn builder_encodes_response_port_and_padding_independently() {
    let rp = NatRequestBuilder::binding(TID).response_port(51234).build().expect("rp");
    let msg = decode(&rp).expect("decode rp");
    assert_eq!(msg.response_port(), Some(Ok(51234)));

    let pad = NatRequestBuilder::binding(TID).padding(120).build().expect("pad");
    let msg = decode(&pad).expect("decode pad");
    assert_eq!(msg.attribute(PADDING).map(<[u8]>::len), Some(120));
}

#[test]
fn response_readers_separate_xor_alt_and_plain_addresses() {
    let xor_a = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 32853);
    let plain_a = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)), 32853);
    let other_a = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 2)), 3479);
    let origin_a = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1)), 3478);

    let bytes = binding_success_nat(&TID, Some(xor_a), Some(plain_a), Some(other_a), Some(origin_a), Some(16));
    let msg = decode(&bytes).expect("decode");
    assert_eq!(msg.xor_mapped(), Some(Ok(xor_a)));
    assert_eq!(msg.mapped_plain(), Some(Ok(plain_a)));
    assert_eq!(msg.other_address(), Some(Ok(other_a)));
    assert_eq!(msg.response_origin(), Some(Ok(origin_a)));
    assert_eq!(msg.xor_mapped_alt(), None);
    assert_eq!(msg.attribute(PADDING).map(<[u8]>::len), Some(16));
}

#[test]
fn legacy_and_alt_readers_work() {
    let a = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 32853);
    let alt_val = write_address(a, Some(&TID));
    let plain_val = write_address(a, None);
    let unk_val = [0x00, 0x03, 0x00, 0x27];
    let bytes = message(
        BINDING_SUCCESS,
        &TID,
        &[
            (XOR_MAPPED_ADDRESS_ALT, &alt_val),
            (CHANGED_ADDRESS, &plain_val),
            (SOURCE_ADDRESS, &plain_val),
            (UNKNOWN_ATTRIBUTES, &unk_val),
        ],
    );
    let msg = decode(&bytes).expect("decode");
    assert_eq!(msg.xor_mapped(), None);
    assert_eq!(msg.xor_mapped_alt(), Some(Ok(a)));
    assert_eq!(msg.changed_address(), Some(Ok(a)));
    assert_eq!(msg.source_address(), Some(Ok(a)));
    assert_eq!(msg.unknown_attributes(), Some(Ok(vec![CHANGE_REQUEST, RESPONSE_PORT])));
}

#[test]
fn rfc5769_xor_mapped_vector_matches() {
    let a = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 32853);
    let val = write_address(a, Some(&TID));
    assert_eq!(val, [0x00, 0x01, 0xa1, 0x47, 0xe1, 0x12, 0xa6, 0x43]);
    assert_eq!(read_address(&val, Some(&TID)), Ok(a));
}

#[test]
fn ipv6_readers_xor_with_the_transaction_and_read_plain_addresses_as_they_are() {
    let a =
        SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0x1234, 0x5678, 0x11, 0x2233, 0x4455, 0x6677)), 32853);
    let other = SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 2)), 3479);
    // RFC 5769 (2.3): the XOR form of this address with this transaction.
    let xor_val = write_address(a, Some(&TID));
    assert_eq!(
        xor_val,
        [
            0x00, 0x02, 0xa1, 0x47, 0x01, 0x13, 0xa9, 0xfa, 0xa5, 0xd3, 0xf1, 0x79, 0xbc, 0x25, 0xf4, 0xb5, 0xbe, 0xd2,
            0xb9, 0xd9
        ]
    );
    let plain_val = write_address(other, None);
    let bytes = message(
        BINDING_SUCCESS,
        &TID,
        &[(XOR_MAPPED_ADDRESS_ALT, &xor_val), (OTHER_ADDRESS, &plain_val), (RESPONSE_ORIGIN, &plain_val)],
    );
    let msg = decode(&bytes).expect("decode");
    assert_eq!(msg.xor_mapped_alt(), Some(Ok(a)));
    assert_eq!(msg.other_address(), Some(Ok(other)));
    assert_eq!(msg.response_origin(), Some(Ok(other)));
    // The XOR form read as plain is another address: the readers do not mix the two.
    assert_ne!(read_address(&xor_val, None), Ok(a));
}

/// Each reader of this module on hostile values: a value of each length up to
/// 24 bytes, each family byte, and odd and wrong lengths. Each one gives an
/// answer, never a panic.
#[test]
fn readers_answer_each_hostile_value_with_no_panic() {
    let kinds = [
        XOR_MAPPED_ADDRESS,
        XOR_MAPPED_ADDRESS_ALT,
        MAPPED_ADDRESS,
        OTHER_ADDRESS,
        CHANGED_ADDRESS,
        RESPONSE_ORIGIN,
        SOURCE_ADDRESS,
        CHANGE_REQUEST,
        RESPONSE_PORT,
        UNKNOWN_ATTRIBUTES,
    ];
    let mut faults = 0usize;
    for kind in kinds {
        for len in 0..=24usize {
            for family in [0u8, 1, 2, 3, 0xff] {
                let mut value: Vec<u8> = (0..len).map(|i| (i as u8).wrapping_mul(37)).collect();
                if len > 1 {
                    value[1] = family;
                }
                let bytes = message(BINDING_SUCCESS, &TID, &[(kind, &value)]);
                let msg = decode(&bytes).expect("a framed message decodes");
                let answers = [
                    msg.xor_mapped().map(|r| r.is_ok()),
                    msg.xor_mapped_alt().map(|r| r.is_ok()),
                    msg.mapped_plain().map(|r| r.is_ok()),
                    msg.other_address().map(|r| r.is_ok()),
                    msg.changed_address().map(|r| r.is_ok()),
                    msg.response_origin().map(|r| r.is_ok()),
                    msg.source_address().map(|r| r.is_ok()),
                    msg.change_request().map(|r| r.is_ok()),
                    msg.response_port().map(|r| r.is_ok()),
                    msg.unknown_attributes().map(|r| r.is_ok()),
                ];
                // Exactly one reader names the attribute; the others give `None`.
                assert_eq!(answers.iter().filter(|a| a.is_some()).count(), 1, "kind 0x{kind:04x}, {len} bytes");
                faults += answers.iter().filter(|a| **a == Some(false)).count();
            }
        }
    }
    assert!(faults > 0);
}

#[test]
fn each_cut_of_a_nat_response_is_refused_with_no_panic() {
    let a = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 32853);
    let bytes = binding_success_nat(&TID, Some(a), Some(a), Some(a), Some(a), Some(7));
    for cut in 0..bytes.len() {
        assert!(decode(&bytes[..cut]).is_err(), "a cut at {cut} bytes");
    }
    // A length that says more than the message holds: refused, not read past the end.
    let mut long = bytes.clone();
    long[HEADER + 2] = 0xff;
    assert!(decode(&long).is_err());
}
