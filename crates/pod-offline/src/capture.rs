//! One record format for the bytes of a link, and a merge by time.
//!
//! A capture file starts with `pod-offline capture 1`. Each record is one
//! text line:
//!
//! ```text
//! <nanoseconds> <in|out> <0|1> <hex bytes>
//! ```
//!
//! The third field is the redaction marker. [`redact`] replaces each secret
//! of the caller with `<redacted>` before the line is written, so the file
//! never holds the secret. [`merge`] orders the records of several files by
//! the timestamp, then by the file's place in the list, then by the line:
//! the same inputs give the same order on each run.

use std::fmt;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Error;
use crate::hex;

/// The first line of a capture file.
pub const HEADER: &str = "pod-offline capture 1";
/// The bytes that replace a secret.
pub const MARKER: &[u8] = b"<redacted>";

/// The side of the link that gave the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
}

impl Direction {
    pub fn word(self) -> &'static str {
        match self {
            Direction::In => "in",
            Direction::Out => "out",
        }
    }

    fn parse(word: &str) -> Result<Direction, Error> {
        match word {
            "in" => Ok(Direction::In),
            "out" => Ok(Direction::Out),
            other => Err(Error::Capture { why: format!("the direction {other:?} is not in or out") }),
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// One record of a capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub timestamp: u64,
    pub direction: Direction,
    pub redacted: bool,
    pub bytes: Vec<u8>,
    /// The place of the source file in the merge, and its line.
    pub source: usize,
    pub seq: u64,
}

/// A capture file that is open for writing.
pub struct Capture {
    file: File,
    path: PathBuf,
    secrets: Vec<Vec<u8>>,
    seq: u64,
}

impl Capture {
    /// Make or replace `path`, with `secrets` that the records redact.
    pub fn create(path: impl Into<PathBuf>, secrets: Vec<Vec<u8>>) -> Result<Capture, Error> {
        let path = path.into();
        let mut file = File::create(&path).map_err(|e| Error::file(&path, e))?;
        file.write_all(format!("{HEADER}\n").as_bytes()).map_err(|e| Error::file(&path, e))?;
        Ok(Capture { file, path, secrets, seq: 0 })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write one record and give its redaction marker.
    pub fn record(&mut self, timestamp: u64, direction: Direction, bytes: &[u8]) -> Result<bool, Error> {
        self.write(timestamp, direction, bytes, false)
    }

    /// Write one record that a source file marked already.
    fn write(&mut self, timestamp: u64, direction: Direction, bytes: &[u8], was_marked: bool) -> Result<bool, Error> {
        let (bytes, found) = redact(bytes, &self.secrets);
        let redacted = was_marked || found;
        let line = format!("{timestamp} {} {} {}\n", direction.word(), u8::from(redacted), hex::encode(&bytes));
        self.file.write_all(line.as_bytes()).map_err(|e| Error::file(&self.path, e))?;
        self.file.flush().map_err(|e| Error::file(&self.path, e))?;
        self.seq += 1;
        Ok(redacted)
    }
}

/// Replace each secret of `secrets` with [`MARKER`]. It gives the bytes and
/// whether one secret was found.
pub fn redact(bytes: &[u8], secrets: &[Vec<u8>]) -> (Vec<u8>, bool) {
    let mut secrets: Vec<&[u8]> = secrets.iter().map(Vec::as_slice).filter(|s| !s.is_empty()).collect();
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let mut out = Vec::with_capacity(bytes.len());
    let mut marked = false;
    let mut at = 0;
    'outer: while at < bytes.len() {
        for secret in &secrets {
            if bytes[at..].starts_with(secret) {
                out.extend_from_slice(MARKER);
                at += secret.len();
                marked = true;
                continue 'outer;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    (out, marked)
}

/// The records of one capture file, in the order of their lines.
pub fn records(path: impl AsRef<Path>) -> Result<Vec<Record>, Error> {
    read(path.as_ref(), 0)
}

/// Write the records of each file of `paths` into `out`, ordered by the
/// timestamp, then by the place of the file, then by the line. The output
/// is redacted again with `secrets`.
pub fn merge(paths: &[PathBuf], out: impl Into<PathBuf>, secrets: Vec<Vec<u8>>) -> Result<Vec<Record>, Error> {
    let mut all = Vec::new();
    for (source, path) in paths.iter().enumerate() {
        all.extend(read(path, source)?);
    }
    all.sort_by(|a, b| {
        a.timestamp.cmp(&b.timestamp).then_with(|| a.source.cmp(&b.source)).then_with(|| a.seq.cmp(&b.seq))
    });
    let mut capture = Capture::create(out, secrets)?;
    for record in &mut all {
        let marked = record.redacted;
        record.redacted = capture.write(record.timestamp, record.direction, &record.bytes, marked)?;
    }
    Ok(all)
}

fn read(path: &Path, source: usize) -> Result<Vec<Record>, Error> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::file(path, e))?;
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(Error::Capture { why: format!("{}: no line {HEADER:?}", path.display()) });
    }
    let mut out = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(4, ' ');
        let timestamp = parts
            .next()
            .and_then(|text| text.parse::<u64>().ok())
            .ok_or_else(|| Error::Capture { why: format!("{}: a bad timestamp", path.display()) })?;
        let direction = parts
            .next()
            .ok_or_else(|| Error::Capture { why: format!("{}: no direction", path.display()) })
            .and_then(Direction::parse)?;
        let redacted = match parts.next() {
            Some("0") => false,
            Some("1") => true,
            other => return Err(Error::Capture { why: format!("{}: the marker {other:?}", path.display()) }),
        };
        let bytes = parts
            .next()
            .and_then(hex::decode)
            .ok_or_else(|| Error::Capture { why: format!("{}: the bytes are not hex", path.display()) })?;
        out.push(Record { timestamp, direction, redacted, bytes, source, seq: out.len() as u64 });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_goes_and_the_marker_stays() {
        let secrets = vec![b"hunter2".to_vec(), b"correct horse".to_vec()];
        let (bytes, marked) = redact(b"a hunter2 and correct horse b", &secrets);
        assert!(marked);
        assert_eq!(bytes, b"a <redacted> and <redacted> b");
        assert!(!bytes.windows(7).any(|w| w == b"hunter2"));
    }

    #[test]
    fn a_long_secret_wins_over_a_short_one() {
        let secrets = vec![b"abc".to_vec(), b"abcdef".to_vec()];
        let (bytes, marked) = redact(b"abcdef", &secrets);
        assert!(marked);
        assert_eq!(bytes, MARKER);
    }

    #[test]
    fn a_record_carries_its_direction_and_its_marker() {
        let dir = std::env::temp_dir().join(format!("pod-offline-capture-unit-{}", std::process::id()));
        let path = dir.join("one.cap");
        std::fs::create_dir_all(&dir).unwrap();
        let mut capture = Capture::create(&path, vec![b"secret".to_vec()]).unwrap();
        assert!(capture.record(5, Direction::Out, b"a secret b").unwrap());
        let read = records(&path).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].direction, Direction::Out);
        assert!(read[0].redacted);
        assert_eq!(read[0].bytes, b"a <redacted> b");
        let _ = std::fs::remove_dir_all(dir);
    }
}
