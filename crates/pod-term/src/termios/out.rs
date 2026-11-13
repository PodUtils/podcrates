//! The output column and `OPOST`: `do_output_char` of Linux `n_tty`.
//!
//! **The echo and the child's output go through the same processing.** In
//! `n_tty` an echoed `\n` becomes `\r\n` by `ONLCR` as a written one does,
//! and both move the one column that the erase of a tab reads. So one
//! [`Out`] holds the column for the two paths.

use super::Termios;
use super::flags::{IUTF8, OCRNL, ONLCR, ONLRET, ONOCR, OPOST};

/// Whether a byte is a control character: `0x00` to `0x1f`, and DEL.
///
/// **ASCII only.** The `ctype` of the kernel also marks `0x80` to `0x9f`;
/// those are UTF-8 continuation bytes, and a `^X` of one breaks the
/// character on the screen (call C288).
pub fn is_cntrl(c: u8) -> bool {
    c < 0x20 || c == 0x7f
}

/// Whether a byte continues a UTF-8 character, under `IUTF8` only.
pub fn is_continuation(state: &Termios, c: u8) -> bool {
    state.iflag & IUTF8 != 0 && c & 0xc0 == 0x80
}

/// The column of the cursor, as the discipline counts it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Out {
    /// The column now. Only `OPOST` output and the echo of a `^X` move it,
    /// as in `n_tty`.
    pub column: usize,
    /// The column where the line under edit starts: the erase of a tab
    /// counts from it.
    pub canon_column: usize,
}

impl Out {
    /// One byte out, after `OPOST`, `ONLCR`, `OCRNL`, `ONOCR` and `ONLRET`.
    pub fn put(&mut self, state: &Termios, c: u8, buf: &mut Vec<u8>) {
        let o = state.oflag;
        if o & OPOST == 0 {
            buf.push(c);
            return;
        }
        match c {
            b'\n' => {
                if o & ONLRET != 0 {
                    self.column = 0;
                }
                if o & ONLCR != 0 {
                    self.column = 0;
                    self.canon_column = 0;
                    buf.extend_from_slice(b"\r\n");
                    return;
                }
                self.canon_column = self.column;
            }
            b'\r' => {
                if o & ONOCR != 0 && self.column == 0 {
                    return;
                }
                if o & OCRNL != 0 {
                    if o & ONLRET != 0 {
                        self.column = 0;
                        self.canon_column = 0;
                    }
                    buf.push(b'\n');
                    return;
                }
                self.column = 0;
                self.canon_column = 0;
            }
            b'\t' => self.column += 8 - (self.column & 7),
            0x08 => self.column = self.column.saturating_sub(1),
            _ if !is_cntrl(c) && !is_continuation(state, c) => self.column += 1,
            _ => {}
        }
        buf.push(c);
    }

    /// A control character as `^X`, and DEL as `^?`. Not processed by
    /// `OPOST`: `n_tty` writes the pair as it is, and counts two columns.
    pub fn ctl(&mut self, c: u8, buf: &mut Vec<u8>) {
        buf.push(b'^');
        buf.push(c ^ 0x40);
        self.column += 2;
    }

    /// `n` backspaces, at most to column 0: the erase of a tab. Not
    /// processed by `OPOST`, as in `n_tty`.
    pub fn back(&mut self, n: usize, buf: &mut Vec<u8>) {
        for _ in 0..n.min(self.column) {
            buf.push(0x08);
            self.column -= 1;
        }
    }
}
