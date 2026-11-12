//! The client of a tunnel through the relay: the connect information from
//! the management API, one relay session at a time, and a stream for each
//! `direct-tcpip` channel to a port of the host.
//!
//! The reconnect: when the session is gone at the open of a stream (or the
//! caller asks), the client ends the old session, which ends each stream
//! of it with `ConnectionAborted`, and tries up to
//! [`Settings::reconnect_tries`] times, each try with a fresh connect token
//! from the management API, the waits of [`Settings::backoff`] between
//! them. A refused user token or a tunnel that is gone ends the tries at
//! once. A stream that the far end closed is not opened again: the crate
//! opens a stream only when the caller asks.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use pod_core::{Class, Event, Failure, Report};
use tokio::sync::Mutex;

use crate::failure::{Step, channel_class, fail, session_gone, ssh, wrap};
use crate::info::Tunnel;
use crate::live::{self, Live};
use crate::management::{Locator, Management};
use crate::service;
use crate::settings::{Settings, Way};
use crate::stream::Stream;
use crate::token::UserToken;

/// The client of one tunnel through the relay.
#[derive(Clone)]
pub struct Relay {
    inner: Arc<Inner>,
}

struct Inner {
    management: Management,
    way: Way,
    settings: Settings,
    locator: Locator,
    user: UserToken,
    report: Report,
    live: Mutex<Option<Live>>,
    sessions: AtomicU64,
    tunnel: StdMutex<Tunnel>,
}

impl fmt::Debug for Relay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Relay")
            .field("locator", &self.inner.locator)
            .field("user", &self.inner.user)
            .field("sessions", &self.sessions())
            .finish_non_exhaustive()
    }
}

impl Relay {
    /// Get the connect information of `locator` with the token of `user`,
    /// and open the relay session: one try, whose failure names its step
    /// (management, WebSocket, SSH). The lines go to the events of
    /// `report`, with no token.
    pub async fn connect(
        way: Way,
        settings: Settings,
        locator: Locator,
        user: UserToken,
        report: Report,
    ) -> Result<Relay, Failure> {
        let management = Management::new(way.http.clone(), &settings);
        let inner = Inner {
            management,
            way,
            settings,
            locator,
            user,
            report,
            live: Mutex::new(None),
            sessions: AtomicU64::new(0),
            tunnel: StdMutex::new(Tunnel::default()),
        };
        let relay = Relay { inner: Arc::new(inner) };
        let live = relay.try_once().await?;
        *relay.inner.live.lock().await = Some(live);
        Ok(relay)
    }

    /// The ids of the tunnel, as the last connect information gave them.
    pub fn tunnel(&self) -> Tunnel {
        self.inner.tunnel.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The relay sessions opened so far: 1 after the connect, one more for
    /// each reconnect.
    pub fn sessions(&self) -> u64 {
        self.inner.sessions.load(Ordering::SeqCst)
    }

    /// One try: fresh connect information, then a relay session.
    async fn try_once(&self) -> Result<Live, Failure> {
        let inner = &self.inner;
        let info = inner.management.connect_info(&inner.locator, &inner.user).await?;
        *inner.tunnel.lock().unwrap_or_else(|e| e.into_inner()) = info.tunnel.clone();
        let number = inner.sessions.load(Ordering::SeqCst) + 1;
        let live = live::open(&inner.way, &inner.settings, &info, number).await?;
        inner.sessions.store(number, Ordering::SeqCst);
        inner.report.event(Event::info(
            "devtunnel.connected",
            format!(
                "Dev Tunnels: {} through the relay {} (host {}, session {number})",
                info.tunnel.shown(),
                live.relay,
                live.host_id
            ),
        ));
        Ok(live)
    }

    /// End the session of `slot` and open a new one, within the tries of
    /// the settings.
    async fn renew(&self, slot: &mut Option<Live>) -> Result<(), Failure> {
        let inner = &self.inner;
        if let Some(old) = slot.take() {
            old.end().await;
        }
        let tries = inner.settings.reconnect_tries.max(1);
        inner.report.event(Event::warn(
            "devtunnel.reconnect",
            format!("Dev Tunnels: the relay session of {} ended; a reconnect starts", inner.locator),
        ));
        let mut last = None;
        for n in 1..=tries {
            if n > 1 {
                tokio::time::sleep((inner.settings.backoff)(n - 1)).await;
            }
            match self.try_once().await {
                Ok(live) => {
                    *slot = Some(live);
                    inner.report.decision("devtunnel.reconnect", "connected");
                    return Ok(());
                }
                Err(failure) => {
                    tracing::debug!(try_number = n, why = %failure.chain(), "a reconnect try failed");
                    let stop = matches!(failure.class, Class::Refused | Class::NotFound | Class::Usage | Class::Config)
                        && crate::failure::step_of(&failure) == Some(Step::Management);
                    last = Some(failure);
                    if stop {
                        break;
                    }
                }
            }
        }
        inner.report.decision("devtunnel.reconnect", "failed");
        let last = last.expect("at least one try");
        Err(wrap(Step::Reconnect, format!("the tunnel {} did not connect again", inner.locator), last))
    }

    /// End the session and open a new one with a fresh connect token. Each
    /// open stream ends.
    pub async fn reconnect(&self) -> Result<(), Failure> {
        let mut slot = self.inner.live.lock().await;
        self.renew(&mut slot).await
    }

    /// The SSH handle of a live session: a reconnect first when the
    /// session is gone.
    async fn handle(&self) -> Result<(Arc<russh::client::Handle<crate::ssh::Host>>, u64), Failure> {
        let mut slot = self.inner.live.lock().await;
        if !slot.as_ref().is_some_and(Live::alive) {
            self.renew(&mut slot).await?;
        }
        let live = slot.as_ref().expect("a live session after a renew");
        Ok((live.handle.clone(), live.number))
    }

    /// A stream to `port` of the tunnel's host: a `direct-tcpip` channel to
    /// the fact `connect-host` and `port`. A session that is gone connects
    /// again first; a session that ends during the open connects again,
    /// once, and the open goes on the new one. A refusal of the host is of
    /// the step [`Step::Channel`] and is not tried again.
    pub async fn open(&self, port: u16) -> Result<Stream, Failure> {
        let timeout = self.inner.settings.timeout;
        let mut retried = false;
        loop {
            let (handle, number) = self.handle().await?;
            let host = service::connect_host();
            let opened = handle.channel_open_direct_tcpip(host, u32::from(port), host, 0);
            let e = match tokio::time::timeout(timeout, opened).await {
                Ok(Ok(channel)) => return Ok(Stream::new(channel, port, number)),
                Err(_) => {
                    return Err(fail(
                        Step::Channel,
                        Class::Timeout,
                        format!("no answer for port {port} within {timeout:?}"),
                    ));
                }
                Ok(Err(e)) => e,
            };
            if let russh::Error::ChannelOpenFailure(why) = &e {
                return Err(fail(Step::Channel, channel_class(why), format!("the host refused port {port}: {why:?}")));
            }
            if !session_gone(&e) || retried {
                return Err(ssh(Step::Channel, format!("the open of port {port}"), &e));
            }
            retried = true;
            let mut slot = self.inner.live.lock().await;
            // Another open may have connected again since.
            if slot.as_ref().is_some_and(|live| live.number == number) {
                self.renew(&mut slot).await?;
            }
        }
    }

    /// End the relay session. Each open stream ends; a later open connects
    /// again.
    pub async fn close(&self) {
        if let Some(live) = self.inner.live.lock().await.take() {
            live.end().await;
        }
    }
}
