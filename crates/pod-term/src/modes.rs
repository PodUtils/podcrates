//! The modes of a terminal that a stream of output set, and the bytes that
//! put a new terminal in the same modes.
//!
//! A kept shell gives a client that attaches the last bytes of its output.
//! The modes that the program set before those bytes (the alternate screen,
//! the mouse reports, bracketed paste, a hidden cursor) are not in them. A
//! [`Modes`] reads the whole stream, and [`Modes::restore`] gives the bytes
//! that a client sends to its terminal before the replay. It reads and
//! writes bytes only.
//!
//! The modes:
//!
//! | Mode | Set | Reset | Default |
//! | --- | --- | --- | --- |
//! | cursor keys (DECCKM) | `CSI ? 1 h` | `CSI ? 1 l` | off |
//! | origin (DECOM) | `CSI ? 6 h` | `CSI ? 6 l` | off |
//! | autowrap (DECAWM) | `CSI ? 7 h` | `CSI ? 7 l` | on |
//! | cursor shown (DECTCEM) | `CSI ? 25 h` | `CSI ? 25 l` | on |
//! | the alternate screen | `CSI ? 47 h`, `? 1047 h`, `? 1049 h` | the same with `l` | off |
//! | the mouse level | `CSI ? 9 h`, `? 1000 h`, `? 1002 h`, `? 1003 h` | the same with `l` | none |
//! | the mouse encoding | `CSI ? 1005 h`, `? 1006 h`, `? 1015 h` | the same with `l` | none |
//! | focus reports | `CSI ? 1004 h` | `CSI ? 1004 l` | off |
//! | bracketed paste | `CSI ? 2004 h` | `CSI ? 2004 l` | off |
//! | the application keypad | `ESC =` | `ESC >` | off |
//! | the G0 set | `ESC ( 0` (DEC graphics) | `ESC ( B` (ASCII) | ASCII |
//!
//! `ESC c` (RIS) puts each mode at its default. Several modes in one CSI
//! (`CSI ? 1 ; 2004 h`) count each; a mode that is not in the table is
//! skipped, and the others of the same CSI count.
//!
//! The rules of xterm and tmux, read in their source (xterm `charproc.c`,
//! `dpmodes`; tmux `input.c`, `input_csi_dispatch_sm_private`):
//!
//! - The mouse level is one value: a set replaces it, and a reset of any
//!   level turns the reports off. The vt100 crate differs: its reset changes
//!   the level only when the reset names the current one.
//! - The mouse encoding is one value, as in xterm and vt100: a set replaces
//!   it, and a reset changes it only when the reset names the current one.
//!   tmux holds 1005 and 1006 as two flags and does not read 1015.
//! - DECSTR (`CSI ! p`, the soft reset that `tput reset` sends) is not read:
//!   xterm resets the cursor keys, the keypad, origin, the cursor and G0 with
//!   it, and tmux does not read it.
//!
//! The reader is the state machine of [`crate::grammar`]: a string (OSC,
//! DCS, SOS, PM, APC) holds no mode, so its text `?1049h` changes nothing;
//! `ESC` starts a new sequence in each state; CAN and SUB end each sequence.
//! A CSI longer than [`CSI_MAX`] bytes is dropped whole. The state lives in
//! the value, so a sequence cut across two chunks reads as one.

use crate::grammar::{self, State};

/// The longest CSI that the reader keeps, from the byte after `ESC [` to the
/// byte before the final byte. A longer one is no mode of the table.
pub const CSI_MAX: usize = 64;

/// The mouse level: which events the terminal reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mouse {
    /// `CSI ? 9 h`: a press (X10).
    X10,
    /// `CSI ? 1000 h`: a press and a release.
    Press,
    /// `CSI ? 1002 h`: also a motion with a button held.
    Button,
    /// `CSI ? 1003 h`: also each motion.
    Any,
}

/// The encoding of a mouse report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEncoding {
    /// `CSI ? 1005 h`: UTF-8.
    Utf8,
    /// `CSI ? 1006 h`: SGR.
    Sgr,
    /// `CSI ? 1015 h`: urxvt.
    Urxvt,
}

/// The set that G0 holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    /// `ESC ( B`.
    Ascii,
    /// `ESC ( 0`: the DEC line drawing set.
    DecGraphics,
}

/// The modes, with no state of the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Values {
    cursor_keys: bool,
    origin: bool,
    autowrap: bool,
    cursor_shown: bool,
    alternate: bool,
    mouse: Option<Mouse>,
    encoding: Option<MouseEncoding>,
    focus: bool,
    paste: bool,
    keypad: bool,
    g0: Charset,
}

/// The values of a new terminal and of a terminal after RIS.
const DEFAULTS: Values = Values {
    cursor_keys: false,
    origin: false,
    autowrap: true,
    cursor_shown: true,
    alternate: false,
    mouse: None,
    encoding: None,
    focus: false,
    paste: false,
    keypad: false,
    g0: Charset::Ascii,
};

/// The modes of a terminal that read a stream of output.
///
/// Two values are equal when their modes are equal; the state of the reader
/// (a sequence cut by the end of a chunk) does not count.
#[derive(Clone)]
pub struct Modes {
    values: Values,
    state: State,
    /// The bytes of the CSI or of the intermediate bytes after `ESC`.
    seq: [u8; CSI_MAX],
    len: usize,
    /// The CSI was longer than [`CSI_MAX`].
    long: bool,
}

impl Default for Modes {
    fn default() -> Self {
        Modes::new()
    }
}

impl std::fmt::Debug for Modes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The bytes of a cut sequence are not shown: a message of a failed
        // check names the modes.
        f.debug_struct("Modes").field("values", &self.values).field("state", &self.state).finish_non_exhaustive()
    }
}

impl PartialEq for Modes {
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}

impl Eq for Modes {}

impl Modes {
    /// The modes of a new terminal.
    pub fn new() -> Self {
        Modes { values: DEFAULTS, state: State::Ground, seq: [0; CSI_MAX], len: 0, long: false }
    }

    /// Reads a chunk of the output.
    pub fn feed(&mut self, chunk: &[u8]) {
        for &b in chunk {
            self.step(b);
        }
    }

    fn step(&mut self, b: u8) {
        let old = self.state;
        let new = grammar::next(old, b);
        self.state = new;
        match (old, new) {
            (_, State::Escape) => self.clear(),
            (State::Escape, State::Csi) => self.clear(),
            // A control byte or DEL inside a CSI is acted on or dropped; it
            // is no part of the CSI.
            (State::Csi, State::Csi) if (0x20..0x7f).contains(&b) => self.keep(b),
            (State::Csi, State::Ground) if (0x40..=0x7e).contains(&b) => self.csi(b),
            (State::Escape | State::EscapeIntermediate, State::EscapeIntermediate) => self.keep(b),
            (State::Escape | State::EscapeIntermediate, State::Ground) if (0x30..=0x7e).contains(&b) => self.esc(b),
            _ => {}
        }
    }

    fn clear(&mut self) {
        self.len = 0;
        self.long = false;
    }

    fn keep(&mut self, b: u8) {
        match self.seq.get_mut(self.len) {
            Some(slot) => {
                *slot = b;
                self.len += 1;
            }
            None => self.long = true,
        }
    }

    /// The end of an escape sequence that is not a CSI.
    fn esc(&mut self, last: u8) {
        let v = &mut self.values;
        match (&self.seq[..self.len], last) {
            (b"", b'=') => v.keypad = true,
            (b"", b'>') => v.keypad = false,
            (b"", b'c') => *v = DEFAULTS,
            (b"(", b'0') => v.g0 = Charset::DecGraphics,
            (b"(", b'B') => v.g0 = Charset::Ascii,
            _ => {}
        }
    }

    /// The end of a CSI: a DEC private set or reset, or no mode.
    fn csi(&mut self, last: u8) {
        let set = match last {
            b'h' => true,
            b'l' => false,
            _ => return,
        };
        let Some(params) = self.seq[..self.len].strip_prefix(b"?") else { return };
        if self.long || !params.iter().all(|&b| b.is_ascii_digit() || b == b';') {
            return;
        }
        for param in params.split(|&b| b == b';') {
            if param.is_empty() {
                continue;
            }
            let n = param.iter().fold(0u32, |n, &d| n.saturating_mul(10).saturating_add(u32::from(d - b'0')));
            if let Ok(n) = u16::try_from(n) {
                apply(&mut self.values, n, set);
            }
        }
    }

    /// The bytes that put a terminal in any state into these modes.
    ///
    /// The order is fixed, so equal modes give equal bytes: the alternate
    /// screen, the reset of each mouse level and encoding, the mouse level
    /// and encoding, the keypad, G0, then the modes 1, 6, 7, 25, 1004 and
    /// 2004, each as a set or a reset. The alternate screen is set with
    /// `CSI ? 1049 h` and reset with `CSI ? 47 l`, which restores no cursor.
    ///
    /// `CSI ? 6 h` and `CSI ? 6 l` move the cursor home in a terminal, so the
    /// caller redraws after these bytes. The cursor style, the scroll
    /// region, the tab stops, the attributes and G1 (`SO`, `SI`) are not
    /// modes of this type. Each byte is 7-bit.
    pub fn restore(&self) -> Vec<u8> {
        let v = &self.values;
        let mut out = Vec::with_capacity(128);
        private(&mut out, if v.alternate { 1049 } else { 47 }, v.alternate);
        for mode in [9, 1000, 1002, 1003, 1005, 1006, 1015] {
            private(&mut out, mode, false);
        }
        if let Some(m) = v.mouse {
            private(&mut out, mouse_mode(m), true);
        }
        if let Some(e) = v.encoding {
            private(&mut out, encoding_mode(e), true);
        }
        out.extend_from_slice(if v.keypad { b"\x1b=" } else { b"\x1b>" });
        out.extend_from_slice(match v.g0 {
            Charset::DecGraphics => b"\x1b(0",
            Charset::Ascii => b"\x1b(B",
        });
        for (mode, set) in
            [(1, v.cursor_keys), (6, v.origin), (7, v.autowrap), (25, v.cursor_shown), (1004, v.focus), (2004, v.paste)]
        {
            private(&mut out, mode, set);
        }
        out
    }

    /// The cursor keys send their application form (DECCKM).
    pub fn cursor_keys(&self) -> bool {
        self.values.cursor_keys
    }

    /// Rows count from the scroll region (DECOM).
    pub fn origin(&self) -> bool {
        self.values.origin
    }

    /// Text wraps at the last column (DECAWM).
    pub fn autowrap(&self) -> bool {
        self.values.autowrap
    }

    /// The cursor shows (DECTCEM).
    pub fn cursor_shown(&self) -> bool {
        self.values.cursor_shown
    }

    /// The alternate screen is in use.
    pub fn alternate(&self) -> bool {
        self.values.alternate
    }

    /// The mouse level, `None` when the terminal reports no mouse event.
    pub fn mouse(&self) -> Option<Mouse> {
        self.values.mouse
    }

    /// The encoding of a mouse report, `None` for the default.
    pub fn mouse_encoding(&self) -> Option<MouseEncoding> {
        self.values.encoding
    }

    /// The terminal reports focus in and out.
    pub fn focus(&self) -> bool {
        self.values.focus
    }

    /// A paste comes between brackets.
    pub fn bracketed_paste(&self) -> bool {
        self.values.paste
    }

    /// The keypad sends its application form (DECKPAM).
    pub fn keypad(&self) -> bool {
        self.values.keypad
    }

    /// The set of G0.
    pub fn g0(&self) -> Charset {
        self.values.g0
    }
}

/// Applies a DEC private set or reset to the values.
fn apply(v: &mut Values, mode: u16, set: bool) {
    match mode {
        1 => v.cursor_keys = set,
        6 => v.origin = set,
        7 => v.autowrap = set,
        25 => v.cursor_shown = set,
        47 | 1047 | 1049 => v.alternate = set,
        9 | 1000 | 1002 | 1003 => v.mouse = if set { mouse(mode) } else { None },
        1004 => v.focus = set,
        1005 | 1006 | 1015 => {
            let e = encoding(mode);
            if set {
                v.encoding = e;
            } else if v.encoding == e {
                v.encoding = None;
            }
        }
        2004 => v.paste = set,
        _ => {}
    }
}

fn mouse(mode: u16) -> Option<Mouse> {
    match mode {
        9 => Some(Mouse::X10),
        1000 => Some(Mouse::Press),
        1002 => Some(Mouse::Button),
        1003 => Some(Mouse::Any),
        _ => None,
    }
}

fn mouse_mode(m: Mouse) -> u16 {
    match m {
        Mouse::X10 => 9,
        Mouse::Press => 1000,
        Mouse::Button => 1002,
        Mouse::Any => 1003,
    }
}

fn encoding(mode: u16) -> Option<MouseEncoding> {
    match mode {
        1005 => Some(MouseEncoding::Utf8),
        1006 => Some(MouseEncoding::Sgr),
        1015 => Some(MouseEncoding::Urxvt),
        _ => None,
    }
}

fn encoding_mode(e: MouseEncoding) -> u16 {
    match e {
        MouseEncoding::Utf8 => 1005,
        MouseEncoding::Sgr => 1006,
        MouseEncoding::Urxvt => 1015,
    }
}

/// `CSI ? mode h` or `CSI ? mode l`.
fn private(out: &mut Vec<u8>, mode: u16, set: bool) {
    out.extend_from_slice(b"\x1b[?");
    out.extend_from_slice(mode.to_string().as_bytes());
    out.push(if set { b'h' } else { b'l' });
}

#[cfg(test)]
mod tests;
