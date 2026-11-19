//! What the caller of one link watches and steers.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tokio::sync::Notify;
use tokio::sync::mpsc::UnboundedSender;

use super::Stall;
use super::hold::Hold;
use crate::resume::Note;

/// What the caller of one link watches and steers.
#[derive(Debug, Default)]
pub struct Watch {
    /// Ends the link at a record boundary, with [`super::End::Stopped`].
    pub stop: Notify,
    /// Whether a stopped link sends `RETIRE` first: the client moved the
    /// session to another link.
    pub retire: AtomicBool,
    /// The bytes that the link carried, both ways, the records included:
    /// what the relay counts against its cap.
    pub bytes: AtomicU64,
    /// The count of bytes at which `due` is told and the hold starts, once;
    /// 0 is never.
    pub move_at: AtomicU64,
    /// Told when the link's bytes pass `move_at`.
    pub due: Notify,
    /// The hold of the link while the session moves to a new link: it
    /// carries no new bytes then, so that it stays under the relay's cap.
    pub hold: Hold,
    /// Set when this side's application ended its bytes on this link and
    /// its `CLOSE` is due, not at an `END`: the session is closing,
    /// and a move would only delay its `CLOSE`.
    pub app_ended: AtomicBool,
    /// Whether this side ends a retired link (the far end, a node's): after
    /// its last bytes it closes its side, and on a node's road the node's
    /// `close` then follows its data. The client never does: an end of its
    /// side is the operator leg's end, whose Close would cross the node's
    /// bytes still on their way, and the relay ends the node's whole socket
    /// for a frame of an id that it forgot.
    pub ends_links: bool,
    /// The most bytes that this link carries with no acknowledgement, from
    /// the settings of its road; `None` leaves it to the buffer.
    pub window: Option<usize>,
    /// Where a far end says a wait of the link that holds ([`super::STALL`]).
    pub stall: Option<Stall>,
    /// Where the client says a wait of the link that holds
    /// ([`Note::Stall`]).
    pub notes: Option<UnboundedSender<Note>>,
}

impl Watch {
    /// The far end's watch: it ends the links that it retires or that are
    /// retired under it.
    pub fn far_end() -> Watch {
        Watch { ends_links: true, ..Watch::default() }
    }

    /// This watch, for a link whose bytes in flight stay within `window`.
    pub fn within(mut self, window: Option<usize>) -> Watch {
        self.window = window;
        self
    }

    /// This watch, saying each wait that holds to `stall`.
    pub fn stalling(mut self, stall: Stall) -> Watch {
        self.stall = Some(stall);
        self
    }

    /// This watch, saying each wait that holds to `notes` as a note of the
    /// client.
    pub fn noting(mut self, notes: UnboundedSender<Note>) -> Watch {
        self.notes = Some(notes);
        self
    }

    /// A watch that tells `due` when the link passes `move_at` bytes.
    pub fn moving_at(move_at: u64) -> Watch {
        let watch = Watch::default();
        watch.move_at.store(move_at, Ordering::SeqCst);
        watch
    }

    /// Count `n` more bytes of the link, and tell `due` when they pass the
    /// mark. The hold starts in the same call: the task that opens the new
    /// link can wake late (a timer of 15.6 ms on Windows), and until then the
    /// old link would take the application's bytes past the mark
    /// (PodUtils/podssh#327). The task beside the link lifts the hold where
    /// no move follows (`crate::beside`).
    pub(crate) fn count(&self, n: u64) {
        let before = self.bytes.fetch_add(n, Ordering::Relaxed);
        let at = self.move_at.load(Ordering::SeqCst);
        if at > 0 && before < at && before + n >= at {
            self.hold.start();
            self.due.notify_one();
        }
    }

    /// No more moves on this link, and no hold: a move failed, or the
    /// session closes on the link. The mark goes first, so that no later
    /// byte starts the hold again with no task to lift it.
    pub(crate) fn moves_end(&self) {
        self.move_at.store(0, Ordering::SeqCst);
        self.hold.lift();
    }
}
