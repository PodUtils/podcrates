//! The Cap'n Proto RPC subset that registers a tunnel connection.
//!
//! Reference: `rpc.capnp` of Cap'n Proto (the two-party protocol). The
//! control stream carries framed RPC messages one after the other, in the
//! two directions. This module does these exchanges:
//!
//! | Direction | Messages |
//! | --- | --- |
//! | client to edge | `Bootstrap` (question 0); `Call registerConnection` on the bootstrap capability (question 1); `Call unregisterConnection` at shutdown; `Finish` for each answer |
//! | edge to client | `Return` for each question; `Bootstrap` (the client answers with its capability, export 0); `Call` on that capability (the client answers with an `unimplemented` exception); `Finish`, `Release`, `Abort` |
//!
//! The edge uses the bootstrap of the client as a liveness check. Without an answer,
//! the edge marks the tunnel as down.

mod message;
pub(crate) use message::*;

use std::collections::HashMap;
use std::time::Duration;

use crate::fail::{Failure, IoFail, OrFail, Result, bail};
use pod_core::{Class, Stage};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::capnp::{Builder, Message, StructReader};
use super::schema::{
    METHOD_REGISTER_CONNECTION, METHOD_UNREGISTER_CONNECTION, REGISTRATION_SERVER_ID, read_register_results,
    write_register_params,
};

pub use crate::fail::Refused;

pub use super::schema::{ConnectionError, ConnectionOptions, RegistrationDetails, TunnelAuth};

/// The longest time for the registration.
pub const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(15);
/// The text of the edge for a second registration of the same connection
/// index.
pub const DUPLICATE_CONNECTION_ERROR: &str = "connection with this index already registered";

// rpc.capnp: `Message` union tags (u16 at index 0, struct in pointer 0).
const MSG_UNIMPLEMENTED: u16 = 0;
const MSG_ABORT: u16 = 1;
const MSG_CALL: u16 = 2;
const MSG_RETURN: u16 = 3;
const MSG_FINISH: u16 = 4;
const MSG_RELEASE: u16 = 6;
const MSG_BOOTSTRAP: u16 = 8;
// `Return` union tags (u16 at index 3).
const RETURN_RESULTS: u16 = 0;
const RETURN_EXCEPTION: u16 = 1;
// `CapDescriptor` union tags (u16 at index 0).
const CAP_SENDER_HOSTED: u16 = 1;
const CAP_SENDER_PROMISE: u16 = 2;
// `Exception.Type`.
const EXCEPTION_UNIMPLEMENTED: u16 = 3;
/// The capability that the client exports to the edge.
const OUR_EXPORT: u32 = 0;

/// What the edge answered to a question.
#[derive(Debug)]
enum Answer {
    Results(Message),
    Exception(String),
    Other(u16),
}

/// The open control stream. Keep it while the connection is in use. Drop it,
/// or call [`ControlSession::shutdown_graceful`], to end the registration.
pub struct ControlSession {
    shutdown: Option<oneshot::Sender<Duration>>,
    task: JoinHandle<()>,
}

impl ControlSession {
    /// Send `unregisterConnection`, wait for the answer up to `grace`, then
    /// close the control stream.
    pub async fn shutdown_graceful(mut self, grace: Duration) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(grace);
        }
        let _ = tokio::time::timeout(grace + Duration::from_secs(1), &mut self.task).await;
    }
}

impl Drop for ControlSession {
    fn drop(&mut self) {
        if self.shutdown.is_some() {
            self.task.abort();
        }
    }
}

/// Open the control stream on `conn` and register the connection.
pub async fn register_connection(
    conn: &noq::Connection,
    auth: &TunnelAuth,
    tunnel_id: Uuid,
    conn_index: u8,
    options: &ConnectionOptions,
) -> Result<(RegistrationDetails, ControlSession)> {
    let (send, recv) =
        conn.open_bi().await.or_fail(Stage::EdgeRegister, Class::Unavailable, "opening the control stream")?;
    register_connection_on(recv, send, auth, tunnel_id, conn_index, options).await
}

/// Register the connection over a control stream that is already open: a
/// QUIC stream, or the bodies of the HTTP/2 control-stream request.
pub async fn register_connection_on<R, W>(
    reader: R,
    writer: W,
    auth: &TunnelAuth,
    tunnel_id: Uuid,
    conn_index: u8,
    options: &ConnectionOptions,
) -> Result<(RegistrationDetails, ControlSession)>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (incoming_tx, incoming) = mpsc::channel(64);
    let reader_task = tokio::spawn(read_loop(reader, incoming_tx));
    let (result_tx, result_rx) = oneshot::channel();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let mut session = Session { writer, incoming, answers: HashMap::new(), next_question: 0 };
    let register = encode_register(auth, tunnel_id, conn_index, options);
    let task = tokio::spawn(async move {
        session.run(register, result_tx, shutdown_rx).await;
        reader_task.abort();
    });
    let details = tokio::time::timeout(DEFAULT_RPC_TIMEOUT, result_rx)
        .await
        .map_err(|_| crate::fail::new(Stage::EdgeRegister, Class::Timeout, "registerConnection timed out"))?
        .map_err(|_| {
            crate::fail::new(
                Stage::EdgeRegister,
                Class::Unavailable,
                "the control stream closed before the registration",
            )
        })??;
    info!(uuid = %details.uuid, location = %details.location, conn_index, "registered with the edge");
    Ok((details, ControlSession { shutdown: Some(shutdown_tx), task }))
}

async fn read_loop<R>(mut reader: R, tx: mpsc::Sender<Message>)
where
    R: AsyncRead + Unpin,
{
    loop {
        match Message::read(&mut reader).await {
            Ok(Some(message)) => {
                if tx.send(message).await.is_err() {
                    return;
                }
            }
            Ok(None) => return,
            Err(err) => {
                debug!(error = %err.chain(), "the control stream sent a bad message");
                return;
            }
        }
    }
}

/// The parameters of `registerConnection`, written later into a `Call`.
struct RegisterArgs {
    auth: TunnelAuth,
    tunnel_id: Uuid,
    conn_index: u8,
    options: ConnectionOptions,
}

/// The name of the client in its version text: the version text
/// `podnet/0.4.0` gives `podnet`.
fn client_name(version: &str) -> &str {
    version.split('/').next().unwrap_or(version)
}

fn encode_register(auth: &TunnelAuth, tunnel_id: Uuid, conn_index: u8, options: &ConnectionOptions) -> RegisterArgs {
    RegisterArgs { auth: auth.clone(), tunnel_id, conn_index, options: options.clone() }
}

struct Session<W> {
    writer: W,
    incoming: mpsc::Receiver<Message>,
    answers: HashMap<u32, Answer>,
    next_question: u32,
}

impl<W: AsyncWrite + Unpin> Session<W> {
    async fn send(&mut self, b: Builder) -> Result<()> {
        self.writer.write_all(&b.to_bytes()).await.io_fail(Stage::EdgeRegister, "writing to the control stream")?;
        self.writer.flush().await.io_fail(Stage::EdgeRegister, "writing to the control stream")?;
        Ok(())
    }

    fn question(&mut self) -> u32 {
        let id = self.next_question;
        self.next_question += 1;
        id
    }

    /// Wait for the answer to `question`. Serve the messages of the edge
    /// during the wait.
    async fn wait(&mut self, question: u32) -> Result<Answer> {
        loop {
            if let Some(answer) = self.answers.remove(&question) {
                return Ok(answer);
            }
            let message = self.incoming.recv().await.or_fail(
                Stage::EdgeRegister,
                Class::Unavailable,
                "the control stream closed",
            )?;
            if !self.handle(message).await? {
                bail!(Stage::EdgeRegister, Class::Protocol, "the edge aborted the control stream");
            }
        }
    }

    /// Handle one message from the edge. Return false when the stream must end.
    async fn handle(&mut self, message: Message) -> Result<bool> {
        let kind = message.root()?.get_u16(0);
        if kind == MSG_RETURN {
            let (answer_id, answer) = {
                let ret =
                    message.root()?.get_struct(0)?.or_fail(Stage::EdgeRegister, Class::Protocol, "an empty Return")?;
                let answer = match ret.get_u16(3) {
                    RETURN_RESULTS => None,
                    RETURN_EXCEPTION => Some(Answer::Exception(exception_reason(&ret)?)),
                    other => Some(Answer::Other(other)),
                };
                (ret.get_u32(0), answer)
            };
            let answer = answer.unwrap_or(Answer::Results(message));
            self.answers.insert(answer_id, answer);
            return Ok(true);
        }
        let root = message.root()?;
        match kind {
            MSG_BOOTSTRAP => {
                let question =
                    root.get_struct(0)?.or_fail(Stage::EdgeRegister, Class::Protocol, "an empty Bootstrap")?.get_u32(0);
                self.send(our_bootstrap_return(question)).await?;
            }
            MSG_CALL => {
                let call = root.get_struct(0)?.or_fail(Stage::EdgeRegister, Class::Protocol, "an empty Call")?;
                let question = call.get_u32(0);
                let interface = call.get_u64(1);
                let method = call.get_u16(2);
                debug!(interface = %format!("{interface:#x}"), method, "the edge called the client");
                self.send(exception_return(question, "not implemented by the client")).await?;
            }
            MSG_FINISH | MSG_RELEASE => {}
            MSG_ABORT => {
                let reason = root.get_struct(0)?.map(|e| e.get_text(0).unwrap_or_default()).unwrap_or_default();
                warn!(%reason, "the edge aborted the control stream");
                return Ok(false);
            }
            MSG_UNIMPLEMENTED => debug!("the edge did not understand a message of the client"),
            other => debug!(kind = other, "ignored an RPC message"),
        }
        Ok(true)
    }

    async fn run(
        &mut self,
        register: RegisterArgs,
        result: oneshot::Sender<Result<RegistrationDetails>>,
        shutdown: oneshot::Receiver<Duration>,
    ) {
        let import = match self.bootstrap().await {
            Ok(import) => import,
            Err(err) => {
                let _ = result.send(Err(err));
                return;
            }
        };
        let registered = self.register(import, &register).await;
        let ok = registered.is_ok();
        let _ = result.send(registered);
        if !ok {
            return;
        }
        // Serve the edge until the stream ends or a shutdown is asked.
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                grace = &mut shutdown => {
                    if let Ok(grace) = grace {
                        self.unregister(import, grace).await;
                    }
                    let _ = self.writer.shutdown().await;
                    return;
                }
                message = self.incoming.recv() => {
                    let Some(message) = message else {
                        debug!("the edge closed the control stream");
                        return;
                    };
                    match self.handle(message).await {
                        Ok(true) => {}
                        Ok(false) => return,
                        Err(err) => {
                            debug!(error = %err.chain(), "control stream error");
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Ask for the bootstrap capability of the edge. Return its import id.
    async fn bootstrap(&mut self) -> Result<u32> {
        let question = self.question();
        let mut b = Builder::new();
        let root = b.init_root(1, 1);
        b.set_u16(root, 0, MSG_BOOTSTRAP);
        let boot = b.init_struct(root, 0, 1, 1);
        b.set_u32(boot, 0, question);
        self.send(b).await?;
        let import = match self.wait(question).await? {
            Answer::Results(message) => imported_capability(&message)?,
            Answer::Exception(reason) => {
                bail!(Stage::EdgeRegister, Class::Protocol, "the edge refused the bootstrap: {reason}")
            }
            Answer::Other(kind) => {
                bail!(Stage::EdgeRegister, Class::Protocol, "the edge answered the bootstrap with kind {kind}")
            }
        };
        // Keep the capability: releaseResultCaps = false.
        self.send(finish(question, false)).await?;
        Ok(import)
    }

    async fn call(
        &mut self,
        import: u32,
        method: u16,
        params: impl FnOnce(&mut Builder, super::capnp::StructRef),
    ) -> Result<u32> {
        let question = self.question();
        let mut b = Builder::new();
        let root = b.init_root(1, 1);
        b.set_u16(root, 0, MSG_CALL);
        // Call: 3 data words, 3 pointers.
        let call = b.init_struct(root, 0, 3, 3);
        b.set_u32(call, 0, question);
        b.set_u64(call, 1, REGISTRATION_SERVER_ID);
        b.set_u16(call, 2, method);
        // sendResultsTo = caller (u16 at index 3 stays 0).
        // MessageTarget: 1 data word, 1 pointer; importedCap.
        let target = b.init_struct(call, 0, 1, 1);
        b.set_u32(target, 0, import);
        // Payload: content (pointer 0), capTable (pointer 1, empty).
        let payload = b.init_struct(call, 1, 0, 2);
        params(&mut b, payload);
        self.send(b).await?;
        Ok(question)
    }

    async fn register(&mut self, import: u32, args: &RegisterArgs) -> Result<RegistrationDetails> {
        let question = self
            .call(import, METHOD_REGISTER_CONNECTION, |b, payload| {
                write_register_params(b, payload, 0, &args.auth, args.tunnel_id, args.conn_index, &args.options);
            })
            .await?;
        let answer = self.wait(question).await?;
        self.send(finish(question, true)).await?;
        match answer {
            Answer::Results(message) => {
                let results = results_struct(&message)?;
                match read_register_results(&results)? {
                    Ok(details) => Ok(details),
                    Err(err) => Err(Failure::from(Refused {
                        client: client_name(&args.options.version).to_string(),
                        cause: err.cause.clone(),
                        should_retry: err.should_retry,
                        retry_after: Duration::from_nanos(err.retry_after.max(0) as u64),
                    })),
                }
            }
            Answer::Exception(reason) => {
                bail!(
                    Stage::EdgeRegister,
                    Class::Protocol,
                    "registerConnection failed: {reason} ({} speaks {})",
                    client_name(&args.options.version),
                    crate::cloudflare::speaks()
                )
            }
            Answer::Other(kind) => {
                bail!(
                    Stage::EdgeRegister,
                    Class::Protocol,
                    "registerConnection answered with kind {kind} ({} speaks {})",
                    client_name(&args.options.version),
                    crate::cloudflare::speaks()
                )
            }
        }
    }

    async fn unregister(&mut self, import: u32, grace: Duration) {
        let Ok(question) = self
            .call(import, METHOD_UNREGISTER_CONNECTION, |b, payload| {
                b.init_struct(payload, 0, 0, 0);
            })
            .await
        else {
            return;
        };
        match tokio::time::timeout(grace, self.wait(question)).await {
            Ok(Ok(_)) => {
                let _ = self.send(finish(question, true)).await;
                debug!("unregistered the connection");
            }
            Ok(Err(err)) => debug!(error = %err.chain(), "unregisterConnection failed"),
            Err(_) => debug!("unregisterConnection timed out"),
        }
    }
}

#[cfg(test)]
mod tests;
