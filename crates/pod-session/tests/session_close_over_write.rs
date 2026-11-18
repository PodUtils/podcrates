//! A link that the relay ends with its own reason (a WebSocket Close with a
//! code) while the client writes: the write fails first, with `broken
//! pipe`, and the read gives the relay's reason a moment later. The end of
//! the link names the relay's reason, not the failed write
//! (PodUtils/podssh#314).

use std::io::{Error, ErrorKind};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};

use pod_session::client::{self, Outcome};
use pod_session::{Acceptance, Ask, Decoder, End, Ended, Hello, Nonce, OsEntropy, Record, Role, SessionId, Settings};

const PIPE: usize = 256 * 1024;
const LIMIT: Duration = Duration::from_secs(30);
/// The words of the relay's Close, as a road of WebSocket gives them.
const REASON: &str = "the relay closed the link: 1009 session byte cap";

/// A link that the relay cuts: after the cut, each write fails, and the end
/// of the bytes still to read is the relay's reason.
struct Cut {
    inner: DuplexStream,
    cut: Arc<AtomicBool>,
}

impl AsyncRead for Cut {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) if buf.filled().len() == before && self.cut.load(Ordering::SeqCst) => {
                Poll::Ready(Err(Error::other(REASON)))
            }
            other => other,
        }
    }
}

impl AsyncWrite for Cut {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        if self.cut.load(Ordering::SeqCst) {
            return Poll::Ready(Err(ErrorKind::BrokenPipe.into()));
        }
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// The far end's `GREETING`, then its `ACCEPT` of a new session once the
/// client's `OPEN` came.
async fn greet_and_accept(far: &mut DuplexStream) {
    let hello = Hello { version: 1, role: Role::NODE, nonce: Nonce([7; 32]), features: vec!["replay.v1".into()] };
    far.write_all(&Record::Greeting(hello).to_bytes().unwrap()).await.unwrap();
    let mut decoder = Decoder::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        if let Some(record) = decoder.next().unwrap() {
            assert!(matches!(record, Record::Open { .. }), "{record:?}");
            break;
        }
        let n = far.read(&mut buf).await.unwrap();
        assert!(n > 0, "the client ended the link");
        decoder.push(&buf[..n]);
    }
    // RFC 7748's public key of Bob: any key that is not of low order.
    let public: [u8; 32] = [
        0xde, 0x9e, 0xdb, 0x7d, 0x7b, 0x7d, 0xc1, 0xb4, 0xd3, 0x5b, 0x61, 0xc2, 0xec, 0xe4, 0x35, 0x37, 0x3f, 0x83,
        0x43, 0xc8, 0x5b, 0x78, 0x67, 0x4d, 0xad, 0xfc, 0x7e, 0x14, 0x6f, 0x88, 0x2b, 0x4f,
    ];
    let accept = Record::Accept(Acceptance::New { id: SessionId([5; 16]), public });
    far.write_all(&accept.to_bytes().unwrap()).await.unwrap();
}

/// The relay cuts the link while the application writes: the write fails
/// at once, and the relay's reason comes to the read 300 ms later, inside
/// the drain. The link ends with the relay's reason.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_relays_close_wins_over_the_failed_write() {
    let (inner, mut far) = tokio::io::duplex(PIPE);
    let cut = Arc::new(AtomicBool::new(false));
    let link = Cut { inner, cut: cut.clone() };
    let script = tokio::spawn(async move {
        greet_and_accept(&mut far).await;
        // The client's bytes come; after the first 64 KiB, the relay cuts.
        let mut buf = vec![0u8; 64 * 1024];
        let mut seen = 0;
        while seen < 64 * 1024 {
            let n = far.read(&mut buf).await.unwrap();
            assert!(n > 0, "the client ended the link");
            seen += n;
        }
        cut.store(true, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(300)).await;
        drop(far);
    });
    let client =
        tokio::time::timeout(LIMIT, client::start(link, Ask::New, Settings::default(), &mut OsEntropy)).await.unwrap();
    let client = client.unwrap();
    let (mut app, app_end) = tokio::io::duplex(PIPE);
    let run = tokio::spawn(client.run(app_end));
    let writes = tokio::spawn(async move {
        let chunk = vec![7u8; 16 * 1024];
        while app.write_all(&chunk).await.is_ok() {}
    });
    let outcome = tokio::time::timeout(LIMIT, run).await.unwrap().unwrap();
    tokio::time::timeout(LIMIT, script).await.unwrap().unwrap();
    writes.abort();
    match &outcome {
        Outcome::Layer(Ended { end: End::Lost(Some(e)), .. }) => {
            assert!(e.to_string().contains("1009 session byte cap"), "the end names the failed write: {e}")
        }
        other => panic!("the link did not end as a loss with a cause: {other:?}"),
    }
}
