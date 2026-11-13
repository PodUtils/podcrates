//! The dial of the poll carriers: the open of a session, then the up lane
//! (the POSTs of the bytes of the link) and the down lane (the form of the
//! dial) in tasks of their own, around the byte stream of the link.

use std::sync::Arc;

use pod_identity::{KeyName, PublicKey};
use pod_tls::peer;
use tokio::io::{AsyncReadExt as _, DuplexStream};
use tokio::sync::{Notify, watch};

use super::lane::{Dialing, Failed, Lane};
use super::pipes::{Ends, State, pipes, until};
use super::{ACK_EVERY, ALPN, BUSY_LIMIT, Form, Poll, PostUrl, down};
use crate::carrier::{BoxIo, CarrierError, CarrierErrorKind};
use crate::pin;

pub(super) async fn connect(p: &Poll, url: &PostUrl, peer: &KeyName) -> Result<(BoxIo, PublicKey), CarrierError> {
    let name = p.form.name();
    let config = peer::client_config(p.provider.clone(), p.identity.clone(), peer.clone(), &[ALPN])
        .map_err(|e| CarrierError::new(name, CarrierErrorKind::Unavailable, e.to_string()))?;
    let target = url.target();
    let server_name = pin::server_name(name, &target)?;
    let mut id = [0u8; 16];
    p.provider.secure_random.fill(&mut id).map_err(|_| {
        CarrierError::new(name, CarrierErrorKind::Unavailable, "the provider gave no random bytes for the session")
    })?;
    let d = Arc::new(Dialing {
        name,
        form: p.form,
        config,
        way: p.way.clone(),
        target,
        server_name,
        host: url.authority(),
        base: url.path.trim_end_matches('/').to_string(),
        id: format!("{:032x}", u128::from_be_bytes(id)),
        own: p.identity.public(),
        connect_timeout: p.connect_timeout,
        handshake_timeout: p.handshake_timeout,
        answer_timeout: p.answer_timeout,
        hold: p.hold,
        idle: p.idle,
        ends: Ends::new(),
    });
    let mut up = Lane::new(d.clone());
    let key = open(&mut up).await?;
    let (io, far) = pipes(p.form.batch(), d.ends.clone());
    let (received, acks) = watch::channel(0u64);
    let kick = Arc::new(Notify::new());
    tokio::spawn(lane_up(up, far.to_peer, acks, kick.clone()));
    tokio::spawn(down::lane_down(Lane::new(d), far.from_peer, received, kick));
    Ok((Box::new(io), key))
}

/// The open of the session: its answer comes after the listener took the
/// key of this end, so a refused key is an error of the dial. A 429 waits
/// for its Retry-After; the third in a row ends the dial.
async fn open(lane: &mut Lane) -> Result<PublicKey, CarrierError> {
    let d = lane.d.clone();
    let target = d.target("open", "");
    let mut busy = 0;
    loop {
        let asked = lane.ask("POST", &target, Some(&[]), None, d.handshake_timeout, &[201]).await;
        match asked {
            Ok(head) => {
                let len = lane.length(&head, 4096).map_err(fatal)?;
                lane.body(len).await.map_err(fatal)?;
                lane.done(&head);
                let Some(key) = lane.key else { unreachable!("a lane answered with no key") };
                return Ok(key);
            }
            Err(Failed::Busy(after)) => {
                busy += 1;
                if busy >= BUSY_LIMIT {
                    let text = format!("the listener at {} answered 429 {BUSY_LIMIT} times in a row", d.target);
                    return Err(d.err(CarrierErrorKind::Unavailable, text));
                }
                tokio::time::sleep(after).await;
            }
            Err(failed) => return Err(fatal(failed)),
        }
    }
}

fn fatal(failed: Failed) -> CarrierError {
    match failed {
        Failed::Again(e) | Failed::Fatal(e) => e,
        Failed::Busy(_) | Failed::NoPush => unreachable!("an open answered with a status of a lane"),
    }
}

/// Wait until the bytes down that came are `ACK_EVERY` past `acked`.
async fn ack_due(received: &mut watch::Receiver<u64>, acked: u64) {
    loop {
        if *received.borrow_and_update() >= acked + ACK_EVERY {
            return;
        }
        if received.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

/// The up lane: each read of the link goes in one POST, again after a
/// failure with the same offset. The end of the link's bytes is a POST
/// with `fin`. A push acknowledges its bytes in an empty POST.
async fn lane_up(mut lane: Lane, mut from: DuplexStream, mut received: watch::Receiver<u64>, kick: Arc<Notify>) {
    let d = lane.d.clone();
    let push = matches!(d.form, Form::Sse | Form::Chunked);
    let mut buf = vec![0u8; d.form.batch()];
    let (mut seq, mut acked) = (0u64, 0u64);
    let mut state = d.ends.watch();
    loop {
        let (n, fin) = tokio::select! {
            read = from.read(&mut buf) => match read {
                Ok(0) | Err(_) => (0, true),
                Ok(n) => (n, false),
            },
            () = ack_due(&mut received, acked), if push => (0, false),
            () = until(&mut state, &[State::Failed]) => return,
        };
        let ack = *received.borrow();
        let target = d.target("up", &format!("&seq={seq}&ack={ack}{}", if fin { "&fin=1" } else { "" }));
        loop {
            let sent = tokio::select! {
                sent = post(&mut lane, &target, &buf[..n]) => sent,
                () = until(&mut state, &[State::Failed]) => return,
            };
            match sent {
                Ok(next) if next == seq + n as u64 => break,
                Ok(next) => {
                    let text = format!("the listener acknowledged {next}, not {}", seq + n as u64);
                    return d.fail(&d.err(CarrierErrorKind::Io(std::io::ErrorKind::InvalidData), text));
                }
                Err(failed) => {
                    if let Err(e) = lane.retry.after(&d, failed).await {
                        return d.fail(&e);
                    }
                }
            }
        }
        lane.retry.ok();
        seq += n as u64;
        acked = ack;
        kick.notify_one();
        if fin {
            return;
        }
    }
}

/// One POST of the up lane: the next offset that the listener waits for.
async fn post(lane: &mut Lane, target: &str, body: &[u8]) -> Result<u64, Failed> {
    let d = lane.d.clone();
    let head = lane.ask("POST", target, Some(body), None, d.answer_timeout, &[202]).await?;
    let len = lane.length(&head, 4096)?;
    lane.body(len).await?;
    lane.done(&head);
    head.field("pod-ack").and_then(|v| v.parse().ok()).ok_or_else(|| {
        Failed::Fatal(d.err(CarrierErrorKind::Io(std::io::ErrorKind::InvalidData), "a 202 with no Pod-Ack"))
    })
}
