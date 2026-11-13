//! **The termios discipline: the canonical editing and the echo.**
//!
//! Each test sets one flag group of a Linux pty and asserts the exact bytes
//! of each destination: the lines for the child, the echo for the user.
//! Each expected sequence is the one of Linux `n_tty` for the same state.
//! The signals, the mapping of input and output, the raw mode and the
//! changes of the state are in `termios_modes.rs`.

use pod_term::termios::LINE_MAX;
use pod_term::termios::flags::*;
use pod_term::termios::{Termios, Tty};

mod common;
use common::tty::{sane, type_in, with};

// ───────────────────────────────── canonical editing

#[test]
fn a_line_reaches_the_child_only_at_its_end() {
    let mut t = sane();
    let got = type_in(&mut t, b"ab");
    assert!(got.child.is_empty(), "{}", got.show());
    assert_eq!(got.user, b"ab");
    let got = type_in(&mut t, b"\r");
    assert_eq!(got.child, [b"ab\n".to_vec()], "{}", got.show());
    assert_eq!(got.user, b"\r\n", "the echo of the newline, after ONLCR");
}

#[test]
fn verase_removes_one_character_and_rubs_it_out() {
    let mut t = sane();
    let got = type_in(&mut t, b"abc\x7f\r");
    assert_eq!(got.user, b"abc\x08 \x08\r\n", "{}", got.show());
    assert_eq!(got.child, [b"ab\n".to_vec()]);
}

#[test]
fn an_erase_on_an_empty_line_echoes_nothing() {
    // `n_tty` rings no bell here: the erase has nothing to act on.
    let mut t = sane();
    let got = type_in(&mut t, b"\x7f\x15\x17");
    assert_eq!(got, Default::default(), "{}", got.show());
}

#[test]
fn vkill_with_echoke_rubs_out_each_character() {
    let mut t = sane();
    let got = type_in(&mut t, b"ab\x15");
    assert_eq!(got.user, b"ab\x08 \x08\x08 \x08", "{}", got.show());
    assert_eq!(type_in(&mut t, b"\r").child, [b"\n".to_vec()], "the line was empty");
}

#[test]
fn vkill_without_echoke_echoes_the_kill_character_and_a_newline() {
    let mut t = with(|s| s.lflag &= !ECHOKE);
    let got = type_in(&mut t, b"ab\x15");
    assert_eq!(got.user, b"ab^U\r\n", "{}", got.show());
    assert_eq!(type_in(&mut t, b"c\r").child, [b"c\n".to_vec()]);
    let mut t = with(|s| s.lflag &= !(ECHOKE | ECHOK));
    assert_eq!(type_in(&mut t, b"ab\x15").user, b"ab^U", "no newline with no ECHOK");
}

#[test]
fn vwerase_removes_the_last_word_and_the_blanks_after_it() {
    let mut t = sane();
    let got = type_in(&mut t, b"foo bar  \x17");
    assert_eq!(got.user, [b"foo bar  ".as_slice(), &b"\x08 \x08".repeat(5)].concat(), "{}", got.show());
    assert_eq!(type_in(&mut t, b"\r").child, [b"foo \n".to_vec()]);
}

#[test]
fn vwerase_stops_at_a_mark_of_punctuation() {
    // The `ALTWERASE` rule of `n_tty`: a word is letters, digits and `_`.
    let mut t = sane();
    assert_eq!(type_in(&mut t, b"a.b\x17\r").child, [b"a.\n".to_vec()]);
}

#[test]
fn veof_sends_the_line_with_no_byte_added() {
    let mut t = sane();
    let got = type_in(&mut t, b"ab\x04");
    assert_eq!(got.child, [b"ab".to_vec()], "the line, and no 0x04: {}", got.show());
    assert_eq!(got.user, b"ab", "VEOF is not echoed");
    let got = type_in(&mut t, b"\x04");
    assert_eq!(got.child, [Vec::<u8>::new()], "an empty line: a read of 0 bytes, the end of file");
}

#[test]
fn veol_and_veol2_end_a_line_and_stay_in_it() {
    let mut t = with(|s| {
        s.cc[VEOL] = b';';
        s.cc[VEOL2] = b'|';
    });
    let got = type_in(&mut t, b"a;b|");
    assert_eq!(got.child, [b"a;".to_vec(), b"b|".to_vec()], "{}", got.show());
    assert_eq!(got.user, b"a;b|");
    let mut t = with(|s| {
        s.cc[VEOL2] = b'|';
        s.lflag &= !IEXTEN;
    });
    assert_eq!(type_in(&mut t, b"c|\r").child, [b"c|\n".to_vec()], "VEOL2 needs IEXTEN");
}

#[test]
fn a_disabled_character_matches_no_byte() {
    // `VEOL` is 0 in the sane state: a NUL is data, not the end of a line.
    let mut t = sane();
    let got = type_in(&mut t, b"a\0b\r");
    assert_eq!(got.child, [b"a\0b\n".to_vec()], "{}", got.show());
    assert_eq!(got.user, b"a^@b\r\n");
}

#[test]
fn vlnext_makes_the_next_byte_data() {
    let mut t = sane();
    let got = type_in(&mut t, b"\x16\x03\r");
    assert!(got.signals.is_empty(), "{}", got.show());
    assert_eq!(got.child, [b"\x03\n".to_vec()]);
    assert_eq!(got.user, b"^\x08^C\r\n", "a caret, a backspace, then the echo over it");
}

#[test]
fn vreprint_echoes_the_line_on_a_new_row() {
    let mut t = sane();
    let got = type_in(&mut t, b"ab\x12");
    assert_eq!(got.user, b"ab^R\r\nab", "{}", got.show());
    assert_eq!(type_in(&mut t, b"\r").child, [b"ab\n".to_vec()], "the line is not changed");
}

#[test]
fn the_extended_characters_need_iexten() {
    let mut t = with(|s| s.lflag &= !IEXTEN);
    let got = type_in(&mut t, b"a\x17\x16\x12\r");
    assert_eq!(got.child, [b"a\x17\x16\x12\n".to_vec()], "{}", got.show());
    assert_eq!(got.user, b"a^W^V^R\r\n");
}

#[test]
fn the_line_holds_at_most_line_max_bytes() {
    // After the limit, data is echoed and dropped with no bell, as on a
    // Linux pty; the newline still ends the line.
    let mut t = sane();
    let got = type_in(&mut t, &vec![b'a'; LINE_MAX + 2]);
    assert_eq!(got.user, vec![b'a'; LINE_MAX + 2], "each byte is echoed, and no bell rings");
    assert_eq!(t.pending_len(), LINE_MAX);
    let line = type_in(&mut t, b"\r").child;
    assert_eq!(line.len(), 1);
    assert_eq!(line[0].len(), LINE_MAX + 1, "the line and its newline");
    assert_eq!(line[0].last(), Some(&b'\n'));
}

#[test]
fn an_erase_under_iutf8_removes_a_whole_character() {
    let mut t = with(|s| s.iflag |= IUTF8);
    let got = type_in(&mut t, "aé\x7f\r".as_bytes());
    assert_eq!(got.child, [b"a\n".to_vec()], "{}", got.show());
    assert_eq!(got.user, "aé\x08 \x08\r\n".as_bytes(), "one rub out for the character");
    let mut t = sane();
    let got = type_in(&mut t, "aé\x7f\r".as_bytes());
    assert_eq!(got.child, [b"a\xc3\n".to_vec()], "with no IUTF8, one byte: {}", got.show());
}

#[test]
fn the_erase_of_a_tab_goes_back_to_its_tab_stop() {
    let mut t = sane();
    let got = type_in(&mut t, b"ab\t\x7f");
    assert_eq!(got.user, [b"ab\t".as_slice(), &[0x08; 6]].concat(), "{}", got.show());
    // The line starts at column 2, after the child's prompt: the tab stop
    // is 4 columns after the `b`.
    let mut t = sane();
    assert_eq!(t.output(b"$ "), b"$ ");
    let got = type_in(&mut t, b"ab\t\x7f");
    assert_eq!(got.user, [b"ab\t".as_slice(), &[0x08; 4]].concat(), "{}", got.show());
}

#[test]
fn a_control_character_rubs_out_two_cells() {
    let mut t = sane();
    let got = type_in(&mut t, b"\x16\x01\x7f");
    assert_eq!(got.user, b"^\x08^A\x08 \x08\x08 \x08", "{}", got.show());
    assert_eq!(t.pending_len(), 0);
}

// ───────────────────────────────── echo

#[test]
fn without_echo_nothing_is_echoed_and_the_line_still_arrives() {
    let mut t = with(|s| s.lflag &= !ECHO);
    let got = type_in(&mut t, b"ab\x7fc\x12\x15d\r");
    assert!(got.user.is_empty(), "{}", got.show());
    assert_eq!(got.child, [b"d\n".to_vec()]);
}

#[test]
fn echonl_echoes_the_newline_with_no_echo() {
    let mut t = with(|s| {
        s.lflag &= !ECHO;
        s.lflag |= ECHONL;
    });
    let got = type_in(&mut t, b"ab\r");
    assert_eq!(got.user, b"\r\n", "{}", got.show());
    assert_eq!(got.child, [b"ab\n".to_vec()]);
}

#[test]
fn echoctl_shows_a_control_as_a_caret_and_del_as_caret_question() {
    let mut t = sane();
    let got = type_in(&mut t, b"\x16\x7f\x16\x1b\r");
    assert_eq!(got.user, b"^\x08^?^\x08^[\r\n", "{}", got.show());
    assert_eq!(got.child, [b"\x7f\x1b\n".to_vec()]);
}

#[test]
fn without_echoctl_a_control_echoes_as_itself() {
    let mut t = with(|s| s.lflag &= !ECHOCTL);
    let got = type_in(&mut t, b"\x16\x01\r");
    assert_eq!(got.user, b"\x01\r\n", "no caret for VLNEXT, and the byte as it is: {}", got.show());
}

#[test]
fn without_echoe_an_erase_echoes_the_erase_character() {
    let mut t = with(|s| s.lflag &= !ECHOE);
    let got = type_in(&mut t, b"ab\x7f\r");
    assert_eq!(got.user, b"ab^?\r\n", "{}", got.show());
    assert_eq!(got.child, [b"a\n".to_vec()]);
}

#[test]
fn the_echo_goes_through_opost() {
    let mut t = with(|s| s.oflag &= !ONLCR);
    assert_eq!(type_in(&mut t, b"a\r").user, b"a\n");
    let mut t = with(|s| s.oflag &= !OPOST);
    assert_eq!(type_in(&mut t, b"a\r").user, b"a\n");
}

#[test]
fn a_tab_echoes_as_a_tab_and_not_as_caret_i() {
    let mut t = sane();
    assert_eq!(type_in(&mut t, b"\t").user, b"\t");
}

#[test]
fn debug_does_not_show_the_line() {
    // A line under edit can be a password typed with echo on.
    let mut t = Tty::new(Termios::sane());
    type_in(&mut t, b"hunter2");
    let text = format!("{t:?}");
    assert!(!text.contains("hunter2"), "{text}");
    assert!(text.contains("pending_len: 7"), "{text}");
}
