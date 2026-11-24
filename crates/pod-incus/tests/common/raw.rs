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

/// What the stand-in does after its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    /// Close the connection, with the rest of the request unread.
    Close,
    /// Keep the connection open and send no more bytes; shut its read side
    /// and drop what came: the writer of a body gets EPIPE, the reader of
    /// the answer gets nothing more.
    Hold,
    /// [`After::Hold`], with the write side shut too: the reader of the
    /// answer gets its end, and no error of a reset.
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
                if after != After::Close {
                    let stream = stream.into_std().expect("the stream of the stand-in");
                    let how = if after == After::Shut { std::net::Shutdown::Both } else { std::net::Shutdown::Read };
                    let _ = stream.shutdown(how);
                    let mut stream = UnixStream::from_std(stream).expect("the stream of the stand-in");
                    // The bytes in the queue keep a blocked writer asleep:
                    // with them read, its next write sees the shut side.
                    let mut sink = vec![0u8; 64 * 1024];
                    while matches!(stream.read(&mut sink).await, Ok(n) if n > 0) {}
                    holding.lock().unwrap().push(stream);
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
