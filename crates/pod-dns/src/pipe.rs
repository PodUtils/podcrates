//! One connection to one resolver of DNS over HTTPS for DNS messages of the
//! wire form (RFC 8484, a GET with `?dns=`), through the caller's dial: a
//! carrier of bytes over DNS (`pod-link`'s dns carrier behind an HTTP proxy)
//! sends each of its queries through it. Over HTTP/2 ([`Doh::http2`]) the
//! questions share the connection at the same time; over HTTP/1.1 a
//! question waits for the answer of the one before it. A failed question
//! closes the connection, and the next question opens a new one; a question
//! on a kept connection that fails goes once more on a new one, as the
//! server may have closed it.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use base64::Engine as _;
use pod_core::Dial;
use pod_proxy::Missed;
use pod_proxy::http::{self, Response};
use pod_tls::Context;
use tokio::sync::Mutex;

use crate::doh::{Doh, Form};
use crate::get::{host_header, open};
use crate::kept::Conn;
use crate::settings::Settings;
use crate::way::Way;
use crate::{MAX_ANSWER, h2};

/// The connection to one resolver, and what opens it again.
pub struct Pipe {
    resolver: Doh,
    host: String,
    path: String,
    settings: Settings,
    context: Arc<Context>,
    dial: Arc<dyn Dial>,
    conn: Mutex<Option<Conn>>,
    opened: AtomicUsize,
}

impl std::fmt::Debug for Pipe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pipe")
            .field("host", &self.host)
            .field("port", &self.resolver.port)
            .field("path", &self.path)
            .field("http2", &self.resolver.http2)
            .field("dial", &self.dial.label())
            .finish_non_exhaustive()
    }
}

impl Pipe {
    /// The connection to `resolver` through `dial`, with the trust, the user
    /// agent and the time of a connection of `settings` and the TLS of
    /// `context`. The resolver is asked by its name through a dial whose far
    /// end resolves names ([`Dial::names_inside`]), else by its address. A
    /// resolver with no wire form is an error. No connection opens before
    /// the first question.
    pub fn new(resolver: Doh, settings: Settings, context: Arc<Context>, dial: Arc<dyn Dial>) -> Result<Pipe, String> {
        let path = resolver.wire.clone().ok_or_else(|| format!("{} serves no wire form", resolver.name))?;
        let host = resolver.hosts(dial.names_inside(), &[]).remove(0);
        let conn = Mutex::new(None);
        Ok(Pipe { resolver, host, path, settings, context, dial, conn, opened: AtomicUsize::new(0) })
    }

    /// The host that a question asks: the name or the address of the
    /// resolver.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The connections that the pipe opened.
    pub fn connections(&self) -> usize {
        self.opened.load(Ordering::SeqCst)
    }

    /// One DNS message `message` to the resolver, and its answer: the status,
    /// the headers and the body (the DNS message of the answer at a `200`).
    /// The caller bounds the time: a question that it drops closes an
    /// HTTP/1.1 connection, and the next question opens a new one.
    pub async fn ask(&self, message: &[u8]) -> Result<Response, Missed> {
        let code = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(message);
        let mark = if self.path.contains('?') { '&' } else { '?' };
        let target = format!("{}{mark}dns={code}", self.path);
        match self.send(&target).await {
            (Err(_), true) => self.send(&target).await.0,
            (answer, _) => answer,
        }
    }

    /// One GET of `target` on the connection, opened when there is none, and
    /// whether the connection was kept from a question before. A failure
    /// closes the connection.
    async fn send(&self, target: &str) -> (Result<Response, Missed>, bool) {
        let accept = Form::Wire.accept();
        let mut slot = self.conn.lock().await;
        let kept = slot.is_some();
        if slot.is_none() {
            let way = Way { settings: &self.settings, context: &self.context, dial: &*self.dial, kept: None };
            match open(&way, &self.resolver, &self.host, self.settings.connect).await {
                Ok(conn) => *slot = Some(conn),
                Err(missed) => return (Err(missed), false),
            }
            self.opened.fetch_add(1, Ordering::SeqCst);
        }
        match slot.take() {
            Some(Conn::Two(conn)) => {
                let (handle, this) = (conn.handle(), self.opened.load(Ordering::SeqCst));
                *slot = Some(Conn::Two(conn));
                // The other questions go on while this one waits.
                drop(slot);
                let answer = h2::send(handle, &self.host, target, accept, MAX_ANSWER).await;
                // Close the connection of this question, not a newer one.
                if answer.is_err() && self.opened.load(Ordering::SeqCst) == this {
                    *self.conn.lock().await = None;
                }
                (answer.map_err(Missed::words), kept)
            }
            Some(Conn::One(tls)) => {
                let host = host_header(&self.resolver, &self.host);
                let headers = [("Accept", accept)];
                let agent = &self.settings.user_agent;
                let answer = http::exchange_keep(*tls, agent, "GET", &host, target, &headers, b"", MAX_ANSWER).await;
                match answer {
                    Ok((response, back)) => {
                        *slot = back.map(|tls| Conn::One(Box::new(tls)));
                        (Ok(response), kept)
                    }
                    Err(e) => (Err(Missed::words(e.to_string())), kept),
                }
            }
            None => (Err(Missed::words("no connection")), kept),
        }
    }
}
