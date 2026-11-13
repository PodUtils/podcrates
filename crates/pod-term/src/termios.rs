//! The third discipline: a tty that follows the termios state of its child.
//!
//! **For a child that has a tty in user space and sets its own modes.** A
//! server with no `/dev/ptmx` can still give its child a tty: a filter sends
//! the child's tty `ioctl`s to the server, which answers them from one
//! termios state. This discipline reads that same state, so the modes that
//! the child sets with `TCSETS` are the modes that it gets. The child draws
//! its own prompt, and a shell with readline keeps its own history; this
//! discipline has neither.
//!
//! ## The rules: Linux `n_tty`
//!
//! Each rule below is the rule of `drivers/tty/n_tty.c`, in its order:
//!
//! | Step | Rule |
//! | --- | --- |
//! | 1 | after `VLNEXT`, the byte is data (after `ISTRIP`), with its echo |
//! | 2 | `ISTRIP` clears the eighth bit |
//! | 3 | `IXON`: `VSTOP` and `VSTART` stop and start the output, and are not data; with `IXANY`, each other byte starts it |
//! | 4 | `ISIG`: `VINTR`, `VQUIT`, `VSUSP` give SIGINT, SIGQUIT, SIGTSTP; the line is flushed unless `NOFLSH`; the byte is echoed with `ECHO` |
//! | 5 | `IGNCR` drops `\r`, else `ICRNL` makes it `\n`; else `INLCR` makes `\n` a `\r` |
//! | 6 | `ICANON`: `VERASE`, `VKILL`, `VWERASE`, `VLNEXT`, `VREPRINT`, then `\n`, `VEOF`, `VEOL`, `VEOL2` end the line |
//! | 7 | each other byte is data: echoed with `ECHO`, and kept in the line, or sent at once with no `ICANON` |
//!
//! The extended characters (`VWERASE`, `VLNEXT`, `VREPRINT`, `VEOL2`) need
//! `IEXTEN`, and `VREPRINT` also `ECHO`. A `c_cc` of 0 matches no byte.
//!
//! ## Where the caller acts, not this discipline
//!
//! - **The child's reads.** In the canonical mode each [`Event::ToChild`] is
//!   one line, and the child reads at most one line in one call; an empty
//!   one is `VEOF` on an empty line, a read of 0 bytes. With no `ICANON`,
//!   each byte goes at once, and `VMIN` and `VTIME` ([`Termios::vmin`],
//!   [`Termios::vtime`]) are the caller's rule for a read.
//! - **The flush.** [`Event::Signal`] with `flush` and [`When::Flush`] drop
//!   the line here. The bytes that the child did not read yet, and the
//!   output not yet written to the user, are the caller's: it drops them
//!   too, as `n_tty` flushes its read buffer and the output of the driver.
//! - **The drain.** `TCSETSW` sets the state after the output drains. This
//!   discipline holds no output, so the caller calls [`Tty::set`] with
//!   [`When::Drain`] when the output that it holds is written.
//! - **The signals.** An [`Event::Signal`] names the signal; the caller
//!   sends it to the foreground process group.
//! - **The stopped output.** From [`Event::StopOutput`] to
//!   [`Event::StartOutput`] the caller holds each byte for the user (the
//!   echo and the child's output), and reads no more output of the child,
//!   as a write of the child waits on a stopped Linux tty.
//!
//! ## What is kept as data, not applied
//!
//! `IGNBRK`, `BRKINT`, `IGNPAR`, `PARMRK`, `INPCK` (a channel has no break
//! and no parity), `IXOFF`, `IMAXBEL`, `IUCLC`, `OLCUC`, `XCASE`,
//! `ECHOPRT`, `TOSTOP`, `PENDIN`, `FLUSHO`, `EXTPROC`, the delays, each
//! `c_cflag`, `VDISCARD` and `VSWTC`: the discipline keeps them in the
//! state, so that `TCGETS` answers them, and does not apply them (call
//! C287).
//!
//! ## The module split
//!
//! | file | holds |
//! | --- | --- |
//! | [`termios.rs`](self) | the events, the input dispatch |
//! | `state` | [`Termios`] and [`When`] |
//! | [`flags`] | the Linux values of the flags and of the indexes of `c_cc` |
//! | `edit` | the canonical editing and the echo |
//! | `out` | `OPOST` and the column |

use crate::window::Size;
use flags::*;
use out::Out;
pub use state::{Termios, When};

mod edit;
pub mod flags;
mod out;
mod state;

/// The longest line of the canonical mode, with no terminator. After it, a
/// byte of data is echoed and dropped, with no bell, as on a Linux pty; a
/// terminator, an erase and a signal still act (`N_TTY_BUF_SIZE` of Linux,
/// less one).
pub const LINE_MAX: usize = 4095;

/// A signal for the foreground process group of the child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// `VINTR`.
    Int,
    /// `VQUIT`.
    Quit,
    /// `VSUSP`.
    Tstp,
    /// A new window size.
    Winch,
}

impl Signal {
    /// The Linux number of the signal.
    pub fn number(self) -> i32 {
        match self {
            Signal::Int => 2,
            Signal::Quit => 3,
            Signal::Tstp => 20,
            Signal::Winch => 28,
        }
    }
}

/// One consequence of input, of a new state or of a new size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Bytes for the child. In the canonical mode, one line, which can be
    /// empty (`VEOF` on an empty line: a read of 0 bytes).
    ToChild(Vec<u8>),
    /// Bytes for the user: the echo, after `OPOST`.
    ToUser(Vec<u8>),
    /// A signal for the foreground process group. With `flush`, the caller
    /// drops the input that the child did not read and the output that it
    /// holds and did not write, before the events that follow.
    Signal {
        /// The signal.
        sig: Signal,
        /// The input was flushed (no `NOFLSH`).
        flush: bool,
    },
    /// `VSTOP` under `IXON`: the caller holds the bytes for the user.
    StopOutput,
    /// `VSTART`, a signal, or any byte under `IXANY`: the caller writes the
    /// bytes that it held.
    StartOutput,
}

/// The events of one call, with adjacent bytes of the same kind joined.
#[derive(Default)]
pub(crate) struct Sink {
    events: Vec<Event>,
}

impl Sink {
    /// The buffer of echo bytes at the end of the list.
    fn user(&mut self) -> &mut Vec<u8> {
        if !matches!(self.events.last(), Some(Event::ToUser(_))) {
            self.events.push(Event::ToUser(Vec::new()));
        }
        match self.events.last_mut() {
            Some(Event::ToUser(v)) => v,
            _ => unreachable!("a ToUser was pushed above"),
        }
    }

    /// One byte for the child, with no `ICANON`: joined to the bytes before.
    fn byte(&mut self, c: u8) {
        match self.events.last_mut() {
            Some(Event::ToChild(v)) => v.push(c),
            _ => self.events.push(Event::ToChild(vec![c])),
        }
    }

    /// One line for the child: never joined, since each is one read.
    fn line(&mut self, line: Vec<u8>) {
        self.events.push(Event::ToChild(line));
    }

    /// The events, with no empty echo.
    fn done(mut self) -> Vec<Event> {
        self.events.retain(|e| !matches!(e, Event::ToUser(v) if v.is_empty()));
        self.events
    }
}

/// The discipline: a termios state, the line under edit, and the column.
pub struct Tty {
    state: Termios,
    size: Size,
    line: Vec<u8>,
    lnext: bool,
    stopped: bool,
    out: Out,
}

impl Tty {
    /// A tty with this state, and no size yet (0 by 0).
    pub fn new(state: Termios) -> Tty {
        Tty { state, size: Size::new(0, 0), line: Vec::new(), lnext: false, stopped: false, out: Out::default() }
    }

    /// The tty with its first size, as `pty-req` carried it. No event: no
    /// child runs yet.
    pub fn sized(mut self, size: Size) -> Tty {
        self.size = size;
        self
    }

    /// The state, for the answer to `TCGETS`.
    pub fn state(&self) -> &Termios {
        &self.state
    }

    /// The window size, for the answer to `TIOCGWINSZ`.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Whether the output is stopped by `VSTOP`.
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// The count of bytes in the line under edit. This is not the answer to
    /// `FIONREAD`: in the canonical mode Linux counts no partial line.
    pub fn pending_len(&self) -> usize {
        self.line.len()
    }

    /// Bytes from the user in, their consequences out, in order.
    pub fn input(&mut self, bytes: &[u8]) -> Vec<Event> {
        let mut sink = Sink::default();
        for &b in bytes {
            self.byte(b, &mut sink);
        }
        sink.done()
    }

    /// Bytes from the child in, bytes for the user out, after `OPOST`.
    pub fn output(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(bytes.len());
        for &c in bytes {
            self.out.put(&self.state, c, &mut buf);
        }
        buf
    }

    /// A new state: `TCSETS`, `TCSETSW` or `TCSETSF`.
    ///
    /// **A switch of `ICANON` acts as in `n_tty`.** From canonical to not, the
    /// line under edit goes to the child as data, since a read of the child
    /// now takes each byte; [`When::Flush`] drops it first. From not to
    /// canonical, the bytes that the child did not read stay the caller's,
    /// readable as they are. A pending `VLNEXT` ends at each switch. A
    /// state with no `IXON` starts a stopped output.
    pub fn set(&mut self, state: Termios, when: When) -> Vec<Event> {
        let mut sink = Sink::default();
        if when == When::Flush {
            self.flush();
        }
        if state.iflag & IXON == 0 {
            self.start(&mut sink);
        }
        let switched = (self.state.lflag ^ state.lflag) & ICANON != 0;
        self.state = state;
        if switched {
            self.lnext = false;
            if !state.canonical() && !self.line.is_empty() {
                let line = std::mem::take(&mut self.line);
                sink.line(line);
            }
        }
        sink.done()
    }

    /// Drop the pending input: `TCFLSH` with `TCIFLUSH`, and the first step
    /// of [`When::Flush`].
    pub fn flush(&mut self) {
        self.line.clear();
        self.lnext = false;
    }

    /// A new window size: `window-change`, or `TIOCSWINSZ` of the child.
    /// **SIGWINCH only when the size changed**, as `tty_do_resize` of Linux.
    pub fn resize(&mut self, size: Size) -> Vec<Event> {
        if size == self.size {
            return vec![];
        }
        self.size = size;
        vec![Event::Signal { sig: Signal::Winch, flush: false }]
    }

    /// One byte from the user, in the order of `n_tty`.
    fn byte(&mut self, b: u8, sink: &mut Sink) {
        let s = self.state;
        let strip = |c: u8| if s.iflag & ISTRIP != 0 { c & 0x7f } else { c };
        let any = s.iflag & (IXON | IXANY) == IXON | IXANY;
        if self.lnext {
            self.lnext = false;
            if any {
                self.start(sink);
            }
            self.data(strip(b), false, sink);
            return;
        }
        let mut c = strip(b);
        if s.iflag & IXON != 0 {
            if s.matches(c, VSTOP) {
                if !self.stopped {
                    self.stopped = true;
                    sink.events.push(Event::StopOutput);
                }
                return;
            }
            if s.matches(c, VSTART) {
                self.start(sink);
                return;
            }
        }
        if s.lflag & ISIG != 0 {
            let sig = [(VINTR, Signal::Int), (VQUIT, Signal::Quit), (VSUSP, Signal::Tstp)]
                .into_iter()
                .find(|&(i, _)| s.matches(c, i));
            if let Some((_, sig)) = sig {
                self.signal(sig, c, sink);
                return;
            }
        }
        if any {
            self.start(sink);
        }
        if c == b'\r' {
            if s.iflag & IGNCR != 0 {
                return;
            }
            if s.iflag & ICRNL != 0 {
                c = b'\n';
            }
        } else if c == b'\n' && s.iflag & INLCR != 0 {
            c = b'\r';
        }
        if s.canonical() && self.canonical(c, sink) {
            return;
        }
        self.data(c, true, sink);
    }

    /// The output starts again, if it was stopped.
    fn start(&mut self, sink: &mut Sink) {
        if self.stopped {
            self.stopped = false;
            sink.events.push(Event::StartOutput);
        }
    }

    /// A signal character: flush unless `NOFLSH`, then echo with `ECHO`.
    /// Under `IXON` it also starts the output, so that its echo shows.
    fn signal(&mut self, sig: Signal, c: u8, sink: &mut Sink) {
        let flush = self.state.lflag & NOFLSH == 0;
        if flush {
            self.flush();
        }
        sink.events.push(Event::Signal { sig, flush });
        if self.state.iflag & IXON != 0 {
            self.start(sink);
        }
        if self.state.lflag & ECHO != 0 {
            self.echo_char(c, sink);
        }
    }

    /// A byte of data. **With no `ICANON`, a `\n` from the user echoes as a
    /// newline, not as `^J`**; a literal one after `VLNEXT` (`plain` false)
    /// echoes as `^J`, as in `n_tty`.
    fn data(&mut self, c: u8, plain: bool, sink: &mut Sink) {
        let s = self.state;
        if s.lflag & ECHO != 0 {
            if plain && c == b'\n' {
                self.echo_raw(c, sink);
            } else {
                self.mark_canon_column();
                self.echo_char(c, sink);
            }
        }
        if s.canonical() {
            if self.line.len() < LINE_MAX {
                self.line.push(c);
            }
        } else {
            sink.byte(c);
        }
    }
}

impl std::fmt::Debug for Tty {
    /// The state and the size, and not the line: a line under edit can be a
    /// password typed at a prompt with echo on.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tty")
            .field("state", &self.state)
            .field("size", &self.size)
            .field("pending_len", &self.line.len())
            .finish_non_exhaustive()
    }
}
