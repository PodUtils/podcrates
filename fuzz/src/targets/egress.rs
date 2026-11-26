//! The fuzz targets of `pod-egress` (feature `fuzzing`).

use pod_egress::fuzzing::{
    Command, MAX_DATA, MAX_REQUEST, Message, Packet, encode, iap_ack, iap_data, iap_message as iap_read,
    trojan_parse, trojan_request, wisp_answer, wisp_packet as read,
};

/// The newest version of Wisp that the client speaks.
const VERSION: (u8, u8) = (2, 1);

/// A message of a Wisp server (`fuzzing::wisp_packet`): a packet that is
/// read writes back to a message that reads to the same packet; the answer
/// to an INFO names a version no newer than the server's and the client's.
pub fn wisp_packet(data: &[u8]) {
    let Ok((stream, packet)) = read(data) else { return };
    assert_eq!(read(&encode(stream, &packet)), Ok((stream, packet.clone())));
    if let Packet::Info(info) = packet
        && let Ok((ours, opened)) = wisp_answer(&info)
    {
        let version = (ours.major, ours.minor);
        assert!(version <= (info.major, info.minor) && version <= VERSION, "{version:?}");
        assert_eq!(opened.version, version);
    }
}

/// A message of IAP's relay (`fuzzing::iap_message`): it reads the same
/// twice; data of 1 to 16 KiB and an acknowledgement write back to a message
/// that reads to the same; the reason of a long close is one line with no
/// control character.
pub fn iap_message(data: &[u8]) {
    let Ok(message) = iap_read(data) else { return };
    assert!(iap_read(data) == Ok(message.clone()));
    match &message {
        Message::Data(bytes) if (1..=MAX_DATA).contains(&bytes.len()) => {
            let again = iap_data(bytes).expect("data of 1 to 16 KiB writes");
            assert!(iap_read(&again) == Ok(message));
        }
        Message::Ack(n) => assert!(iap_read(&iap_ack(*n)) == Ok(message)),
        Message::LongClose { reason, .. } => assert!(!reason.contains(char::is_control), "{reason:?}"),
        _ => {}
    }
}

/// The password of the target `trojan_request`: a made-up value.
const TROJAN_PASSWORD: &str = "a-password-of-the-fuzz-target";

/// A Trojan request at a server (`fuzzing::trojan_parse`): the first byte
/// picks whether the hash of the password goes first, so that the bytes
/// reach the address. A request that parses is at most `MAX_REQUEST` bytes,
/// no prefix of it is a request, and a CONNECT writes back to its bytes;
/// bytes of `MAX_REQUEST` or more are never "too short".
pub fn trojan_request_accept(data: &[u8]) {
    let Some((&pick, rest)) = data.split_first() else { return };
    let password = pod_egress::fuzzing::Password::new(TROJAN_PASSWORD).expect("a password");
    let mut bytes = Vec::new();
    if pick & 1 == 0 {
        bytes.extend_from_slice(password.hex());
    }
    bytes.extend_from_slice(rest);
    match trojan_parse(&bytes, &password) {
        Ok(Some(request)) => {
            assert!((60..=MAX_REQUEST).contains(&request.used) && request.used <= bytes.len());
            assert_eq!(trojan_parse(&bytes[..request.used], &password), Ok(Some(request.clone())));
            assert_eq!(trojan_parse(&bytes[..request.used - 1], &password), Ok(None));
            if request.command == Command::Connect && !request.host.is_empty() {
                let host = if request.host.contains(':') { format!("[{}]", request.host) } else { request.host.clone() };
                let again = trojan_request(TROJAN_PASSWORD, &host, request.port).expect("a host that parsed writes");
                assert_eq!(again, bytes[..request.used], "{request:?}");
            }
        }
        Ok(None) => assert!(bytes.len() < MAX_REQUEST, "{} bytes are no request yet", bytes.len()),
        Err(_) => {}
    }
}
