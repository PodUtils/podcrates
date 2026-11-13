//! The dialer's side of the DNS carrier: a [`crate::datagram::Datagrams`]
//! that sends each datagram up in the labels of a query name and takes each
//! datagram down from the TXT records of the answer. A driver task starts a
//! query for each datagram, at the rate and the count in flight of
//! [`DnsSettings`]; when the ARQ has nothing to send, it polls, so the
//! listener can send its bytes down. A reader task matches each answer to
//! its query by the DNS ID.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use pod_core::Datagrams as AddressedDatagrams;
use pod_dns::txt;
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};
use tokio::time::{Instant, sleep, sleep_until, timeout};

use super::settings::DnsSettings;
use super::wire;
use crate::datagram::{DatagramFuture, Datagrams};

/// The largest answer that the reader takes.
const MAX_ANSWER: usize = 1 << 16;

/// The waiters for an answer, by DNS ID.
type Pending = Arc<Mutex<HashMap<u16, oneshot::Sender<Vec<u8>>>>>;

/// The dialer's datagram carrier over DNS.
pub(super) struct DnsClient {
    mtu: usize,
    up: mpsc::Sender<Vec<u8>>,
    down: AsyncMutex<mpsc::Receiver<Vec<u8>>>,
    /// Its drop ends the reader task.
    _stop: oneshot::Sender<()>,
}

impl DnsClient {
    /// Start the carrier over `socket`, toward `server`, under `zone`, with
    /// the session id `session`.
    pub(super) fn new(
        socket: Arc<dyn AddressedDatagrams>,
        server: SocketAddr,
        zone: String,
        session: u32,
        settings: DnsSettings,
    ) -> Result<DnsClient, String> {
        let mtu = wire::payload_mtu(&zone)?;
        let (up_tx, up_rx) = mpsc::channel(settings.depth);
        let (down_tx, down_rx) = mpsc::channel(settings.depth);
        let (stop_tx, stop_rx) = oneshot::channel();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        tokio::spawn(read(socket.clone(), pending.clone(), stop_rx));
        tokio::spawn(drive(Driver { socket, server, zone, session, settings, pending }, up_rx, down_tx));
        Ok(DnsClient { mtu, up: up_tx, down: AsyncMutex::new(down_rx), _stop: stop_tx })
    }
}

impl Datagrams for DnsClient {
    fn mtu(&self) -> usize {
        self.mtu
    }

    fn send<'a>(&'a self, datagram: &'a [u8]) -> DatagramFuture<'a, ()> {
        Box::pin(async move {
            // A full queue loses the datagram, as a lossy carrier does.
            let _ = self.up.try_send(datagram.to_vec());
            Ok(())
        })
    }

    fn recv(&self) -> DatagramFuture<'_, Vec<u8>> {
        Box::pin(async move {
            let mut down = self.down.lock().await;
            match down.recv().await {
                Some(datagram) => Ok(datagram),
                // The driver ended: no datagram comes again.
                None => std::future::pending().await,
            }
        })
    }
}

impl std::fmt::Debug for DnsClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DnsClient").field("mtu", &self.mtu).finish_non_exhaustive()
    }
}

/// What the driver and its queries share.
struct Driver {
    socket: Arc<dyn AddressedDatagrams>,
    server: SocketAddr,
    zone: String,
    session: u32,
    settings: DnsSettings,
    pending: Pending,
}

/// Read the socket and give each answer to the query that waits for its ID.
async fn read(socket: Arc<dyn AddressedDatagrams>, pending: Pending, mut stop: oneshot::Receiver<()>) {
    let mut buf = vec![0u8; MAX_ANSWER];
    loop {
        tokio::select! {
            _ = &mut stop => return,
            got = pod_core::datagrams::recv_from(&*socket, &mut buf) => {
                let Ok((n, _)) = got else { return };
                if n < 2 {
                    continue;
                }
                let id = u16::from_be_bytes([buf[0], buf[1]]);
                let waiter = pending.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
                if let Some(waiter) = waiter {
                    let _ = waiter.send(buf[..n].to_vec());
                }
            }
        }
    }
}

/// Start a query for each datagram up, or a poll when none waits.
async fn drive(driver: Driver, mut up: mpsc::Receiver<Vec<u8>>, down: mpsc::Sender<Vec<u8>>) {
    let limit = Arc::new(tokio::sync::Semaphore::new(driver.settings.in_flight));
    let period = driver.settings.period();
    let driver = Arc::new(driver);
    let mut next_at = Instant::now();
    let mut counter = 0u32;
    let mut idle = driver.settings.poll_min;
    loop {
        sleep_until(next_at).await;
        next_at = Instant::now() + period;
        let Ok(permit) = limit.clone().acquire_owned().await else { return };
        let payload = tokio::select! {
            d = up.recv() => match d {
                Some(d) => {
                    idle = driver.settings.poll_min;
                    d
                }
                None => return,
            },
            () = sleep(idle) => {
                idle = (idle * 2).min(driver.settings.poll_max);
                Vec::new()
            }
        };
        counter = counter.wrapping_add(1);
        let (driver, down) = (driver.clone(), down.clone());
        tokio::spawn(async move {
            let _permit = permit;
            if let Ok(datagrams) = query(&driver, counter, &payload).await {
                for d in datagrams {
                    if down.send(d).await.is_err() {
                        return;
                    }
                }
            }
        });
    }
}

/// One query of `payload`, and the datagrams of its answer. A failure or a
/// time out is no datagram: the ARQ sends the frame again.
async fn query(driver: &Driver, counter: u32, payload: &[u8]) -> io::Result<Vec<Vec<u8>>> {
    let id = counter as u16;
    let name = wire::encode_query(&driver.zone, driver.session, counter, payload)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let message = txt::query(id, &name, txt::TXT, Some(driver.settings.answer))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let (tx, rx) = oneshot::channel();
    driver.pending.lock().unwrap_or_else(PoisonError::into_inner).insert(id, tx);
    let answer = async {
        pod_core::datagrams::send_to(&*driver.socket, &message, driver.server).await?;
        rx.await.map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))
    };
    let result = timeout(driver.settings.query_timeout, answer).await;
    // Remove a waiter that timed out or failed.
    driver.pending.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
    let Ok(Ok(message)) = result else { return Ok(Vec::new()) };
    match txt::read(&message, id, &name, txt::TXT) {
        Ok(answer) => Ok(answer.txt()),
        Err(_) => Ok(Vec::new()),
    }
}
