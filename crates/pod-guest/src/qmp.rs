//! The monitor of the emulator, in QMP, and the key names of a US layout.
//!
//! QMP, not HMP: HMP is a debug console of lines with no error report, and
//! a call that fails looks like a call that worked. In QMP each request is
//! JSON, each request gets a response, and a refusal is an `"error"`
//! member.
//!
//! No JSON dependency: the crate sends `qmp_capabilities`, `send-key` and
//! `quit`, with key names from a fixed table. The requests are formatted,
//! and a response is read for an `"error"` member. An error object that
//! [`response_ok`] does not find reads as success; so each request that the
//! crate sends is one whose refusal the guest also shows.
//!
//! A local user who connects to the socket can type into the console. The
//! per-run directory that holds it has the mode 0700
//! ([`crate::scratch_dir`]).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use pod_core::{Class, Failure, Stage};

use crate::{Result, failure};

/// How long one press is held, in milliseconds: long enough for a full
/// keystroke in the console of the guest, short enough to type a command
/// in about one second.
pub const HOLD_MS: u32 = 30;

/// One live QMP connection.
pub struct Monitor {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

/// An I/O failure of the QMP connection.
fn io(what: &str, e: std::io::Error) -> Failure {
    Failure::caused(Stage::Host, Class::of_io(e.kind()), format!("qmp {what}: {e}"), e)
}

impl Monitor {
    /// Connect to `path`, wait up to `wait` for the socket, and do the
    /// handshake of the capabilities.
    ///
    /// The emulator makes the socket when it starts, so a caller that just
    /// started it polls.
    pub fn connect(path: &Path, wait: Duration) -> Result<Monitor> {
        let start = Instant::now();
        loop {
            if let Ok(s) = UnixStream::connect(path) {
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
                let reader = BufReader::new(s.try_clone().map_err(|e| io("connect", e))?);
                let mut m = Monitor { writer: s, reader };
                // The greeting comes first with no request; read it before
                // a request, or the first response read is the greeting.
                m.read_line()?;
                m.call("qmp_capabilities")?;
                return Ok(m);
            }
            if start.elapsed() >= wait {
                return Err(failure(
                    Class::Timeout,
                    format!("the QMP socket {} never appeared within {}s", path.display(), wait.as_secs()),
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).map_err(|e| io("read", e))?;
        if n == 0 {
            return Err(failure(Class::Unavailable, "qmp closed the connection"));
        }
        Ok(line)
    }

    /// Send one request with no arguments, `{"execute":EXECUTE}`, and read
    /// its response.
    pub fn call(&mut self, execute: &str) -> Result<()> {
        self.send(&format!("{{\"execute\":\"{execute}\"}}"))
    }

    fn send(&mut self, body: &str) -> Result<()> {
        self.writer
            .write_all(body.as_bytes())
            .and_then(|()| self.writer.write_all(b"\n"))
            .and_then(|()| self.writer.flush())
            .map_err(|e| io("write", e))?;
        // An event is not a response. QMP can send `RESET`, `STOP`,
        // `POWERDOWN` and others at any time; an event read as the response
        // moves each later response to the wrong request. Skip the events.
        let line = loop {
            let line = self.read_line()?;
            if !is_event(&line) {
                break line;
            }
        };
        if response_ok(&line) {
            Ok(())
        } else {
            Err(failure(Class::Protocol, format!("qmp refused {body}: {}", line.trim())))
        }
    }

    /// Press `qcodes` together: a shifted character is the modifiers and
    /// the key in one `send-key`.
    ///
    /// `hold-time` is set: with no value, the emulator releases a key on its
    /// own timer, and the time that a key is down then depends on the time
    /// of the next request.
    pub fn keys(&mut self, qcodes: &[&str]) -> Result<()> {
        if qcodes.is_empty() {
            return Err(failure(Class::Usage, "no key to press"));
        }
        self.send(&send_key(qcodes))
    }

    /// Ask the emulator to exit. The response can be missing: the emulator
    /// can close the connection during `quit`, and that is not a failure.
    pub fn quit(&mut self) {
        let _ = self.writer.write_all(b"{\"execute\":\"quit\"}\n").and_then(|()| self.writer.flush());
        let _ = self.reader.read_line(&mut String::new());
    }
}

/// The `send-key` request for `qcodes`, pressed together.
pub fn send_key(qcodes: &[&str]) -> String {
    let keys: Vec<String> = qcodes.iter().map(|k| format!("{{\"type\":\"qcode\",\"data\":\"{k}\"}}")).collect();
    format!("{{\"execute\":\"send-key\",\"arguments\":{{\"keys\":[{}],\"hold-time\":{HOLD_MS}}}}}", keys.join(","))
}

/// True when a line is an event, not a response.
///
/// An event has `"event"`; a response has `"return"` or `"error"`. A line
/// with none of them is a response, as the greeting is.
pub fn is_event(line: &str) -> bool {
    line.contains("\"event\"") && !line.contains("\"return\"")
}

/// True when a QMP response line has no `"error"`.
///
/// A response that is neither is success, so each request of the crate is
/// one whose refusal the guest also shows.
pub fn response_ok(line: &str) -> bool {
    !line.contains("\"error\"")
}

/// The `send-key` name of a character, and true when it needs shift, for a
/// US layout. `None`: the character has no key.
pub fn key(c: char) -> Option<(&'static str, bool)> {
    Some(match c {
        'a'..='z' => (LETTER[usize::from(c as u8 - b'a')], false),
        'A'..='Z' => (LETTER[usize::from(c as u8 - b'A')], true),
        '0'..='9' => (DIGIT[usize::from(c as u8 - b'0')], false),
        ' ' => ("spc", false),
        '!' => ("1", true),
        '"' => ("apostrophe", true),
        '#' => ("3", true),
        '$' => ("4", true),
        '%' => ("5", true),
        '&' => ("7", true),
        '\'' => ("apostrophe", false),
        '(' => ("9", true),
        ')' => ("0", true),
        '*' => ("8", true),
        '+' => ("equal", true),
        ',' => ("comma", false),
        '-' => ("minus", false),
        '.' => ("dot", false),
        '/' => ("slash", false),
        ':' => ("semicolon", true),
        ';' => ("semicolon", false),
        '<' => ("comma", true),
        '=' => ("equal", false),
        '>' => ("dot", true),
        '?' => ("slash", true),
        '@' => ("2", true),
        '[' => ("bracket_left", false),
        '\\' => ("backslash", false),
        ']' => ("bracket_right", false),
        '^' => ("6", true),
        '_' => ("minus", true),
        '`' => ("grave_accent", false),
        '{' => ("bracket_left", true),
        '|' => ("backslash", true),
        '}' => ("bracket_right", true),
        '~' => ("grave_accent", true),
        _ => return None,
    })
}

const LETTER: [&str; 26] = [
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w",
    "x", "y", "z",
];
const DIGIT: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_response_is_not_ok_and_a_return_is() {
        assert!(response_ok("{\"return\": {}}"));
        assert!(response_ok("{\"QMP\": {\"version\": {}}}"));
        assert!(!response_ok("{\"error\": {\"class\": \"GenericError\", \"desc\": \"nope\"}}"));
        assert!(!response_ok("{\"error\": null}"));
    }

    #[test]
    fn an_event_is_skipped_and_not_read_as_a_response() {
        assert!(is_event("{\"event\": \"RESET\", \"data\": {}}"));
        assert!(is_event("{\"event\": \"STOP\"}"));
        assert!(!is_event("{\"return\": {}}"));
        assert!(!is_event("{\"error\": \"nope\"}"));
        // A response that names an event in a string is a response.
        assert!(!is_event("{\"return\": \"event\"}"));
    }

    #[test]
    fn the_key_request_presses_a_modifier_and_a_key_together() {
        // The shape that the emulator parses, with no JSON dependency.
        assert_eq!(
            send_key(&["shift", "semicolon"]),
            "{\"execute\":\"send-key\",\"arguments\":{\"keys\":[{\"type\":\"qcode\",\"data\":\"shift\"},{\"type\":\"qcode\",\"data\":\"semicolon\"}],\"hold-time\":30}}"
        );
    }

    /// Each printable ASCII character has a key, and a character outside
    /// the table has none.
    #[test]
    fn each_printable_ascii_character_has_a_key() {
        for c in ' '..='~' {
            assert!(key(c).is_some(), "no key for {c:?}");
        }
        assert_eq!(key('a'), Some(("a", false)));
        assert_eq!(key('A'), Some(("a", true)));
        assert_eq!(key(':'), Some(("semicolon", true)));
        assert_eq!(key('\t'), None);
        assert_eq!(key('é'), None);
    }
}
