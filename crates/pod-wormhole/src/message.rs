//! The pure reading of what a mailbox says: a JSON object in a frame, the
//! message of a mailbox, the `pake` body, the hex of a body, and the words
//! of the mailbox that a line shows. No function here does I/O, so a fuzz
//! target runs each on untrusted bytes.

use serde_json::Value;

/// The most of the mailbox's own words that a line shows.
pub const WORDS_MOST: usize = 200;

/// A message of the open mailbox: the side that added it, its phase and its
/// body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub side: String,
    pub phase: String,
    pub body: Vec<u8>,
}

/// The JSON object that a frame of the mailbox holds; `None` for another
/// value or for bytes that are not JSON.
pub fn object(payload: &[u8]) -> Option<Value> {
    serde_json::from_slice::<Value>(payload).ok().filter(Value::is_object)
}

/// The `message` of a mailbox: its side, its phase and its body in hex;
/// `None` when one of the three is missing or is not text, or when the body
/// is not hex.
pub fn message_of(message: &Value) -> Option<Message> {
    let text = |key: &str| message[key].as_str().map(str::to_string);
    let body = message["body"].as_str().and_then(unhex)?;
    Some(Message { side: text("side")?, phase: text("phase")?, body })
}

/// The SPAKE2 message of a `pake` phase's body.
pub fn pake_of(body: &[u8]) -> Option<Vec<u8>> {
    let value: Value = serde_json::from_slice(body).ok()?;
    unhex(value["pake_v1"].as_str()?)
}

/// The mailbox's own words, on one line and cut short: a string as it is,
/// another value as JSON.
pub fn words(value: &Value) -> String {
    let text = match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    pod_core::text::one_line(&text).chars().take(WORDS_MOST).collect()
}

/// Lower-case hex, as the mailbox carries a body.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// The bytes of hex in either case; `None` for another text.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let digit = |c: u8| (c as char).to_digit(16);
    text.as_bytes().chunks(2).map(|pair| u8::try_from(digit(pair[0])? * 16 + digit(pair[1])?).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hex_goes_both_ways_and_refuses_what_is_not_hex() {
        assert_eq!(hex(&[0, 0x5a, 0xff]), "005aff");
        assert_eq!(unhex("005AfF"), Some(vec![0, 0x5a, 0xff]));
        for bad in ["0", "0g", "zz", "a b "] {
            assert_eq!(unhex(bad), None, "{bad:?}");
        }
        assert_eq!(unhex(""), Some(Vec::new()));
    }

    #[test]
    fn the_mailboxs_words_are_one_short_line() {
        assert_eq!(words(&json!("down for\nmaintenance")), "down for maintenance");
        assert_eq!(words(&json!({ "a": 1 })), "{\"a\":1}");
        assert_eq!(words(&json!("x".repeat(500))).len(), WORDS_MOST);
    }

    #[test]
    fn a_pake_body_is_read_as_magic_wormhole_writes_it() {
        assert_eq!(pake_of(br#"{"pake_v1": "53ab"}"#), Some(vec![0x53, 0xab]));
        assert_eq!(pake_of(br#"{"pake_v2": "53ab"}"#), None);
        assert_eq!(pake_of(b"not json"), None);
    }

    #[test]
    fn a_message_needs_its_side_its_phase_and_a_body_in_hex() {
        let whole = json!({ "type": "message", "side": "ab", "phase": "pake", "body": "00ff" });
        let read = message_of(&whole).expect("a message");
        assert_eq!((read.side.as_str(), read.phase.as_str(), read.body.as_slice()), ("ab", "pake", &[0u8, 0xff][..]));
        for key in ["side", "phase", "body"] {
            let mut cut = whole.clone();
            cut.as_object_mut().unwrap().remove(key);
            assert_eq!(message_of(&cut), None, "no {key}");
        }
        assert_eq!(message_of(&json!({ "side": "a", "phase": "0", "body": "xyz" })), None, "a body that is no hex");
        assert_eq!(message_of(&json!({ "side": 1, "phase": "0", "body": "00" })), None, "a side that is no text");
    }

    #[test]
    fn a_frame_is_one_json_object_or_nothing() {
        assert!(object(br#"{"type":"ack"}"#).is_some());
        for other in [&b"[]"[..], b"\"x\"", b"7", b"null", b"{", b"", &[0xff, 0xfe]] {
            assert_eq!(object(other), None, "{other:?}");
        }
    }
}
