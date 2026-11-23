//! One session over two carriers (PodUtils/podnet#473 row E7-1,
//! PodUtils/podnet@T-302): the bytes of the dialer go up on one carrier,
//! the bytes of the listener come down on another (for example the dns
//! carrier up and the simplex poll down). The link sees one byte stream; its
//! credits and its acknowledgements for the bytes up ride in the bytes of
//! the listener, on the down half, and the slower half sets the
//! throughput of its direction.
//!
//! * The dialer dials both carriers and writes a hello on each: `PODSPLIT`,
//!   the version 1, the half (1 up, 2 down) and a session of 128 random
//!   bits. The listener joins the two halves of a session, checks that they
//!   proved the same key, and writes `JOINED` down; the dial ends there.
//! * Then each half carries frames of a type and a length: on its main
//!   direction the bytes of one end (`DATA`, at most 64 KiB) and their
//!   `END`, on its other direction beats alone. Each direction carries a
//!   `BEAT` after a silence of [`Split::beat`] (10 s), so each end sees
//!   each half live both ways.
//! * A half that is silent for [`Split::lost_after`] (30 s), fails, or ends
//!   before the bytes of its peer ended, ends the session with the error
//!   [`Lost`] (`up-carrier lost` or `down-carrier lost`, kind
//!   `ConnectionAborted`). No half takes over the bytes of the other
//!   (call C492).
//! * The dial address is `UP|DOWN` (each in the form of its carrier), the
//!   listen address the same; [`Split::connect`] and [`Split::bind`] take
//!   the two apart.

mod listen;
mod run;
pub mod wire;

use std::fmt;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt as _;
use tokio::time::timeout;

pub use listen::SplitListener;

use crate::carrier::{BoxIo, Carrier, CarrierError, CarrierErrorKind, Facts, Listener, Pending};
use run::{End, Timing};

/// The name of the carrier.
pub const NAME: &str = "split";

/// The silence of a direction after which its sender writes a beat.
pub const BEAT: Duration = Duration::from_secs(10);

/// The silence of a direction after which its half is lost.
pub const LOST_AFTER: Duration = Duration::from_secs(30);

/// The time limit of the hellos and of the join.
pub const JOIN_TIMEOUT: Duration = Duration::from_secs(30);

/// One half of a split session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Half {
    /// The bytes of the dialer.
    Up,
    /// The bytes of the listener.
    Down,
}

impl Half {
    pub fn name(self) -> &'static str {
        match self {
            Half::Up => "up",
            Half::Down => "down",
        }
    }
}

/// The loss of a half: the error inside the `io::Error` of a read or a
/// write of a split session. Its text starts with `up-carrier lost` or
/// `down-carrier lost`, also in the text of a link's error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lost {
    pub half: Half,
    pub why: String,
}

impl fmt::Display for Lost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-carrier lost: {}", self.half.name(), self.why)
    }
}

impl std::error::Error for Lost {}

/// The loss in `e`, when a split session gave it.
pub fn lost(e: &io::Error) -> Option<&Lost> {
    e.get_ref()?.downcast_ref::<Lost>()
}

/// The half that a text of an error names as lost (`up-carrier lost`,
/// `down-carrier lost`): the error of a link keeps the text alone.
pub fn lost_in(text: &str) -> Option<Half> {
    if text.contains("up-carrier lost") {
        Some(Half::Up)
    } else if text.contains("down-carrier lost") {
        Some(Half::Down)
    } else {
        None
    }
}

/// A session over an up carrier and a down carrier.
pub struct Split {
    up: Arc<dyn Carrier>,
    down: Arc<dyn Carrier>,
    /// The silence of a direction after which its sender writes a beat.
    pub beat: Duration,
    /// The silence of a direction after which its half is lost (more than
    /// `beat`).
    pub lost_after: Duration,
    /// The time limit of the hellos and of the join.
    pub join_timeout: Duration,
}

impl fmt::Debug for Split {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Split")
            .field("up", &self.up.name())
            .field("down", &self.down.name())
            .field("beat", &self.beat)
            .field("lost_after", &self.lost_after)
            .field("join_timeout", &self.join_timeout)
            .finish()
    }
}

fn err(kind: CarrierErrorKind, text: impl Into<String>) -> CarrierError {
    CarrierError::new(NAME, kind, text)
}

fn lost_error(half: Half, e: CarrierError) -> CarrierError {
    CarrierError::new(e.carrier, e.kind, format!("the {} half: {e}", half.name()))
}

impl Split {
    pub fn new(up: Arc<dyn Carrier>, down: Arc<dyn Carrier>) -> Split {
        Split { up, down, beat: BEAT, lost_after: LOST_AFTER, join_timeout: JOIN_TIMEOUT }
    }

    /// The times: each more than 0, `lost_after` more than `beat`.
    pub fn check(&self) -> Result<(), CarrierError> {
        if self.beat.is_zero() || self.join_timeout.is_zero() || self.lost_after <= self.beat {
            return Err(err(CarrierErrorKind::Usage, "a time is 0, or lost_after is not more than beat"));
        }
        Ok(())
    }

    fn timing(&self) -> Timing {
        Timing { beat: self.beat, lost_after: self.lost_after }
    }

    /// The up address and the down address of `UP|DOWN`.
    pub fn addresses(text: &str) -> Result<(&str, &str), CarrierError> {
        match text.split_once('|') {
            Some((up, down)) if !up.is_empty() && !down.is_empty() => Ok((up, down)),
            _ => Err(err(CarrierErrorKind::Usage, format!("{text:?}: give UP|DOWN, the address of each carrier"))),
        }
    }

    /// Dial `up` on the up carrier and `down` on the down carrier, and join
    /// them at the listener.
    pub async fn connect(&self, up: &str, down: &str) -> Result<BoxIo, CarrierError> {
        self.check()?;
        let mut session = [0u8; 16];
        fill(&mut session)?;
        let (up_io, down_io) = tokio::join!(self.up.dial(up), self.down.dial(down));
        let (mut up_io, mut down_io) =
            (up_io.map_err(|e| lost_error(Half::Up, e))?, down_io.map_err(|e| lost_error(Half::Down, e))?);
        let join = async {
            up_io.write_all(&wire::hello(true, &session)).await?;
            up_io.flush().await?;
            down_io.write_all(&wire::hello(false, &session)).await?;
            down_io.flush().await?;
            match wire::read(&mut down_io).await? {
                Some((wire::kind::JOINED, _)) => Ok(()),
                other => Err(io::Error::new(io::ErrorKind::InvalidData, format!("no join down: {other:?}"))),
            }
        };
        match timeout(self.join_timeout, join).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(CarrierError::io(NAME, "the join of the two halves", e)),
            Err(_) => {
                let text = format!("the listener joined the two halves in no {:?}", self.join_timeout);
                return Err(err(CarrierErrorKind::Io(io::ErrorKind::TimedOut), text));
            }
        }
        let end = End { out: up_io, out_half: Half::Up, into: down_io, in_half: Half::Down };
        Ok(Box::new(run::run(end, self.timing())))
    }

    /// Listen at `up` on the up carrier and at `down` on the down carrier.
    pub async fn bind(&self, up: &str, down: &str) -> Result<SplitListener, CarrierError> {
        self.check()?;
        let up = self.up.listen(up).await.map_err(|e| lost_error(Half::Up, e))?;
        let down = self.down.listen(down).await.map_err(|e| lost_error(Half::Down, e))?;
        Ok(listen::start(self, up, down))
    }
}

/// Random bytes of the system for the id of a session.
fn fill(out: &mut [u8; 16]) -> Result<(), CarrierError> {
    use pod_core::entropy::{Entropy, OsEntropy};
    OsEntropy.fill(out).map_err(|e| err(CarrierErrorKind::Unavailable, format!("no random bytes for a session: {e}")))
}

impl Carrier for Split {
    fn name(&self) -> &'static str {
        NAME
    }

    /// Ordered and reliable when both halves are; the cost of the dearer
    /// half.
    fn facts(&self) -> Facts {
        let (up, down) = (self.up.facts(), self.down.facts());
        let cost = up.cost.max(down.cost);
        Facts {
            ordered: up.ordered && down.ordered,
            reliable: up.reliable && down.reliable,
            duplex: true,
            mtu: None,
            cost,
            rate: crate::carrier::Rate::priced(cost),
        }
    }

    /// Each half can be used.
    fn available(&self) -> Result<(), CarrierError> {
        self.check()?;
        self.up.available().map_err(|e| lost_error(Half::Up, e))?;
        self.down.available().map_err(|e| lost_error(Half::Down, e))
    }

    /// Dial `UP|DOWN` ([`Split::addresses`]).
    fn dial<'a>(&'a self, address: &'a str) -> Pending<'a, BoxIo> {
        Box::pin(async move {
            let (up, down) = Split::addresses(address)?;
            self.connect(up, down).await
        })
    }

    /// Listen at `UP|DOWN`.
    fn listen<'a>(&'a self, address: &'a str) -> Pending<'a, Box<dyn Listener>> {
        Box::pin(async move {
            let (up, down) = Split::addresses(address)?;
            Ok(Box::new(self.bind(up, down).await?) as Box<dyn Listener>)
        })
    }
}
