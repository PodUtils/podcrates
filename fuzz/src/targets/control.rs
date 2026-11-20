//! The fuzz target of the lines of `pod-control`.

use pod_control::wire::{self, Code, Inbound};

/// One line of the control API (`pod_control::wire::parse`; new in
/// `pod-control`, T-103): untrusted bytes of a peer. A line that does not
/// read is `bad-request`; a line that reads, written again, reads back to
/// the same id, method, `for` and outcome. The params and the result keep
/// their keys: serde_json reads a float with no exact round trip.
pub fn control_line(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else { return };
    match wire::parse(text) {
        Err((_, fault)) => assert_eq!(fault.code, Code::BadRequest, "{fault}"),
        Ok(Inbound::Request(r)) => {
            let line = wire::request_line(r.id, &r.method, &r.params, r.for_id);
            let Ok(Inbound::Request(back)) = wire::parse(line.trim_end()) else { panic!("{line}") };
            assert_eq!((back.id, &back.method, back.for_id), (r.id, &r.method, r.for_id));
            assert_eq!(keys(&back.params), keys(&r.params));
        }
        Ok(Inbound::Answer { id, outcome }) => {
            let line = wire::answer_line(id, &outcome);
            let Ok(Inbound::Answer { id: back_id, outcome: back }) = wire::parse(line.trim_end()) else {
                panic!("{line}")
            };
            assert_eq!(back_id, id);
            match (back, outcome) {
                (Ok(b), Ok(o)) => assert_eq!(keys(&b), keys(&o)),
                (Err(b), Err(o)) => assert_eq!(b, o),
                other => panic!("{other:?}"),
            }
        }
    }
}

fn keys(v: &serde_json::Value) -> Vec<String> {
    v.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default()
}
