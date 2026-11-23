//! A spool of files: store and forward for a peer that comes later.
//!
//! A spool is a directory. Each entry is one file that a person can read
//! and repair with a text editor:
//!
//! ```text
//! pod-offline spool entry 1
//! sequence: 42
//! sender: <64 hex digits>
//! length: 5
//! checksum: <64 hex digits, the SHA-256 of the body>
//!
//! <body>
//! ```
//!
//! The name of the file is `<sequence>-<sender>.entry`. [`Spool::receive`]
//! verifies the bytes and gives the entry; the file stays until the caller
//! calls [`Spool::consume`]. A process that dies between the two loses
//! nothing: the entry is on the disk until the receiver takes it.
//!
//! [`Spool::check`] reports each entry that does not verify, with its file
//! and its offset, and the receiver never takes one of them.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use pod_identity::{Identity, PublicKey};

use crate::hex;

const HEADER: &str = "pod-offline spool entry 1\n";
const EXTENSION: &str = "entry";

/// One entry of the spool, verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub sequence: u64,
    pub sender: PublicKey,
    pub digest: [u8; 32],
    pub body: Vec<u8>,
    file: PathBuf,
}

impl Entry {
    /// The file of the entry.
    pub fn path(&self) -> &Path {
        &self.file
    }
}

/// An entry that does not verify, with the place of the fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub file: PathBuf,
    pub offset: u64,
    pub why: String,
}

/// A directory of entries.
#[derive(Debug, Clone)]
pub struct Spool {
    dir: PathBuf,
}

impl Spool {
    /// Take `dir` as a spool, and make it when it does not exist.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Spool, crate::Error> {
        let dir = dir.into();
        fs::create_dir_all(&dir).map_err(|e| crate::Error::file(&dir, e))?;
        Ok(Spool { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Write one entry for `sequence` from `sender`. The body may be empty.
    /// The file appears whole or not at all.
    pub fn put(&self, sender: &Identity, sequence: u64, body: &[u8]) -> Result<Entry, crate::Error> {
        let public = sender.public();
        let digest = hex::sha256(body);
        let mut bytes = Vec::with_capacity(body.len() + 256);
        bytes.extend_from_slice(HEADER.as_bytes());
        bytes.extend_from_slice(format!("sequence: {sequence}\n").as_bytes());
        bytes.extend_from_slice(format!("sender: {}\n", public.hex()).as_bytes());
        bytes.extend_from_slice(format!("length: {}\n", body.len()).as_bytes());
        bytes.extend_from_slice(format!("checksum: {}\n\n", hex::encode(&digest)).as_bytes());
        bytes.extend_from_slice(body);
        let file = self.dir.join(format!("{sequence:020}-{}.{EXTENSION}", public.hex()));
        let mut handle =
            fs::OpenOptions::new().write(true).create_new(true).open(&file).map_err(|e| match e.kind() {
                std::io::ErrorKind::AlreadyExists => crate::Error::SequenceTaken { sequence, sender: public.hex() },
                _ => crate::Error::file(&file, e),
            })?;
        if let Err(e) = handle.write_all(&bytes).and_then(|()| handle.sync_all()) {
            let _ = fs::remove_file(&file);
            return Err(crate::Error::file(&file, e));
        }
        Ok(Entry { sequence, sender: public, digest, body: body.to_vec(), file })
    }

    /// Each entry that verifies, in the order of its sequence and its file
    /// name. An entry that does not verify is left for [`Spool::check`].
    pub fn entries(&self) -> Result<Vec<Entry>, crate::Error> {
        let mut files = self.files()?;
        files.sort();
        let mut out = Vec::new();
        for file in files {
            if let Ok(entry) = self.read(&file) {
                out.push(entry);
            }
        }
        out.sort_by(|a, b| a.sequence.cmp(&b.sequence).then_with(|| a.file.cmp(&b.file)));
        Ok(out)
    }

    /// The first entry that verifies, and the one with the lowest sequence.
    /// The entry stays on the disk until [`Spool::consume`].
    pub fn receive(&self) -> Result<Option<Entry>, crate::Error> {
        Ok(self.entries()?.into_iter().next())
    }

    /// Remove the file of `entry`, after the receiver took it.
    pub fn consume(&self, entry: &Entry) -> Result<(), crate::Error> {
        fs::remove_file(&entry.file).map_err(|e| crate::Error::file(&entry.file, e))
    }

    /// Each entry that does not verify, with its file and the offset of the
    /// part that is wrong: the header at its line, the body at its first
    /// byte.
    pub fn check(&self) -> Result<Vec<Fault>, crate::Error> {
        let mut files = self.files()?;
        files.sort();
        let mut faults = Vec::new();
        for file in files {
            let bytes = match fs::read(&file) {
                Ok(bytes) => bytes,
                Err(e) => {
                    faults.push(Fault { file, offset: 0, why: e.to_string() });
                    continue;
                }
            };
            if let Err(fault) = parse(&file, &bytes) {
                match fault {
                    crate::Error::EntryCorrupt { path, offset, why } => faults.push(Fault { file: path, offset, why }),
                    other => faults.push(Fault { file, offset: 0, why: other.to_string() }),
                }
            }
        }
        Ok(faults)
    }

    fn read(&self, file: &Path) -> Result<Entry, crate::Error> {
        let bytes = fs::read(file).map_err(|e| crate::Error::file(file, e))?;
        parse(file, &bytes)
    }

    fn files(&self) -> Result<Vec<PathBuf>, crate::Error> {
        let mut out = Vec::new();
        let listing = fs::read_dir(&self.dir).map_err(|e| crate::Error::file(&self.dir, e))?;
        for entry in listing {
            let entry = entry.map_err(|e| crate::Error::file(&self.dir, e))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some(EXTENSION) {
                out.push(path);
            }
        }
        Ok(out)
    }
}

fn corrupt(file: &Path, offset: usize, why: impl Into<String>) -> crate::Error {
    crate::Error::EntryCorrupt { path: file.to_path_buf(), offset: offset as u64, why: why.into() }
}

/// Read one entry. The body starts after the first empty line, and its
/// bytes need not be text.
fn parse(file: &Path, bytes: &[u8]) -> Result<Entry, crate::Error> {
    if !bytes.starts_with(HEADER.as_bytes()) {
        return Err(corrupt(file, 0, format!("no line {HEADER:?}")));
    }
    let rel = bytes[HEADER.len()..]
        .windows(2)
        .position(|window| window == b"\n\n")
        .ok_or_else(|| corrupt(file, HEADER.len(), "no empty line after the header"))?;
    let body_start = HEADER.len() + rel + 2;
    let header =
        std::str::from_utf8(&bytes[..body_start]).map_err(|_| corrupt(file, 0, "the header is not UTF-8 text"))?;
    let mut sequence = None;
    let mut sender = None;
    let mut length = None;
    let mut checksum = None;
    let mut at = HEADER.len();
    for line in header[HEADER.len()..].split_inclusive('\n') {
        if line == "\n" {
            break;
        }
        let line_at = at;
        at += line.len();
        let Some((key, value)) = line.trim_end().split_once(':') else {
            return Err(corrupt(file, line_at, "a header line has no colon"));
        };
        let value = value.trim();
        match key {
            "sequence" => sequence = Some(value.parse::<u64>().map_err(|_| corrupt(file, line_at, "a bad sequence"))?),
            "sender" => {
                sender = Some(PublicKey::parse(value).ok_or_else(|| corrupt(file, line_at, "a bad sender key"))?);
            }
            "length" => length = Some(value.parse::<usize>().map_err(|_| corrupt(file, line_at, "a bad length"))?),
            "checksum" => {
                let raw = hex::decode(value).ok_or_else(|| corrupt(file, line_at, "a bad checksum"))?;
                let digest: [u8; 32] = raw.try_into().map_err(|_| corrupt(file, line_at, "a checksum is 32 bytes"))?;
                checksum = Some(digest);
            }
            other => return Err(corrupt(file, line_at, format!("a header line names {other:?}"))),
        }
    }
    let (Some(sequence), Some(sender), Some(length), Some(checksum)) = (sequence, sender, length, checksum) else {
        return Err(corrupt(file, body_start, "a header field is missing"));
    };
    let body = &bytes[body_start..];
    if body.len() != length {
        let why = format!("the body is {} bytes, and the header says {length}", body.len());
        return Err(corrupt(file, body_start, why));
    }
    let digest = hex::sha256(body);
    if digest != checksum {
        return Err(corrupt(file, body_start, "the body does not match its checksum"));
    }
    Ok(Entry { sequence, sender, digest, body: body.to_vec(), file: file.to_path_buf() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pod-offline-spool-unit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn an_entry_round_trips_with_its_checksum_and_its_sender() {
        let dir = scratch("round");
        let spool = Spool::open(&dir).unwrap();
        let sender = Identity::from_seed(&[7; 32]);
        let entry = spool.put(&sender, 4, b"hello").unwrap();
        assert_eq!(entry.sender, sender.public());
        assert_eq!(entry.sequence, 4);
        let read = spool.receive().unwrap().unwrap();
        assert_eq!(read, entry);
        spool.consume(&read).unwrap();
        assert!(spool.receive().unwrap().is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_body_that_changed_is_a_fault_at_the_body() {
        let dir = scratch("corrupt");
        let spool = Spool::open(&dir).unwrap();
        let sender = Identity::from_seed(&[8; 32]);
        let entry = spool.put(&sender, 1, b"abcdef").unwrap();
        let mut bytes = fs::read(entry.path()).unwrap();
        let body_start = bytes.len() - 6;
        bytes[body_start + 2] ^= 0xff;
        fs::write(entry.path(), &bytes).unwrap();
        let faults = spool.check().unwrap();
        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0].file, entry.path());
        assert_eq!(faults[0].offset, body_start as u64);
        assert!(spool.receive().unwrap().is_none());
        let _ = fs::remove_dir_all(dir);
    }
}
