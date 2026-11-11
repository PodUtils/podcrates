//! What a node keeps across its sockets and its pairs: whether its next stop
//! is for the binary that takes its place, how many of its sockets heard a
//! role that rotates stop tokens, what its line of the spares said last, and
//! where its lines go. A process-wide value in podssh; here a value that the
//! caller gives each socket (`Settings::node`) and reads back.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use pod_core::{Event, Events, NoEvents};

/// The state of one node.
pub struct NodeState {
    upgrading: AtomicBool,
    rotates: AtomicU64,
    spares_said: AtomicU8,
    /// Where the node's lines go: its spares, a session's close, the budget
    /// of its layer.
    pub events: Arc<dyn Events>,
}

impl Default for NodeState {
    fn default() -> NodeState {
        NodeState::new(Arc::new(NoEvents))
    }
}

impl std::fmt::Debug for NodeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeState")
            .field("upgrading", &self.upgrading.load(Ordering::SeqCst))
            .field("rotates", &self.rotates.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl NodeState {
    /// A node whose lines go to `events`.
    pub fn new(events: Arc<dyn Events>) -> NodeState {
        NodeState {
            upgrading: AtomicBool::new(false),
            rotates: AtomicU64::new(0),
            spares_said: AtomicU8::new(0),
            events,
        }
    }

    /// Send a line of the node.
    pub fn tell(&self, event: Event) {
        self.events.event(event);
    }

    /// This node's next stop is for the binary that takes its place, or no
    /// longer is: its `close` of each session says which.
    pub fn set_upgrading(&self, on: bool) {
        self.upgrading.store(on, Ordering::SeqCst);
    }

    /// Whether the next stop is for the binary that takes its place.
    pub fn upgrading(&self) -> bool {
        self.upgrading.load(Ordering::SeqCst)
    }

    /// A socket heard a role that rotates stop tokens.
    pub(crate) fn heard_rotation(&self) {
        self.rotates.fetch_add(1, Ordering::SeqCst);
    }

    /// How many sockets of this node heard a role that rotates the pair's
    /// stop token.
    pub fn offers(&self) -> u64 {
        self.rotates.load(Ordering::SeqCst)
    }

    /// What the line of the spares said last, and now `now`: the last.
    pub(crate) fn spares_said(&self, now: u8) -> u8 {
        self.spares_said.swap(now, Ordering::SeqCst)
    }
}
