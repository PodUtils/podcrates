//! The userspace line discipline, for a host with no pty.
//!
//! **This crate never touches the network.** It owns echo, line editing,
//! history, signal characters, and window size. The bytes arrive over SSH
//! from the SSH layer of the consumer and leave again; nothing here dials,
//! reads a socket, or knows a relay exists.
//!
//! ## Where the bytes come from
//!
//! The relay has no pty, tty, terminal, isatty or termios concept: its
//! documentation (`llms-full.txt`) names none of them. So the pty question
//! is never "what does the relay give us" but "does the SSH **server** grant
//! a pty, and can the client present one locally". When `/dev/ptmx` does not
//! open on the local host, the answer has to be yes: this crate implements
//! the terminal itself, in userspace, in Rust, with no `LD_PRELOAD` and no C
//! compiler.
//!
//! ## Three disciplines, one binary
//!
//! A [`session::Session`] chooses between the first two **from three
//! facts**, never by guessing ([`session::Facts`]): a pty below, a line
//! discipline below, and the caller's selection. The absence of a pty alone
//! never selects the cooked mode. The third is for a caller that gives its
//! child a tty in user space and answers the child's tty `ioctl`s: that
//! caller holds the termios state, and so it builds the third itself (call
//! C289):
//!
//! | Mode | When | Who owns the screen |
//! | --- | --- | --- |
//! | [`echo::Discipline`] | selected, with no pty and no discipline below | this crate: echo, editing, history, a static prompt |
//! | [`passthrough::Passthrough`] | each other case | the program, the pty or the discipline below: each byte as it came |
//! | [`termios::Tty`] | a tty in user space, whose child sets its own termios | the child, under the rules of Linux `n_tty` for the state that it set: no prompt, no history |
//!
//! **A full-screen program is a separate mode, not a richer cooked mode.** The
//! echo discipline **drops** cursor addressing, because a discipline that
//! interprets sequences it has not tested corrupts somebody's terminal. A `vi`
//! cannot run under those rules, so `vi` gets the other mode, where this
//! crate owns only window size and signals, and touches no other byte.
//!
//! ## What was transcribed, and what was not
//!
//! **The echo discipline is a transcription, not an invention.** The SSH
//! server of podvm uses this same discipline, and about 40 named tests pin
//! its byte rules. Each constant here cites the line of that server's
//! `session.rs` that it comes from, and each refusal is in the catalogue
//! below. A citation `session.rs:NN` names a line of that file, not of
//! [`session`].
//!
//! **One behaviour is deliberately NOT transcribed.** That server *refuses*
//! window size, because `sshd`'s `ForceCommand` consumes size changes before
//! any byte reaches it (the server of podvm). A client that is not behind
//! `ForceCommand` issues `window-change` itself. So [`window`] honours a
//! resize and **defers** one that arrives mid-frame, and does not drop it. A
//! copy of the refusal would give a window that cannot be resized.
//!
//! ## The refusal rule
//!
//! **A refusal rings the bell and changes nothing.** Silence would read as
//! acceptance, and a user who cannot tell "not supported" from "did nothing"
//! will file it as a bug in the wrong place.
//!
//! ## The 500-line rule
//!
//! The `session.rs` of that server is 1132 lines in one file, and this crate
//! does not copy it as one. Each module below holds one thing and names it:
//!
//! - [`bytes`] — rendering bytes legibly, so a failing assertion names them
//! - [`echo`] — the line under edit: the byte rules, transcribed
//! - [`escape`] — the escape-sequence parser: what counts as one key
//! - [`grammar`] — where a stream of output may start, and the ring of a
//!   kept shell's output that drops bytes only up to such a place
//! - [`window`] — window size, deferred while a frame is mid-draw
//! - [`passthrough`] — the second discipline, for a program that owns the screen
//! - [`termios`] — the third discipline, which follows the termios of its child
//! - [`refusal`] — the refusal catalogue that the cooked discipline answers from
//! - [`session`] — the driver that chooses between them

pub mod bytes;
pub mod echo;
pub mod escape;
pub mod grammar;
pub mod passthrough;
pub mod refusal;
pub mod session;
pub mod termios;
pub mod window;

pub use echo::{BELL, Discipline, EL, Event, HISTORY_CAP, IDLE, LINE_CAP, PROMPT, Sig};
pub use passthrough::Passthrough;
pub use refusal::{REFUSALS, Refusal};
pub use session::{Facts, Mode, Session};
pub use termios::{Termios, Tty, When};
pub use window::{Size, Window};
