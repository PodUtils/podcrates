//! The frames of `pod-link` (T-300).

/// The bytes of one direction of a link (`pod_link::Decoder`; new in
/// `pod-link`, T-300), pushed in pieces whose sizes the first byte sets;
/// its low bit asks for the preface first. Each frame that comes out keeps
/// the rules of the protocol (a stream frame has an id, a body has at most
/// 1 MiB) and encodes to bytes that decode to the same frame; after an
/// error, each call gives the same error, and nothing after it is read.
pub fn link_frame(data: &[u8]) {
    use pod_link::{Decoder, Frame, MAX_RECORD};
    let Some((&first, bytes)) = data.split_first() else { return };
    let step = usize::from(first >> 1) % 64 + 1;
    let mut decoder = if first & 1 == 1 { Decoder::with_preface() } else { Decoder::new() };
    for piece in bytes.chunks(step) {
        decoder.push(piece);
        loop {
            match decoder.next() {
                Ok(Some(frame)) => {
                    match &frame {
                        Frame::Open(id) | Frame::Close(id) | Frame::Reset(id, _) => assert_ne!(*id, 0),
                        Frame::Data(id, body) => {
                            assert_ne!(*id, 0);
                            assert!(!body.is_empty() && body.len() <= MAX_RECORD, "{} bytes", body.len());
                        }
                        Frame::Window(_, credit) => assert_ne!(*credit, 0),
                        Frame::Ping(_) | Frame::Pong(_) | Frame::GoAway(_) => {}
                    }
                    let mut again = Vec::new();
                    frame.encode(&mut again).expect("a decoded frame encodes");
                    assert_eq!(again.len(), frame.wire_len());
                    let mut other = Decoder::new();
                    other.push(&again);
                    assert_eq!(other.next().expect("its bytes decode"), Some(frame));
                    assert_eq!(other.buffered(), 0);
                }
                Ok(None) => break,
                Err(e) => {
                    decoder.push(b"\x01\x00\x00\x00\x01\x00\x00\x00");
                    assert_eq!(decoder.next().unwrap_err(), e);
                    return;
                }
            }
        }
    }
}

/// The unit of the carrier of `link_arq`.
pub const ARQ_MTU: usize = 1200;

/// `pod_link::arq::Arq` takes the datagrams of a stranger: the input is a
/// byte of setup (the bytes that this end writes: `b * 64`), then datagrams,
/// each a step of the clock in tens of milliseconds, a length (u16, big
/// endian) and its bytes. After each datagram the ARQ sends what it has and
/// fires its timer when its time passed. No panic; each datagram that it
/// sends decodes and fits the unit; a `DATA` frame of one sequence always
/// carries the same fragment, also when it goes again; an error stays.
pub fn link_arq(data: &[u8]) {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    use pod_link::arq::wire::Datagram;
    use pod_link::arq::{Arq, ArqSettings};

    let Some((&setup, mut rest)) = data.split_first() else { return };
    let Ok(mut arq) = Arq::new(ArqSettings::default(), ARQ_MTU) else { return };
    let _ = arq.write(&vec![0x5a; usize::from(setup) * 64]);
    let mut now = Instant::now();
    let mut sent: HashMap<u32, (u16, u16, Vec<u8>)> = HashMap::new();
    let mut drain = |arq: &mut Arq, now: Instant| {
        while let Some(out) = arq.poll_transmit(now) {
            assert!(out.len() <= ARQ_MTU, "{} bytes over the unit", out.len());
            match Datagram::decode(&out).expect("a datagram of the ARQ decodes") {
                Datagram::Data { seq, index, count, payload } => {
                    let fragment = (index, count, payload);
                    let first = sent.entry(seq).or_insert_with(|| fragment.clone());
                    assert!(*first == fragment, "the sequence {seq} carries two fragments");
                }
                Datagram::Fin { .. } | Datagram::Ack { .. } | Datagram::Probe => {}
            }
        }
    };
    drain(&mut arq, now);
    while rest.len() >= 3 {
        let step = Duration::from_millis(u64::from(rest[0]) * 10);
        let len = usize::from(u16::from_be_bytes([rest[1], rest[2]])).min(rest.len() - 3);
        let (datagram, after) = rest[3..].split_at(len);
        rest = after;
        now += step;
        let failed = arq.error().cloned();
        arq.on_datagram(now, datagram);
        if let Some(due) = arq.poll_timeout()
            && due <= now
        {
            arq.on_timeout(now);
        }
        drain(&mut arq, now);
        if let Some(e) = failed {
            assert_eq!(arq.error(), Some(&e), "an error of the ARQ stays");
        }
    }
}

/// The bytes of one connection, given in reads of `step` bytes at most.
struct Pieces<'a> {
    bytes: &'a [u8],
    step: usize,
    at: usize,
}

impl tokio::io::AsyncRead for Pieces<'_> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let n = (self.bytes.len() - self.at).min(self.step).min(buf.remaining());
        buf.put_slice(&self.bytes[self.at..self.at + n]);
        self.at += n;
        std::task::Poll::Ready(Ok(()))
    }
}

impl tokio::io::AsyncWrite for Pieces<'_> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Ok(data.len()))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

/// The output of `future`, which reads bytes in memory and never waits.
fn ready<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
        std::task::Poll::Ready(output) => output,
        std::task::Poll::Pending => panic!("a read of bytes in memory waited"),
    }
}

/// The bytes of one connection of the poll carriers to the reader of
/// HTTP/1.1 and of SSE (`pod_link::poll::fuzzing`; new in `pod-link`,
/// T-422, T-518). The first byte sets the size of each read: 1 to 64
/// bytes, or as many as the reader takes for 0. The target reads a head, then its body: each chunk of a chunked
/// body (at most two batches, as the dialer reads) goes into the reader of
/// SSE, and a body of a Content-Length of at most one batch is read whole;
/// then the next head, until an end or an error. No panic; a head takes at
/// most 4 KiB with its blank line; a chunk is not empty and has at most its
/// limit; a body has its length; an event gives at most 8 KiB.
pub fn link_poll_wire(data: &[u8]) {
    use pod_link::poll::BATCH;
    use pod_link::poll::fuzzing::{Conn, EVENT, Event, Events, MAX_HEAD};

    let Some((&first, bytes)) = data.split_first() else { return };
    let step = if first == 0 { usize::MAX } else { usize::from(first) % 64 + 1 };
    let mut conn = Conn::new(Pieces { bytes, step, at: 0 });
    loop {
        let before = conn.io.at - conn.buffered();
        let Ok(Some(head)) = ready(conn.head()) else { return };
        let taken = conn.io.at - conn.buffered() - before;
        assert!(taken <= MAX_HEAD, "a head of {taken} bytes");
        let _ = (head.status(), head.request(), head.close(), head.field("accept"));
        if head.chunked() {
            let mut events = Events::default();
            loop {
                let chunk = match ready(conn.chunk(2 * BATCH)) {
                    Ok(Some(chunk)) => chunk,
                    Ok(None) => break,
                    Err(_) => return,
                };
                assert!(!chunk.is_empty() && chunk.len() <= 2 * BATCH, "a chunk of {} bytes", chunk.len());
                if events.push(&chunk).is_err() {
                    return;
                }
                loop {
                    match events.next() {
                        Ok(Some(Event::Data(bytes))) => assert!(bytes.len() <= EVENT, "an event of {}", bytes.len()),
                        Ok(Some(Event::Ping | Event::Fin)) => {}
                        Ok(None) => break,
                        Err(_) => return,
                    }
                }
            }
            continue;
        }
        match head.length() {
            Ok(Some(len)) if len <= BATCH => match ready(conn.body(len)) {
                Ok(body) => assert_eq!(body.len(), len),
                Err(_) => return,
            },
            Ok(_) => {}
            Err(_) => return,
        }
    }
}

/// The largest datagram of the carrier of `link_kcp`.
pub const KCP_MTU: usize = 1200;

std::thread_local! {
    /// The panics of this thread: in the code of the crate `kcp`, and in
    /// other code.
    static PANICS: std::cell::Cell<(u32, u32)> = const { std::cell::Cell::new((0, 0)) };
}

/// The source of the crate `kcp`, as a panic names it.
fn of_kcp(file: &str) -> bool {
    file.replace('\\', "/").contains("/kcp-0.")
}

/// A panic hook over the one before it (libFuzzer's aborts): a panic in
/// the code of the crate `kcp` is counted and unwinds, so that the guard of
/// the carrier acts on it; another panic is counted and goes to the hook
/// before.
fn count_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let kcp = info.location().is_some_and(|at| of_kcp(at.file()));
            PANICS.with(|p| {
                let (k, o) = p.get();
                p.set(if kcp { (k + 1, o) } else { (k, o + 1) });
            });
            if !kcp {
                before(info);
            }
        }));
    });
}

/// The datagrams of a peer, each after its step of the clock; then
/// silence. It keeps the size of the largest datagram sent.
struct Script {
    datagrams: std::sync::Mutex<std::collections::VecDeque<(std::time::Duration, Vec<u8>)>>,
    largest: std::sync::atomic::AtomicUsize,
}

impl pod_link::datagram::Datagrams for Script {
    fn mtu(&self) -> usize {
        KCP_MTU
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> pod_link::datagram::DatagramFuture<'a, ()> {
        self.largest.fetch_max(datagram.len(), std::sync::atomic::Ordering::Relaxed);
        Box::pin(async { Ok(()) })
    }

    fn recv(&self) -> pod_link::datagram::DatagramFuture<'_, Vec<u8>> {
        let next = self.datagrams.lock().unwrap_or_else(std::sync::PoisonError::into_inner).pop_front();
        Box::pin(async move {
            let Some((step, datagram)) = next else { return std::future::pending().await };
            tokio::time::sleep(step).await;
            Ok(datagram)
        })
    }
}

/// The stream of KCP of the udp carrier (`pod_link::udp::KcpStream`, the
/// crate `kcp` 0.6.0 in its task; T-424, T-521) takes the datagrams of a
/// stranger. The input is a byte of setup (the stream writes `setup * 64`
/// bytes), then datagrams, each a step of the clock in tens of
/// milliseconds, a length (u16, big endian) and its bytes. The stream takes
/// the conversation of the first datagram, ends after a silence of 1 s, and
/// is read until it ends, on a paused clock. No panic outside the code of
/// `kcp`; the stream ends with an error; no datagram that it sends passes
/// the unit; and when the code of `kcp` panicked (its arithmetic overflows
/// on forged values in a build with overflow checks), the guard of the
/// carrier ended the stream with `BrokenPipe` and the panic went no
/// further than the task of the carrier (call C279).
pub fn link_kcp(data: &[u8]) {
    run_kcp(data);
}

/// The run of `link_kcp`: the panics in the code of `kcp` and the error that
/// ended the stream; none for an input with no byte of setup.
pub(crate) fn run_kcp(data: &[u8]) -> Option<(u32, std::io::Error)> {
    use std::sync::Arc;
    use std::time::Duration;

    use pod_link::udp::{KcpSettings, KcpStream};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let (&setup, mut rest) = data.split_first()?;
    let mut datagrams = std::collections::VecDeque::new();
    while rest.len() >= 3 {
        let step = Duration::from_millis(u64::from(rest[0]) * 10);
        let len = usize::from(u16::from_be_bytes([rest[1], rest[2]])).min(rest.len() - 3);
        let (datagram, after) = rest[3..].split_at(len);
        datagrams.push_back((step, datagram.to_vec()));
        rest = after;
    }
    let script = Arc::new(Script { datagrams: std::sync::Mutex::new(datagrams), largest: Default::default() });
    count_panics();
    PANICS.with(|p| p.set((0, 0)));
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_time().start_paused(true).build().expect("a runtime");
    let run = async {
        let settings = KcpSettings {
            mtu: KCP_MTU,
            interval: Duration::from_millis(100),
            idle: Duration::from_secs(1),
            ..KcpSettings::default()
        };
        let mut stream = KcpStream::new(script.clone(), None, settings).expect("the settings");
        stream.write_all(&vec![0x5a; usize::from(setup) * 64]).await.ok();
        let mut buf = vec![0u8; 4096];
        let read = async {
            loop {
                match stream.read(&mut buf).await {
                    Ok(n) => assert_ne!(n, 0, "the stream of KCP has no end of its own"),
                    Err(e) => return e,
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(3600), read).await.expect("the stream ends with an error")
    };
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runtime.block_on(run)));
    drop(runtime);
    let (kcp, other) = PANICS.with(|p| p.get());
    let end = match ran {
        Ok(end) => end,
        Err(_) => panic!("a panic left the task of the carrier ({kcp} in the code of kcp)"),
    };
    assert_eq!(other, 0, "a panic outside the code of kcp");
    let largest = script.largest.load(std::sync::atomic::Ordering::Relaxed);
    assert!(largest <= KCP_MTU, "a datagram of {largest} bytes");
    if kcp > 0 {
        assert_eq!(end.kind(), std::io::ErrorKind::BrokenPipe, "the guard ended the stream: {end}");
    }
    Some((kcp, end))
}
