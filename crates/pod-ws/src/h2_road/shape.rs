//! The shape of this client's HTTP/2 as data: the SETTINGS that it sends and
//! their order, the connection's window, the order of its request's fields,
//! the PRIORITY frames, and the padding of a DATA frame above a threshold. A
//! road that claims to look like a browser needs the browser's shape as a
//! value that a test compares field by field, not as numbers that each
//! call site sets.
//!
//! [`Shape::browser`] is the shape of Chrome as Akamai's passive HTTP/2
//! fingerprints record it: `1:65536;3:1000;4:6291456`, a connection window
//! increment of 15,663,105, the pseudo-fields `m,a,s,p`, and no PRIORITY
//! frame; a later capture of a live connection adds `6:262144` after them.
//! [`Shape::firefox`] is Firefox's:
//! `1:65536;4:131072;5:16384|12517377|3:0:0:201,5:0:0:101,7:0:0:1,9:0:7:1,11:0:3:1|m,p,a,s`.
//! [`Shape::existing`] is the shape that this road has sent until now, and
//! the default: another one is opt-in, so that no compatibility test changes
//! its bytes without a caller asking.
//!
//! [`Shape::apply`] sets every SETTINGS parameter that the shape names,
//! including `SETTINGS_MAX_CONCURRENT_STREAMS` (id 3), the connection's
//! window as the target from which h2 computes the WINDOW_UPDATE increment,
//! and the order of the request's pseudo-fields of [`Shape::headers`]: the
//! fork pod-h2 of `vendor/h2` takes it as a setting of the client. A
//! pseudo-field that the shape does not name (`:protocol` of a browser) comes
//! after them, in the order of h2. A PRIORITY frame or a padded DATA frame is data alone: h2 0.4 has no
//! call for either, so it is recorded in the shape and asserted against a
//! reference, and the live connection sends neither.

use super::{CONNECTION_WINDOW, STREAM_WINDOW};

/// A SETTINGS parameter, by its number (RFC 9113, section 6.5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting {
    pub id: u16,
    pub value: u32,
}

/// SETTINGS_HEADER_TABLE_SIZE.
pub const HEADER_TABLE_SIZE: u16 = 0x1;
/// SETTINGS_ENABLE_PUSH.
pub const ENABLE_PUSH: u16 = 0x2;
/// SETTINGS_MAX_CONCURRENT_STREAMS.
pub const MAX_CONCURRENT_STREAMS: u16 = 0x3;
/// SETTINGS_INITIAL_WINDOW_SIZE.
pub const INITIAL_WINDOW_SIZE: u16 = 0x4;
/// SETTINGS_MAX_FRAME_SIZE.
pub const MAX_FRAME_SIZE: u16 = 0x5;
/// SETTINGS_MAX_HEADER_LIST_SIZE.
pub const MAX_HEADER_LIST_SIZE: u16 = 0x6;
/// SETTINGS_ENABLE_CONNECT_PROTOCOL (RFC 8441).
pub const ENABLE_CONNECT_PROTOCOL: u16 = 0x8;

/// A PRIORITY frame (RFC 7540, section 6.3): the stream that it names, the
/// stream it depends on, whether the dependency is exclusive, and the
/// weight as the frame carries it, from 1 to 256.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Priority {
    pub stream: u32,
    pub depends: u32,
    pub exclusive: bool,
    pub weight: u16,
}

impl Priority {
    /// The frame's bytes: its header, the dependency, and the weight less 1.
    pub fn frame(&self) -> Vec<u8> {
        let depends = if self.exclusive { self.depends | 0x8000_0000 } else { self.depends };
        let weight = u8::try_from(self.weight.saturating_sub(1).min(255)).unwrap_or(255);
        [&[0, 0, 5, 0x2, 0][..], &self.stream.to_be_bytes()[..], &depends.to_be_bytes()[..], &[weight]].concat()
    }
}

/// The padding of a DATA frame: a frame whose payload is larger than
/// `above` bytes is padded to `to` bytes, with the pad's length in the
/// frame's own field (RFC 9113, section 6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Padding {
    pub above: usize,
    pub to: usize,
}

/// The shape of one client's HTTP/2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    /// The SETTINGS parameters, in the order of the frame.
    pub settings: &'static [Setting],
    /// The connection's window target. h2 raises stream 0 from its initial
    /// 65,535 bytes to this target, so the WINDOW_UPDATE on the wire carries
    /// the increment `connection_window - 65_535`.
    pub connection_window: u32,
    /// The order of the request's fields: the pseudo-fields first.
    pub headers: &'static [&'static str],
    /// The PRIORITY frames, in the order of their streams.
    pub priority: &'static [Priority],
    /// The padding of a large DATA frame, where the shape pads.
    pub padding: Option<Padding>,
}

impl Shape {
    /// The shape that this road has sent: the windows of this module's
    /// constants, no push, the CONNECT's fields in the order that the
    /// request sets them, no PRIORITY frame and no padding.
    pub const fn existing() -> Shape {
        Shape {
            settings: &[
                Setting { id: INITIAL_WINDOW_SIZE, value: STREAM_WINDOW },
                Setting { id: ENABLE_PUSH, value: 0 },
            ],
            connection_window: CONNECTION_WINDOW,
            headers: &[
                ":method",
                ":scheme",
                ":authority",
                ":path",
                ":protocol",
                "sec-websocket-version",
                "user-agent",
                "x-relay-token",
            ],
            priority: &[],
            padding: None,
        }
    }

    /// Chrome's shape, as Akamai's passive fingerprints record it: the paper
    /// reads Chrome 58 as `1:65536;3:1000;4:6291456|15663105|0|m,a,s,p`, and
    /// a later capture of a live connection adds
    /// `SETTINGS_MAX_HEADER_LIST_SIZE` 262144 after them.
    pub const fn browser() -> Shape {
        Shape {
            settings: &[
                Setting { id: HEADER_TABLE_SIZE, value: 65_536 },
                Setting { id: MAX_CONCURRENT_STREAMS, value: 1_000 },
                Setting { id: INITIAL_WINDOW_SIZE, value: 6_291_456 },
                Setting { id: MAX_HEADER_LIST_SIZE, value: 262_144 },
            ],
            // Chrome's WINDOW_UPDATE increment is 15,663,105 from the
            // connection's initial 65,535, so the target is 15,728,640.
            connection_window: 15_728_640,
            headers: &[":method", ":authority", ":scheme", ":path"],
            priority: &[],
            padding: None,
        }
    }

    /// Firefox's shape, as Akamai's paper records it:
    /// `1:65536;4:131072;5:16384|12517377|3:0:0:201,5:0:0:101,7:0:0:1,9:0:7:1,11:0:3:1|m,p,a,s`.
    pub const fn firefox() -> Shape {
        Shape {
            settings: &[
                Setting { id: HEADER_TABLE_SIZE, value: 65_536 },
                Setting { id: INITIAL_WINDOW_SIZE, value: 131_072 },
                Setting { id: MAX_FRAME_SIZE, value: 16_384 },
            ],
            // Firefox's increment is 12,517,377 from the initial 65,535.
            connection_window: 12_582_912,
            headers: &[":method", ":path", ":authority", ":scheme"],
            priority: &[
                Priority { stream: 3, depends: 0, exclusive: false, weight: 201 },
                Priority { stream: 5, depends: 0, exclusive: false, weight: 101 },
                Priority { stream: 7, depends: 0, exclusive: false, weight: 1 },
                Priority { stream: 9, depends: 7, exclusive: false, weight: 1 },
                Priority { stream: 11, depends: 3, exclusive: false, weight: 1 },
            ],
            padding: None,
        }
    }

    /// This shape with the padding of a large DATA frame: a payload above
    /// `above` bytes goes to `to` bytes. A caller that says it pads, and not
    /// a browser's own shape.
    pub const fn padded(self, above: usize, to: usize) -> Shape {
        Shape { padding: Some(Padding { above, to }), ..self }
    }

    /// The value of a SETTINGS parameter, or none.
    pub fn setting(&self, id: u16) -> Option<u32> {
        self.settings.iter().find(|s| s.id == id).map(|s| s.value)
    }

    /// The SETTINGS frame's bytes: its header, then each parameter's 16-bit
    /// number and 32-bit value, in the shape's order.
    pub fn settings_frame(&self) -> Vec<u8> {
        let len = 6 * self.settings.len();
        let mut out = vec![(len >> 16) as u8, (len >> 8) as u8, len as u8, 0x4, 0, 0, 0, 0, 0];
        for setting in self.settings {
            out.extend_from_slice(&setting.id.to_be_bytes());
            out.extend_from_slice(&setting.value.to_be_bytes());
        }
        out
    }

    /// The WINDOW_UPDATE frame that h2 sends for the connection's window:
    /// its header, then the increment above the initial 65,535 bytes.
    pub fn window_frame(&self) -> Vec<u8> {
        let initial = 65_535;
        let increment = self.connection_window.saturating_sub(initial) & 0x7fff_ffff;
        let mut out = vec![0, 0, 4, 0x8, 0, 0, 0, 0, 0];
        out.extend_from_slice(&increment.to_be_bytes());
        out
    }

    /// The bytes of each PRIORITY frame, in the shape's order.
    pub fn priority_frames(&self) -> Vec<Vec<u8>> {
        self.priority.iter().map(Priority::frame).collect()
    }

    /// The length that a DATA frame's payload takes, padding included, or
    /// none where the shape pads not.
    pub fn pads(&self, payload: usize) -> Option<usize> {
        let padding = self.padding?;
        (payload > padding.above && padding.to > payload).then_some(padding.to)
    }

    /// The pseudo-fields of [`Shape::headers`], in their order.
    fn pseudo_fields(&self) -> Vec<h2::client::PseudoField> {
        use h2::client::PseudoField;
        let field = |name: &str| match name {
            ":method" => Some(PseudoField::Method),
            ":scheme" => Some(PseudoField::Scheme),
            ":authority" => Some(PseudoField::Authority),
            ":path" => Some(PseudoField::Path),
            ":protocol" => Some(PseudoField::Protocol),
            _ => None,
        };
        self.headers.iter().filter_map(|name| field(name)).collect()
    }

    /// Sets on `builder` each SETTINGS parameter that the library takes, the
    /// connection's window, and the order of the request's pseudo-fields.
    pub fn apply(&self, builder: &mut h2::client::Builder) {
        if let Some(value) = self.setting(INITIAL_WINDOW_SIZE) {
            builder.initial_window_size(value);
        }
        if let Some(value) = self.setting(ENABLE_PUSH) {
            builder.enable_push(value != 0);
        }
        if let Some(value) = self.setting(MAX_CONCURRENT_STREAMS) {
            builder.max_concurrent_streams(value);
        }
        if let Some(value) = self.setting(MAX_FRAME_SIZE) {
            builder.max_frame_size(value);
        }
        if let Some(value) = self.setting(HEADER_TABLE_SIZE) {
            builder.header_table_size(value);
        }
        if let Some(value) = self.setting(MAX_HEADER_LIST_SIZE) {
            builder.max_header_list_size(value);
        }
        builder.initial_connection_window_size(self.connection_window);
        builder.pseudo_order(&self.pseudo_fields());
    }

    /// The shape in words, for a line of `-v` or a doctor.
    pub fn words(&self) -> String {
        let settings: Vec<String> = self.settings.iter().map(|s| format!("{}:{}", s.id, s.value)).collect();
        let window = self.connection_window.saturating_sub(65_535);
        let headers = self.headers.join(",");
        format!("SETTINGS {} | window {window} | {headers} | {} PRIORITY", settings.join(";"), self.priority.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The recorded references: Chrome's WINDOW_UPDATE increment and
    /// Firefox's five PRIORITY frames, as a capture of each writes them.
    const CHROME_WINDOW_INCREMENT: u32 = 15_663_105;
    const FIREFOX_WINDOW_INCREMENT: u32 = 12_517_377;
    const FIREFOX_PRIORITY: [&str; 5] = [
        "00000502000000000300000000c8",
        "0000050200000000050000000064",
        "0000050200000000070000000000",
        "0000050200000000090000000700",
        "00000502000000000b0000000300",
    ];

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The SETTINGS parameters of a frame, in the order of the frame.
    fn settings_in_order(frame: &[u8]) -> Vec<(u16, u32)> {
        assert_eq!(frame[3], 0x4, "a SETTINGS frame");
        let len = (usize::from(frame[0]) << 16) | (usize::from(frame[1]) << 8) | usize::from(frame[2]);
        let (parameters, rest) = frame[9..9 + len].as_chunks::<6>();
        assert!(rest.is_empty(), "a SETTINGS payload is whole parameters");
        parameters
            .iter()
            .map(|c| (u16::from_be_bytes([c[0], c[1]]), u32::from_be_bytes([c[2], c[3], c[4], c[5]])))
            .collect()
    }

    /// The increment of a WINDOW_UPDATE frame.
    fn window_increment(frame: &[u8]) -> u32 {
        assert_eq!(frame[3], 0x8, "a WINDOW_UPDATE frame");
        u32::from_be_bytes([frame[9], frame[10], frame[11], frame[12]]) & 0x7fff_ffff
    }

    /// H2-1: the browser's shape is data, and its first frames are the
    /// fields that a recorded reference holds, in order: the SETTINGS of
    /// Chrome, their order, and the increment of its connection's window.
    #[test]
    fn the_browser_shape_is_the_recorded_reference_field_by_field() {
        let chrome = Shape::browser();
        assert_eq!(
            settings_in_order(&chrome.settings_frame()),
            vec![
                (HEADER_TABLE_SIZE, 65_536),
                (MAX_CONCURRENT_STREAMS, 1_000),
                (INITIAL_WINDOW_SIZE, 6_291_456),
                (MAX_HEADER_LIST_SIZE, 262_144),
            ],
            "Chrome's SETTINGS, in order, id 3 among them"
        );
        assert_eq!(window_increment(&chrome.window_frame()), CHROME_WINDOW_INCREMENT);
        assert_eq!(chrome.connection_window, CHROME_WINDOW_INCREMENT + 65_535);
        assert_eq!(chrome.headers, vec![":method", ":authority", ":scheme", ":path"]);
        assert!(chrome.priority.is_empty() && chrome.padding.is_none(), "Chrome sends neither");
    }

    /// Firefox's shape carries the PRIORITY frames, in their order, with the
    /// weight that the frame writes one less of.
    #[test]
    fn the_firefox_shape_carries_the_recorded_priority_frames() {
        let firefox = Shape::firefox();
        let frames: Vec<String> = firefox.priority_frames().iter().map(|f| hex(f)).collect();
        assert_eq!(frames, FIREFOX_PRIORITY.to_vec());
        assert_eq!(firefox.headers, vec![":method", ":path", ":authority", ":scheme"]);
        assert_eq!(firefox.setting(MAX_FRAME_SIZE), Some(16_384));
        // The paper's own line, field by field.
        let ids: Vec<String> = firefox.settings.iter().map(|s| format!("{}:{}", s.id, s.value)).collect();
        assert_eq!(ids.join(";"), "1:65536;4:131072;5:16384");
        assert_eq!(window_increment(&firefox.window_frame()), FIREFOX_WINDOW_INCREMENT);
        assert_eq!(firefox.connection_window, FIREFOX_WINDOW_INCREMENT + 65_535);
    }

    /// The shape of this road is the shape that it sent before, and the
    /// browser's is another: no caller gets new bytes without asking.
    #[test]
    fn the_existing_shape_is_the_default_and_differs_from_the_browsers() {
        let existing = Shape::existing();
        assert_eq!(existing.setting(INITIAL_WINDOW_SIZE), Some(STREAM_WINDOW));
        assert_eq!(existing.connection_window, CONNECTION_WINDOW);
        assert!(existing.priority.is_empty() && existing.padding.is_none());
        assert_ne!(existing, Shape::browser());
        assert_ne!(Shape::browser(), Shape::firefox());
        assert_eq!(existing.headers[..5], [":method", ":scheme", ":authority", ":path", ":protocol"]);
        assert!(existing.words().contains("PRIORITY"));
    }

    /// The order of the pseudo-fields that `apply` gives h2 is the order of
    /// each shape's data, and a field that is not a pseudo-field is not in it.
    #[test]
    fn the_pseudo_fields_of_each_shape_are_its_headers_in_order() {
        use h2::client::PseudoField::{Authority, Method, Path, Protocol, Scheme};
        assert_eq!(Shape::browser().pseudo_fields(), vec![Method, Authority, Scheme, Path]);
        assert_eq!(Shape::firefox().pseudo_fields(), vec![Method, Path, Authority, Scheme]);
        assert_eq!(Shape::existing().pseudo_fields(), vec![Method, Scheme, Authority, Path, Protocol]);
    }

    /// H2-1: the padding is a rule of the shape, above a threshold.
    #[test]
    fn the_padding_applies_above_its_threshold() {
        let padded = Shape::browser().padded(1_024, 16_384);
        assert_eq!(padded.pads(1_024), None);
        assert_eq!(padded.pads(1_025), Some(16_384));
        assert_eq!(padded.pads(16_384), None, "a payload of the target is not padded again");
        assert_eq!(Shape::browser().pads(1_025), None, "a browser's own shape pads no DATA");
    }
}
