//! The emulator's monitor, spoken as QMP. `TODO/milestones.md` T-1112.
//!
//! ⛔ **QMP rather than HMP, and the reason is that HMP is a debug console.**
//! The human monitor's `sendkey` works, and the first revision of this
//! driver used it, but its syntax is not an interface: it is line-oriented
//! text with no error reporting beyond a printed string, and a call that
//! fails looks exactly like a call that worked. QMP is the documented one:
//! requests are JSON, every request has a response, and a refusal is a
//! `"error"` member rather than a sentence on a console nobody reads.
//!
//! ⚠ **No JSON dependency is taken for this.** The driver sends three
//! requests ever, `qmp_capabilities`, `send-key` and `quit`, with key
//! names drawn from a fixed table, so the request bodies are built by
//! formatting and the responses are read for the presence of `"error"`.
//! A JSON parser would be a dependency bought to inspect three strings,
//! and the strings it would inspect come from a closed set this file
//! already owns. What that costs is stated: an error object that
//! `response_ok` did not recognise would read as success, which is why the
//! only requests sent are ones whose refusal would also be visible as the
//! guest not doing the thing.
//!
//! ⚠ **The monitor socket is a way into the guest.** A local user who can
//! connect to it can type into the console, so the per-run directory that
//! holds it is created mode 0700 and the socket is inside that directory.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

/// How long one press is held, in milliseconds: long enough that the
/// guest's console input sees a complete keystroke, short enough that the
/// whole command is typed in about a second.
pub const HOLD_MS: u32 = 30;

/// One live QMP connection.
pub struct Monitor {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Monitor {
    /// Connect to `path`, waiting up to `wait` for the socket to appear,
    /// and complete the capabilities handshake.
    ///
    /// ⚠ The emulator creates the socket when it starts, so a caller that
    /// has just spawned it polls rather than assuming: connecting to a
    /// socket that is not there yet is a race the emulator always wins.
    pub fn connect(path: &Path, wait: Duration) -> Result<Monitor, String> {
        let start = Instant::now();
        loop {
            if let Ok(s) = UnixStream::connect(path) {
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
                let reader = BufReader::new(s.try_clone().map_err(|e| e.to_string())?);
                let mut m = Monitor { writer: s, reader };
                // The greeting arrives unprompted; read it before asking
                // for anything, or the first response read is the greeting.
                m.read_line()?;
                m.call("qmp_capabilities")?;
                return Ok(m);
            }
            if start.elapsed() >= wait {
                return Err(format!(
                    "the QMP socket {} never appeared within {}s",
                    path.display(),
                    wait.as_secs()
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn read_line(&mut self) -> Result<String, String> {
        let mut line = String::new();
        let n = self
            .reader
            .read_line(&mut line)
            .map_err(|e| format!("qmp read: {e}"))?;
        if n == 0 {
            return Err("qmp closed the connection".to_string());
        }
        Ok(line)
    }

    /// Send one request and read its response. `body` is the JSON with the
    /// `execute` member; nothing else is sent from here.
    pub fn call(&mut self, execute: &str) -> Result<(), String> {
        self.send(&format!("{{\"execute\":\"{execute}\"}}"))
    }

    fn send(&mut self, body: &str) -> Result<(), String> {
        self.writer
            .write_all(body.as_bytes())
            .and_then(|()| self.writer.write_all(b"\n"))
            .and_then(|()| self.writer.flush())
            .map_err(|e| format!("qmp write: {e}"))?;
        // ⛔ **An event is not a response.** QMP may emit `RESET`, `STOP`,
        // `POWERDOWN` and friends at any moment, and reading one as the
        // answer to the request just sent desynchronises the connection:
        // that request's real response is then read as the *next* request's,
        // and a driver typing a command into a console presses keys against
        // a reply that belongs to something else. Events are skipped until
        // the response arrives.
        let line = loop {
            let line = self.read_line()?;
            if !is_event(&line) {
                break line;
            }
        };
        if response_ok(&line) {
            Ok(())
        } else {
            Err(format!("qmp refused {body}: {}", line.trim()))
        }
    }

    /// Press `qcodes` together, which is how a shifted character is
    /// spelled: the modifiers and the key in one `send-key`.
    ///
    /// ⚠ **`hold-time` is set explicitly.** Without it the emulator holds
    /// each press for its own default and releases it on a timer, so how long
    /// a key is down depends on when the *next* request arrives. That is the
    /// one thing a driver typing a command into a console cannot have: the
    /// release has to be a property of the request, not of the pacing.
    pub fn keys(&mut self, qcodes: &[&str]) -> Result<(), String> {
        if qcodes.is_empty() {
            return Err("no key to press".to_string());
        }
        let keys: Vec<String> = qcodes
            .iter()
            .map(|k| format!("{{\"type\":\"qcode\",\"data\":\"{k}\"}}"))
            .collect();
        self.send(&format!(
            "{{\"execute\":\"send-key\",\"arguments\":{{\"keys\":[{}],\"hold-time\":{HOLD_MS}}}}}",
            keys.join(",")
        ))
    }

    /// Ask the emulator to exit. The response may not arrive: the emulator
    /// is allowed to close the connection while servicing `quit`, so a
    /// read failure here is not a failure of the request.
    pub fn quit(&mut self) {
        let _ = self
            .writer
            .write_all(b"{\"execute\":\"quit\"}\n")
            .and_then(|()| self.writer.flush());
        let _ = self.reader.read_line(&mut String::new());
    }
}

/// True where a line is an asynchronous event rather than a response.
///
/// ⚠ Matched on the member rather than parsed. An event carries `"event"`
/// and a response carries `"return"` or `"error"`, so the three are
/// distinguishable without a parser; a line with none of them is treated as
/// a response, which is what the greeting already relies on.
pub fn is_event(line: &str) -> bool {
    line.contains("\"event\"") && !line.contains("\"return\"")
}

/// True where a QMP response line carries no `"error"`.
///
/// ⛔ A response that is neither is treated as success, so every caller
/// states what it would do about a refusal twice: once here and once in
/// the guest's own behaviour. The requests this driver sends are all of
/// that kind, which is why there is no parser.
pub fn response_ok(line: &str) -> bool {
    !line.contains("\"error\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_response_is_not_ok_and_a_return_is() {
        assert!(response_ok("{\"return\": {}}"));
        assert!(response_ok("{\"QMP\": {\"version\": {}}}"));
        assert!(!response_ok(
            "{\"error\": {\"class\": \"GenericError\", \"desc\": \"nope\"}}"
        ));
        assert!(!response_ok("{\"error\": null}"));
    }

    #[test]
    fn an_event_is_skipped_and_not_read_as_a_response() {
        assert!(is_event("{\"event\": \"RESET\", \"data\": {}}"));
        assert!(is_event("{\"event\": \"STOP\"}"));
        assert!(!is_event("{\"return\": {}}"));
        assert!(!is_event("{\"error\": \"nope\"}"));
        // a payload that merely mentions an event in a string is a response
        assert!(!is_event("{\"return\": \"event\"}"));
    }

    #[test]
    fn the_key_request_presses_a_modifier_and_a_key_together() {
        // The shape the emulator parses, built without a JSON dependency.
        let keys = ["shift", "semicolon"];
        let bodies: Vec<String> = keys
            .iter()
            .map(|k| format!("{{\"type\":\"qcode\",\"data\":\"{k}\"}}"))
            .collect();
        let body = format!(
            "{{\"execute\":\"send-key\",\"arguments\":{{\"keys\":[{}],\"hold-time\":{HOLD_MS}}}}}",
            bodies.join(",")
        );
        assert_eq!(
            body,
            "{\"execute\":\"send-key\",\"arguments\":{\"keys\":[{\"type\":\"qcode\",\"data\":\"shift\"},{\"type\":\"qcode\",\"data\":\"semicolon\"}],\"hold-time\":30}}"
        );
    }
}
