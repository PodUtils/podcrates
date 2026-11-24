//! A stand-in that answers each connection with bytes that a test makes,
//! after the head of the request, and then closes the connection or holds
//! it: a socket that takes the connection and never answers, a body that
//! stalls, an answer at the first bytes of an upload.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pod_incus::{Client, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;

/// What the stand-in does after its answer. Each form gives the client the
/// same cut on Linux and on macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    /// Close the connection, with the rest of the request unread: the
    /// writer of a body gets a reset (Linux) or EPIPE (macOS).
    Close,
    /// Keep the connection open, and read and send no more bytes: the
    /// writer of a body stalls until `write_timeout`, the reader of the
    /// answer gets nothing more. A shutdown of the read side does not do
    /// this: on macOS it drops the queue and tells the writer nothing, so
    /// the writer waits as here, and on Linux the writer gets EPIPE.
    Hold,
    /// Shut the read side, read what came, and close: the writer of a body
    /// gets EPIPE and the reader of the answer its end, with no reset.
    Shut,
}

pub struct Raw {
    pub socket: PathBuf,
    /// The connections that the stand-in holds.
    held: Arc<Mutex<Vec<UnixStream>>>,
    task: JoinHandle<()>,
}

impl Raw {
    pub fn start(answer: Vec<u8>, after: After) -> Raw {
        let socket = super::socket_path("raw");
        let listener = UnixListener::bind(&socket).expect("bind the socket of the stand-in");
        let held = Arc::new(Mutex::new(Vec::new()));
        let holding = held.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                if pod_proxy::http::read_request(&mut stream, Vec::new()).await.is_err() {
                    continue;
                }
                if !answer.is_empty() && stream.write_all(&answer).await.is_err() {
                    continue;
                }
                match after {
                    After::Close => {}
                    After::Hold => holding.lock().unwrap().push(stream),
                    After::Shut => {
                        // Linux: each later write of the peer fails, so the
                        // read empties the queue and the close sends no
                        // reset. macOS: the shutdown drops the queue, and
                        // the close gives the peer EPIPE.
                        let stream = stream.into_std().expect("the stream of the stand-in");
                        let _ = stream.shutdown(std::net::Shutdown::Read);
                        let mut stream = UnixStream::from_std(stream).expect("the stream of the stand-in");
                        let mut sink = vec![0u8; 64 * 1024];
                        while matches!(stream.read(&mut sink).await, Ok(n) if n > 0) {}
                    }
                }
            }
        });
        Raw { socket, held, task }
    }

    pub fn settings(&self) -> Settings {
        let mut settings = Settings::new(&self.socket);
        settings.user_agent = "pod-incus-test".into();
        settings
    }

    pub fn client(&self) -> Client {
        Client::new(self.settings())
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        self.task.abort();
        self.held.lock().unwrap().clear();
        let _ = std::fs::remove_file(&self.socket);
    }
}
