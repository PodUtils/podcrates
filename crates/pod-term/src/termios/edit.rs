//! The canonical editing: erase, word erase, kill, literal next and
//! reprint, with their echo. `eraser` and `n_tty_receive_char_special` of
//! Linux `n_tty`.

use super::flags::*;
use super::out::{is_cntrl, is_continuation};
use super::{Sink, Tty};

/// What an erase character removes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kill {
    /// One character: `VERASE`.
    Char,
    /// One word: `VWERASE`.
    Word,
    /// The line: `VKILL`.
    Line,
}

/// A byte of a word for `VWERASE`: a letter, a digit or `_`, as the
/// `ALTWERASE` rule of `n_tty` says. A byte from `0x80` is also one, so that
/// a UTF-8 word is one word (call C288).
fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

impl Tty {
    /// A byte that the canonical mode reads as a control. Returns false for a
    /// byte that is data.
    ///
    /// **The order is the order of `n_tty`**: erase, kill and word erase;
    /// then literal next, reprint, `\n`, `VEOF`, `VEOL` and `VEOL2`. A `c_cc`
    /// of 0 matches no byte, so a NUL is data while `VEOL` is not set.
    pub(super) fn canonical(&mut self, c: u8, sink: &mut Sink) -> bool {
        let s = self.state;
        let ext = s.lflag & IEXTEN != 0;
        let echo = s.lflag & ECHO != 0;
        if s.matches(c, VERASE) || s.matches(c, VKILL) || (ext && s.matches(c, VWERASE)) {
            self.erase(c, sink);
        } else if ext && s.matches(c, VLNEXT) {
            self.lnext = true;
            // `^` and a backspace: the next echo writes over the caret.
            if echo && s.lflag & ECHOCTL != 0 {
                self.echo_raw(b'^', sink);
                self.echo_raw(0x08, sink);
            }
        } else if ext && echo && s.matches(c, VREPRINT) {
            self.echo_char(c, sink);
            self.echo_raw(b'\n', sink);
            for b in self.line.clone() {
                self.echo_char(b, sink);
            }
        } else if c == b'\n' {
            if echo || s.lflag & ECHONL != 0 {
                self.echo_raw(b'\n', sink);
            }
            self.end_line(Some(c), sink);
        } else if s.matches(c, VEOF) {
            self.end_line(None, sink);
        } else if s.matches(c, VEOL) || (ext && s.matches(c, VEOL2)) {
            if echo {
                self.mark_canon_column();
                self.echo_char(c, sink);
            }
            self.end_line(Some(c), sink);
        } else {
            return false;
        }
        true
    }

    /// The line goes to the child, with its terminator. **`VEOF` adds no
    /// byte**: the read of the child returns the line as it is, and an empty
    /// line is a read of 0 bytes, the end of file.
    fn end_line(&mut self, terminator: Option<u8>, sink: &mut Sink) {
        let mut line = std::mem::take(&mut self.line);
        line.extend(terminator);
        sink.line(line);
    }

    /// `VERASE`, `VWERASE` or `VKILL`. **An empty line echoes nothing**, as
    /// in `n_tty`: there is no bell for an erase at the start of a line.
    fn erase(&mut self, c: u8, sink: &mut Sink) {
        let s = self.state;
        if self.line.is_empty() {
            return;
        }
        let echo = s.lflag & ECHO != 0;
        let kind = if s.matches(c, VERASE) {
            Kill::Char
        } else if s.matches(c, VWERASE) {
            Kill::Word
        } else {
            if !echo {
                self.line.clear();
                return;
            }
            // Without the three flags, the kill echoes `^U` and a newline,
            // and erases nothing on the screen.
            if s.lflag & (ECHOK | ECHOKE | ECHOE) != ECHOK | ECHOKE | ECHOE {
                self.line.clear();
                self.echo_char(c, sink);
                if s.lflag & ECHOK != 0 {
                    self.echo_raw(b'\n', sink);
                }
                return;
            }
            Kill::Line
        };
        let mut seen_word = false;
        while !self.line.is_empty() {
            // One character, with each of its UTF-8 bytes under `IUTF8`.
            let mut head = self.line.len() - 1;
            while is_continuation(&s, self.line[head]) && head > 0 {
                head -= 1;
            }
            let ch = self.line[head];
            if is_continuation(&s, ch) {
                break;
            }
            if kind == Kill::Word {
                if is_word(ch) {
                    seen_word = true;
                } else if seen_word {
                    break;
                }
            }
            self.line.truncate(head);
            if echo {
                self.rub_out(ch, kind, sink);
            }
            if kind == Kill::Char {
                break;
            }
        }
    }

    /// The echo of one erased character.
    fn rub_out(&mut self, ch: u8, kind: Kill, sink: &mut Sink) {
        let s = self.state;
        let echoctl = s.lflag & ECHOCTL != 0;
        if kind == Kill::Char && s.lflag & ECHOE == 0 {
            self.echo_char(s.cc[VERASE], sink);
        } else if ch == b'\t' {
            self.rub_out_tab(sink);
        } else {
            // A `^X` takes two cells, and a control with no `ECHOCTL` none.
            let cells = match (is_cntrl(ch), echoctl) {
                (true, true) => 2,
                (true, false) => 0,
                (false, _) => 1,
            };
            for _ in 0..cells {
                for b in [0x08, b' ', 0x08] {
                    self.echo_raw(b, sink);
                }
            }
        }
    }

    /// The erase of a tab: back to the tab stop that the line before it
    /// reached. The cells are counted from the last tab of the line, or from
    /// the column where the line started (`echo_erase_tab` of `n_tty`).
    fn rub_out_tab(&mut self, sink: &mut Sink) {
        let s = self.state;
        let mut cells = 0usize;
        let mut after_tab = false;
        for &b in self.line.iter().rev() {
            if b == b'\t' {
                after_tab = true;
                break;
            } else if is_cntrl(b) {
                if s.lflag & ECHOCTL != 0 {
                    cells += 2;
                }
            } else if !is_continuation(&s, b) {
                cells += 1;
            }
        }
        if !after_tab {
            cells += self.out.canon_column;
        }
        self.out.back(8 - (cells & 7), sink.user());
    }

    /// The echo of one byte: `^X` for a control under `ECHOCTL`, except a
    /// tab; each other byte through `OPOST`.
    pub(super) fn echo_char(&mut self, c: u8, sink: &mut Sink) {
        if self.state.lflag & ECHOCTL != 0 && is_cntrl(c) && c != b'\t' {
            self.out.ctl(c, sink.user());
        } else {
            self.echo_raw(c, sink);
        }
    }

    /// The echo of one byte through `OPOST`, with no `^X`.
    pub(super) fn echo_raw(&mut self, c: u8, sink: &mut Sink) {
        let state = self.state;
        self.out.put(&state, c, sink.user());
    }

    /// The first echo of a line marks the column where the line starts.
    pub(super) fn mark_canon_column(&mut self) {
        if self.line.is_empty() {
            self.out.canon_column = self.out.column;
        }
    }
}
