//! The modes of a stream of output, against a parser of another project.
//!
//! anstyle-parse, vte's state machine, reads the bytes as a terminal does;
//! a small reader of its dispatches applies the rules of the modes. It
//! checks each mode of `pod_term::modes`, also the modes that the model
//! terminal of the test kit does not read (7, 1004, 1015, 1047 and G0).
//!
//! `Ring::modes_at` against the model terminal and vte, on the captures of
//! vim, less and top (`fixtures/`): a terminal that reads `restore` of the
//! modes at a cut and then the bytes from the cut has the modes of a
//! terminal that read the whole stream.

use std::time::Instant;

use anstyle_parse::{DefaultCharAccumulator, Params, Parser, Perform};
use pod_term::grammar::Ring;
use pod_term::modes::{Charset, Modes, Mouse, MouseEncoding};
use pod_testkit::screen::Screen;

mod captures;

use captures::{CHUNK, CUTS, boundaries, fixtures};

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
    let new = Screen::new(24, 80);
    let restored = Screen::of(24, 80, &Modes::new().restore());
    assert_eq!((restored.rows(), restored.cursor(), restored.alternate()), (new.rows(), new.cursor(), new.alternate()));
}

/// A stream of the modes that the model terminal reads, under the rules
/// that it shares with xterm: no reset of another mouse level, no 1015.
const MODEL: &[u8] = b"\x1b[?1;2004h\x1b[?1049h\x1b[?1002h\x1b[?1006h\x1b=\x1b[?25ltext\x1b[?1000h\
\x1b[?1005h\x1b[?25h\x1b>\x1b[?2004l\x1b[?1000l\x1b[?1049l\x1b[?1;9hmore\x1b[?1005l";

/// The captures, then the synthetic streams; whether the model terminal
/// reads each with the rules of `Modes`.
fn streams() -> Vec<(String, Vec<u8>, bool)> {
    let mut all: Vec<(String, Vec<u8>, bool)> = fixtures().into_iter().map(|(n, b)| (n, b, true)).collect();
    all.push(("model".into(), MODEL.to_vec(), true));
    all.push(("synthetic".into(), SYNTHETIC.to_vec(), false));
    all
}

fn ring_of(bytes: &[u8], capacity: usize) -> Ring {
    let mut ring = Ring::new(capacity);
    let now = Instant::now();
    for chunk in bytes.chunks(CHUNK) {
        ring.push(chunk, now);
    }
    ring
}

/// The modes of a whole stream, as each reader gives them.
struct Whole {
    ours: Modes,
    vte: Plain,
    /// The model terminal's, when it reads the stream with the rules of
    /// `Modes`.
    model: Option<pod_testkit::screen::Modes>,
}

impl Whole {
    fn of(bytes: &[u8], model: bool) -> Whole {
        Whole { ours: ours(bytes, CHUNK), vte: vte(bytes), model: model.then(|| Screen::of(24, 80, bytes).modes()) }
    }

    /// Checks that `restore` of `modes` and then `rest` give the modes of
    /// the whole stream to each reader.
    fn check(&self, what: &str, modes: &Modes, rest: &[u8]) {
        let mut replay = modes.restore();
        replay.extend_from_slice(rest);
        assert_eq!(ours(&replay, CHUNK), self.ours, "{what}: Modes");
        assert_eq!(vte(&replay), self.vte, "{what}: vte");
        if let Some(model) = self.model {
            assert_eq!(Screen::of(24, 80, &replay).modes(), model, "{what}: the model");
        }
    }
}

#[test]
fn restore_of_the_modes_at_a_cut_and_the_rest_give_the_modes_of_the_whole_stream() {
    for (name, bytes, model) in streams() {
        let whole = Whole::of(&bytes, model);
        let ring = ring_of(&bytes, bytes.len());
        assert_eq!(ring.start(), Some(0), "{name}");
        let mut cuts = boundaries(&bytes);
        cuts.push(bytes.len() as u64);
        let step = cuts.len().div_ceil(CUTS).max(1);
        for &at in cuts.iter().step_by(step) {
            let modes = ring.modes_at(at).unwrap_or_else(|| panic!("{name}: no modes at {at}"));
            assert_eq!(modes, ours(&bytes[..at as usize], CHUNK), "{name}: the modes at {at}");
            whole.check(&format!("{name} cut at {at}"), &modes, &bytes[at as usize..]);
        }
    }
    // The captures hold modes: vim sets the alternate screen, the cursor
    // keys and the keypad.
    let vi = fixtures().into_iter().find(|(n, _)| n == "vi.bin").expect("vi.bin").1;
    let ring = ring_of(&vi, vi.len());
    let first = vi.windows(8).position(|w| w == b"\x1b[?1049h").expect("vim switches screens") + 8;
    assert!(ring.modes_at(first as u64).expect("modes").alternate());
}

#[test]
fn a_ring_that_drops_bytes_keeps_the_modes_of_what_it_dropped() {
    for (name, bytes, model) in streams() {
        let whole = Whole::of(&bytes, model);
        let found = boundaries(&bytes);
        for capacity in [40, 300, bytes.len() / 3, bytes.len() / 2].into_iter().filter(|&c| c < bytes.len()) {
            let ring = ring_of(&bytes, capacity);
            let Some(start) = ring.start() else { continue };
            let end = ring.end();
            assert!(start > 0, "{name}: a ring of {capacity} dropped nothing");
            let modes = ring.modes_at(start).expect("modes at the start");
            whole.check(&format!("{name} in {capacity} bytes"), &modes, &ring.kept());
            for &at in found.iter().filter(|&&at| at >= start).chain([&end]) {
                let at_modes = ring.modes_at(at).expect("modes at a boundary");
                assert_eq!(at_modes, ours(&bytes[..at as usize], CHUNK), "{name} in {capacity} bytes, at {at}");
            }
            assert_eq!(ring.modes_at(start - 1), None, "{name}: before the start");
            assert_eq!(ring.modes_at(end + 1), None, "{name}: after the end");
        }
    }
}

#[test]
fn a_ring_with_no_start_gives_no_modes() {
    let ring = ring_of(b"\x1b[?1049h\x1b]0;a long title", 4);
    assert_eq!(ring.start(), None);
    assert_eq!((ring.modes_at(0), ring.modes_at(ring.end())), (None, None));
}
