//! The retransmission timeout of RFC 6298, within a floor and a ceiling.

use std::time::Duration;

/// The estimate of the round trip and the RTO that it gives.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Rto {
    srtt: Option<Duration>,
    rttvar: Duration,
    rto: Duration,
    floor: Duration,
    ceiling: Duration,
}

impl Rto {
    pub(crate) fn new(initial: Duration, floor: Duration, ceiling: Duration) -> Rto {
        Rto { srtt: None, rttvar: Duration::ZERO, rto: initial.clamp(floor, ceiling), floor, ceiling }
    }

    /// Take one sample of the round trip: of a frame sent once (Karn).
    pub(crate) fn sample(&mut self, rtt: Duration) {
        match self.srtt {
            None => {
                self.srtt = Some(rtt);
                self.rttvar = rtt / 2;
            }
            Some(srtt) => {
                let delta = srtt.abs_diff(rtt);
                self.rttvar = (self.rttvar * 3 + delta) / 4;
                self.srtt = Some((srtt * 7 + rtt) / 8);
            }
        }
        let srtt = self.srtt.unwrap_or(rtt);
        self.rto = (srtt + self.rttvar * 4).clamp(self.floor, self.ceiling);
    }

    /// The RTO of a frame after `backoff` timeouts: the RTO doubled each
    /// time, up to the ceiling.
    pub(crate) fn backed_off(&self, backoff: u8) -> Duration {
        let factor = 1u32 << backoff.min(16);
        self.rto.saturating_mul(factor).min(self.ceiling)
    }

    /// The RTO now.
    pub(crate) fn rto(&self) -> Duration {
        self.rto
    }

    /// The smoothed round trip, after the first sample.
    pub(crate) fn srtt(&self) -> Option<Duration> {
        self.srtt
    }
}
