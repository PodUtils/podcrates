//! A Wisp road that outlives its connection. A [`Wisp`] ends with its
//! WebSocket (its Close, a failure, or the server's CLOSE of stream 0), and
//! a road on it fails each dial after that end. A [`Reopening`] keeps the
//! URL, the [`Connect`] and the end of input, and opens a new connection on
//! the next dial after the old one ended: no task opens a connection with
//! no dial. One dial opens [`TRIES`] connections at most, with
//! `pod_core::backoff` between two, before its deadline; a failure that a
//! new try cannot change (a refused credential, a bad configuration) stops
//! the tries. After a dial that found no connection, each dial before the
//! end of the backoff fails at once with the same words, so that many dials
//! do not each knock on a server that is down.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use pod_core::{BoxFuture, Class, Connected, Failure, Target};
use pod_ws::url::WsUrl;
use tokio::time::Instant;

use super::{AtEnd, Connect, Opened, Wisp, WispError};

/// The most connections that one dial opens.
pub const TRIES: u32 = 3;

/// The connection, and the failures to open one.
#[derive(Default)]
struct State {
    wisp: Option<Arc<Wisp>>,
    /// The connections in a row that did not open.
    failures: u32,
    /// The last failure to open, and the instant before which no dial
    /// opens a connection.
    quiet: Option<(Instant, WispError)>,
}

/// A Wisp road that opens a new connection on the next dial after its
/// connection ended.
pub struct Reopening {
    url: WsUrl,
    connect: Connect,
    at_end: AtEnd,
    shown: String,
    tries: u32,
    state: Mutex<State>,
    /// Held while a dial opens a connection: the others wait, and take it.
    opening: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for Reopening {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reopening")
            .field("shown", &self.shown)
            .field("at_end", &self.at_end)
            .field("tries", &self.tries)
            .field("open", &self.is_open())
            .finish_non_exhaustive()
    }
}

impl Reopening {
    /// A road with no connection yet: the first dial opens one. Each
    /// stream's end of input does what `at_end` says.
    pub fn new(url: WsUrl, connect: Connect, at_end: AtEnd) -> Reopening {
        let shown = url.shown();
        let (state, opening) = (Mutex::default(), tokio::sync::Mutex::new(()));
        Reopening { url, connect, at_end, shown, tries: TRIES, state, opening }
    }

    /// A road whose first connection opens now, one try: a failure is the
    /// caller's at once (a check of the server before its first dial).
    pub async fn connect(url: WsUrl, connect: Connect, at_end: AtEnd) -> Result<Reopening, WispError> {
        let road = Reopening::new(url, connect, at_end);
        let wisp = Wisp::connect(&road.url, &road.connect).await?;
        road.state().wisp = Some(Arc::new(wisp));
        Ok(road)
    }

    /// The most connections that one dial opens: 1 or more.
    pub fn with_tries(mut self, tries: u32) -> Reopening {
        self.tries = tries.max(1);
        self
    }

    /// The server in a line: its URL with no query.
    pub fn shown(&self) -> &str {
        &self.shown
    }

    /// Whether a connection is open now.
    pub fn is_open(&self) -> bool {
        self.current().is_some()
    }

    /// What the opening of the open connection settled.
    pub fn opened(&self) -> Option<Opened> {
        self.current().map(|wisp| wisp.opened().clone())
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The open connection; one that ended is let go.
    fn current(&self) -> Option<Arc<Wisp>> {
        let mut state = self.state();
        if state.wisp.as_ref().is_some_and(|wisp| !wisp.is_open()) {
            state.wisp = None;
        }
        state.wisp.clone()
    }

    /// The open connection, or a new one before `deadline`.
    async fn connection(&self, target: &Target, deadline: Instant) -> Result<Arc<Wisp>, Failure> {
        if let Some(wisp) = self.current() {
            return Ok(wisp);
        }
        let late = || crate::road::late(target, &self.shown);
        let _opening = tokio::time::timeout_at(deadline, self.opening.lock()).await.map_err(|_| late())?;
        // Another dial may have opened one while this one waited.
        if let Some(wisp) = self.current() {
            return Ok(wisp);
        }
        if let Some((until, why)) = self.state().quiet.clone() {
            let left = until.saturating_duration_since(Instant::now());
            if !left.is_zero() {
                return Err(quiet(why, left));
            }
        }
        let mut tried = 0;
        loop {
            tried += 1;
            let opened = tokio::time::timeout_at(deadline, Wisp::connect(&self.url, &self.connect)).await;
            let e = match opened {
                Ok(Ok(wisp)) => {
                    let wisp = Arc::new(wisp);
                    let mut state = self.state();
                    (state.wisp, state.failures, state.quiet) = (Some(wisp.clone()), 0, None);
                    return Ok(wisp);
                }
                Ok(Err(e)) => e,
                Err(_) => {
                    self.state().failures += 1;
                    return Err(late());
                }
            };
            let wait = {
                let mut state = self.state();
                state.failures += 1;
                pod_core::backoff::backoff(state.failures)
            };
            let resume = Instant::now() + wait;
            if tried >= self.tries || !worth_again(&e) || resume >= deadline {
                self.state().quiet = Some((resume, e.clone()));
                return Err(e.into());
            }
            tokio::time::sleep_until(resume).await;
        }
    }
}

/// Whether a new connection can meet another end: the server or the path
/// to it was down or slow, not a refusal or a bad configuration.
fn worth_again(e: &WispError) -> bool {
    matches!(e.stage_and_class().1, Class::Unavailable | Class::Timeout | Class::TempFail)
}

/// The failure of a dial in the quiet time after a failure to open.
fn quiet(why: WispError, left: Duration) -> Failure {
    let (stage, class) = why.stage_and_class();
    let secs = left.as_secs_f64().ceil() as u64;
    let text = format!("{why} (no new connection to the Wisp server within {secs} s)");
    Failure::caused(stage, class, text, why)
}

impl pod_core::Dial for Reopening {
    fn label(&self) -> String {
        format!("wisp {}", self.shown)
    }

    fn proxy_label(&self, _target: &Target) -> Option<String> {
        Some(self.shown.clone())
    }

    /// A stream to `target` on the open connection, or on a new one; a
    /// stream whose connection ended under it is tried once more on a new
    /// connection.
    fn dial<'a>(&'a self, target: &'a Target, deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            let mut again = true;
            loop {
                let wisp = self.connection(target, deadline).await?;
                match super::stream_to(&wisp, target, deadline, self.at_end, &self.shown).await {
                    Err(_) if again && !wisp.is_open() => again = false,
                    other => return other,
                }
            }
        })
    }

    fn names_inside(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wisp::Reason;
    use pod_ws::ConnectError;

    #[test]
    fn only_a_server_that_was_down_or_slow_is_tried_again() {
        let down = WispError::Connect(ConnectError::Refused { status: 503, body: String::new() });
        assert!(worth_again(&down) && worth_again(&WispError::Gone { code: None, reason: String::new() }));
        assert!(!worth_again(&WispError::Refused(Reason::WRONG_PASSWORD)), "a refused credential");
        assert!(!worth_again(&WispError::Connect(ConnectError::Config("ws://".into()))));
        assert!(!worth_again(&WispError::Opening("not Wisp".into())));
        let line = quiet(down, Duration::from_millis(1500)).to_string();
        assert!(line.ends_with("(no new connection to the Wisp server within 2 s)"), "{line}");
    }
}
