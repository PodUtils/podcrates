use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;

/// A way that keeps one waker, the last one, as the UDP socket of tokio
/// does: it has room or not, it is gone or not, and it holds the datagrams
/// that it took and those that wait to be read.
#[derive(Debug, Default)]
struct Way {
    full: AtomicBool,
    gone: AtomicBool,
    parked: Mutex<Option<Waker>>,
    sent: Mutex<Vec<(SocketAddr, Vec<u8>)>>,
    inbox: Mutex<VecDeque<(Vec<u8>, SocketAddr)>>,
}

impl Way {
    fn drain(&self) {
        self.full.store(false, Ordering::SeqCst);
        if let Some(waker) = self.parked.lock().unwrap().take() {
            waker.wake();
        }
    }
}

impl Datagrams for Way {
    fn poll_writable(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.full.load(Ordering::SeqCst) {
            return Poll::Ready(Ok(()));
        }
        *self.parked.lock().unwrap() = Some(cx.waker().clone());
        Poll::Pending
    }

    fn try_send(&self, t: &pod_core::Transmit<'_>) -> io::Result<()> {
        if self.gone.load(Ordering::SeqCst) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        if t.contents.len() > 1500 {
            return Err(io::Error::other("message too long"));
        }
        self.sent.lock().unwrap().push((t.destination, t.contents.to_vec()));
        Ok(())
    }

    fn poll_recv(
        &self,
        _cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [pod_core::RecvMeta],
    ) -> Poll<io::Result<usize>> {
        let mut n = 0;
        while n < bufs.len().min(meta.len()) {
            let Some((data, from)) = self.inbox.lock().unwrap().pop_front() else { break };
            bufs[n][..data.len()].copy_from_slice(&data);
            meta[n] = pod_core::RecvMeta { addr: from, len: data.len(), stride: data.len(), dst_ip: None };
            n += 1;
        }
        if n == 0 { Poll::Pending } else { Poll::Ready(Ok(n)) }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok("192.0.2.9:4433".parse().unwrap())
    }
}

/// A waker that counts its wakes.
struct Count(AtomicUsize);

impl Wake for Count {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn counted() -> (Arc<Count>, Waker) {
    let count = Arc::new(Count(AtomicUsize::new(0)));
    (count.clone(), Waker::from(count))
}

const TO: &str = "192.0.2.1:7844";

fn transmit(contents: &[u8]) -> Transmit<'_> {
    Transmit { destination: TO.parse().unwrap(), ecn: None, contents, segment_size: None, src_ip: None }
}

/// noq makes a sender for the endpoint and one for each connection. A way
/// that keeps one waker would wake the last sender that waited, and the
/// other would wait for ever: the fan-out wakes each.
#[test]
fn two_senders_on_a_full_way_each_wake() {
    let way = Arc::new(Way::default());
    way.full.store(true, Ordering::SeqCst);
    let socket = DatagramSocket::new(way.clone());
    let (mut a, mut b) = (socket.create_sender(), socket.create_sender());
    let ((count_a, waker_a), (count_b, waker_b)) = (counted(), counted());
    let packet = [1u8; 40];
    assert!(a.as_mut().poll_send(&transmit(&packet), &mut Context::from_waker(&waker_a)).is_pending());
    assert!(b.as_mut().poll_send(&transmit(&packet), &mut Context::from_waker(&waker_b)).is_pending());
    way.drain();
    assert_eq!(count_a.0.load(Ordering::SeqCst), 1, "the first sender was not woken");
    assert_eq!(count_b.0.load(Ordering::SeqCst), 1, "the second sender was not woken");
    assert!(a.as_mut().poll_send(&transmit(&packet), &mut Context::from_waker(&waker_a)).is_ready());
    assert!(b.as_mut().poll_send(&transmit(&packet), &mut Context::from_waker(&waker_b)).is_ready());
    assert_eq!(way.sent.lock().unwrap().len(), 2);
    // Each sender sent and waits no more: a later wake of the way wakes
    // none of them.
    way.full.store(true, Ordering::SeqCst);
    way.drain();
    assert_eq!((count_a.0.load(Ordering::SeqCst), count_b.0.load(Ordering::SeqCst)), (1, 1));
}

/// A way that is gone ends the send with its error; a datagram that the way
/// refuses for another cause is lost, as UDP loses one.
#[test]
fn a_gone_way_ends_the_send_and_a_refused_datagram_is_lost() {
    let way = Arc::new(Way::default());
    let socket = DatagramSocket::new(way.clone());
    let mut sender = socket.create_sender();
    let (_, waker) = counted();
    let mut cx = Context::from_waker(&waker);
    let large = [0u8; 2000];
    assert!(matches!(sender.as_mut().poll_send(&transmit(&large), &mut cx), Poll::Ready(Ok(()))));
    assert!(way.sent.lock().unwrap().is_empty(), "the refused datagram was not sent");
    way.gone.store(true, Ordering::SeqCst);
    match sender.as_mut().poll_send(&transmit(&[1, 2, 3]), &mut cx) {
        Poll::Ready(Err(e)) => assert_eq!(e.kind(), io::ErrorKind::BrokenPipe),
        other => panic!("{other:?}"),
    }
}

/// Each datagram that the way gives comes to noq with its sender and its
/// length, several in one receive.
#[test]
fn a_receive_carries_each_datagram_with_its_sender() {
    let way = Arc::new(Way::default());
    let from: SocketAddr = TO.parse().unwrap();
    way.inbox.lock().unwrap().extend([(b"one".to_vec(), from), (b"three".to_vec(), from)]);
    let mut socket = DatagramSocket::new(way.clone());
    let (_, waker) = counted();
    let (mut x, mut y) = ([0u8; 16], [0u8; 16]);
    let mut bufs = [IoSliceMut::new(&mut x), IoSliceMut::new(&mut y)];
    let mut meta = [RecvMeta::default(); 2];
    match socket.poll_recv(&mut Context::from_waker(&waker), &mut bufs, &mut meta) {
        Poll::Ready(Ok(2)) => {}
        other => panic!("{other:?}"),
    }
    assert_eq!((meta[0].addr, meta[0].len, meta[1].len), (from, 3, 5));
    assert_eq!(&x[..3], b"one");
    assert_eq!(socket.local_addr().unwrap(), "192.0.2.9:4433".parse::<SocketAddr>().unwrap());
}
