//! The fuzz target of the MQTT meeting point of `pod-wormhole`: the bytes
//! come from a public broker, which any client of it can fill.

use pod_wormhole::mqtt::packet::{decode, encode};
use pod_wormhole::mqtt::topic;

use crate::text;

/// The bound of a packet in the target: small, so that a length past it
/// is refused before its body.
const MOST: usize = 1 << 16;

/// The packets of MQTT 3.1.1 that a meeting point reads
/// (`pod_wormhole::mqtt::packet::decode`; new in `pod-wormhole`, T-261):
/// a whole packet takes no more bytes than there are, writes again to
/// bytes that read back to the same packet, and each shorter cut of it
/// waits for more; a topic of any AppID and nameplate is letters and
/// digits.
pub fn mqtt_packet(data: &[u8]) {
    if let Ok(Some((packet, used))) = decode(data, MOST) {
        assert!(used <= data.len() && used >= 2, "{used} of {} bytes", data.len());
        let bytes = encode(&packet).expect("a packet that was read is written");
        assert_eq!(decode(&bytes, MOST), Ok(Some((packet.clone(), bytes.len()))), "{packet:?}");
        assert_eq!(decode(&data[..used - 1], MOST), Ok(None), "a cut of {packet:?}");
    }
    let Some(typed) = text(data) else { return };
    let (appid, nameplate) = typed.split_once('\n').unwrap_or((typed, ""));
    let made = topic(appid, nameplate);
    assert!(made.len() == 42 && made.bytes().all(|b| b.is_ascii_alphanumeric()), "{made:?}");
}
