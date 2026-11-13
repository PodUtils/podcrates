//! The down lane of a dial, in its form: the long poll, the simplex poll,
//! or a push of SSE or of raw chunks. Each request names the offset of the
//! next byte that the link needs (`since`), so a request again after a cut
//! gives each byte one time.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncWriteExt as _, DuplexStream};
use tokio::sync::{Notify, watch};

use super::lane::{Failed, Lane};
use super::pipes::{State, until};
use super::wire::{Event, Events};
use super::{BACKOFF_FIRST, BACKOFF_MAX, Form, MIN_HOLD, PING_EVERY};
use crate::carrier::CarrierErrorKind;

/// What one request of the down lane gave.
enum Got {
    /// Bytes for the link.
    Moved,
    /// Nothing yet.
    Empty,
    /// The end of the bytes down.
    Fin,
    /// The link dropped its byte stream.
    Gone,
}

pub(super) async fn lane_down(mut lane: Lane, mut to: DuplexStream, received: watch::Sender<u64>, kick: Arc<Notify>) {
    let d = lane.d.clone();
    let mut form = d.form;
    let (mut since, mut hold, mut wait) = (0u64, d.hold, BACKOFF_FIRST);
    let mut state = d.ends.watch();
    loop {
        let step = tokio::select! {
            () = until(&mut state, &[State::Gone, State::Failed]) => return,
            step = once(&mut lane, form, &mut since, hold, &mut to, &received) => step,
        };
        match step {
            Ok(Got::Fin) => {
                let _ = to.shutdown().await;
                return;
            }
            Ok(Got::Gone) => return,
            Ok(Got::Moved) => {
                lane.retry.ok();
                wait = BACKOFF_FIRST;
            }
            Ok(Got::Empty) => {
                lane.retry.ok();
                if form == Form::Simplex {
                    // Poll less often while the link is quiet; a POST of the
                    // link (an ask that waits for its answer) ends the wait.
                    tokio::select! {
                        () = tokio::time::sleep(wait) => wait = (wait * 2).min(BACKOFF_MAX),
                        () = kick.notified() => wait = BACKOFF_FIRST,
                    }
                }
            }
            Err(Failed::NoPush) => {
                tracing::debug!(target = %d.target, "the listener answers no push: the dial takes the long poll");
                form = Form::LongPoll;
            }
            Err(failed) => {
                if form == Form::LongPoll && matches!(failed, Failed::Again(_)) {
                    // A proxy may cut a held request: hold less.
                    hold = (hold / 2).max(MIN_HOLD);
                }
                if let Err(e) = lane.retry.after(&d, failed).await {
                    return d.fail(&e);
                }
            }
        }
    }
}

/// Give `bytes` to the link: false when the link dropped its stream.
async fn deliver(to: &mut DuplexStream, bytes: &[u8], since: &mut u64, received: &watch::Sender<u64>) -> bool {
    if to.write_all(bytes).await.is_err() {
        return false;
    }
    *since += bytes.len() as u64;
    received.send_replace(*since);
    true
}

async fn once(
    lane: &mut Lane,
    form: Form,
    since: &mut u64,
    hold: Duration,
    to: &mut DuplexStream,
    received: &watch::Sender<u64>,
) -> Result<Got, Failed> {
    let d = lane.d.clone();
    let (method, target, accept, limit, want) = match form {
        Form::LongPoll => {
            let rest = format!("&since={since}&hold={}", hold.as_secs());
            ("POST", d.target("down", &rest), None, hold + d.answer_timeout, &[200][..])
        }
        Form::Simplex => ("GET", d.target("out", &format!("&since={since}")), None, d.answer_timeout, &[200, 204][..]),
        Form::Sse => {
            let target = d.target("push", &format!("&since={since}"));
            ("GET", target, Some("text/event-stream"), d.answer_timeout, &[200][..])
        }
        Form::Chunked => {
            let target = d.target("push", &format!("&since={since}"));
            ("GET", target, Some("application/octet-stream"), d.answer_timeout, &[200][..])
        }
    };
    let body = (method == "POST").then_some(&[][..]);
    let head = lane.ask(method, &target, body, accept, limit, want).await?;
    let fin = head.field("pod-fin") == Some("1");
    if matches!(form, Form::LongPoll | Form::Simplex) {
        if head.status() == Some(204) {
            lane.done(&head);
            return Ok(Got::Empty);
        }
        let len = lane.length(&head, form.batch())?;
        let bytes = tokio::time::timeout(d.answer_timeout, lane.body(len)).await.unwrap_or_else(|_| {
            Err(Failed::Again(d.err(CarrierErrorKind::Io(io::ErrorKind::TimedOut), "a body that did not come")))
        })?;
        lane.done(&head);
        if !deliver(to, &bytes, since, received).await {
            return Ok(Got::Gone);
        }
        return Ok(if fin {
            Got::Fin
        } else if bytes.is_empty() {
            Got::Empty
        } else {
            Got::Moved
        });
    }
    if !head.chunked() {
        let text = "a push that is not chunked";
        return Err(Failed::Fatal(d.err(CarrierErrorKind::Io(io::ErrorKind::InvalidData), text)));
    }
    // The listener sends a ping of SSE each 15 s; a raw push has no ping of
    // its own, and the link's keepalive moves in it.
    let silence = if form == Form::Sse { PING_EVERY * 3 } else { d.idle };
    let (mut events, mut moved) = (Events::default(), false);
    lane.retry.ok();
    loop {
        let chunk = match lane.chunk(form.batch() * 2, silence).await {
            Ok(chunk) => chunk,
            Err(failed) if moved => {
                // Bytes came: the cut is no failure of the lane.
                tracing::debug!(error = ?failed, "a push was cut");
                return Ok(Got::Moved);
            }
            Err(failed) => return Err(failed),
        };
        let Some(chunk) = chunk else {
            lane.done(&head);
            if form == Form::Chunked {
                return Ok(Got::Fin);
            }
            let text = "a push of SSE ended with no event fin";
            return Err(Failed::Again(d.err(CarrierErrorKind::Io(io::ErrorKind::UnexpectedEof), text)));
        };
        if form == Form::Chunked {
            if !deliver(to, &chunk, since, received).await {
                return Ok(Got::Gone);
            }
            moved = true;
            continue;
        }
        let bad = |e: io::Error| Failed::Fatal(d.err(CarrierErrorKind::Io(e.kind()), e.to_string()));
        events.push(&chunk).map_err(bad)?;
        while let Some(event) = events.next().map_err(bad)? {
            match event {
                Event::Ping => {}
                Event::Fin => return Ok(Got::Fin),
                Event::Data(bytes) => {
                    if !deliver(to, &bytes, since, received).await {
                        return Ok(Got::Gone);
                    }
                    moved = true;
                }
            }
        }
    }
}
