//! **The termios discipline: signals, the mapping of input and output, the
//! raw mode, and the changes of the state.**
//!
//! Each expected sequence is the one of Linux `n_tty` for the same state.
//! The canonical editing and the echo are in `termios.rs`.

use pod_term::termios::flags::*;
use pod_term::termios::{Event, Signal, Termios, Tty, When};
use pod_term::window::Size;

mod common;
use common::tty::{change, got, sane, type_in, with};

/// A tty in the raw mode of `cfmakeraw`, changed by `f`.
fn raw(f: impl FnOnce(&mut Termios)) -> Tty {
    with(|s| {
        s.make_raw();
        f(s);
    })
}

// ───────────────────────────────── signals

#[test]
fn vintr_gives_sigint_flushes_the_line_and_echoes_caret_c() {
    let mut t = sane();
    let got = type_in(&mut t, b"ab\x03");
    assert_eq!(got.signals, [(Signal::Int, true)], "{}", got.show());
    assert_eq!(got.user, b"ab^C");
    assert_eq!(type_in(&mut t, b"\r").child, [b"\n".to_vec()], "the line was flushed");
    // The signal comes before its echo: the caller drops what it holds,
    // then writes the `^C`.
    let events = t.input(b"\x03");
    assert_eq!(events, [Event::Signal { sig: Signal::Int, flush: true }, Event::ToUser(b"^C".to_vec())]);
}

#[test]
fn noflsh_keeps_the_line_after_a_signal() {
    let mut t = with(|s| s.lflag |= NOFLSH);
    let got = type_in(&mut t, b"ab\x03\r");
    assert_eq!(got.signals, [(Signal::Int, false)], "{}", got.show());
    assert_eq!(got.child, [b"ab\n".to_vec()], "the line was kept");
}

#[test]
fn vquit_and_vsusp_give_sigquit_and_sigtstp() {
    let mut t = sane();
    let got = type_in(&mut t, b"\x1c\x1a");
    assert_eq!(got.signals, [(Signal::Quit, true), (Signal::Tstp, true)], "{}", got.show());
    assert_eq!(got.user, b"^\\^Z");
}

#[test]
fn without_isig_the_signal_characters_are_data() {
    let mut t = with(|s| s.lflag &= !ISIG);
    let got = type_in(&mut t, b"\x03\x1c\r");
    assert!(got.signals.is_empty(), "{}", got.show());
    assert_eq!(got.child, [b"\x03\x1c\n".to_vec()]);
}

#[test]
fn a_signal_is_not_echoed_without_echo() {
    let mut t = with(|s| s.lflag &= !ECHO);
    let got = type_in(&mut t, b"\x03");
    assert_eq!(got.signals, [(Signal::Int, true)]);
    assert!(got.user.is_empty(), "{}", got.show());
}

#[test]
fn the_signals_have_their_linux_numbers() {
    let numbers = [Signal::Int, Signal::Quit, Signal::Tstp, Signal::Winch].map(Signal::number);
    assert_eq!(numbers, [2, 3, 20, 28]);
}

#[test]
fn a_resize_gives_sigwinch_once_for_each_change() {
    let mut t = sane().sized(Size::new(24, 80));
    assert_eq!(t.size(), Size::new(24, 80));
    assert!(t.resize(Size::new(24, 80)).is_empty(), "the same size: no signal");
    let events = t.resize(Size::new(50, 160));
    assert_eq!(events, [Event::Signal { sig: Signal::Winch, flush: false }]);
    assert_eq!(t.size(), Size::new(50, 160), "the answer to TIOCGWINSZ");
}

// ───────────────────────────────── input mapping

#[test]
fn icrnl_makes_a_carriage_return_a_newline() {
    let mut t = raw(|s| s.iflag |= ICRNL);
    assert_eq!(type_in(&mut t, b"a\r").child, [b"a\n".to_vec()]);
}

#[test]
fn igncr_drops_a_carriage_return_before_icrnl_maps_it() {
    let mut t = with(|s| s.iflag |= IGNCR);
    let got = type_in(&mut t, b"a\r\n");
    assert_eq!(got.child, [b"a\n".to_vec()], "{}", got.show());
    assert_eq!(got.user, b"a\r\n", "one newline");
}

#[test]
fn without_icrnl_a_carriage_return_is_data_and_echoes_as_caret_m() {
    let mut t = with(|s| s.iflag &= !ICRNL);
    let got = type_in(&mut t, b"a\r\n");
    assert_eq!(got.child, [b"a\r\n".to_vec()], "{}", got.show());
    assert_eq!(got.user, b"a^M\r\n");
}

#[test]
fn inlcr_makes_a_newline_a_carriage_return() {
    let mut t = raw(|s| s.iflag |= INLCR);
    assert_eq!(type_in(&mut t, b"\n").child, [b"\r".to_vec()]);
    // In the canonical mode the `\r` that INLCR gives is data: ICRNL acts
    // on a typed `\r` only.
    let mut t = with(|s| s.iflag |= INLCR);
    let got = type_in(&mut t, b"a\n\r");
    assert_eq!(got.child, [b"a\r\n".to_vec()], "{}", got.show());
    assert_eq!(got.user, b"a^M\r\n");
}

#[test]
fn istrip_clears_the_eighth_bit_before_the_signal_characters() {
    let mut t = with(|s| s.iflag |= ISTRIP);
    let got = type_in(&mut t, &[0xe1, 0x83]);
    assert_eq!(got.user, b"a^C", "{}", got.show());
    assert_eq!(got.signals, [(Signal::Int, true)]);
}

#[test]
fn ixon_stops_and_starts_the_output_and_its_characters_are_not_data() {
    let mut t = sane();
    let got = type_in(&mut t, b"\x13a");
    assert_eq!(got.flow, [Event::StopOutput], "{}", got.show());
    assert_eq!(got.user, b"a", "the echo, which the caller holds");
    assert!(t.stopped());
    assert!(type_in(&mut t, b"\x13").flow.is_empty(), "a second stop changes nothing");
    let got = type_in(&mut t, b"\x11\r");
    assert_eq!(got.flow, [Event::StartOutput], "{}", got.show());
    assert_eq!(got.child, [b"a\n".to_vec()], "no ^S and no ^Q in the line");
    assert!(!t.stopped());
}

#[test]
fn ixany_and_a_signal_start_the_output() {
    let mut t = with(|s| s.iflag |= IXANY);
    type_in(&mut t, b"\x13");
    let got = type_in(&mut t, b"y");
    assert_eq!(got.flow, [Event::StartOutput], "{}", got.show());
    let mut t = sane();
    type_in(&mut t, b"\x13");
    assert_eq!(type_in(&mut t, b"y").flow, [], "no IXANY: a byte does not start it");
    let events = t.input(b"\x03");
    let start = events.iter().position(|e| *e == Event::StartOutput);
    assert_eq!(start, Some(1), "after the signal, before its echo: {events:?}");
}

#[test]
fn vlnext_makes_a_flow_character_data_and_clearing_ixon_starts_the_output() {
    let mut t = sane();
    let got = type_in(&mut t, b"\x16\x13\r");
    assert!(got.flow.is_empty(), "{}", got.show());
    assert_eq!(got.child, [b"\x13\n".to_vec()]);
    assert_eq!(got.user, b"^\x08^S\r\n");
    type_in(&mut t, b"\x13");
    assert_eq!(change(&mut t, |s| s.iflag &= !IXON).flow, [Event::StartOutput]);
    assert_eq!(type_in(&mut t, b"\x13\r").child, [b"\x13\n".to_vec()], "with no IXON, ^S is data");
}

// ───────────────────────────────── output mapping

#[test]
fn onlcr_makes_a_newline_cr_lf() {
    let mut t = sane();
    assert_eq!(t.output(b"a\nb\n"), b"a\r\nb\r\n");
}

#[test]
fn without_opost_the_output_passes_as_it_is() {
    let mut t = with(|s| s.oflag &= !OPOST);
    assert_eq!(t.output(b"a\nb\r"), b"a\nb\r");
}

#[test]
fn ocrnl_makes_a_carriage_return_a_newline() {
    // The `\n` that OCRNL gives does not take ONLCR, as in `n_tty`.
    let mut t = with(|s| s.oflag |= OCRNL);
    assert_eq!(t.output(b"a\rb"), b"a\nb");
}

#[test]
fn onocr_drops_a_carriage_return_at_column_zero() {
    let mut t = with(|s| s.oflag |= ONOCR);
    assert_eq!(t.output(b"\rab\r"), b"ab\r");
}

#[test]
fn onlret_puts_the_column_at_zero_after_a_newline() {
    let mut t = with(|s| s.oflag = OPOST | ONOCR | ONLRET);
    assert_eq!(t.output(b"ab\n\r"), b"ab\n", "the `\\r` after the newline is at column 0");
}

// ───────────────────────────────── the raw mode

#[test]
fn with_no_icanon_each_byte_goes_to_the_child_at_once() {
    let mut t = raw(|_| {});
    let got = type_in(&mut t, b"ab\x03\x7f\r");
    assert_eq!(got.child, [b"ab\x03\x7f\r".to_vec()], "{}", got.show());
    assert!(got.user.is_empty() && got.signals.is_empty(), "{}", got.show());
    assert_eq!(t.output(b"x\n"), b"x\n", "no OPOST");
}

#[test]
fn with_no_icanon_and_echo_the_erase_character_is_data() {
    let mut t = with(|s| s.lflag &= !ICANON);
    let got = type_in(&mut t, b"a\x7f");
    // The events keep their order: an echo comes between two bytes.
    assert_eq!(got.child_bytes(), b"a\x7f", "{}", got.show());
    assert_eq!(got.user, b"a^?");
}

#[test]
fn with_no_icanon_a_newline_echoes_as_a_newline() {
    let mut t = with(|s| s.lflag &= !ICANON);
    let got = type_in(&mut t, b"\r");
    assert_eq!(got.child, [b"\n".to_vec()], "{}", got.show());
    assert_eq!(got.user, b"\r\n", "not `^J`");
}

#[test]
fn vmin_and_vtime_are_data_for_the_caller() {
    let mut t = raw(|s| {
        s.cc[VMIN] = 0;
        s.cc[VTIME] = 5;
    });
    assert_eq!((t.state().vmin(), t.state().vtime()), (0, 5));
    assert_eq!(type_in(&mut t, b"a").child, [b"a".to_vec()], "the byte goes at once");
}

// ───────────────────────────────── a change of the state

#[test]
fn tcsets_to_raw_sends_the_line_under_edit() {
    let mut t = sane();
    type_in(&mut t, b"ab");
    let got = change(&mut t, Termios::make_raw);
    assert_eq!(got.child, [b"ab".to_vec()], "{}", got.show());
    assert_eq!(t.pending_len(), 0);
}

#[test]
fn tcsetsf_drops_the_line_under_edit() {
    let mut t = sane();
    type_in(&mut t, b"ab");
    let mut s = *t.state();
    s.make_raw();
    assert!(t.set(s, When::Flush).is_empty(), "nothing goes to the child");
    assert_eq!(t.pending_len(), 0);
    assert_eq!(type_in(&mut t, b"c").child, [b"c".to_vec()]);
}

#[test]
fn tcsetsw_acts_at_once_since_the_caller_times_the_drain() {
    let mut t = sane();
    type_in(&mut t, b"ab");
    let mut s = *t.state();
    s.lflag &= !ECHO;
    assert!(t.set(s, When::Drain).is_empty());
    let got = type_in(&mut t, b"c\r");
    assert!(got.user.is_empty(), "{}", got.show());
    assert_eq!(got.child, [b"abc\n".to_vec()], "the line was kept");
    assert_eq!(t.state(), &s, "the answer to the next TCGETS");
}

#[test]
fn a_switch_of_icanon_ends_a_pending_vlnext() {
    let mut t = sane();
    type_in(&mut t, b"\x16");
    change(&mut t, |s| s.lflag &= !ICANON);
    let got = type_in(&mut t, b"\x03");
    assert_eq!(got.signals, [(Signal::Int, true)], "the ^C is a signal: {}", got.show());
}

#[test]
fn flush_drops_the_line_under_edit() {
    let mut t = sane();
    type_in(&mut t, b"ab");
    t.flush();
    assert_eq!(type_in(&mut t, b"\r").child, [b"\n".to_vec()]);
}

#[test]
fn the_sane_state_has_the_values_of_a_linux_pty() {
    // `stty -g` on a new Linux pty: 500:5:bf:8a3b:3:1c:7f:15:4:0:1:0:11:13:1a:0:12:f:17:16:0.
    let s = Termios::sane();
    assert_eq!((s.iflag, s.oflag, s.cflag, s.lflag), (0x500, 0x5, 0xbf, 0x8a3b));
    let cc = [3, 0x1c, 0x7f, 0x15, 4, 0, 1, 0, 0x11, 0x13, 0x1a, 0, 0x12, 0xf, 0x17, 0x16, 0, 0, 0];
    assert_eq!(s.cc, cc);
    assert_eq!(Termios::default(), s);
    assert_eq!(got(Tty::new(s).input(b"")), Default::default(), "no input, no event");
}
