//! The keeper of a TCP relay's allocation (RFC 6062, 4.6): the control
//! connection stays open while the data connection lives, and a Refresh on
//! it before the lifetime ends (PodUtils/podnet#485, TURN-2). The task ends
//! with the allocation when [`crate::RelayedStream`] is dropped.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_core::entropy::Entropy;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::stun::{self, Class, LIFETIME, REFRESH, TransactionId, message_type};

use super::exchange::{self, Auth, Value};
use super::relay::Status;
use super::wire::Conn;
use super::{Clock, Error, Limits};

/// Everything that the control connection needs while the data connection
/// lives.
pub(super) struct Keeping {
    pub(super) control: Conn,
    pub(super) auth: Auth,
    pub(super) username: String,
    pub(super) software: Option<String>,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) limits: Limits,
    pub(super) lifetime: Duration,
    pub(super) granted: Instant,
    pub(super) entropy: Box<dyn Entropy + Send>,
    pub(super) status: Arc<Mutex<Status>>,
}

impl Keeping {
    /// The task that refreshes the allocation until a refresh fails or the
    /// stream is dropped.
    pub(super) fn spawn(self) -> JoinHandle<()> {
        let Keeping {
            mut control,
            mut auth,
            username,
            software,
            clock,
            limits,
            mut lifetime,
            mut granted,
            mut entropy,
            status,
        } = self;
        tokio::spawn(async move {
            loop {
                let due = refresh_at(granted, lifetime, limits.refresh_before);
                clock.sleep_until(due).await;
                match refresh_once(
                    &mut control,
                    &mut auth,
                    &username,
                    software.as_deref(),
                    &clock,
                    &limits,
                    &mut entropy,
                )
                .await
                {
                    Ok(next) => {
                        lifetime = next;
                        granted = clock.now();
                        if let Ok(mut status) = status.lock() {
                            status.lifetime = next;
                        }
                    }
                    Err(error) => {
                        tracing::debug!(%error, "TURN: the allocation's refresh failed");
                        return;
                    }
                }
            }
        })
    }
}

/// When to refresh a thing given at `given` for `lifetime`: `before` its
/// end, and not before half of its lifetime.
fn refresh_at(given: Instant, lifetime: Duration, before: Duration) -> Instant {
    given + lifetime.saturating_sub(before).max(lifetime / 2)
}

async fn refresh_once(
    control: &mut Conn,
    auth: &mut Auth,
    username: &str,
    software: Option<&str>,
    clock: &Arc<dyn Clock>,
    limits: &Limits,
    entropy: &mut Box<dyn Entropy + Send>,
) -> Result<Duration, Error> {
    let mut stale = 0u32;
    loop {
        let id = new_id(entropy)?;
        let secs = u32::try_from(limits.lifetime.as_secs()).unwrap_or(u32::MAX);
        let attributes = [(LIFETIME, Value::Bytes(secs.to_be_bytes().to_vec()))];
        let request = exchange::request_with(Some(auth), username, software, REFRESH, &id, &attributes);
        let reply = exchange::exchange_on(control, &request, &id, clock.as_ref(), limits, REFRESH).await?;
        let read = stun::decode(&reply).map_err(Error::Decode)?;
        let kind = read.kind;
        if kind == message_type(REFRESH, Class::Success) {
            exchange::check_with(Some(auth), &reply, kind)?;
            return super::lifetime_of(&read);
        }
        if kind != message_type(REFRESH, Class::Error) {
            let why = "an answer of another type to a Refresh request".to_string();
            return Err(Error::Protocol { kind, why });
        }
        let (code, reason) = match read.error() {
            Some(Ok(error)) => error,
            Some(Err(why)) => return Err(Error::Protocol { kind, why }),
            None => return Err(Error::Protocol { kind, why: "an error answer with no ERROR-CODE".into() }),
        };
        if code == 438 && stale < limits.stale_nonces {
            let Some(nonce) = read.attribute(stun::NONCE) else {
                return Err(Error::Server { method: REFRESH, code, reason });
            };
            auth.nonce = nonce.to_vec();
            stale += 1;
            continue;
        }
        return Err(Error::Server { method: REFRESH, code, reason });
    }
}

fn new_id(entropy: &mut Box<dyn Entropy + Send>) -> Result<TransactionId, Error> {
    let mut id = [0u8; 12];
    entropy.fill(&mut id).map_err(|_| Error::NoRandom)?;
    Ok(id)
}
