use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use super::*;
use crate::stun::{decode, message};

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
        NatRequestBuilder::binding(TID).padding(65_532).build().unwrap_err(),
        NatBuildError::PaddingTooLarge(65_532)
    );
    // Max standalone PADDING is 65_528 (4-byte TLV header + 65_528 = 65_532 <= u16::MAX).
    let max_ok = NatRequestBuilder::binding(TID).padding(65_528).build().expect("max pad fits");
    assert!(decode(&max_ok).is_ok());
    // Adding SOFTWARE shrinks available padding space.
    assert_eq!(
        NatRequestBuilder::binding(TID).software("probe").padding(65_528).build().unwrap_err(),
        NatBuildError::PaddingTooLarge(65_528)
    );
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
