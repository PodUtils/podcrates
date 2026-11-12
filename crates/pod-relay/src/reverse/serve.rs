//! One node socket: the sessions on it, until the socket ends.
//!
//! One task, the writer, owns the write half and the state of each session,
//! so no frame goes out without its id, before its `ready`, or after its
//! `close` (`docs/reverse.md`, "Node"). The reader routes data by id to the
//! local side of each session, and never waits for one: a local side that
//! reads no more ends its own session. An opener task per `open` asks the
//! handler, and only its answer queues `ready` or `reject`. A role that
//! registers tokens of one leg says so in its `hello`, and the handler sends
//! its registrations through the writer too (podssh T-615, `pod_e2e::legs`).
//! The sessions' bytes wait in a line each, a turn each (podssh T-648, `super::fair`).
//! A node that wants sessions readied ahead asks at the `hello` that offers
//! them (podssh T-650, `super::spares`).

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pod_ws::SessionError;
use pod_ws::frame;
use pod_ws::session::{RelaySession, close_code_and_reason};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use super::close_say::CloseSay;
use super::closes::RelayClose;
use super::control::{self, NodeInbound, NodeLimits};
use super::fair::Fair;
use super::framing::SessionId;
use super::framing::legs::{chunk_for_node, decode_node_frame};
use super::node::Handler;
use super::route::{self, Copies, Pending, Routes};
use super::sessions::Sessions;
use super::state::NodeState;

/// How one socket ended.
#[derive(Debug)]
pub enum End {
    /// The relay closed the socket, with its code and reason.
    Closed(RelayClose),
    /// The socket failed with no Close: a broken link, or unanswered pings.
    Failed(SessionError),
    /// The caller stopped the node: each session got `close`, the socket a
    /// Close 1000.
    Stopped,
    /// The relay's `hello` states a dialect of the reverse road that this
    /// node does not read (podssh T-643): its sentence. The node closed the socket
    /// as at a stop; a new one would meet the same.
    Unread(String),
}

/// What a node needs for one socket, and how long it connects again after
/// it lost one.
#[derive(Debug, Clone)]
pub struct Settings {
    /// How long the handler may take to open the local side: under the 15 s
    /// after which the relay ends the session itself.
    pub open_limit: Duration,
    /// How often to ping the relay, and how many silent intervals mean the
    /// socket is dead. The relay answers a Ping on a node socket (as
    /// measured).
    pub ping_every: Duration,
    pub pings_allowed: u32,
    /// How long a node that lost its socket connects again when the relay
    /// answers `409`: the relay can still hold the old socket, and the
    /// sessions that the resumable layer keeps wait as long (podssh T-261).
    pub rejoin: Duration,
    /// How long a node connects again when the relay answers `409` at its
    /// first registration: the relay can still hold the socket of the node
    /// that ran just before this one, as at a restart (podssh T-416). Zero exits at
    /// the first `409`.
    pub first_conflict: Duration,
    /// The sessions that the relay role keeps readied ahead for this node's
    /// operators, where its `hello` offers them (podssh T-650): none by default, as
    /// each is a session at rest here; `super::spares::MOST` at most.
    pub spares: u8,
    /// What the node keeps across its sockets, and where its lines go.
    pub node: Arc<NodeState>,
    /// The plain mode's half close (podnet T-196): the end of a local side's
    /// writes does not end its session, as the relay has no half close and
    /// the operator's last bytes may still come; the session ends when none
    /// came for this long ([`HALF_CLOSE_IDLE`] in podnet). `None`: the end of
    /// the local side's writes ends the session at once.
    pub half_close: Option<Duration>,
    /// How long the bytes that wait for a session's local side may take to
    /// go after the relay closed the session; then the node drops them and
    /// the local side, which may never read (podnet T-194).
    pub drain: Duration,
}

/// The default of [`Settings::first_conflict`].
pub const FIRST_CONFLICT: Duration = Duration::from_secs(30);

/// The wait for an operator's last bytes after a local side's half close,
/// in podnet's plain mode ([`Settings::half_close`]).
pub const HALF_CLOSE_IDLE: Duration = Duration::from_secs(2);

/// The default of [`Settings::drain`]: the time that a local side has to
/// open (`open_limit`), as a local side is on this host.
pub const DRAIN: Duration = Duration::from_secs(10);

impl Default for Settings {
    fn default() -> Self {
        Settings {
            open_limit: Duration::from_secs(10),
            ping_every: pod_ws::LIVENESS_EVERY,
            pings_allowed: pod_ws::LIVENESS_ALLOWED,
            rejoin: pod_session::resume::RESUME_DEADLINE,
            first_conflict: FIRST_CONFLICT,
            spares: 0,
            node: Arc::default(),
            half_close: None,
            drain: DRAIN,
        }
    }
}

/// A request to the writer, the only task that writes to the socket.
pub(super) enum Out {
    Opened(SessionId),
    Ready(SessionId),
    Reject(SessionId, String),
    Data(SessionId, Vec<u8>),
    /// The relay closed the session, with its reason when it gave one (podssh T-701).
    Closed(SessionId, Option<String>),
    /// The local side ended: the relay is told with `close {id}`.
    LocalEnd(SessionId),
    /// The local side reads no more: `close {id}`, with the reason.
    Stalled(SessionId),
    /// A text frame of the node's own: a token of one leg (podssh T-615).
    Text(String),
    Stop,
}

/// Why a node ends a session whose local side reads no more.
const STALLED: &str = "the node's local side does not read";

/// The writer's two queues. Control goes first: a `ready`, a refusal, or
/// the relay's close of a session never waits behind megabytes of another
/// session's bytes, as the relay ends a session that the node does not
/// answer within 15 s, and a `ready` that comes later ends the whole socket
/// with `1003 unknown session id` (podssh T-410); the relay's close drops the
/// session's queued bytes before they go out for an id that it forgot. A
/// session's bytes, and the `close` after its last ones, keep their order.
/// Data waits in a line for each session, a turn each (podssh T-648): an
/// interactive session's echo waits for one read of each other session, not
/// for a bulk copy's backlog.
#[derive(Clone)]
pub(super) struct Outbox {
    control: mpsc::Sender<Out>,
    data: Fair<SessionId, Out>,
}

/// The reads of a session's local side that may wait for the socket, and of
/// all its sessions: the one queue's 256 of before, shared out, so that a
/// node's memory stays as it was. A read is `CHUNK_BYTES` at most.
const SESSION_READS: usize = 4;
const SOCKET_READS: usize = 256;

impl Outbox {
    pub(super) async fn send(&self, out: Out) -> Result<(), ()> {
        match out {
            Out::Data(id, _) => self.data.push(id, out, 1).await.map_err(|_| ()),
            // After the session's last bytes, with no wait for their room.
            Out::LocalEnd(id) => self.data.push_last(id, out).map_err(|_| ()),
            other => self.control.send(other).await.map_err(|_| ()),
        }
    }
}

/// Serve the sessions of `session` until the socket ends or `stop` fires.
pub async fn serve<S, H, F>(session: RelaySession<S>, handler: Arc<H>, settings: Settings, stop: &mut F) -> End
where
    S: AsyncRead + AsyncWrite + Send + 'static,
    H: Handler,
    F: Future<Output = ()> + Unpin,
{
    let session = Arc::new(session);
    let ((control, control_rx), data) = (mpsc::channel(1024), Fair::new(SESSION_READS, SOCKET_READS));
    let tx = Outbox { control, data: data.clone() };
    let mut writer = Writer(tokio::spawn(write(session.clone(), control_rx, data, settings.node.clone())));
    let (routes, pending): (Routes, Pending) = Default::default();
    let copies = Copies { open_limit: settings.open_limit, half_close: settings.half_close };
    let mut limits = NodeLimits::conservative();
    let liveness = session.watch_liveness(settings.ping_every, settings.pings_allowed);
    tokio::pin!(liveness);
    let end = loop {
        let frame = tokio::select! {
            frame = session.read_frame() => frame,
            dead = &mut liveness => match super::closes::pending(&session).await {
                Some(close) => break End::Closed(close),
                None => break End::Failed(dead),
            },
            () = &mut *stop => {
                let _ = tx.send(Out::Stop).await;
                break End::Stopped;
            }
        };
        match frame {
            Ok(f) if f.opcode == frame::OPCODE_TEXT => match control::parse_inbound(&f.payload) {
                Ok(NodeInbound::Hello(hello)) => {
                    // A dialect that this node does not read ends it here,
                    // not at a later frame that one side drops (podssh T-643).
                    if let Err(skew) = control::accept(hello.version.as_ref()) {
                        let _ = tx.send(Out::Stop).await;
                        break End::Unread(skew.sentence("this node", "the relay"));
                    }
                    limits.apply(&hello);
                    // A role that registers tokens of one leg (podssh T-615): the
                    // handler registers them on this socket from now on.
                    super::legs::heard(&settings.node, &hello);
                    if pod_e2e::legs::offered(&hello.extra) {
                        handler.attach(Some(frames(&tx)));
                    }
                    // Sessions readied ahead, which this node asks for (podssh T-650).
                    if let Some(ask) = super::spares::heard(&settings.node, &hello, settings.spares) {
                        let _ = tx.send(Out::Text(ask)).await;
                    }
                }
                Ok(NodeInbound::Open { id }) => {
                    let Ok(id) = SessionId::parse(id.as_bytes()) else { continue };
                    if route::known(&routes, &pending, &id) {
                        tracing::debug!("the relay opened a session that is open");
                        continue;
                    }
                    if route::live(&routes, &pending) >= limits.max_sessions as usize {
                        let reason = format!("the node has its {} sessions", limits.max_sessions);
                        let _ = tx.send(Out::Reject(id, reason)).await;
                        continue;
                    }
                    let _ = tx.send(Out::Opened(id)).await;
                    route::lock(&pending).insert(id);
                    tokio::spawn(route::open(id, handler.clone(), copies, tx.clone(), routes.clone(), pending.clone()));
                }
                Ok(NodeInbound::Close { id, reason }) => {
                    let Ok(id) = SessionId::parse(id.as_bytes()) else { continue };
                    // Dropping the route ends the local writer once its bytes
                    // went, within `drain`; the local side's late bytes find
                    // no session and are dropped.
                    route::closed_by_relay(&routes, &id, settings.drain);
                    let _ = tx.send(Out::Closed(id, reason)).await;
                }
                // The role took a token of one leg for an operator (podssh T-615).
                Ok(NodeInbound::Spent { digest }) => handler.spent(&digest),
                // Not a message that a node answers.
                Ok(NodeInbound::Unknown) | Err(_) => {}
            },
            Ok(f) if f.opcode == frame::OPCODE_BINARY => {
                let Ok((id, payload)) = decode_node_frame(&f.payload) else { continue };
                // Data for no live session (a late frame of a closed one) is
                // dropped; the socket and the other sessions go on.
                if route::offer(&routes, id, payload) {
                    let _ = tx.send(Out::Stalled(id)).await;
                }
            }
            Ok(f) if f.opcode == frame::OPCODE_CLOSE => {
                let (code, reason) = close_code_and_reason(&f.payload);
                break End::Closed(RelayClose { code: code.unwrap_or(1005), reason, clean: true });
            }
            Ok(_) => {}
            Err(e) => break End::Failed(e),
        }
    };
    // The tokens of one leg wait for the next socket (podssh T-615).
    handler.attach(None);
    route::lock(&routes).clear();
    // No session's bytes join the queue now; a sender that waits for room
    // hears it, as it heard the end of the one queue before.
    tx.data.shut();
    drop(tx);
    // A stop's last frames (each session's `close`, the Close) go out
    // first. A socket that the relay closed, or that broke, takes nothing
    // more, so the node goes on at once (podssh T-255).
    if matches!(end, End::Stopped | End::Unread(_)) {
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut writer.0).await;
    }
    end
}

/// The writer's task, which ends with `serve`: a `serve` that is dropped,
/// as its task's abort drops it, takes the socket with it. The writer holds
/// the socket, and lived on while a session's tasks held the outbox, so the
/// far side never saw the node go (podssh T-601).
struct Writer(tokio::task::JoinHandle<Result<(), SessionError>>);

impl Drop for Writer {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The text frames of the handler's registrations, from a caller that holds
/// a lock and cannot wait (podssh T-615): a frame that finds the control queue full
/// is dropped, and a leg that needed it takes the pair's token.
fn frames(tx: &Outbox) -> pod_e2e::legs::Frames {
    let control = tx.control.clone();
    Arc::new(move |text: String| {
        let _ = control.try_send(Out::Text(text));
    })
}

/// The writer: the one task that writes to the socket. It owns the state of
/// each session, so a frame for an id that is not readied, or is closed,
/// never goes out: it is dropped here.
async fn write<S>(
    session: Arc<RelaySession<S>>,
    mut control: mpsc::Receiver<Out>,
    data: Fair<SessionId, Out>,
    node: Arc<NodeState>,
) -> Result<(), SessionError>
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let mut sessions = Sessions::new();
    let mut said = CloseSay::default();
    loop {
        // Control first (`Outbox`), then one session's turn; the loop ends
        // once both queues closed. A read keeps its room until it went out.
        let (out, _room) = tokio::select! {
            biased;
            Some(out) = control.recv() => (out, None),
            Some(turn) = data.next() => (turn.piece, Some(turn.slot)),
            else => break,
        };
        match out {
            Out::Opened(id) => sessions.opened(id),
            Out::Ready(id) => {
                if sessions.may_send_ready(&id).is_ok()
                    && let Some(text) = json(control::ready(&id))
                {
                    session.send_text(&text).await?;
                    sessions.readied(id);
                }
            }
            Out::Reject(id, reason) => {
                // A refusal over the session limit answers an id the writer
                // was never told of; the relay waits for an answer to it.
                if let Some(text) = json(control::reject(&id, &reason)) {
                    session.send_text(&text).await?;
                }
                sessions.closed(&id);
            }
            Out::Data(id, bytes) => {
                if sessions.may_send_data(&id).is_err() {
                    continue;
                }
                let Ok(frames) = chunk_for_node(&id, &bytes) else { continue };
                for frame in frames {
                    session.send_binary(&frame).await?;
                }
            }
            Out::Closed(id, reason) => {
                if let Some(line) = said.line(Instant::now(), id.as_str(), reason.as_deref()) {
                    node.tell(pod_core::Event::warn(super::close_say::EVENT, line));
                }
                sessions.closed(&id);
                // Its queued bytes are for an id that the relay forgot.
                data.drop_line(&id);
            }
            Out::LocalEnd(id) => {
                if sessions.state(&id).is_some() {
                    if let Some(text) = json(control::close(&id, None)) {
                        session.send_text(&text).await?;
                    }
                    sessions.closed(&id);
                }
            }
            Out::Stalled(id) => {
                if sessions.state(&id).is_some() {
                    if let Some(text) = json(control::close(&id, Some(STALLED))) {
                        session.send_text(&text).await?;
                    }
                    sessions.closed(&id);
                }
                data.drop_line(&id);
            }
            Out::Text(text) => session.send_text(&text).await?,
            Out::Stop => {
                let live: Vec<SessionId> = sessions.live().map(|(id, _)| *id).collect();
                for id in live {
                    if let Some(text) = json(control::close(&id, Some(super::node::stopping_words(&node)))) {
                        let _ = session.send_text(&text).await;
                    }
                }
                let _ = session.send_close(1000, "").await;
                return Ok(());
            }
        }
    }
    Ok(())
}

/// A control message as the text of a frame, or `None` when the codec could
/// not build it: nothing goes out then, because a frame that the relay
/// refuses closes the whole socket.
fn json(built: Result<Vec<u8>, control::ControlError>) -> Option<String> {
    built.ok().and_then(|bytes| String::from_utf8(bytes).ok())
}
