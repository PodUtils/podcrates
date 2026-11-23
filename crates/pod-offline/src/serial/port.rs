//! The POSIX open of the device of a [`SerialSpec`], raw 8N1, with the
//! settings saved and put back.
//!
//! The open uses `O_NOCTTY`, so that the device never becomes the
//! controlling terminal of the process, and `O_NONBLOCK`, so that the open
//! does not wait for the carrier of a modem; the flag goes off after the
//! settings are in place. `TIOCEXCL` holds the device alone. A failure after
//! a setting changed puts the saved settings back.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};

use super::SerialSpec;
use crate::Error;

/// An open serial line: raw 8N1 at the speed of its address.
pub struct SerialLine {
    file: File,
    saved: libc::termios,
    path: String,
    baud: u32,
    restored: bool,
}

/// Open the device of `spec`. The path must be a character device that is
/// a terminal, and this user must be able to open it.
pub fn open(spec: &SerialSpec) -> Result<SerialLine, Error> {
    let path = spec.path().to_string();
    let meta = std::fs::metadata(&path).map_err(|e| Error::Open { path: path.clone(), source: e })?;
    if !meta.file_type().is_char_device() {
        let why = "not a character device; a serial line is one, such as /dev/ttyUSB0".to_string();
        return Err(Error::Device { path, why });
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
        .open(&path)
        .map_err(|e| Error::Open { path: path.clone(), source: e })?;
    let fd = file.as_raw_fd();
    // SAFETY: isatty only asks what the descriptor is.
    if unsafe { libc::isatty(fd) } != 1 {
        let why = "a character device that is no terminal, so no serial line".to_string();
        return Err(Error::Device { path, why });
    }
    // SAFETY: an ioctl with no argument, on a descriptor that this function
    // holds. The request type differs between systems.
    if unsafe { libc::ioctl(fd, libc::TIOCEXCL as _) } == -1 {
        return Err(settings_failure(&path, "could not hold it alone"));
    }
    let speed = speed(spec.baud()).ok_or(Error::Baud { baud: spec.baud() })?;
    // SAFETY: an all-zero termios is valid for tcgetattr to fill.
    let mut saved: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: tcgetattr writes into the struct that this function owns.
    if unsafe { libc::tcgetattr(fd, &mut saved) } != 0 {
        return Err(settings_failure(&path, "could not read its settings"));
    }
    let wanted = match raw(saved, speed) {
        Ok(wanted) => wanted,
        Err(e) => {
            put_back(fd, &saved);
            return Err(Error::Settings { path, doing: "could not set the speed", source: e });
        }
    };
    // SAFETY: a termios that tcgetattr filled and this function changed.
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &wanted) } != 0 {
        put_back(fd, &saved);
        return Err(settings_failure(&path, "could not set 8 bits, no parity, 1 stop bit"));
    }
    if let Err(e) = blocking(fd) {
        put_back(fd, &saved);
        return Err(Error::Settings { path, doing: "could not clear O_NONBLOCK", source: e });
    }
    Ok(SerialLine { file, saved, path, baud: spec.baud(), restored: false })
}

/// An open line.
impl SerialLine {
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The speed that the address asked for.
    pub fn baud(&self) -> u32 {
        self.baud
    }

    /// Put the saved settings back now, and give the exclusive hold up. A
    /// second call does nothing. The drop of the line calls this.
    pub fn restore(&mut self) -> Result<(), Error> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;
        let fd = self.file.as_raw_fd();
        // SAFETY: settings that tcgetattr read from this descriptor, which
        // is open. TCSADRAIN sends what was written at its own speed first.
        if unsafe { libc::tcsetattr(fd, libc::TCSADRAIN, &self.saved) } != 0 {
            return Err(settings_failure(&self.path, "could not put the settings back"));
        }
        // SAFETY: an ioctl with no argument on a descriptor of this line.
        if unsafe { libc::ioctl(fd, libc::TIOCNXCL as _) } == -1 {
            return Err(settings_failure(&self.path, "could not give the line up"));
        }
        Ok(())
    }
}

impl Read for SerialLine {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl Write for SerialLine {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl Drop for SerialLine {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn settings_failure(path: &str, doing: &'static str) -> Error {
    Error::Settings { path: path.to_string(), doing, source: io::Error::last_os_error() }
}

fn put_back(fd: RawFd, saved: &libc::termios) {
    // SAFETY: settings that tcgetattr read from this descriptor.
    unsafe { libc::tcsetattr(fd, libc::TCSANOW, saved) };
}

/// Raw 8N1 at `speed`: no parity, 1 stop bit, no flow control, the modem
/// lines ignored, the receiver on, and a read that returns as soon as one
/// byte came.
fn raw(mut t: libc::termios, speed: libc::speed_t) -> io::Result<libc::termios> {
    // SAFETY: cfmakeraw and cfsetspeed write only into the struct passed.
    unsafe { libc::cfmakeraw(&mut t) };
    // SAFETY: as above.
    if unsafe { libc::cfsetspeed(&mut t, speed) } != 0 {
        return Err(io::Error::last_os_error());
    }
    t.c_cflag &= !(libc::CSIZE | libc::PARENB | libc::CSTOPB | libc::CRTSCTS);
    t.c_cflag |= libc::CS8 | libc::CLOCAL | libc::CREAD;
    t.c_iflag &= !(libc::IXON | libc::IXOFF | libc::IXANY);
    t.c_cc[libc::VMIN] = 1;
    t.c_cc[libc::VTIME] = 0;
    Ok(t)
}

/// Clear `O_NONBLOCK`, so that a read waits for a byte.
fn blocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl reads the flags of a descriptor that this function holds.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fcntl writes the flags of the same descriptor.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The kernel's value of a speed of the table: Linux has a code for each.
#[cfg(target_os = "linux")]
pub(crate) fn speed(baud: u32) -> Option<libc::speed_t> {
    Some(match baud {
        300 => libc::B300,
        600 => libc::B600,
        1200 => libc::B1200,
        1800 => libc::B1800,
        2400 => libc::B2400,
        4800 => libc::B4800,
        9600 => libc::B9600,
        19_200 => libc::B19200,
        38_400 => libc::B38400,
        57_600 => libc::B57600,
        115_200 => libc::B115200,
        230_400 => libc::B230400,
        460_800 => libc::B460800,
        921_600 => libc::B921600,
        1_000_000 => libc::B1000000,
        1_500_000 => libc::B1500000,
        2_000_000 => libc::B2000000,
        3_000_000 => libc::B3000000,
        4_000_000 => libc::B4000000,
        _ => return None,
    })
}

/// Another Unix takes the number itself; a device that cannot run at it
/// refuses it at `tcsetattr`, and the line says so.
#[cfg(not(target_os = "linux"))]
pub(crate) fn speed(baud: u32) -> Option<libc::speed_t> {
    Some(baud as libc::speed_t)
}

#[cfg(test)]
mod tests {
    use super::speed;

    #[test]
    fn each_speed_of_the_address_table_has_the_kernel_value() {
        for baud in crate::serial::BAUDS {
            assert!(speed(*baud).is_some(), "{baud}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_speed_that_the_kernel_does_not_name_has_no_value() {
        assert!(speed(12_345).is_none());
    }
}
