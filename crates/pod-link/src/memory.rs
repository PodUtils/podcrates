//! The memory carrier: two ends in one process, for tests and for an
//! embedder that runs both peers. Each direction is a ring of a fixed size:
//! a full ring makes the writer wait, and never drops a byte. The end of
//! one side is the end of stream of the other.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::io::DuplexStream;
use tokio::sync::mpsc;

use crate::carrier::{BoxIo, Carrier, CarrierError, CarrierErrorKind, Facts, Listener, Pending};

/// The name of the carrier.
pub const NAME: &str = "memory";

/// The ring of each direction by default: 1 MiB.
pub const DEFAULT_RING: usize = 1 << 20;

/// The smallest ring: 64 KiB.
pub const MIN_RING: usize = 64 << 10;

/// The largest ring: 64 MiB.
pub const MAX_RING: usize = 64 << 20;

/// The dials that wait for a listener's accept.
const BACKLOG: usize = 64;

/// Two connected ends with a ring of `ring` bytes in each direction.
pub fn pair(ring: usize) -> (DuplexStream, DuplexStream) {
    tokio::io::duplex(ring)
}

/// The memory carrier: the names of its listeners and the size of the
/// rings. A clone shares the names; two values of [`Memory::new`] do not.
#[derive(Clone)]
pub struct Memory {
    ring: usize,
    names: Arc<Mutex<HashMap<String, mpsc::Sender<DuplexStream>>>>,
}

impl Memory {
    /// A carrier whose rings have `ring` bytes: [`MIN_RING`] to [`MAX_RING`].
    pub fn new(ring: usize) -> Result<Memory, CarrierError> {
        if !(MIN_RING..=MAX_RING).contains(&ring) {
            return Err(CarrierError::new(
                NAME,
                CarrierErrorKind::Usage,
                format!("a ring of {ring} bytes: give {MIN_RING} to {MAX_RING}"),
            ));
        }
        Ok(Memory { ring, names: Arc::default() })
    }

    /// The bytes of each ring.
    pub fn ring(&self) -> usize {
        self.ring
    }
}

impl Default for Memory {
    fn default() -> Memory {
        Memory { ring: DEFAULT_RING, names: Arc::default() }
    }
}

impl std::fmt::Debug for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Memory").field("ring", &self.ring).finish()
    }
}

fn check_name(name: &str) -> Result<(), CarrierError> {
    if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) {
        return Err(CarrierError::new(
            NAME,
            CarrierErrorKind::Usage,
            "a name of 1 to 255 bytes with no control character is necessary",
        ));
    }
    Ok(())
}

impl Carrier for Memory {
    fn name(&self) -> &'static str {
        NAME
    }

    fn facts(&self) -> Facts {
        Facts::STREAM
    }

    fn available(&self) -> Result<(), CarrierError> {
        Ok(())
    }

    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            check_name(address)?;
            let to = self.names.lock().unwrap_or_else(PoisonError::into_inner).get(address).cloned();
            let missing = || CarrierError::new(NAME, CarrierErrorKind::NotFound, format!("no listener at {address:?}"));
            let to = to.ok_or_else(missing)?;
            let (near, far) = pair(self.ring);
            to.send(far).await.map_err(|_| missing())?;
            Ok(Box::new(near) as BoxIo)
        })
    }

    fn listen<'a>(&'a self, address: &'a str) -> Pending<'a, Box<dyn Listener>> {
        Box::pin(async move {
            check_name(address)?;
            let mut names = self.names.lock().unwrap_or_else(PoisonError::into_inner);
            if names.get(address).is_some_and(|s| !s.is_closed()) {
                return Err(CarrierError::new(
                    NAME,
                    CarrierErrorKind::InUse,
                    format!("another listener holds {address:?}"),
                ));
            }
            let (tx, rx) = mpsc::channel(BACKLOG);
            names.insert(address.to_string(), tx);
            Ok(Box::new(MemoryListener { name: address.to_string(), rx }) as Box<dyn Listener>)
        })
    }
}

/// A listener of the memory carrier. Its drop frees its name.
struct MemoryListener {
    name: String,
    rx: mpsc::Receiver<DuplexStream>,
}

impl Listener for MemoryListener {
    fn address(&self) -> String {
        self.name.clone()
    }

    fn accept(&mut self) -> Pending<'_, BoxIo> {
        Box::pin(async move {
            match self.rx.recv().await {
                Some(io) => Ok(Box::new(io) as BoxIo),
                None => Err(CarrierError::new(NAME, CarrierErrorKind::Unavailable, "the listener is closed")),
            }
        })
    }
}
