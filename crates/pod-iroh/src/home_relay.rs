//! The lines of an endpoint's connection to its home relay
//! (PodUtils/podssh#283, ask 4): one when it ends and iroh dials the relay
//! again, one for each new cause of a failed dial, and one when it is back.
//! On a path that the relay alone carries, the connection of QUIC lives
//! across such a gap, and the resumable layer above it says nothing while
//! QUIC carries its bytes again: these lines say that the gap came, and how
//! long it held.

use std::time::Duration;

use iroh::{Endpoint, Watcher};
use n0_future::StreamExt;
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// One look at a home relay: its host, whether the endpoint is connected
/// to it, and the cause of its last failed dial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub host: String,
    pub connected: bool,
    pub error: Option<String>,
}

/// What the lines remember of each home relay.
#[derive(Debug, Default)]
pub struct Relays {
    seen: Vec<Seen>,
}

#[derive(Debug)]
struct Seen {
    host: String,
    connected: bool,
    /// When the connection ended, while it is not back.
    lost: Option<Instant>,
    /// The last cause said.
    error: Option<String>,
}

impl Relays {
    /// The lines of the change from the last look to `now`'s `statuses`. A
    /// relay seen for the first time gives none, connected or not: the
    /// first dial is no new dial.
    pub fn look(&mut self, now: Instant, statuses: &[Status]) -> Vec<String> {
        let mut lines = Vec::new();
        for status in statuses {
            let Some(seen) = self.seen.iter_mut().find(|s| s.host == status.host) else {
                self.seen.push(Seen {
                    host: status.host.clone(),
                    connected: status.connected,
                    lost: None,
                    error: status.error.clone(),
                });
                continue;
            };
            let host = &status.host;
            match (seen.connected, status.connected) {
                (true, false) => {
                    let why = status.error.as_deref().map(|why| format!(" ({why})")).unwrap_or_default();
                    lines.push(format!("the connection to the iroh relay {host} ended{why}; dialling it again"));
                    seen.lost = Some(now);
                    seen.error = status.error.clone();
                }
                (false, true) => {
                    if let Some(lost) = seen.lost.take() {
                        let after = now.saturating_duration_since(lost);
                        lines.push(format!("the connection to the iroh relay {host} is back after {}", seconds(after)));
                    }
                    seen.error = None;
                }
                (false, false) if status.error.is_some() && status.error != seen.error => {
                    let why = status.error.as_deref().unwrap_or_default();
                    lines.push(format!("a new dial of the iroh relay {host} failed: {why}"));
                    seen.error = status.error.clone();
                }
                _ => {}
            }
            seen.connected = status.connected;
        }
        lines
    }
}

fn seconds(d: Duration) -> String {
    format!("{:.1} s", d.as_secs_f64())
}

/// Say each change of the connection of `endpoint` to its home relays with
/// `say` ([`Relays::look`]), until the endpoint closes.
pub fn say_changes<S>(endpoint: &Endpoint, say: S) -> JoinHandle<()>
where
    S: Fn(String) + Send + 'static,
{
    let endpoint = endpoint.clone();
    tokio::spawn(async move {
        let mut changes = endpoint.home_relay_status().stream();
        let mut relays = Relays::default();
        let closed = endpoint.closed();
        tokio::pin!(closed);
        loop {
            let statuses = tokio::select! {
                () = &mut closed => return,
                next = changes.next() => match next {
                    Some(statuses) => statuses,
                    None => return,
                },
            };
            let statuses: Vec<Status> = statuses
                .iter()
                .map(|s| Status {
                    host: s.url().host_str().unwrap_or_default().to_string(),
                    connected: s.is_connected(),
                    error: s.last_error().map(|e| pod_core::text::one_line(&e.to_string())),
                })
                .collect();
            for line in relays.look(Instant::now(), &statuses) {
                say(line);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(connected: bool, error: Option<&str>) -> Vec<Status> {
        vec![Status { host: "relay.example".into(), connected, error: error.map(str::to_string) }]
    }

    #[tokio::test(start_paused = true)]
    async fn a_lost_connection_gives_a_line_each_new_cause_one_and_its_return_one() {
        let mut relays = Relays::default();
        assert!(relays.look(Instant::now(), &status(false, None)).is_empty(), "the first dial");
        assert!(relays.look(Instant::now(), &status(true, None)).is_empty(), "the first connection");
        let lost = relays.look(Instant::now(), &status(false, Some("ping timeout")));
        assert_eq!(lost, ["the connection to the iroh relay relay.example ended (ping timeout); dialling it again"]);
        assert!(relays.look(Instant::now(), &status(false, Some("ping timeout"))).is_empty(), "the same cause");
        let failed = relays.look(Instant::now(), &status(false, Some("connect timed out")));
        assert_eq!(failed, ["a new dial of the iroh relay relay.example failed: connect timed out"]);
        tokio::time::sleep(Duration::from_millis(12_500)).await;
        let back = relays.look(Instant::now(), &status(true, None));
        assert_eq!(back, ["the connection to the iroh relay relay.example is back after 12.5 s"]);
        assert!(relays.look(Instant::now(), &status(true, None)).is_empty(), "no change");
        let lost = relays.look(Instant::now(), &status(false, None));
        assert_eq!(lost, ["the connection to the iroh relay relay.example ended; dialling it again"]);
    }
}
