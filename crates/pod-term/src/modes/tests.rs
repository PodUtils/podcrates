//! The reader's own tests: each mode, the rules of the mouse, the strings
//! and the sequences that hold no mode, and `restore`. The tests against a
//! model terminal and a parser of another project are `tests/modes.rs`.

use proptest::prelude::*;

use super::*;

fn read(bytes: &[u8]) -> Modes {
    let mut m = Modes::new();
    m.feed(bytes);
    m
}

/// Each flag mode with its accessor: its number and its default.
type Flag = (u16, fn(&Modes) -> bool, bool);

const FLAGS: [Flag; 9] = [
    (1, Modes::cursor_keys, false),
    (6, Modes::origin, false),
    (7, Modes::autowrap, true),
    (25, Modes::cursor_shown, true),
    (47, Modes::alternate, false),
    (1047, Modes::alternate, false),
    (1049, Modes::alternate, false),
    (1004, Modes::focus, false),
    (2004, Modes::bracketed_paste, false),
];

#[test]
fn a_new_terminal_has_each_mode_at_its_default() {
    let m = Modes::new();
    for (mode, get, default) in FLAGS {
        assert_eq!(get(&m), default, "mode {mode}");
    }
    assert_eq!((m.mouse(), m.mouse_encoding(), m.keypad(), m.g0()), (None, None, false, Charset::Ascii));
    assert_eq!(m, Modes::default());
}

#[test]
fn each_flag_mode_is_read_alone_and_as_the_second_parameter() {
    for (mode, get, _) in FLAGS {
        // Mode 3 (DECCOLM) is not a mode of this type: it is skipped.
        for prefix in ["", "3;"] {
            let set = format!("\x1b[?{prefix}{mode}h");
            let reset = format!("{set}\x1b[?{prefix}{mode}l");
            assert!(get(&read(set.as_bytes())), "{set:?}");
            assert!(!get(&read(reset.as_bytes())), "{reset:?}");
        }
    }
}

#[test]
fn several_modes_in_one_csi_count_each() {
    let m = read(b"\x1b[?1;2004;25;7l\x1b[?1;2004h");
    assert!(m.cursor_keys() && m.bracketed_paste());
    assert!(!m.cursor_shown() && !m.autowrap());
}

#[test]
fn a_set_of_a_mouse_level_replaces_it_and_a_reset_of_any_level_ends_the_reports() {
    assert_eq!(read(b"\x1b[?1000h").mouse(), Some(Mouse::Press));
    assert_eq!(read(b"\x1b[?1000h\x1b[?1002h").mouse(), Some(Mouse::Button));
    assert_eq!(read(b"\x1b[?1002;1003h").mouse(), Some(Mouse::Any));
    assert_eq!(read(b"\x1b[?9h").mouse(), Some(Mouse::X10));
    // xterm and tmux: a reset of a level that is not the current one also
    // ends the reports.
    assert_eq!(read(b"\x1b[?1002h\x1b[?1000l").mouse(), None);
    assert_eq!(read(b"\x1b[?1003h\x1b[?1003l").mouse(), None);
}

#[test]
fn a_mouse_encoding_is_one_value_that_a_reset_of_another_keeps() {
    assert_eq!(read(b"\x1b[?1006h").mouse_encoding(), Some(MouseEncoding::Sgr));
    assert_eq!(read(b"\x1b[?1006h\x1b[?1015h").mouse_encoding(), Some(MouseEncoding::Urxvt));
    assert_eq!(read(b"\x1b[?1005h\x1b[?1006l").mouse_encoding(), Some(MouseEncoding::Utf8));
    assert_eq!(read(b"\x1b[?1005h\x1b[?1005l").mouse_encoding(), None);
}

#[test]
fn the_keypad_and_g0_are_escape_sequences() {
    assert!(read(b"\x1b=").keypad());
    assert!(!read(b"\x1b=\x1b>").keypad());
    assert_eq!(read(b"\x1b(0").g0(), Charset::DecGraphics);
    assert_eq!(read(b"\x1b(0\x1b(B").g0(), Charset::Ascii);
    // Another set in G0, or the DEC set in G1, is no mode of this type.
    assert_eq!(read(b"\x1b(0\x1b(A").g0(), Charset::DecGraphics);
    assert_eq!(read(b"\x1b)0").g0(), Charset::Ascii);
}

#[test]
fn ris_puts_each_mode_at_its_default() {
    let busy = b"\x1b[?1;6;1049;1003;1006;1004;2004h\x1b[?7;25l\x1b=\x1b(0";
    let m = read(busy);
    assert_ne!(m, Modes::new());
    let mut reset = busy.to_vec();
    reset.extend_from_slice(b"\x1bc");
    assert_eq!(read(&reset), Modes::new());
}

#[test]
fn a_string_holds_no_mode() {
    for s in [&b"\x1b]0;?1049h\x07"[..], b"\x1b]0;\x1b[?1049h", b"\x1bP\x1b[?2004h", b"\x1b_?25l\x1b\\"] {
        let m = read(s);
        // An ESC in a string starts a new sequence, as in a terminal.
        let started = s.windows(3).filter(|w| w == b"\x1b[?").count();
        assert_eq!(m == Modes::new(), started == 0, "{s:?}");
    }
    assert_eq!(read(b"\x1b]2;\x1b[?1049h is text\x07"), read(b"\x1b[?1049h"));
}

#[test]
fn can_and_sub_end_a_csi() {
    assert_eq!(read(b"\x1b[?10\x1849h"), Modes::new());
    assert_eq!(read(b"\x1b[?10\x1a49h"), Modes::new());
    // Another control byte is acted on, and the CSI goes on.
    assert!(read(b"\x1b[?10\r49h").alternate());
}

#[test]
fn a_csi_that_is_not_a_dec_private_mode_changes_nothing() {
    for s in [&b"\x1b[1049h"[..], b"\x1b[>1049h", b"\x1b[<1049h", b"\x1b[=1049h", b"\x1b[?1049$h", b"\x1b[?1049p"] {
        assert_eq!(read(s), Modes::new(), "{s:?}");
    }
    // A parameter past u16, an empty one or an unknown mode is skipped.
    assert!(read(b"\x1b[?70000;;5;2004h").bracketed_paste());
    assert!(read(b"\x1b[?002004h").bracketed_paste());
}

#[test]
fn a_csi_past_its_limit_is_dropped_whole() {
    let mut long = b"\x1b[?".to_vec();
    long.extend(std::iter::repeat_n(b'0', CSI_MAX));
    long.extend_from_slice(b"2004h");
    assert_eq!(read(&long), Modes::new());
    assert!(read(b"\x1b[?2004h").bracketed_paste());
}

#[test]
fn equal_modes_are_equal_whatever_the_reader_waits_for() {
    let mut cut = read(b"\x1b[?2004h");
    cut.feed(b"\x1b[?10");
    assert_eq!(cut, read(b"\x1b[?2004h"));
    cut.feed(b"49h");
    assert!(cut.alternate());
}

#[test]
fn restore_resets_the_alternate_screen_with_47_and_sets_it_with_1049() {
    let off = Modes::new().restore();
    assert!(off.starts_with(b"\x1b[?47l"), "{:?}", String::from_utf8_lossy(&off));
    let on = read(b"\x1b[?47h").restore();
    assert!(on.starts_with(b"\x1b[?1049h"), "{:?}", String::from_utf8_lossy(&on));
}

#[test]
fn restore_resets_each_mouse_mode_before_it_sets_one() {
    let bytes = read(b"\x1b[?1002;1006h").restore();
    let text = String::from_utf8(bytes).expect("ASCII");
    let set = text.find("\x1b[?1002h").expect("the level is set");
    for reset in ["9", "1000", "1002", "1003", "1005", "1006", "1015"] {
        let at = text.find(&format!("\x1b[?{reset}l")).unwrap_or_else(|| panic!("{reset} is reset"));
        assert!(at < set, "the reset of {reset} after the set");
    }
    assert!(text.find("\x1b[?1006h").expect("the encoding is set") > set);
}

#[test]
fn restore_of_a_new_terminal_is_a_new_terminal() {
    let mut m = Modes::new();
    m.feed(&Modes::new().restore());
    assert_eq!(m, Modes::new());
}

fn any_modes() -> impl Strategy<Value = Modes> {
    let pieces = prop::sample::select(vec![
        &b"\x1b[?1h"[..],
        b"\x1b[?1l",
        b"\x1b[?6h",
        b"\x1b[?7l",
        b"\x1b[?25l",
        b"\x1b[?47h",
        b"\x1b[?1047l",
        b"\x1b[?1049h",
        b"\x1b[?9h",
        b"\x1b[?1000h",
        b"\x1b[?1002h",
        b"\x1b[?1003h",
        b"\x1b[?1000l",
        b"\x1b[?1005h",
        b"\x1b[?1006h",
        b"\x1b[?1015h",
        b"\x1b[?1006l",
        b"\x1b[?1004h",
        b"\x1b[?2004h",
        b"\x1b=",
        b"\x1b>",
        b"\x1b(0",
        b"\x1b(B",
        b"\x1bc",
        b"text",
    ]);
    prop::collection::vec(pieces, 0..24).prop_map(|pieces| read(&pieces.concat()))
}

proptest! {
    /// Bytes of `restore` put a terminal in any state into the modes; each
    /// byte is 7-bit and the bytes are few.
    #[test]
    fn restore_puts_any_modes_into_any_modes(from in any_modes(), to in any_modes()) {
        let bytes = to.restore();
        prop_assert!(bytes.iter().all(u8::is_ascii));
        prop_assert!(bytes.len() < 256, "{} bytes", bytes.len());
        let mut m = from;
        m.feed(&bytes);
        prop_assert_eq!(m, to);
    }

    /// A stream read in any chunks gives the modes of the whole.
    #[test]
    fn each_split_gives_the_modes_of_the_whole(cuts in prop::collection::vec(0usize..200, 0..8)) {
        let stream = b"\x1b[?1;2004h\x1b]0;?1049h\x07\x1b[?1049h\x1b=\x1b(0x\x1b[?1003;1006h\x1b[?25l\x1b[?10\x1849h\x1b[?7l";
        let whole = read(stream);
        let mut cuts: Vec<usize> = cuts.into_iter().map(|c| c % (stream.len() + 1)).collect();
        cuts.sort_unstable();
        let mut m = Modes::new();
        let mut from = 0;
        for cut in cuts.into_iter().chain([stream.len()]) {
            m.feed(&stream[from..cut]);
            from = cut;
        }
        prop_assert_eq!(m, whole);
    }
}
