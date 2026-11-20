//! The monitor of the emulator, in QMP, and the key names of a US layout.
//!
//! QMP, not HMP: HMP is a debug console of lines with no error report, and
//! a call that fails looks like a call that worked. In QMP each request is
//! JSON, each request gets a response, and a refusal is an `"error"`
//! member.
//!
//! No JSON dependency: the crate sends `qmp_capabilities`, `send-key`,
//! `screendump` and `quit`, with key names from a fixed table and a path
//! in a JSON string ([`json_text`]), for a warm start ([`crate::warm`])
//! `stop`, `cont`, `migrate` and the queries of the status, and the change
//! of a medium ([`crate::swap`]). The requests are formatted. A response is
//! read for an `"error"` member, for the words of a status
//! ([`json_member`]), and for the lock of a drive in an entry of
//! `query-block` ([`list_entries`], [`json_bool`]). An error object that
//! [`response_ok`] does not find reads as success; so each request that the
//! crate sends is one whose refusal the guest also shows.
//!
//! [`Monitor`] gives each request an `id`, and takes the answer of a
//! request by it (or by the order of the requests, from a monitor that
//! gives back no `id`). The caller sets the bound of one answer
//! ([`Monitor::set_answer_wait`], [`Monitor::ask_within`]); an answer
//! that comes after its bound is read by [`Monitor::late`], and never as
//! the answer of a later request. Each event is kept for
//! [`Monitor::take_events`]. QEMU answers no request while it holds its
//! lock, for example for the last part of a migration.
//!
//! A local user who connects to the socket can type into the console. The
//! per-run directory that holds it has the mode 0700
//! ([`crate::scratch_dir`]).

use std::path::Path;

use pod_core::Class;

use crate::{Result, failure};

mod monitor;

pub use monitor::{ANSWER_WAIT, EVENTS_KEPT, Monitor};

/// How long one press is held, in milliseconds: long enough for a full
/// keystroke in the console of the guest, short enough to type a command
/// in about one second.
pub const HOLD_MS: u32 = 30;

/// The requests of the console of the guest.
impl Monitor {
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

    /// Write the screen of the guest to `file` (`screendump`, a PPM image).
    /// A path that is not UTF-8 is a [`Class::Usage`] failure: JSON holds
    /// text.
    pub fn screendump(&mut self, file: &Path) -> Result<()> {
        let path =
            file.to_str().ok_or_else(|| failure(Class::Usage, format!("{}: not a UTF-8 path", file.display())))?;
        self.send(&format!("{{\"execute\":\"screendump\",\"arguments\":{{\"filename\":{}}}}}", json_text(path)))
    }
}

/// The `send-key` request for `qcodes`, pressed together.
pub fn send_key(qcodes: &[&str]) -> String {
    let keys: Vec<String> = qcodes.iter().map(|k| format!("{{\"type\":\"qcode\",\"data\":\"{k}\"}}")).collect();
    format!("{{\"execute\":\"send-key\",\"arguments\":{{\"keys\":[{}],\"hold-time\":{HOLD_MS}}}}}", keys.join(","))
}

/// `text` as a JSON string, with its quotes.
pub fn json_text(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The text of the first member `key` of a response line whose value is a
/// string, with no escape in it; `None` when there is no such member.
///
/// Enough for the words that the crate reads (`"status": "inmigrate"`):
/// the emulator writes them with no escape.
pub fn json_member(line: &str, key: &str) -> Option<String> {
    let name = format!("\"{key}\"");
    let mut rest = line;
    while let Some(at) = rest.find(&name) {
        rest = &rest[at + name.len()..];
        let Some(value) = rest.trim_start().strip_prefix(':') else { continue };
        let Some(value) = value.trim_start().strip_prefix('"') else { continue };
        let end = value.find(['"', '\\'])?;
        return (value.as_bytes()[end] == b'"').then(|| value[..end].to_string());
    }
    None
}

/// The value of the first member `key` of `line` that is `true` or
/// `false`; `None` when there is no such member.
pub fn json_bool(line: &str, key: &str) -> Option<bool> {
    let name = format!("\"{key}\"");
    let mut rest = line;
    while let Some(at) = rest.find(&name) {
        rest = &rest[at + name.len()..];
        let Some(value) = rest.trim_start().strip_prefix(':') else { continue };
        let value = value.trim_start();
        if value.starts_with("true") {
            return Some(true);
        }
        if value.starts_with("false") {
            return Some(false);
        }
    }
    None
}

/// The objects in the list of a response (`{"return": [{..}, {..}]}`),
/// each as its text.
///
/// The emulator writes the members of an object in no fixed order, so a
/// caller reads the members of one entry in the text of that entry only.
pub fn list_entries(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    let (mut text, mut escape) = (false, false);
    for (i, c) in line.char_indices() {
        if text {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                text = false;
            }
            continue;
        }
        match c {
            '"' => text = true,
            '{' | '[' => {
                if c == '{' && depth == 2 {
                    start = i;
                }
                depth += 1;
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                if c == '}' && depth == 2 {
                    out.push(&line[start..=i]);
                }
            }
            _ => {}
        }
    }
    out
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
mod tests;
