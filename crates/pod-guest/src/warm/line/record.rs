//! The record beside a saved stream, and its check at a load.

use std::path::Path;

use pod_core::Class;

use super::Transport;
use crate::warm::Cold;
use crate::{Result, failure, io_at};

/// The first line of a record: its layout.
pub const RECORD: &str = "pod-guest state 1";

/// What a load needs beside the stream: how it was written, its length,
/// the caller's notes, and the fingerprint that it was saved under.
///
/// The text is [`RECORD`], then `transport WORD`, `bytes N`, one line
/// `note KEY VALUE` for each note, the line `fingerprint`, and the
/// fingerprint as it is to the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The transport that wrote the stream; a load takes the same one.
    pub transport: Transport,
    /// The length of the stream.
    pub bytes: u64,
    /// The caller's values, in order ([`super::Save::notes`]).
    pub notes: Vec<(String, String)>,
    /// The text of the fingerprint of the save.
    pub fingerprint: String,
}

impl Record {
    /// The text of the record.
    pub fn text(&self) -> String {
        let mut t = format!("{RECORD}\ntransport {}\nbytes {}\n", self.transport.word(), self.bytes);
        for (key, value) in &self.notes {
            t.push_str(&format!("note {key} {value}\n"));
        }
        t.push_str("fingerprint\n");
        t.push_str(&self.fingerprint);
        t
    }

    /// Read the text of a record; the error says what is wrong.
    pub fn parse(text: &str) -> std::result::Result<Record, String> {
        let mut rest = text.strip_prefix(RECORD).and_then(|r| r.strip_prefix('\n')).ok_or("not a record")?;
        let (mut transport, mut bytes, mut notes) = (None, None, Vec::new());
        loop {
            let (line, after) = rest.split_once('\n').ok_or("no fingerprint")?;
            rest = after;
            if line == "fingerprint" {
                break;
            }
            match line.split_once(' ') {
                Some(("transport", w)) => transport = Some(Transport::of(w).ok_or("an unknown transport")?),
                Some(("bytes", n)) => bytes = Some(n.parse::<u64>().map_err(|_| "a length that is not a number")?),
                Some(("note", kv)) => {
                    let (k, v) = kv.split_once(' ').ok_or("a note with no value")?;
                    notes.push((k.to_string(), v.to_string()));
                }
                _ => return Err(format!("an unknown line {line:?}")),
            }
        }
        Ok(Record {
            transport: transport.ok_or("no transport")?,
            bytes: bytes.ok_or("no length")?,
            notes,
            fingerprint: rest.to_string(),
        })
    }

    /// The value of the note `key`.
    pub fn note(&self, key: &str) -> Option<&str> {
        self.notes.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// Refuse a note that the text of a record cannot hold.
pub(super) fn check_notes(notes: &[(String, String)]) -> Result<()> {
    for (key, value) in notes {
        let word = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !word || value.contains(['\n', '\r']) {
            return Err(failure(
                Class::Usage,
                format!("the note {key:?} {value:?}: a key is one word, a value one line"),
            ));
        }
    }
    Ok(())
}

/// The record of the state in `stream` and `record`, where the state is
/// whole and was saved under `fingerprint` (the text of this run).
///
/// A failure with the cause [`Cold`]: [`Cold::Missing`] for no record, no
/// stream, a record that does not parse or a stream of another length;
/// [`Cold::Stale`] with the first line that differs for another layout or
/// another fingerprint. Another I/O error has no such cause.
pub fn check(stream: &Path, record: &Path, fingerprint: &str) -> Result<Record> {
    let bytes = match std::fs::read(record) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Cold::Missing.failure()),
        Err(e) => return Err(io_at(record, e)),
    };
    let text = String::from_utf8(bytes).map_err(|_| Cold::Missing.failure())?;
    let head = text.lines().next().unwrap_or_default();
    if head != RECORD {
        return Err(Cold::Stale { saved: head.to_string(), now: RECORD.to_string() }.failure());
    }
    let r = Record::parse(&text).map_err(|_| Cold::Missing.failure())?;
    if r.fingerprint != fingerprint {
        let (saved, now) = first_difference(&r.fingerprint, fingerprint);
        return Err(Cold::Stale { saved, now }.failure());
    }
    match std::fs::metadata(stream) {
        Ok(m) if m.is_file() && m.len() == r.bytes => Ok(r),
        Ok(_) => Err(Cold::Missing.failure()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Cold::Missing.failure()),
        Err(e) => Err(io_at(stream, e)),
    }
}

/// The first line of each text that differs from the other; two empty
/// lines where the texts are the same.
pub fn first_difference(saved: &str, now: &str) -> (String, String) {
    let mut a = saved.lines();
    let mut b = now.lines();
    loop {
        match (a.next(), b.next()) {
            (Some(x), Some(y)) if x == y => {}
            (None, None) => return (String::new(), String::new()),
            (x, y) => return (x.unwrap_or_default().to_string(), y.unwrap_or_default().to_string()),
        }
    }
}
