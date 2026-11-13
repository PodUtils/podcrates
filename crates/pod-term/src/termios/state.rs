//! The termios state, and when a new one applies.

use super::flags::*;

/// A termios state: the four flag words and `c_cc`, as `TCGETS` gives them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Termios {
    /// `c_iflag`: the input flags.
    pub iflag: u32,
    /// `c_oflag`: the output flags.
    pub oflag: u32,
    /// `c_cflag`: the control flags. Kept as data.
    pub cflag: u32,
    /// `c_lflag`: the local flags.
    pub lflag: u32,
    /// `c_cc`: the special characters, by the indexes of [`super::flags`].
    pub cc: [u8; NCCS],
}

impl Termios {
    /// The state of a new Linux pty (`tty_std_termios`, with the `c_cflag`
    /// of the pty driver): canonical, with echo and signals; `^C`, `^\`, DEL,
    /// `^U`, `^D`, `^Z`, `^R`, `^W`, `^V`. `stty -g` prints it as
    /// `500:5:bf:8a3b:3:1c:7f:15:4:0:1:0:11:13:1a:0:12:f:17:16:0`.
    pub const fn sane() -> Termios {
        let mut cc = [0u8; NCCS];
        cc[VINTR] = 0x03;
        cc[VQUIT] = 0x1c;
        cc[VERASE] = 0x7f;
        cc[VKILL] = 0x15;
        cc[VEOF] = 0x04;
        cc[VMIN] = 1;
        cc[VSTART] = 0x11;
        cc[VSTOP] = 0x13;
        cc[VSUSP] = 0x1a;
        cc[VREPRINT] = 0x12;
        cc[VDISCARD] = 0x0f;
        cc[VWERASE] = 0x17;
        cc[VLNEXT] = 0x16;
        Termios {
            iflag: ICRNL | IXON,
            oflag: OPOST | ONLCR,
            cflag: B38400 | CS8 | CREAD,
            lflag: ISIG | ICANON | ECHO | ECHOE | ECHOK | ECHOCTL | ECHOKE | IEXTEN,
            cc,
        }
    }

    /// The raw mode of `cfmakeraw`: no editing, no echo, no signals, no
    /// mapping, no `OPOST`; `VMIN` 1 and `VTIME` 0.
    pub fn make_raw(&mut self) {
        self.iflag &= !(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL | IXON);
        self.oflag &= !OPOST;
        self.lflag &= !(ECHO | ECHONL | ICANON | ISIG | IEXTEN);
        self.cflag = (self.cflag & !(CSIZE | PARENB)) | CS8;
        self.cc[VMIN] = 1;
        self.cc[VTIME] = 0;
    }

    /// Whether `c` is the special character at `index`. **A `c_cc` of 0
    /// matches no byte**, as `_POSIX_VDISABLE` of Linux.
    pub fn matches(&self, c: u8, index: usize) -> bool {
        let v = self.cc[index];
        v != DISABLED && v == c
    }

    /// Whether the canonical mode is on.
    pub fn canonical(&self) -> bool {
        self.lflag & ICANON != 0
    }

    /// `VMIN`: with no `ICANON`, the least count of bytes of a read.
    pub fn vmin(&self) -> u8 {
        self.cc[VMIN]
    }

    /// `VTIME`: with no `ICANON`, the time limit of a read, in tenths of a
    /// second.
    pub fn vtime(&self) -> u8 {
        self.cc[VTIME]
    }
}

impl Default for Termios {
    fn default() -> Self {
        Termios::sane()
    }
}

/// When a new state applies: the three `TCSETS*` requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// `TCSETS`: now.
    Now,
    /// `TCSETSW`: after the output drains. The caller calls at that time.
    Drain,
    /// `TCSETSF`: after the output drains, and the pending input is dropped
    /// first. A line under edit is dropped, not sent.
    Flush,
}
