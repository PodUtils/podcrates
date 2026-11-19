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
//! dead. A move that fails lifts the hold, and the old link goes on.

use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;

/// Whether the link is held, and a wake for each turn of it.
#[derive(Debug, Default)]
pub struct Hold {
    on: AtomicBool,
    turned: Notify,
}

impl Hold {
    /// Hold the link: a move to a new link started.
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
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

    use super::super::super::decode::Decoder;
    use super::super::super::link::Link;
    use super::super::super::record::Record;
    use super::super::{Carry, Watch, run};

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
}
