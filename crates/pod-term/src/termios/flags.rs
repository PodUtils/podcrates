//! The flags and the indexes of `c_cc`, with their Linux values.
//!
//! The values are those of `include/uapi/asm-generic/termbits.h`, which
//! x86-64, aarch64 and riscv64 use. A child that sets its state with
//! `TCSETS` sends these numbers, so the discipline reads them as they are.

// ─────────── c_iflag

/// Ignore a break condition. Kept as data.
pub const IGNBRK: u32 = 0o000001;
/// A break gives SIGINT. Kept as data: an SSH channel has no break.
pub const BRKINT: u32 = 0o000002;
/// Ignore parity errors. Kept as data.
pub const IGNPAR: u32 = 0o000004;
/// Mark parity errors. Kept as data.
pub const PARMRK: u32 = 0o000010;
/// Check input parity. Kept as data.
pub const INPCK: u32 = 0o000020;
/// Clear the eighth bit of each input byte.
pub const ISTRIP: u32 = 0o000040;
/// Input `\n` becomes `\r`.
pub const INLCR: u32 = 0o000100;
/// Drop input `\r`.
pub const IGNCR: u32 = 0o000200;
/// Input `\r` becomes `\n`.
pub const ICRNL: u32 = 0o000400;
/// Input upper case becomes lower case. Kept as data.
pub const IUCLC: u32 = 0o001000;
/// `VSTOP` and `VSTART` stop and start the output.
pub const IXON: u32 = 0o002000;
/// Any byte starts the output again, under `IXON`.
pub const IXANY: u32 = 0o004000;
/// Flow control of the input. Kept as data.
pub const IXOFF: u32 = 0o010000;
/// A bell when the line is full. Kept as data: a Linux pty rings none.
pub const IMAXBEL: u32 = 0o020000;
/// The input is UTF-8: an erase removes a full character.
pub const IUTF8: u32 = 0o040000;

// ─────────── c_oflag

/// Process the output: the flags below apply only with this one.
pub const OPOST: u32 = 0o000001;
/// Output lower case becomes upper case. Kept as data.
pub const OLCUC: u32 = 0o000002;
/// Output `\n` becomes `\r\n`.
pub const ONLCR: u32 = 0o000004;
/// Output `\r` becomes `\n`.
pub const OCRNL: u32 = 0o000010;
/// No output `\r` at column 0.
pub const ONOCR: u32 = 0o000020;
/// Output `\n` also returns the carriage: the column is 0 after it.
pub const ONLRET: u32 = 0o000040;
/// Fill characters for a delay. Kept as data.
pub const OFILL: u32 = 0o000100;
/// The fill character is DEL. Kept as data.
pub const OFDEL: u32 = 0o000200;

// ─────────── c_cflag (kept as data: a channel has no line speed)

/// The speed 38400, the speed of a Linux pty.
pub const B38400: u32 = 0o000017;
/// Eight bits for each character.
pub const CS8: u32 = 0o000060;
/// The mask of the character size.
pub const CSIZE: u32 = 0o000060;
/// The receiver is on.
pub const CREAD: u32 = 0o000200;
/// Parity on.
pub const PARENB: u32 = 0o000400;
/// Hang up at the last close.
pub const HUPCL: u32 = 0o002000;

// ─────────── c_lflag

/// `VINTR`, `VQUIT` and `VSUSP` give a signal.
pub const ISIG: u32 = 0o000001;
/// The canonical mode: a line buffer with editing.
pub const ICANON: u32 = 0o000002;
/// Upper case shown with a backslash. Kept as data.
pub const XCASE: u32 = 0o000004;
/// Echo the input.
pub const ECHO: u32 = 0o000010;
/// `VERASE` erases the character on the screen, not only in the line.
pub const ECHOE: u32 = 0o000020;
/// `VKILL` echoes a newline after the kill character.
pub const ECHOK: u32 = 0o000040;
/// Echo `\n` with no `ECHO`, in the canonical mode.
pub const ECHONL: u32 = 0o000100;
/// A signal character does not flush the input.
pub const NOFLSH: u32 = 0o000200;
/// A background write gives SIGTTOU. Kept as data.
pub const TOSTOP: u32 = 0o000400;
/// Echo a control character as `^X`, and DEL as `^?`.
pub const ECHOCTL: u32 = 0o001000;
/// Erase with `\` and `/` marks. Kept as data.
pub const ECHOPRT: u32 = 0o002000;
/// `VKILL` erases each character of the line on the screen.
pub const ECHOKE: u32 = 0o004000;
/// Discard the output. Kept as data.
pub const FLUSHO: u32 = 0o010000;
/// Reprint the pending input at the next byte. Kept as data.
pub const PENDIN: u32 = 0o040000;
/// `VWERASE`, `VLNEXT`, `VEOL2` and `VREPRINT` are special.
pub const IEXTEN: u32 = 0o100000;
/// External processing. Kept as data.
pub const EXTPROC: u32 = 0o200000;

// ─────────── the indexes of c_cc

/// Interrupt: SIGINT.
pub const VINTR: usize = 0;
/// Quit: SIGQUIT.
pub const VQUIT: usize = 1;
/// Erase one character.
pub const VERASE: usize = 2;
/// Erase the line.
pub const VKILL: usize = 3;
/// End of file: the line goes to the child with no byte added.
pub const VEOF: usize = 4;
/// The time limit of a read with no `ICANON`, in tenths of a second.
pub const VTIME: usize = 5;
/// The least count of bytes of a read with no `ICANON`.
pub const VMIN: usize = 6;
/// Switch. Kept as data.
pub const VSWTC: usize = 7;
/// Start the output. Kept as data.
pub const VSTART: usize = 8;
/// Stop the output. Kept as data.
pub const VSTOP: usize = 9;
/// Suspend: SIGTSTP.
pub const VSUSP: usize = 10;
/// An extra end of line.
pub const VEOL: usize = 11;
/// Reprint the line.
pub const VREPRINT: usize = 12;
/// Discard the output. Kept as data.
pub const VDISCARD: usize = 13;
/// Erase one word.
pub const VWERASE: usize = 14;
/// The next byte is literal.
pub const VLNEXT: usize = 15;
/// A second extra end of line.
pub const VEOL2: usize = 16;
/// The count of `c_cc` in the `struct termios` of the kernel (`TCGETS`).
pub const NCCS: usize = 19;

/// A `c_cc` of this value matches no byte (`_POSIX_VDISABLE` of Linux).
pub const DISABLED: u8 = 0;
