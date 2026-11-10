use std::future::poll_fn;
use std::sync::atomic::AtomicUsize;
use std::task::{Wake, Waker};

use super::*;

#[test]
fn send_poller_reports_ready_when_the_queue_has_room() {
    let (out, _rx) = mpsc::channel::<(SocketAddr, Vec<u8>)>(4);
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    assert!(matches!(poll_room(&out, &SendReady::default(), &mut cx), Poll::Ready(Ok(()))));
}

/// A waker that counts its wakes.
struct Count(AtomicUsize);

impl Wake for Count {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

// podnet's T-225: a full queue parks the poller; a drain after that must
// wake it.
#[test]
fn a_drain_after_pending_wakes_the_send_poller() {
    let (out, mut rx) = mpsc::channel::<(SocketAddr, Vec<u8>)>(1);
    let to: SocketAddr = "192.0.2.1:7844".parse().unwrap();
    out.try_send((to, vec![0])).unwrap();
    let ready = SendReady::default();
    let count = Arc::new(Count(AtomicUsize::new(0)));
    let waker = Waker::from(count.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(poll_room(&out, &ready, &mut cx).is_pending());
    // The writer task takes one datagram and tells the poller.
    rx.try_recv().unwrap();
    ready.wake();
    assert_eq!(count.0.load(Ordering::SeqCst), 1, "the poller was not woken");
    assert!(matches!(poll_room(&out, &ready, &mut cx), Poll::Ready(Ok(()))));
}

/// One end of a pair of links in memory: what one end sends, the other
/// receives, from the address of the sender.
struct Half {
    local: SocketAddr,
    to: mpsc::UnboundedSender<(Vec<u8>, SocketAddr)>,
    from: tokio::sync::Mutex<mpsc::UnboundedReceiver<(Vec<u8>, SocketAddr)>>,
}

fn pair() -> (Arc<Half>, Arc<Half>) {
    let (a_tx, a_rx) = mpsc::unbounded_channel();
    let (b_tx, b_rx) = mpsc::unbounded_channel();
    let a = Half { local: "10.0.0.1:1000".parse().unwrap(), to: b_tx, from: tokio::sync::Mutex::new(a_rx) };
    let b = Half { local: "10.0.0.2:2000".parse().unwrap(), to: a_tx, from: tokio::sync::Mutex::new(b_rx) };
    (Arc::new(a), Arc::new(b))
}

impl Link for Half {
    fn send_to(&self, datagram: Vec<u8>, _to: SocketAddr) -> BoxFuture<'_, io::Result<()>> {
        Box::pin(async move { self.to.send((datagram, self.local)).map_err(|_| broken()) })
    }

    fn recv_from(&self, max: usize) -> BoxFuture<'_, io::Result<(Vec<u8>, SocketAddr)>> {
        Box::pin(async move {
            let (mut data, from) = self.from.lock().await.recv().await.ok_or_else(broken)?;
            data.truncate(max);
            Ok((data, from))
        })
    }

    fn local_addr(&self) -> SocketAddr {
        self.local
    }
}

#[tokio::test]
async fn a_link_carries_datagrams_both_ways_with_their_senders() {
    let (a, b) = pair();
    let (qa, qb) = (QueuedDatagrams::new(a.clone()), QueuedDatagrams::new(b.clone()));
    assert_eq!(qa.local_addr().unwrap(), a.local);
    pod_core::datagrams::send_to(&qa, b"ping", b.local).await.unwrap();
    let mut buf = [0u8; 64];
    let (n, from) = pod_core::datagrams::recv_from(&qb, &mut buf).await.unwrap();
    assert_eq!((&buf[..n], from), (&b"ping"[..], a.local));
    // Three segments of one transmit are three datagrams.
    let t = Transmit { destination: a.local, contents: b"aaabbbc", segment_size: Some(3), src_ip: None };
    poll_fn(|cx| qb.poll_writable(cx)).await.unwrap();
    qb.try_send(&t).unwrap();
    let mut got = Vec::new();
    for _ in 0..3 {
        let (n, _) = pod_core::datagrams::recv_from(&qa, &mut buf).await.unwrap();
        got.push(buf[..n].to_vec());
    }
    assert_eq!(got, [b"aaa".to_vec(), b"bbb".to_vec(), b"c".to_vec()]);
    assert_eq!((qa.counts(), qb.counts()), ((1, 3), (3, 1)));
}

/// A link that ends: the datagrams that came first are read, then each
/// receive and each send fails at once.
#[tokio::test]
async fn a_link_that_ends_fails_after_its_last_datagram() {
    let (a, b) = pair();
    let qb = QueuedDatagrams::new(b.clone());
    a.to.send((b"last".to_vec(), a.local)).unwrap();
    drop(a);
    let mut buf = [0u8; 16];
    let (n, _) = pod_core::datagrams::recv_from(&qb, &mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"last");
    let e = pod_core::datagrams::recv_from(&qb, &mut buf).await.unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::BrokenPipe);
    let e = pod_core::datagrams::send_to(&qb, b"x", "10.0.0.1:1000".parse().unwrap()).await.unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::BrokenPipe);
}
