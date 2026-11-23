//! A serial line: its address, its frames and its window.
//!
//! * The address is `serial:PATH[,BAUD]`, with a device such as
//!   `/dev/ttyUSB0` and 115200 baud when the address gives none. The open
//!   is raw 8N1 with no flow control, `O_NOCTTY` and `TIOCEXCL`, and it
//!   saves the settings of the device and puts them back at each end
//!   (`open`, `SerialLine`).
//! * [`kiss`]: the AX.25 frames on the line, in KISS framing, with a
//!   maximum of 256 bytes and a named error above it.
//! * [`arq`]: a window of 1 to 7 frames with acknowledgements and a
//!   retransmission timeout that comes from the baud rate.
//!
//! The line here is point-to-point. The frames carry the control byte and
//! the sequence of AX.25, and no address field: two ends of one wire know
//! each other. The README names this limit.

pub mod arq;
pub mod kiss;

#[cfg(all(feature = "serial", unix))]
pub mod port;

#[cfg(all(feature = "serial", unix))]
pub use port::{SerialLine, open};

/// The first word of the address of a serial line.
pub const PREFIX: &str = "serial:";

/// The speed when the address gives none.
pub const DEFAULT_BAUD: u32 = 115_200;

/// The speeds that the kernel names on Linux. Another Unix takes the number
/// itself, and the device refuses a speed that it cannot run.
pub const BAUDS: &[u32] = &[
    300, 600, 1200, 1800, 2400, 4800, 9600, 19200, 38400, 57600, 115_200, 230_400, 460_800, 921_600, 1_000_000,
    1_500_000, 2_000_000, 3_000_000, 4_000_000,
];

/// The device and the speed of `serial:PATH[,BAUD]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialSpec {
    path: String,
    baud: u32,
}

impl SerialSpec {
    /// Read the address. A comma and a number after the path give the
    /// speed; without them the speed is [`DEFAULT_BAUD`]. A path is never
    /// empty, and it holds no white space and no control byte.
    pub fn parse(address: &str) -> Result<SerialSpec, crate::Error> {
        let rest = address
            .strip_prefix(PREFIX)
            .ok_or_else(|| crate::Error::Address { why: format!("{address}: an address starts with {PREFIX}") })?;
        let (path, baud) = match rest.rsplit_once(',') {
            Some((path, speed)) => {
                if speed.is_empty() {
                    return Err(crate::Error::Address { why: format!("{address}: no baud after the comma") });
                }
                if !speed.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(crate::Error::Address { why: format!("{address}: the baud is not a number") });
                }
                let baud: u32 = speed
                    .parse()
                    .map_err(|_| crate::Error::Address { why: format!("{address}: the baud is not a number") })?;
                (path, baud)
            }
            None => (rest, DEFAULT_BAUD),
        };
        if path.is_empty() {
            return Err(crate::Error::Address { why: format!("{address}: no path") });
        }
        if path.contains(',') {
            return Err(crate::Error::Address { why: format!("{address}: a path holds no comma") });
        }
        if path.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(crate::Error::Address { why: format!("{address}: the path holds a space or a control byte") });
        }
        if baud == 0 || !BAUDS.contains(&baud) {
            return Err(crate::Error::Baud { baud });
        }
        Ok(SerialSpec { path: path.to_string(), baud })
    }

    /// The address, as a person writes it.
    pub fn address(&self) -> String {
        format!("{PREFIX}{},{}", self.path, self.baud)
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn baud(&self) -> u32 {
        self.baud
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_alone_keeps_the_default_speed() {
        let spec = SerialSpec::parse("serial:/dev/ttyUSB0").unwrap();
        assert_eq!(spec.path(), "/dev/ttyUSB0");
        assert_eq!(spec.baud(), DEFAULT_BAUD);
        assert_eq!(spec.address(), "serial:/dev/ttyUSB0,115200");
    }

    #[test]
    fn the_comma_gives_the_speed() {
        let spec = SerialSpec::parse("serial:/dev/pts/7,1200").unwrap();
        assert_eq!((spec.path(), spec.baud()), ("/dev/pts/7", 1200));
    }

    #[test]
    fn a_bad_address_is_refused() {
        for bad in [
            "/dev/ttyUSB0",
            "serial:",
            "serial:/dev/x,",
            "serial:/dev/x,fast",
            "serial:/dev/x,1200,2400",
            "serial:/dev/x,12345",
            "serial:/dev/x,0",
            "serial:/dev/tty USB0,1200",
        ] {
            let failed = SerialSpec::parse(bad).err().unwrap_or_else(|| panic!("{bad} parsed"));
            assert!(matches!(failed, crate::Error::Address { .. } | crate::Error::Baud { .. }), "{bad}: {failed}");
        }
    }
}
