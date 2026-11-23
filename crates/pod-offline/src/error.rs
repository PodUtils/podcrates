//! The one failure type of the crate, with a stable name for each kind.
//!
//! A caller matches the variant. [`Error::name`] gives the same kind as one
//! word with hyphens, for a log line and a test. An I/O failure of a file
//! keeps its path and its cause.

use std::error::Error as StdError;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

/// A failure of a carrier or a store.
#[derive(Debug)]
pub enum Error {
    /// The text is not an address of this crate, or it has a bad part.
    Address { why: String },
    /// The baud rate is not one of the speeds of the host.
    Baud { baud: u32 },
    /// The device is not a character device that is a terminal.
    Device { path: String, why: String },
    /// The open of the device failed.
    Open { path: String, source: io::Error },
    /// A setting of the device failed or could not be read back.
    Settings { path: String, doing: &'static str, source: io::Error },
    /// A frame is above the maximum of its carrier. The crate never splits
    /// it in silence.
    FrameTooLarge { size: usize, max: usize },
    /// The bytes of a frame are not in the form of the link.
    Frame { why: String },
    /// A KISS escape byte that is not `TFEND` or `TFESC`.
    KissEscape { byte: u8 },
    /// The window is outside 1 to 7 frames.
    Window { window: u8 },
    /// An acknowledgement names a frame that is not in flight.
    Ack { seq: u8 },
    /// The modem or the line is not free. `retry_after` is the rest of the
    /// turnaround.
    Busy { retry_after: Duration },
    /// The modem is receiving. A half-duplex link does not transmit now.
    Receiving,
    /// The speed is outside the range of the modem.
    Speed { bps: u32 },
    /// The modem answered with a word that this driver does not know.
    Modem { why: String },
    /// A file could not be read or written.
    File { path: PathBuf, source: io::Error },
    /// The spool or its entry is not in its form.
    Spool { path: PathBuf, why: String },
    /// An entry of the same sequence and sender is in the spool already.
    SequenceTaken { sequence: u64, sender: String },
    /// An entry of the spool does not match its checksum or its header.
    EntryCorrupt { path: PathBuf, offset: u64, why: String },
    /// The TXT record of an announcement is not in its form.
    Txt { why: String },
    /// A manifest is not in its form or does not match its files.
    Manifest { why: String },
    /// A delta was made for another base bundle.
    WrongBase { expected: String, found: String },
    /// A file does not match the hash of the manifest.
    HashMismatch { path: PathBuf, expected: String, found: String },
    /// A capture record is not in its form.
    Capture { why: String },
    /// The code is not eight words of the list.
    Code { why: String },
    /// The code was used. A code is single-use.
    CodeUsed,
    /// The offer is past its expiry, or its window has closed.
    CodeExpired,
    /// A signature does not verify, or it is not over the expected words.
    Proof,
    /// The radio side of an announcement failed.
    Radio { why: String },
    /// The replication log is not in its form.
    Log { why: String },
}

impl Error {
    /// The kind of the failure as one stable word.
    pub fn name(&self) -> &'static str {
        match self {
            Error::Address { .. } => "address",
            Error::Baud { .. } => "baud",
            Error::Device { .. } => "device",
            Error::Open { .. } => "open",
            Error::Settings { .. } => "settings",
            Error::FrameTooLarge { .. } => "frame-too-large",
            Error::Frame { .. } => "frame",
            Error::KissEscape { .. } => "kiss-escape",
            Error::Window { .. } => "window",
            Error::Ack { .. } => "ack",
            Error::Busy { .. } => "busy",
            Error::Receiving => "receiving",
            Error::Speed { .. } => "speed",
            Error::Modem { .. } => "modem",
            Error::File { .. } => "file",
            Error::Spool { .. } => "spool",
            Error::SequenceTaken { .. } => "sequence-taken",
            Error::EntryCorrupt { .. } => "entry-corrupt",
            Error::Txt { .. } => "txt",
            Error::Manifest { .. } => "manifest",
            Error::WrongBase { .. } => "wrong-base",
            Error::HashMismatch { .. } => "hash-mismatch",
            Error::Capture { .. } => "capture",
            Error::Code { .. } => "code",
            Error::CodeUsed => "code-used",
            Error::CodeExpired => "code-expired",
            Error::Proof => "proof",
            Error::Radio { .. } => "radio",
            Error::Log { .. } => "log",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Address { why } => write!(f, "the address: {why}"),
            Error::Baud { baud } => write!(f, "the host has no speed of {baud} baud"),
            Error::Device { path, why } => write!(f, "{path}: {why}"),
            Error::Open { path, source } => write!(f, "{path}: the open failed: {source}"),
            Error::Settings { path, doing, source } => write!(f, "{path}: {doing}: {source}"),
            Error::FrameTooLarge { size, max } => {
                write!(f, "the frame is {size} bytes, above the carrier maximum of {max}")
            }
            Error::Frame { why } => write!(f, "the frame: {why}"),
            Error::KissEscape { byte } => write!(f, "a KISS escape byte {byte:#04x} is neither TFEND nor TFESC"),
            Error::Window { window } => write!(f, "the window is {window} frames, outside 1 to 7"),
            Error::Ack { seq } => write!(f, "the acknowledgement {seq} names no frame in flight"),
            Error::Busy { retry_after } => write!(f, "the line is busy for {retry_after:?} more"),
            Error::Receiving => f.write_str("the modem is receiving; a half-duplex link does not transmit now"),
            Error::Speed { bps } => write!(f, "the speed {bps} bit/s is outside 300 to 3000"),
            Error::Modem { why } => write!(f, "the modem: {why}"),
            Error::File { path, source } => write!(f, "{}: {source}", path.display()),
            Error::Spool { path, why } => write!(f, "the spool {}: {why}", path.display()),
            Error::SequenceTaken { sequence, sender } => {
                write!(f, "the spool has sequence {sequence} of the sender {sender} already")
            }
            Error::EntryCorrupt { path, offset, why } => {
                write!(f, "{}: offset {offset}: {why}", path.display())
            }
            Error::Txt { why } => write!(f, "the TXT record: {why}"),
            Error::Manifest { why } => write!(f, "the manifest: {why}"),
            Error::WrongBase { expected, found } => {
                write!(f, "the delta expects the base {expected}, and the bundle is {found}")
            }
            Error::HashMismatch { path, expected, found } => {
                write!(f, "{}: the hash is {found}, and the manifest says {expected}", path.display())
            }
            Error::Capture { why } => write!(f, "the capture: {why}"),
            Error::Code { why } => write!(f, "the code: {why}"),
            Error::CodeUsed => f.write_str("the code was used already"),
            Error::CodeExpired => f.write_str("the offer is past its expiry"),
            Error::Proof => f.write_str("the signature does not verify"),
            Error::Radio { why } => write!(f, "the radio: {why}"),
            Error::Log { why } => write!(f, "the replication log: {why}"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::Open { source, .. } | Error::Settings { source, .. } | Error::File { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl Error {
    /// A file failure with its path.
    pub fn file(path: impl Into<PathBuf>, source: io::Error) -> Error {
        Error::File { path: path.into(), source }
    }
}
