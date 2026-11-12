//! The codec of STUN of `pod-turn` (T-377).

/// One message of STUN (`pod_turn::stun::decode`; the codec of podssh's
/// `stun.rs`, moved out of `pod-proxy` with T-377): a message that decodes
/// writes back to bytes that decode to the same type, id and attributes;
/// its type reads back to its method and class; the readers of its
/// attributes and the checks of its integrity and its fingerprint give an
/// answer, never a panic. A refusal names an offset inside the input and
/// the type of the header when the input has one, as `parse` does.
pub fn stun_message(data: &[u8]) {
    use pod_turn::stun;
    let read = match stun::decode(data) {
        Ok(read) => read,
        Err(e) => {
            assert!(e.offset <= data.len(), "an offset of {} in {} bytes", e.offset, data.len());
            let kind = (data.len() >= 2).then(|| u16::from_be_bytes([data[0], data[1]]));
            assert_eq!(e.kind, kind);
            assert_eq!(stun::parse(data).unwrap_err(), e.to_string());
            return;
        }
    };
    let again = stun::message(read.kind, &read.id, &read.attributes);
    let back = stun::decode(&again).expect("a written message decodes");
    assert_eq!((back.kind, back.id, &back.attributes), (read.kind, read.id, &read.attributes));
    let (method, class) = (stun::method_of(read.kind), stun::class_of(read.kind));
    assert_eq!(stun::message_type(method, class), read.kind);
    let _ = read.mapped();
    let _ = read.error();
    let _ = stun::integrity(data, b"key");
    let _ = stun::integrity_sha256(data, b"key");
    let _ = stun::fingerprint_ok(data);
    if let Some(value) = read.attribute(stun::PASSWORD_ALGORITHMS) {
        let _ = stun::password_algorithms(value);
    }
    if let Some(value) = read.attribute(stun::NONCE) {
        let _ = stun::nonce_features(value);
    }
}
