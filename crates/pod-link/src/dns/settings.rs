//! The settings of the DNS carrier where podnet gives a value (8 datagrams
//! in flight, 20 queries a second, a backoff of 0 to 2 s, an answer of 1232
//! bytes with EDNS0; PodUtils/podnet#468, row E2-3) and where it gives none
//! (call C274).

use std::fmt;
use std::time::Duration;

/// The EDNS0 size of the answers, so a path does not fragment them (DNS Flag
/// Day 2020).
pub const EDNS_SIZE: u16 = 1232;

/// What the DNS carrier sends and waits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DnsSettings {
    /// The queries that the dialer starts in one second (a token bucket).
    /// podnet: 20. A loopback test gives more.
    pub queries_per_second: f64,
    /// The datagrams up in flight: the queries that wait for an answer at
    /// once. podnet: 8. 1 to 1024.
    pub in_flight: usize,
    /// The wait for the answer of one query; a query with no answer is a
    /// lost datagram, and the ARQ sends it again.
    pub query_timeout: Duration,
    /// The shortest wait before a poll when the dialer has nothing to send:
    /// podnet's backoff floor (near 0). The rate still bounds the polls.
    pub poll_min: Duration,
    /// The longest wait between polls when the dialer has nothing to send:
    /// it polls so that the listener can send its bytes down. The wait
    /// doubles from [`DnsSettings::poll_min`] to here and resets at the next
    /// datagram up. podnet's backoff ceiling, 2 s.
    pub poll_max: Duration,
    /// The largest answer that the listener builds, its OPT record included.
    pub answer: u16,
    /// The datagrams that wait in each direction before more are lost.
    pub depth: usize,
}

impl Default for DnsSettings {
    fn default() -> DnsSettings {
        DnsSettings {
            queries_per_second: 20.0,
            in_flight: 8,
            query_timeout: Duration::from_secs(2),
            poll_min: Duration::from_millis(5),
            poll_max: Duration::from_secs(2),
            answer: EDNS_SIZE,
            depth: 256,
        }
    }
}

impl DnsSettings {
    /// The shortest wait between the start of two queries.
    pub(crate) fn period(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.queries_per_second)
    }

    /// Each value against its limits.
    pub fn check(&self) -> Result<(), DnsSettingsError> {
        use DnsSettingsError as E;
        if !self.queries_per_second.is_finite() || self.queries_per_second <= 0.0 {
            return Err(E::Rate(self.queries_per_second));
        }
        if !(1..=1024).contains(&self.in_flight) {
            return Err(E::InFlight(self.in_flight));
        }
        if self.query_timeout.is_zero() || self.poll_min.is_zero() || self.poll_max < self.poll_min {
            return Err(E::Zero);
        }
        if self.answer < 512 {
            return Err(E::Answer(self.answer));
        }
        if self.depth == 0 {
            return Err(E::Zero);
        }
        Ok(())
    }
}

/// A setting of the DNS carrier out of its limits.
#[derive(Debug, Clone, PartialEq)]
pub enum DnsSettingsError {
    Rate(f64),
    InFlight(usize),
    Answer(u16),
    Zero,
}

impl fmt::Display for DnsSettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use DnsSettingsError as E;
        match self {
            E::Rate(v) => write!(f, "{v} queries a second: give more than 0"),
            E::InFlight(v) => write!(f, "{v} datagrams in flight: give 1 to 1024"),
            E::Answer(v) => write!(f, "an answer of {v} bytes: give 512 or more"),
            E::Zero => f.write_str("a time or a depth of 0"),
        }
    }
}

impl std::error::Error for DnsSettingsError {}
