//! The hold of the old link of a move (PodUtils/podssh#325). A relay
//! counts each byte of a link against its cap. After the move is due, the
//! old link carries on until the new link greets; at the rate of a loopback
//! that is more than the room left under a small cap. While the hold is on,
//! the old link takes no new bytes of the application, sends no `DATA` that
//! waits for the writer, and acknowledges no new bytes of the far end: the
//! far end then sends its window at most, and waits. Each byte held is in
//! the replay buffer or still in the application's pipe, and the new link
//! carries it. A `PONG` still answers each `PING`, with the last offset
//! acknowledged, so the heartbeat of neither end takes the held link for
//! dead. The hold starts at the byte that passes the mark of a move
//! (`Watch::count`, PodUtils/podssh#327), or when a move at the link's age
//! starts. A move that fails lifts the hold, and so does a session that
//! closes on the link (`Watch::moves_end`); the old link then goes on.

use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;

/// Whether the link is held, and a wake for each turn of it.
#[derive(Debug, Default)]
pub struct Hold {
    on: AtomicBool,
    turned: Notify,
}

impl Hold {
    /// Hold the link: a move to a new link is due or started.
    pub fn start(&self) {
        self.on.store(true, Ordering::SeqCst);
        self.turned.notify_waiters();
    }

    /// Let the link go on: the move failed, or the session closes on it.
    pub fn lift(&self) {
        self.on.store(false, Ordering::SeqCst);
        self.turned.notify_waiters();
    }

    /// Whether the link is held.
    pub fn on(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }

    /// Until the hold is `on`, or is not.
    pub(super) async fn until(&self, on: bool) {
        loop {
            // Made before the check: a `Notified` gets each later
            // `notify_waiters`, so a turn between the two is not missed.
            let turned = self.turned.notified();
            if self.on() == on {
                return;
            }
            turned.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
    use tokio::time::Instant;

    use super::super::super::decode::Decoder;
    use super::super::super::link::Link;
    use super::super::super::record::Record;
    use super::super::{CHUNK, Carry, End, Ended, Watch, run};
    use crate::beside::{Plan, SpareWait, next};
    use crate::hearing::Hearing;
    use crate::resume::Next;

    /// The records that came on `far` within `wait`.
    async fn records(far: &mut DuplexStream, decoder: &mut Decoder, wait: Duration) -> Vec<Record> {
        let mut buf = vec![0u8; 1 << 16];
        let _ = tokio::time::timeout(wait, async {
            loop {
                match far.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => decoder.push(&buf[..n]),
                }
            }
        })
        .await;
        let mut got = Vec::new();
        while let Ok(Some(record)) = decoder.next() {
            got.push(record);
        }
        got
    }

    /// A held link takes no byte of the application and acknowledges no
    /// byte of the far end; it answers a `PING` with the offset acknowledged
    /// before the hold. Once the hold lifts, as after a failed move, the
    /// application's bytes and the `ACK` go out.
    #[tokio::test(start_paused = true)]
    async fn a_held_link_takes_no_bytes_and_acknowledges_none_until_the_hold_lifts() {
        let (mut app, mut user) = tokio::io::duplex(1 << 16);
        let (link, mut far) = tokio::io::duplex(1 << 20);
        let watch = Watch::default();
        watch.hold.start();
        let mut carry = Carry::new(Link::new(0, 0).with_replay(1 << 20));
        let pump = run(&mut app, link, Decoder::new(), &mut carry, &watch);
        tokio::pin!(pump);
        let mut decoder = Decoder::new();

        user.write_all(b"held in the pipe").await.unwrap();
        far.write_all(&Record::Data { offset: 0, bytes: vec![7; 1000] }.to_bytes().unwrap()).await.unwrap();
        far.write_all(&Record::Ping { value: 9 }.to_bytes().unwrap()).await.unwrap();
        let held = tokio::select! {
            _ = pump.as_mut() => panic!("the held link ended"),
            got = records(&mut far, &mut decoder, Duration::from_secs(5)) => got,
        };
        assert_eq!(held, vec![Record::Pong { value: 9, offset: 0 }], "only the answer of the PING, with no new offset");
        let mut delivered = [0u8; 1000];
        tokio::select! {
            _ = pump.as_mut() => panic!("the held link ended"),
            got = user.read_exact(&mut delivered) => { got.unwrap(); }
        }
        assert_eq!(delivered, [7; 1000], "the far end's bytes still reach the application");

        watch.hold.lift();
        let lifted = tokio::select! {
            _ = pump.as_mut() => panic!("the link ended"),
            got = records(&mut far, &mut decoder, Duration::from_secs(5)) => got,
        };
        assert!(lifted.contains(&Record::Ack { offset: 1000 }), "{lifted:?}");
        let data = Record::Data { offset: 0, bytes: b"held in the pipe".to_vec() };
        assert!(lifted.contains(&data), "{lifted:?}");
    }

    /// The hold starts at the byte that passes the mark of a move, not when
    /// the task beside the link wakes: with no such task for 50 ms after the
    /// mark, as behind a slow timer, the link carries the mark and one record
    /// at most (PodUtils/podssh#327). The end of the moves, as after a failed
    /// move, lets the rest go, and no byte after it starts a hold again.
    #[tokio::test(start_paused = true)]
    async fn the_hold_starts_at_the_mark_while_the_task_beside_the_link_sleeps() {
        const MARK: u64 = 256 * 1024;
        const SENT: usize = 1 << 20;
        let (mut app, mut user) = tokio::io::duplex(1 << 22);
        let (link, mut far) = tokio::io::duplex(1 << 22);
        let watch = Watch::moving_at(MARK);
        let mut carry = Carry::new(Link::new(0, 0).with_replay(1 << 21));
        let pump = run(&mut app, link, Decoder::new(), &mut carry, &watch);
        tokio::pin!(pump);

        user.write_all(&vec![5; SENT]).await.unwrap();
        tokio::select! {
            _ = pump.as_mut() => panic!("the link ended"),
            () = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
        let carried = watch.bytes.load(Ordering::SeqCst);
        assert!(carried >= MARK, "the link passed the mark: {carried}");
        assert!(carried <= MARK + (CHUNK + 64) as u64, "the mark and one record at most: {carried}");
        assert!(watch.hold.on(), "the mark started the hold");
        assert!(tokio::time::timeout(Duration::ZERO, watch.due.notified()).await.is_ok(), "the move is due");

        watch.moves_end();
        let mut decoder = Decoder::new();
        let got = tokio::select! {
            _ = pump.as_mut() => panic!("the link ended"),
            got = records(&mut far, &mut decoder, Duration::from_secs(5)) => got,
        };
        let data: usize = got.iter().map(|r| if let Record::Data { bytes, .. } = r { bytes.len() } else { 0 }).sum();
        assert_eq!(data, SENT, "each byte went out on the link once the hold lifted");
        assert!(!watch.hold.on(), "no hold after the end of the moves");
    }

    /// The application ended its bytes, and its last `DATA` passes the mark
    /// of a move: the task beside the link lifts the hold that the mark
    /// started, and the session's `CLOSE` goes on the old link with no wait;
    /// no new link opens (PodUtils/podssh#327).
    #[tokio::test(start_paused = true)]
    async fn a_session_whose_application_ends_at_the_mark_closes_on_the_old_link() {
        let (mut app, mut user) = tokio::io::duplex(1 << 16);
        let (link, mut far) = tokio::io::duplex(1 << 16);
        user.write_all(b"the last bytes").await.unwrap();
        user.shutdown().await.unwrap();
        let watch = Watch::moving_at(1);
        let mut carry = Carry::new(Link::new(0, 0).with_replay(1 << 16));
        let (state, hearing) = (carry.state.clone(), Hearing::default());
        let (said, _notes) = tokio::sync::mpsc::unbounded_channel();
        let plan = Plan {
            moves: true,
            spares: false,
            heartbeat: false,
            move_at: Instant::now() + Duration::from_secs(3600),
            closing: false,
            wait: SpareWait::default(),
            watch: &watch,
            state: &state,
            hearing: &hearing,
            said: &said,
        };
        let mut connect = |_: &Ended| async { Next::<DuplexStream>::Retry("no link".into()) };
        let pump = run(&mut app, link, Decoder::new(), &mut carry, &watch);
        let beside = next(plan, &mut connect);
        tokio::pin!(pump, beside);

        let mut decoder = Decoder::new();
        let got = tokio::select! {
            _ = pump.as_mut() => panic!("the link ended"),
            _ = beside.as_mut() => panic!("a new link beside a closing session"),
            got = records(&mut far, &mut decoder, Duration::from_secs(5)) => got,
        };
        let last = Record::Data { offset: 0, bytes: b"the last bytes".to_vec() };
        assert_eq!(got, vec![last, Record::Close { reason: String::new() }], "the CLOSE went with no wait");
        assert!(!watch.hold.on(), "the hold lifted");
        far.write_all(&Record::Close { reason: String::new() }.to_bytes().unwrap()).await.unwrap();
        let ended = tokio::select! {
            ended = pump.as_mut() => ended,
            _ = beside.as_mut() => panic!("a new link beside a closing session"),
        };
        assert!(matches!(ended.end, End::LocalEnd), "the session closed on the old link: {:?}", ended.end);
    }
}
