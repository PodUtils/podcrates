//! The shape of an HTTP/3 server (PodUtils/podnet#486, QUIC-6): the `h3`
//! ALPN, the control stream, the SETTINGS frame, and a congestion profile
//! from a named set.
//!
//! A distinctive controller is a fingerprint of itself, so the default is
//! the common shape: Cubic, the controller that noq and the large servers
//! take, with the SETTINGS that an HTTP/3 server usually sends. A caller
//! selects another shape by name, as a `--h3-shape` value does, with
//! [`Shape::parse`].
//!
//! [`ShapeReport`] is the data of `doctor`: the name of the congestion
//! profile, and whether the control stream and its SETTINGS were seen.

use pod_core::{Class, Failure, Stage};
use pod_quic::noq::TransportConfig;
use pod_quic::noq::congestion::{Bbr3Config, CubicConfig, NewRenoConfig};

/// The named congestion profiles of a shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Profile {
    /// Cubic (RFC 9438): the common profile, and the default of noq.
    #[default]
    Cubic,
    /// NewReno (RFC 9002, section 7).
    NewReno,
    /// BBRv3, the draft of the model-based controller.
    Bbr,
}

impl Profile {
    /// The name of the profile in the text of a shape.
    pub fn name(self) -> &'static str {
        match self {
            Profile::Cubic => "cubic",
            Profile::NewReno => "newreno",
            Profile::Bbr => "bbr",
        }
    }

    /// Every profile of the set, in the order of the common shape first.
    pub fn all() -> [Profile; 3] {
        [Profile::Cubic, Profile::NewReno, Profile::Bbr]
    }

    /// The profile of `text` (`cubic`, `newreno`, `bbr`); another text is
    /// `Config`.
    pub fn parse(text: &str) -> Result<Profile, Failure> {
        match text.trim().to_ascii_lowercase().as_str() {
            "cubic" => Ok(Profile::Cubic),
            "newreno" | "reno" => Ok(Profile::NewReno),
            "bbr" => Ok(Profile::Bbr),
            other => Err(Failure::new(
                Stage::Warp,
                Class::Config,
                format!("{other:?} is not a congestion profile (cubic, newreno, bbr)"),
            )),
        }
    }

    /// The controller of this profile in `transport`.
    fn configure(self, transport: &mut TransportConfig) {
        match self {
            Profile::Cubic => transport.congestion_controller_factory(std::sync::Arc::new(CubicConfig::default())),
            Profile::NewReno => transport.congestion_controller_factory(std::sync::Arc::new(NewRenoConfig::default())),
            Profile::Bbr => transport.congestion_controller_factory(std::sync::Arc::new(Bbr3Config::default())),
        };
    }
}

/// The SETTINGS of the shape (RFC 9114, section 7.2.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// SETTINGS_QPACK_MAX_TABLE_CAPACITY: 0, the static table only.
    pub qpack_max_table_capacity: u64,
    /// SETTINGS_MAX_FIELD_SECTION_SIZE.
    pub max_field_section_size: u64,
    /// SETTINGS_ENABLE_CONNECT_PROTOCOL: the extended CONNECT (RFC 9220).
    pub enable_connect_protocol: bool,
    /// SETTINGS_H3_DATAGRAM: the HTTP datagrams (RFC 9297).
    pub h3_datagram: bool,
}

impl Default for Settings {
    /// The common shape of a server: no dynamic table, a field section of
    /// 16 KiB, and the extended CONNECT and datagrams on.
    fn default() -> Settings {
        Settings {
            qpack_max_table_capacity: 0,
            max_field_section_size: 16 * 1024,
            enable_connect_protocol: true,
            h3_datagram: true,
        }
    }
}

impl Settings {
    /// The identifiers and values of the SETTINGS frame, in order.
    pub fn pairs(&self) -> Vec<(u64, u64)> {
        let mut out = vec![(0x01, self.qpack_max_table_capacity), (0x06, self.max_field_section_size)];
        if self.enable_connect_protocol {
            out.push((0x08, 1));
        }
        if self.h3_datagram {
            out.push((0x33, 1));
        }
        out
    }

    /// Whether the pairs of a SETTINGS frame carry `(id, value)`.
    pub fn has(pairs: &[(u64, u64)], id: u64) -> Option<u64> {
        pairs.iter().find(|(k, _)| *k == id).map(|(_, v)| *v)
    }
}

/// The shape of an HTTP/3 server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    /// The ALPN of the listener: `h3`.
    pub alpn: Vec<Vec<u8>>,
    pub profile: Profile,
    pub settings: Settings,
    /// The control stream of the server, with a SETTINGS frame, before any
    /// answer.
    pub control_stream: bool,
}

impl Default for Shape {
    fn default() -> Shape {
        Shape::common()
    }
}

impl Shape {
    /// The common shape: the `h3` ALPN, the control stream, the SETTINGS of
    /// a usual server, and Cubic.
    pub fn common() -> Shape {
        Shape {
            alpn: vec![b"h3".to_vec()],
            profile: Profile::Cubic,
            settings: Settings::default(),
            control_stream: true,
        }
    }

    /// The common shape with the congestion profile `profile`.
    pub fn with_profile(profile: Profile) -> Shape {
        Shape { profile, ..Shape::common() }
    }

    /// The shape of a `--h3-shape` value: `common` (Cubic, the default),
    /// `cubic`, `newreno`, `bbr`, or one of those with `-no-control` for a
    /// server that sends no control stream (a negative shape of a test).
    pub fn parse(text: &str) -> Result<Shape, Failure> {
        let text = text.trim().to_ascii_lowercase();
        let (name, control) = match text.strip_suffix("-no-control") {
            Some(name) => (name.to_string(), false),
            None => (text, true),
        };
        let shape = match name.as_str() {
            "" | "common" => Shape::common(),
            other => Shape::with_profile(Profile::parse(other)?),
        };
        Ok(Shape { control_stream: control, ..shape })
    }

    /// The name of the shape: `common` for the default, else the profile.
    pub fn name(&self) -> &'static str {
        match (self, self.control_stream) {
            (shape, true) if *shape == Shape::common() => "common",
            (_, true) => self.profile.name(),
            (_, false) => "no-control",
        }
    }

    /// The transport of the shape, on the timings of `base`, with its
    /// congestion profile.
    pub fn transport(&self, base: &pod_quic::Settings) -> TransportConfig {
        let mut transport = base.transport(true);
        self.profile.configure(&mut transport);
        transport
    }

    /// The payload of the SETTINGS frame.
    pub fn settings_payload(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (id, value) in self.settings.pairs() {
            pod_warp_varint(&mut out, id);
            pod_warp_varint(&mut out, value);
        }
        out
    }
}

/// A QUIC variable-length integer (RFC 9000, section 16).
fn pod_warp_varint(out: &mut Vec<u8>, value: u64) {
    crate::capsule::put_varint(out, value);
}

/// The report of a shape, for `doctor`: the congestion profile and whether
/// the control stream and its SETTINGS were observed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShapeReport {
    /// The name of the shape.
    pub shape: String,
    /// The name of the congestion profile.
    pub congestion: String,
    /// The connection of the peer used the `h3` ALPN.
    pub alpn_h3: bool,
    /// The control stream of the peer was observed.
    pub control_stream: bool,
    /// A SETTINGS frame was observed on the control stream.
    pub settings: bool,
    /// The identifiers of the SETTINGS frame.
    pub setting_ids: Vec<u64>,
}

impl ShapeReport {
    /// The report of `shape`, and of the events that the listener saw.
    pub fn of(shape: &Shape, log: &crate::h3::ShapeLog) -> ShapeReport {
        let events = log.events();
        let control = events.iter().any(|e| matches!(e, crate::h3::ShapeEvent::ControlStream { .. }));
        let settings = events.iter().any(|e| matches!(e, crate::h3::ShapeEvent::SettingsSent(_)));
        let mut ids = Vec::new();
        for event in &events {
            if let crate::h3::ShapeEvent::SettingsSent(pairs) = event {
                ids = pairs.iter().map(|(id, _)| *id).collect();
            }
        }
        ShapeReport {
            shape: shape.name().to_string(),
            congestion: shape.profile.name().to_string(),
            alpn_h3: shape.alpn.iter().any(|a| a == b"h3"),
            control_stream: control,
            settings,
            setting_ids: ids,
        }
    }

    /// The line of `doctor`: the profile and the two observations.
    pub fn line(&self) -> String {
        format!(
            "h3-shape={} congestion={} control-stream={} settings={}",
            self.shape,
            self.congestion,
            yes(self.control_stream),
            yes(self.settings)
        )
    }
}

fn yes(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_common_shape_is_the_default_and_cubic() {
        assert_eq!(Shape::default(), Shape::common());
        assert_eq!(Shape::default().profile, Profile::Cubic);
        assert_eq!(Shape::default().alpn, vec![b"h3".to_vec()]);
        assert!(Shape::default().control_stream);
        assert_eq!(Shape::default().name(), "common");
        let pairs = Shape::common().settings.pairs();
        assert_eq!(Settings::has(&pairs, 0x01), Some(0));
        assert_eq!(Settings::has(&pairs, 0x06), Some(16 * 1024));
        assert_eq!(Settings::has(&pairs, 0x08), Some(1));
        assert_eq!(Settings::has(&pairs, 0x33), Some(1));
    }

    #[test]
    fn a_shape_is_selected_by_name_and_another_name_is_a_config_failure() {
        assert_eq!(Shape::parse("common").unwrap().profile, Profile::Cubic);
        assert_eq!(Shape::parse("").unwrap(), Shape::common());
        assert_eq!(Shape::parse("CUBIC").unwrap().profile, Profile::Cubic);
        assert_eq!(Shape::parse("newreno").unwrap().profile, Profile::NewReno);
        assert_eq!(Shape::parse("bbr").unwrap().profile, Profile::Bbr);
        assert_eq!(Shape::parse("bbr").unwrap().name(), "bbr");
        let none = Shape::parse("cubic-no-control").unwrap();
        assert!(!none.control_stream);
        assert_eq!(none.name(), "no-control");
        for bad in ["bbr4", "fast", "cubic-no"] {
            let err = Shape::parse(bad).unwrap_err();
            assert_eq!(err.class, Class::Config, "{bad}");
        }
    }

    /// Each profile builds its transport: a name of the set is a controller
    /// that noq takes.
    #[test]
    fn every_profile_builds_a_transport() {
        let base = pod_quic::Settings::default();
        for profile in Profile::all() {
            let shape = Shape::with_profile(profile);
            let _ = shape.transport(&base);
            assert!(!shape.profile.name().is_empty());
        }
    }

    /// The report of a run without events says that nothing was observed.
    #[test]
    fn a_report_with_no_events_names_the_two_absences() {
        let shape = Shape::common();
        let log = crate::h3::ShapeLog::default();
        let report = ShapeReport::of(&shape, &log);
        assert_eq!(report.congestion, "cubic");
        assert!(report.alpn_h3);
        assert!(!report.control_stream && !report.settings);
        assert_eq!(report.line(), "h3-shape=common congestion=cubic control-stream=no settings=no");
    }
}
