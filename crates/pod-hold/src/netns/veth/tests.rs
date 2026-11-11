use super::*;

/// podnet's wire: the gateway and the address of the program end of
/// [`Tun::default`], and its MTU.
fn tun() -> Tun {
    Tun::default()
}

fn frame(dst: [u8; 6], src: [u8; 6], kind: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&dst);
    out.extend_from_slice(&src);
    out.extend_from_slice(&kind.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

fn arp_request(target: Ipv4Addr) -> Vec<u8> {
    let mut arp = vec![0, 1, 8, 0, 6, 4, 0, 1];
    arp.extend_from_slice(&GUEST_MAC);
    arp.extend_from_slice(&tun().addr.octets());
    arp.extend_from_slice(&[0; 6]);
    arp.extend_from_slice(&target.octets());
    frame([0xff; 6], GUEST_MAC, ETH_P_ARP, &arp)
}

#[test]
fn the_wire_answers_arp_for_the_gateway() {
    let tun = tun();
    let Frame::Reply(reply) = take_frame(&arp_request(tun.gateway), tun.gateway, tun.mtu) else {
        panic!("no ARP reply for the gateway");
    };
    assert_eq!(&reply[0..6], &GUEST_MAC, "to the program end");
    assert_eq!(&reply[6..12], &GATEWAY_MAC);
    assert_eq!(&reply[14..22], &[0, 1, 8, 0, 6, 4, 0, 2], "an ARP reply");
    assert_eq!(&reply[22..28], &GATEWAY_MAC);
    assert_eq!(&reply[28..32], &tun.gateway.octets());
    assert_eq!(&reply[32..38], &GUEST_MAC);
    assert_eq!(&reply[38..42], &tun.addr.octets());
    // No reply for another address.
    let other = Ipv4Addr::new(10, 66, 255, 1);
    assert_eq!(take_frame(&arp_request(other), tun.gateway, tun.mtu), Frame::Drop);
}

#[test]
fn the_wire_strips_the_header_and_the_padding_of_an_ip_frame() {
    let tun = tun();
    let mut packet = vec![0x45, 0, 0, 28];
    packet.resize(28, 7);
    let mut padded = packet.clone();
    padded.resize(46, 0);
    let got = take_frame(&frame(GATEWAY_MAC, GUEST_MAC, ETH_P_IP, &padded), tun.gateway, tun.mtu);
    assert_eq!(got, Frame::Ip(packet.clone()));
    // The own frames, and frames to another MAC, are dropped.
    let own = frame(GUEST_MAC, GATEWAY_MAC, ETH_P_IP, &padded);
    assert_eq!(take_frame(&own, tun.gateway, tun.mtu), Frame::Drop);
    let other = frame([2, 0, 0, 0, 0, 9], GUEST_MAC, ETH_P_IP, &padded);
    assert_eq!(take_frame(&other, tun.gateway, tun.mtu), Frame::Drop);
    // A frame larger than the MTU (a segment of an offload) is dropped.
    let mut big = vec![0x45, 0, 0x10, 0];
    big.resize(4096, 0);
    let big = frame(GATEWAY_MAC, GUEST_MAC, ETH_P_IP, &big);
    assert_eq!(take_frame(&big, tun.gateway, tun.mtu), Frame::Drop);
    // The frame of a packet of the stack.
    let out = frame_for(&packet).unwrap();
    assert_eq!(out, frame(GUEST_MAC, GATEWAY_MAC, ETH_P_IP, &packet));
}

#[test]
fn the_newlink_request_names_both_ends() {
    let msg = newlink_request("eth0", "wire0", 1500);
    assert_eq!(u32::from_ne_bytes([msg[0], msg[1], msg[2], msg[3]]) as usize, msg.len());
    assert_eq!(u16::from_ne_bytes([msg[4], msg[5]]), RTM_NEWLINK);
    let text = String::from_utf8_lossy(&msg);
    assert!(text.contains("eth0\0"), "{text:?}");
    assert!(text.contains("veth"), "{text:?}");
    assert!(text.contains("wire0\0"), "{text:?}");
    assert_eq!(msg.len() % 4, 0);
    let mut ack = vec![0u8; 36];
    ack[4..6].copy_from_slice(&NLMSG_ERROR.to_ne_bytes());
    assert_eq!(netlink_error(&ack), Some(0));
    ack[16..20].copy_from_slice(&(-libc::EPERM).to_ne_bytes());
    assert_eq!(netlink_error(&ack), Some(-libc::EPERM));
}

// A short answer, or a message that is not an acknowledgement, gives no
// code: the caller reads it as no acknowledgement, not as a success.
#[test]
fn a_short_or_other_netlink_answer_is_no_acknowledgement() {
    let mut ack = vec![0u8; 36];
    ack[4..6].copy_from_slice(&NLMSG_ERROR.to_ne_bytes());
    assert_eq!(netlink_error(&ack[..19]), None);
    ack[4..6].copy_from_slice(&RTM_NEWLINK.to_ne_bytes());
    assert_eq!(netlink_error(&ack), None);
    assert_eq!(netlink_error(&[]), None);
}

// Each name is checked before a system call, so that this test makes no
// interface also when it runs as root in the namespace of the host.
#[test]
fn a_veth_name_that_the_kernel_would_cut_or_refuse_is_a_usage_failure() {
    let named = |name: &str| Tun { name: name.into(), ..tun() };
    for (end, wire) in [("", "wire0"), ("eth0", ""), ("a-name-longer-than-ifnamsiz", "wire0"), ("eth\0", "wire0")] {
        let e = create_veth(&named(end), wire).unwrap_err();
        assert_eq!(e.class, Class::Usage, "{end:?} {wire:?}: {}", e.chain());
    }
    let e = create_veth(&named("wire0"), "wire0").unwrap_err();
    assert_eq!(e.class, Class::Usage);
    assert!(e.text().contains("the two ends"), "{}", e.text());
}

#[test]
fn an_ipv6_frame_and_a_frame_of_another_kind() {
    let tun = tun();
    let mut packet = vec![0x60, 0, 0, 0, 0, 4, 17, 64];
    packet.resize(44, 1);
    let mut padded = packet.clone();
    padded.resize(50, 0);
    let got = take_frame(&frame(GATEWAY_MAC, GUEST_MAC, ETH_P_IPV6, &padded), tun.gateway, tun.mtu);
    assert_eq!(got, Frame::Ip(packet.clone()));
    assert_eq!(frame_for(&packet).unwrap(), frame(GUEST_MAC, GATEWAY_MAC, ETH_P_IPV6, &packet));
    // A length past the frame, a frame under its header, another kind.
    let short = &padded[..43];
    assert_eq!(take_frame(&frame(GATEWAY_MAC, GUEST_MAC, ETH_P_IPV6, short), tun.gateway, tun.mtu), Frame::Drop);
    assert_eq!(take_frame(&GATEWAY_MAC, tun.gateway, tun.mtu), Frame::Drop);
    assert_eq!(take_frame(&frame(GATEWAY_MAC, GUEST_MAC, 0x88cc, &padded), tun.gateway, tun.mtu), Frame::Drop);
    assert_eq!(frame_for(&[0x10, 0]), None);
    assert_eq!(frame_for(&[]), None);
}
