//! The failures of the reader. Each names the place in the image (a byte
//! offset) or the path, so that a caller can report a bad medium.

use std::fmt;
use std::io;

/// What is wrong with the tag of a descriptor (ECMA-167 3/7.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagFault {
    /// The descriptor ends inside its tag.
    Short,
    /// The tag identifier is not the one that the place holds.
    Identifier { want: u16, got: u16 },
    /// The checksum of the 16 bytes of the tag does not match.
    Checksum,
    /// The descriptor version is not 2 or 3.
    Version(u16),
    /// The tag location is not the block that the descriptor is in.
    Location { want: u32, got: u32 },
    /// The CRC length goes past the descriptor.
    CrcLength(u16),
    /// The CRC of the descriptor does not match.
    Crc,
}

/// A failure of the reader.
#[derive(Debug)]
pub enum Error {
    /// The [`crate::ReadAt`] of the caller failed.
    Read { offset: u64, len: usize, source: io::Error },
    /// No anchor volume descriptor pointer at block 256, with a block of
    /// 2048, 512 or 4096 bytes: the image has no UDF.
    NotUdf,
    /// A descriptor whose tag is wrong: its byte offset, what it is, and
    /// the fault.
    Descriptor { offset: u64, what: &'static str, fault: TagFault },
    /// A descriptor whose fields do not agree: its byte offset and what.
    Corrupt { offset: u64, what: &'static str },
    /// A valid structure that this reader does not read.
    Unsupported { what: &'static str, value: String },
    /// A descriptor that the volume must have and does not.
    Missing { what: &'static str },
    /// No entry of this path.
    NotFound { path: String },
    /// A component of the path is not a directory.
    NotADirectory { path: String },
    /// The path names a directory or another kind of file, with its file
    /// type (ECMA-167 4/14.6.6).
    NotAFile { path: String, file_type: u8 },
    /// A path that the reader refuses (`..`, or no component).
    Path { path: String, reason: &'static str },
    /// A bound of [`crate::Limits`], or of a read that the caller bounds.
    Limit { what: &'static str, limit: u64 },
}

impl fmt::Display for TagFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TagFault::Short => f.write_str("the descriptor ends inside its tag"),
            TagFault::Identifier { want, got } => write!(f, "tag identifier {got}, not {want}"),
            TagFault::Checksum => f.write_str("the tag checksum does not match"),
            TagFault::Version(v) => write!(f, "descriptor version {v}"),
            TagFault::Location { want, got } => write!(f, "tag location {got}, not {want}"),
            TagFault::CrcLength(n) => write!(f, "a CRC length of {n} bytes past the descriptor"),
            TagFault::Crc => f.write_str("the descriptor CRC does not match"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Read { offset, len, source } => write!(f, "udf: read of {len} bytes at {offset}: {source}"),
            Error::NotUdf => f.write_str("udf: no anchor volume descriptor pointer at block 256"),
            Error::Descriptor { offset, what, fault } => write!(f, "udf: the {what} at byte {offset}: {fault}"),
            Error::Corrupt { offset, what } => write!(f, "udf: at byte {offset}: {what}"),
            Error::Unsupported { what, value } => write!(f, "udf: {what} {value} is not supported"),
            Error::Missing { what } => write!(f, "udf: the volume has no {what}"),
            Error::NotFound { path } => write!(f, "udf: {path}: not found"),
            Error::NotADirectory { path } => write!(f, "udf: {path}: not a directory"),
            Error::NotAFile { path, file_type } => write!(f, "udf: {path}: file type {file_type}, not a file"),
            Error::Path { path, reason } => write!(f, "udf: the path {path:?}: {reason}"),
            Error::Limit { what, limit } => write!(f, "udf: more than {limit} {what}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Read { source, .. } => Some(source),
            _ => None,
        }
    }
}
