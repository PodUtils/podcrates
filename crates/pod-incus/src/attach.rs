//! A command with its websockets (`wait-for-websocket`, not interactive):
//! the operation gives a secret for each of `0` (stdin), `1` (stdout), `2`
//! (stderr) and `control` in `metadata.fds`, and incus starts the command
//! when 0, 1 and 2 are connected (`GET /1.0/operations/ID/websocket?secret=`
//! on the same Unix socket, an upgrade of HTTP/1.1). The data of a stream
//! goes in binary messages; an empty text message ends it (the barrier of
//! `shared/ws`), as does a Close. The control socket takes
//! `{"command":"signal","signal":N}`. The exit code is the `return` of the
//! final operation. No secret goes into an error or a log.

use pod_core::BoxedIo;
use pod_proxy::http::{Body as AnswerBody, Framing, MAX_HEAD, read_head};
use pod_ws::RelaySession;
use pod_ws::SessionError;
use pod_ws::frame::{OPCODE_BINARY, OPCODE_CLOSE, OPCODE_TEXT};
use tokio::io::AsyncWriteExt;

use crate::client::Client;
use crate::envelope;
use crate::error::{Error, ErrorKind, Result, far_text};
use crate::exec::{Exec, ExecOutput, ExecPost, exec_path, exit_code};
use crate::operation::Operation;
use crate::wire::{HOST, escape};

/// The session of one websocket of an operation.
type Socket = RelaySession<BoxedIo>;

/// A command with its streams apart. Read stdout and stderr to their end,
/// close stdin when its data ends, then [`Attached::wait`].
#[derive(Debug)]
pub struct Attached {
    client: Client,
    call: String,
    /// The operation as incus started it.
    pub operation: Operation,
    pub stdin: Stdin,
    pub stdout: Output,
    pub stderr: Output,
    pub control: Control,
}

/// The stdin of the command.
#[derive(Debug)]
pub struct Stdin {
    socket: Socket,
    call: String,
}

/// The stdout or the stderr of the command.
#[derive(Debug)]
pub struct Output {
    socket: Socket,
    call: String,
    ended: bool,
}

/// The control socket of the command.
#[derive(Debug)]
pub struct Control {
    socket: Socket,
    call: String,
}

fn ws_error(call: &str, e: SessionError) -> Error {
    Error::new(call, ErrorKind::WebSocket(e.to_string()))
}

impl Stdin {
    pub async fn write(&self, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.socket.send_binary(bytes).await.map_err(|e| ws_error(&self.call, e))
    }

    /// The end of stdin: the command reads the end of its file. A socket
    /// that incus closed at the end of the command needs no end.
    pub async fn close(&self) -> Result<()> {
        match self.socket.send_text("").await {
            Err(SessionError::Io {
                kind: std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset,
                ..
            }) => Ok(()),
            other => other.map_err(|e| ws_error(&self.call, e)),
        }
    }
}

impl Output {
    /// The next bytes; `None` at the end of the stream.
    pub async fn read(&mut self) -> Result<Option<Vec<u8>>> {
        while !self.ended {
            let frame = match self.socket.read_frame().await {
                Ok(frame) => frame,
                // incus closes the socket when the command ends.
                Err(SessionError::ClosedWithoutClose(_)) => {
                    self.ended = true;
                    break;
                }
                Err(e) => return Err(ws_error(&self.call, e)),
            };
            match frame.opcode {
                OPCODE_CLOSE => self.ended = true,
                OPCODE_TEXT if frame.payload.is_empty() => self.ended = true,
                OPCODE_BINARY | OPCODE_TEXT if frame.payload.is_empty() => {}
                OPCODE_BINARY | OPCODE_TEXT => return Ok(Some(frame.payload)),
                _ => {}
            }
        }
        Ok(None)
    }

    /// Each byte to the end of the stream, `max` at most.
    pub async fn read_to_end(&mut self, max: usize) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        while let Some(bytes) = self.read().await? {
            if out.len() + bytes.len() > max {
                return Err(Error::usage(&self.call, format!("the output is longer than {max} bytes")));
            }
            out.extend_from_slice(&bytes);
        }
        Ok(out)
    }
}

impl Control {
    /// Send `signal` (SIGTERM is 15) to the command.
    pub async fn signal(&self, signal: i32) -> Result<()> {
        let message = serde_json::json!({ "command": "signal", "signal": signal }).to_string();
        self.socket.send_text(&message).await.map_err(|e| ws_error(&self.call, e))
    }
}

impl Attached {
    /// Give `input` to stdin and close it, read stdout and stderr (each of
    /// `max` bytes at most), all at once, then wait. A reader of one stream
    /// alone can stop the command when the buffer of the other is full. At
    /// the first error the sockets close, and incus ends the command.
    pub async fn output(mut self, input: &[u8], max: usize) -> Result<ExecOutput> {
        let Attached { stdin, stdout, stderr, .. } = &mut self;
        let feed = async {
            // A command that ends with no read of its stdin closes it
            // first: the exit code is still the answer.
            let fed = match stdin.write(input).await {
                Ok(()) => stdin.close().await,
                Err(e) => Err(e),
            };
            if let Err(e) = fed {
                tracing::debug!(error = %e, "incus: the stdin of the command closed before its input ended");
            }
            Ok(())
        };
        let ((), stdout, stderr) = tokio::try_join!(feed, stdout.read_to_end(max), stderr.read_to_end(max))?;
        Ok(ExecOutput { exit_code: self.wait().await?, stdout, stderr })
    }

    /// The wait for the end of the command: its exit code.
    pub async fn wait(self) -> Result<i32> {
        let Attached { client, call, operation, stdin, stdout, stderr, control } = self;
        // incus ends the command when the control socket closes: it closes
        // here only after the end.
        let done = client.finish(&call, &operation.id, client.settings().wait).await;
        drop((stdin, stdout, stderr, control));
        exit_code(&call, &done?)
    }
}

impl Client {
    /// Run `exec` in the instance `name` with its websockets: stdin, stdout
    /// and stderr apart, and the control socket.
    pub async fn exec_attached(&self, name: &str, exec: &Exec) -> Result<Attached> {
        let path = exec_path(name);
        let call = format!("POST {path}");
        if exec.command.is_empty() {
            return Err(Error::usage(&call, "a command needs a program"));
        }
        let request = self.json_request(&call, "POST", &path, &ExecPost::of(exec, false))?;
        let operation = self.begin(&call, request).await?;
        let fds = operation.texts("fds");
        let secret = |fd: &str| {
            fds.get(fd).cloned().ok_or_else(|| {
                Error::protocol(&call, format!("the operation {} gives no websocket for {fd}", operation.id))
            })
        };
        let (control, stdin, stdout, stderr) = (secret("control")?, secret("0")?, secret("1")?, secret("2")?);
        let control = self.websocket(&operation.id, &control).await?;
        let stdin = self.websocket(&operation.id, &stdin).await?;
        let stdout = self.websocket(&operation.id, &stdout).await?;
        let stderr = self.websocket(&operation.id, &stderr).await?;
        let ws_call = format!("GET /1.0/operations/{}/websocket", escape(&operation.id));
        Ok(Attached {
            client: self.clone(),
            call,
            operation,
            stdin: Stdin { socket: stdin, call: ws_call.clone() },
            stdout: Output { socket: stdout, call: ws_call.clone(), ended: false },
            stderr: Output { socket: stderr, call: ws_call.clone(), ended: false },
            control: Control { socket: control, call: ws_call },
        })
    }

    /// The websocket of the operation `id` with `secret`.
    async fn websocket(&self, id: &str, secret: &str) -> Result<Socket> {
        let path = format!("/1.0/operations/{}/websocket", escape(id));
        let call = format!("GET {path}");
        let key =
            pod_ws::handshake::generate_key().map_err(|e| Error::new(&call, ErrorKind::WebSocket(e.to_string())))?;
        let mut stream = self.open(&call).await?;
        let agent = &self.settings().user_agent;
        if agent.contains(['\r', '\n', '\0']) {
            return Err(Error::usage(&call, "the user agent holds CR, LF or NUL"));
        }
        let mut head = format!("GET {} HTTP/1.1\r\nHost: {HOST}\r\n", self.target(&path, &[("secret", secret)]));
        if !agent.is_empty() {
            head.push_str(&format!("User-Agent: {agent}\r\n"));
        }
        head.push_str("Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\n");
        head.push_str(&format!("Sec-WebSocket-Key: {key}\r\n\r\n"));
        stream.write_all(head.as_bytes()).await.map_err(|e| Error::io(&call, e))?;
        stream.flush().await.map_err(|e| Error::io(&call, e))?;
        let (answer, rest) = read_head(&mut stream, Vec::new(), MAX_HEAD).await.map_err(|e| Error::io(&call, e))?;
        if answer.status != 101 {
            let limit = u64::try_from(self.settings().max_body).unwrap_or(u64::MAX);
            let body = AnswerBody::new(&mut stream, rest, Framing::of(&answer, false), Some(limit))
                .map_err(|e| Error::io(&call, e))?
                .to_end()
                .await
                .map_err(|e| Error::io(&call, e))?;
            envelope::read(&call, answer.status, &body)?;
            let error = far_text(&answer.reason);
            return Err(Error::new(&call, ErrorKind::Incus { status: answer.status, code: answer.status, error }));
        }
        let upgraded = answer.header("upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
        let accepted = answer.header("sec-websocket-accept") == Some(pod_ws::handshake::accept_key(&key).as_str());
        if !upgraded || !accepted {
            let why = "a 101 with no Upgrade: websocket or with a Sec-WebSocket-Accept of another key";
            return Err(Error::new(&call, ErrorKind::WebSocket(why.into())));
        }
        tracing::debug!(call, "incus websocket open");
        Ok(RelaySession::new(stream, rest, None, self.settings().write_timeout).naming("incus"))
    }
}
