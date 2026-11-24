//! The modes of a stream of output, against a parser of another project.
//!
//! anstyle-parse, vte's state machine, reads the bytes as a terminal does;
//! a small reader of its dispatches applies the rules of the modes. It
//! checks each mode of `pod_term::modes`, also the modes that the model
//! terminal of the test kit does not read (7, 1004, 1015, 1047 and G0).

use anstyle_parse::{DefaultCharAccumulator, Params, Parser, Perform};
use pod_term::modes::{Charset, Modes, Mouse, MouseEncoding};

/// The modes, as a plain value that both readers give.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plain {
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

const NEW: Plain = Plain {
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

fn plain(m: &Modes) -> Plain {
    Plain {
        cursor_keys: m.cursor_keys(),
        origin: m.origin(),
        autowrap: m.autowrap(),
        cursor_shown: m.cursor_shown(),
        alternate: m.alternate(),
        mouse: m.mouse(),
        encoding: m.mouse_encoding(),
        focus: m.focus(),
        paste: m.bracketed_paste(),
        keypad: m.keypad(),
        g0: m.g0(),
    }
}

impl Perform for Plain {
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: u8) {
        let set = match action {
            b'h' => true,
            b'l' => false,
            _ => return,
        };
        if ignore || intermediates != b"?" {
            return;
        }
        for param in params.iter() {
            let mode = param.first().copied().unwrap_or(0);
            match mode {
                1 => self.cursor_keys = set,
                6 => self.origin = set,
                7 => self.autowrap = set,
                25 => self.cursor_shown = set,
                47 | 1047 | 1049 => self.alternate = set,
                9 | 1000 | 1002 | 1003 => {
                    let level = [(9, Mouse::X10), (1000, Mouse::Press), (1002, Mouse::Button), (1003, Mouse::Any)]
                        .into_iter()
                        .find(|&(n, _)| n == mode)
                        .map(|(_, m)| m);
                    self.mouse = if set { level } else { None };
                }
                1005 | 1006 | 1015 => {
                    let e = [(1005, MouseEncoding::Utf8), (1006, MouseEncoding::Sgr), (1015, MouseEncoding::Urxvt)]
                        .into_iter()
                        .find(|&(n, _)| n == mode)
                        .map(|(_, e)| e);
                    if set {
                        self.encoding = e;
                    } else if self.encoding == e {
                        self.encoding = None;
                    }
                }
                1004 => self.focus = set,
                2004 => self.paste = set,
                _ => {}
            }
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if ignore {
            return;
        }
        match (intermediates, byte) {
            (b"", b'=') => self.keypad = true,
            (b"", b'>') => self.keypad = false,
            (b"", b'c') => *self = NEW,
            (b"(", b'0') => self.g0 = Charset::DecGraphics,
            (b"(", b'B') => self.g0 = Charset::Ascii,
            _ => {}
        }
    }
}

/// The modes that vte's parser gives for `bytes`.
fn vte(bytes: &[u8]) -> Plain {
    let mut parser = Parser::<DefaultCharAccumulator>::new();
    let mut modes = NEW;
    for &b in bytes {
        parser.advance(&mut modes, b);
    }
    modes
}

fn ours(bytes: &[u8], chunk: usize) -> Modes {
    let mut m = Modes::new();
    for piece in bytes.chunks(chunk.max(1)) {
        m.feed(piece);
    }
    m
}

/// A stream that holds each mode that the model terminal does not read, and
/// the sequences that hold no mode.
const SYNTHETIC: &[u8] = b"\x1b[?7l\x1b[?1004h\x1b[?1015h\x1b[?1047h\x1b(0text\x1b]0;?7h\x07\
\x1b[?1;2004h\x1b[?1003;1006h\x1b[?25l\x1b[?10\x1849h\x1b[>1h\x1b[?6h\x1b=\
\x1b[?1047l\x1b[?1006l\x1b[?1049h\x1b(B\x1b[?1002h";

#[test]
fn the_modes_of_each_split_of_a_stream_are_the_modes_that_vte_reads() {
    for end in 0..=SYNTHETIC.len() {
        let bytes = &SYNTHETIC[..end];
        for chunk in [1, 2, 3, 7, 61] {
            assert_eq!(plain(&ours(bytes, chunk)), vte(bytes), "{end} bytes in chunks of {chunk}");
        }
    }
    let whole = plain(&ours(SYNTHETIC, 61));
    assert_ne!(whole, NEW);
    assert!(!whole.autowrap && whole.focus && whole.alternate);
}

#[test]
fn restore_gives_vte_the_modes_from_any_modes() {
    let states = [b"".as_slice(), SYNTHETIC, b"\x1b[?1049h\x1b[?1000h\x1b[?1005h\x1b=", b"\x1bc"];
    for from in states {
        for to in states {
            let mut bytes = from.to_vec();
            bytes.extend_from_slice(&ours(to, 61).restore());
            assert_eq!(vte(&bytes), plain(&ours(to, 61)), "{from:?} then the restore of {to:?}");
        }
    }
}

#[test]
fn restore_of_a_new_terminal_leaves_a_model_terminal_as_new() {
    let new = pod_testkit::screen::Screen::new(24, 80);
    let restored = pod_testkit::screen::Screen::of(24, 80, &Modes::new().restore());
    assert_eq!((restored.rows(), restored.cursor(), restored.alternate()), (new.rows(), new.cursor(), new.alternate()));
}
