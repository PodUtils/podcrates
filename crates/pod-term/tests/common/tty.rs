//! **The harness of the termios discipline: each destination apart.**
//!
//! A line is one read of the child, so the lines stay apart here, and an
//! empty one (the end of file) is visible.

use pod_term::termios::{Event, Signal, Termios, Tty, When};

/// What one input produced, one field for each destination.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Got {
    /// Each `ToChild`, in order: one line each in the canonical mode.
    pub child: Vec<Vec<u8>>,
    /// The echo, joined.
    pub user: Vec<u8>,
    /// Each signal, with its flush.
    pub signals: Vec<(Signal, bool)>,
    /// Each `StopOutput` and `StartOutput`, in order.
    pub flow: Vec<Event>,
}

impl Got {
    /// Each leg, legible, for the message of a failed assertion.
    pub fn show(&self) -> String {
        let child: Vec<String> = self.child.iter().map(|c| pod_term::bytes::quoted(c)).collect();
        format!(
            "child={child:?} user={} signals={:?} flow={:?}",
            pod_term::bytes::quoted(&self.user),
            self.signals,
            self.flow
        )
    }

    /// The bytes for the child, joined.
    pub fn child_bytes(&self) -> Vec<u8> {
        self.child.concat()
    }
}

/// The events, gathered into their destinations.
pub fn got(events: Vec<Event>) -> Got {
    let mut got = Got::default();
    for event in events {
        match event {
            Event::ToChild(b) => got.child.push(b),
            Event::ToUser(b) => got.user.extend_from_slice(&b),
            Event::Signal { sig, flush } => got.signals.push((sig, flush)),
            flow @ (Event::StopOutput | Event::StartOutput) => got.flow.push(flow),
        }
    }
    got
}

/// A tty in the state of a new Linux pty.
pub fn sane() -> Tty {
    Tty::new(Termios::sane())
}

/// A tty in the state of a new Linux pty, changed by `f`.
pub fn with(f: impl FnOnce(&mut Termios)) -> Tty {
    let mut t = Termios::sane();
    f(&mut t);
    Tty::new(t)
}

/// Bytes from the user into a tty.
pub fn type_in(tty: &mut Tty, bytes: &[u8]) -> Got {
    got(tty.input(bytes))
}

/// A new state, by `f` on the current one, with `TCSETS`.
pub fn change(tty: &mut Tty, f: impl FnOnce(&mut Termios)) -> Got {
    let mut t = *tty.state();
    f(&mut t);
    got(tty.set(t, When::Now))
}
