//! The settings of KCP on the udp carrier, and their limits: the rules of
//! podnet's UDP carrier (PodUtils/podnet#468, row E2-1) and the knobs of
//! KCP where podnet gives no value (call C271).

use std::fmt;
use std::time::Duration;

/// The header of a KCP segment, in bytes.
pub const KCP_HEADER: usize = kcp::KCP_OVERHEAD;

/// The largest datagram of podnet's UDP carrier: 1200 bytes.
pub const MTU: usize = 1200;

/// The payload of a segment by default: 1 KiB, so that no datagram needs
/// IP fragmentation.
pub const SEGMENT: usize = 1 << 10;

/// The smallest and the largest window of podnet's rule, in segments.
pub const MIN_WINDOW: u16 = 32;
pub const MAX_WINDOW: u16 = 1024;

/// The silence of the peer that ends a session: podnet's 20 s.
pub const IDLE: Duration = Duration::from_secs(20);

/// The settings of KCP at one end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KcpSettings {
    /// The largest datagram that this end sends and takes. 64 to 65507
    /// bytes; at least a segment and its header.
    pub mtu: usize,
    /// The largest payload of a segment. KCP puts a segment and the
    /// acknowledgements in one datagram of at most `segment` and the header.
    pub segment: usize,
    /// The send window and the receive window, in segments: 32 to 1024.
    pub window: u16,
    /// KCP's "nodelay": an RTO of 30 ms at least that grows by half at each
    /// loss, and no congestion window. It is unfair to the other flows of a
    /// path, so it is off by default.
    pub nodelay: bool,
    /// The interval of KCP's timer: 10 ms to 5 s.
    pub interval: Duration,
    /// A segment goes again after this many acknowledgements of later
    /// segments; 0 sends it again at its RTO alone.
    pub resend: u32,
    /// The silence of the peer that ends the session.
    pub idle: Duration,
}

impl Default for KcpSettings {
    /// podnet's values: datagrams of 1200 bytes, segments of 1 KiB, the
    /// congestion window of KCP, an end after 20 s of silence. A window of
    /// 256 segments, a timer of 10 ms, a fast resend after 2
    /// acknowledgements.
    fn default() -> KcpSettings {
        KcpSettings {
            mtu: MTU,
            segment: SEGMENT,
            window: 256,
            nodelay: false,
            interval: Duration::from_millis(10),
            resend: 2,
            idle: IDLE,
        }
    }
}

impl KcpSettings {
    /// Each value against its limits.
    pub fn check(&self) -> Result<(), KcpSettingsError> {
        use KcpSettingsError as E;
        if !(64..=65507).contains(&self.mtu) {
            return Err(E::Mtu(self.mtu));
        }
        if self.segment < 32 || self.segment + KCP_HEADER > self.mtu {
            return Err(E::Segment(self.segment, self.mtu));
        }
        if !(MIN_WINDOW..=MAX_WINDOW).contains(&self.window) {
            return Err(E::Window(self.window));
        }
        if !(Duration::from_millis(10)..=Duration::from_secs(5)).contains(&self.interval) {
            return Err(E::Interval(self.interval));
        }
        if self.idle < Duration::from_secs(1) {
            return Err(E::Idle(self.idle));
        }
        Ok(())
    }

    /// The interval in the milliseconds of KCP.
    pub(crate) fn interval_ms(&self) -> u32 {
        u32::try_from(self.interval.as_millis()).unwrap_or(5000)
    }
}

/// A setting of KCP out of its limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KcpSettingsError {
    Mtu(usize),
    Segment(usize, usize),
    Window(u16),
    Interval(Duration),
    Idle(Duration),
}

impl fmt::Display for KcpSettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use KcpSettingsError as E;
        match self {
            E::Mtu(v) => write!(f, "a datagram of {v} bytes: give 64 to 65507"),
            E::Segment(v, mtu) => {
                write!(f, "a segment of {v} bytes: give 32 or more, and with its header of {KCP_HEADER} at most {mtu}")
            }
            E::Window(v) => write!(f, "a window of {v} segments: give {MIN_WINDOW} to {MAX_WINDOW}"),
            E::Interval(v) => write!(f, "a timer of {v:?}: give 10 ms to 5 s"),
            E::Idle(v) => write!(f, "a silence of {v:?}: give 1 s or more"),
        }
    }
}

impl std::error::Error for KcpSettingsError {}
