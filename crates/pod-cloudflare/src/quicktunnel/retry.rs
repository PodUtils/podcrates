//! The retry rules of [`super::provision`].

/// The wait of a `Retry-After` value at the clock of the system: a number
/// of seconds, or an HTTP date (IMF-fixdate), by
/// [`pod_core::clock::retry_after`].
pub fn parse_retry_after(value: &str) -> Option<std::time::Duration> {
    pod_core::clock::retry_after(value, std::time::SystemTime::now())
}

/// True for a transport error at the hop `dns`.
pub(super) fn is_dns(err: &pod_core::Failure) -> bool {
    crate::fail::stage_of(err) == Some(pod_core::Stage::Dns)
}

/// The state of the retries of a failure at name resolution.
#[derive(Default)]
pub(super) struct Dns {
    failures: u32,
    step: Option<&'static str>,
}

impl Dns {
    /// True when a retry can help: the first failure is tried once more;
    /// after it, only a change of the step of the resolver chain that
    /// answers for the host of `endpoint` gives a new try.
    pub(super) async fn again(&mut self, host: &crate::Host, endpoint: &str) -> bool {
        let name = endpoint.split("://").nth(1).unwrap_or(endpoint).split(['/', ':']).next().unwrap_or("");
        let step = host.answering_step(name).await;
        self.decide(step)
    }

    /// The decision of [`Dns::again`] for the step `step` that answers now.
    fn decide(&mut self, step: &'static str) -> bool {
        self.failures += 1;
        let again = self.failures <= 1 || self.step != Some(step);
        self.step = Some(step);
        again
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One retry, then a retry only for a new answering step.
    #[test]
    fn a_dns_failure_is_retried_once() {
        let mut dns = Dns::default();
        assert!(dns.decide("none"));
        assert!(!dns.decide("none"));
        assert!(dns.decide("doh"));
        assert!(!dns.decide("doh"));
    }
}
