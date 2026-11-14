//! The JSON of the agent: each request, each answer, and the typed return
//! of each call. Pure functions with no I/O, on each target: the fuzz
//! target `qga_answer` reads the answers with them.
//!
//! A request is one line `{"execute":NAME,"arguments":{..}}`. An answer is
//! one line, an object with a `"return"` member or an `"error"` member
//! (`{"class":..,"desc":..}`). The bytes of a file and of the output of a
//! command are base64 (the standard alphabet, with padding).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value};

/// The byte that the agent writes before the answer of
/// `guest-sync-delimited`, and that the client writes to reset the parser
/// of the agent. It is never a byte of UTF-8 text, so no answer holds it.
pub const SENTINEL: u8 = 0xFF;

/// The line of one request, with no line end. `arguments` is left out when
/// it is `None`.
pub fn request(execute: &str, arguments: Option<Value>) -> String {
    let mut m = Map::new();
    m.insert("execute".into(), Value::String(execute.into()));
    if let Some(a) = arguments {
        m.insert("arguments".into(), a);
    }
    Value::Object(m).to_string()
}

/// One answer of the agent.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// The `"return"` member: the result of the call.
    Return(Value),
    /// The `"error"` member: the class of the error (`GenericError`,
    /// `CommandNotFound` and others) and its text.
    Error { class: String, desc: String },
}

/// Read one answer line. The line end and the spaces around the object do
/// not count. An answer that is not an object with `"return"` or a
/// well-formed `"error"` is an error with the reason.
pub fn answer(line: &[u8]) -> Result<Answer, String> {
    let text = std::str::from_utf8(line).map_err(|_| "an answer that is not UTF-8".to_string())?;
    let v: Value = serde_json::from_str(text.trim()).map_err(|e| format!("an answer that is not JSON: {e}"))?;
    let Value::Object(mut m) = v else {
        return Err("an answer that is not a JSON object".into());
    };
    // The error member first: an answer with both is a refusal.
    if let Some(e) = m.remove("error") {
        let class = e.get("class").and_then(Value::as_str);
        let desc = e.get("desc").and_then(Value::as_str);
        return match (class, desc) {
            (Some(class), Some(desc)) => Ok(Answer::Error { class: class.into(), desc: desc.into() }),
            _ => Err("an error member with no class or no desc string".into()),
        };
    }
    match m.remove("return") {
        Some(r) => Ok(Answer::Return(r)),
        None => Err("an answer with no return and no error member".into()),
    }
}

/// An integer member of an object, or the reason why not.
fn int(ret: &Value, key: &str) -> Result<i64, String> {
    ret.get(key).and_then(Value::as_i64).ok_or_else(|| format!("no integer {key:?} in the return"))
}

/// A boolean member of an object; `false` when it is not there.
fn flag(ret: &Value, key: &str) -> Result<bool, String> {
    match ret.get(key) {
        None => Ok(false),
        Some(v) => v.as_bool().ok_or_else(|| format!("{key:?} of the return is not a boolean")),
    }
}

/// The bytes of a base64 member; empty when it is not there.
fn bytes(ret: &Value, key: &str) -> Result<Vec<u8>, String> {
    match ret.get(key) {
        None => Ok(Vec::new()),
        Some(v) => {
            let s = v.as_str().ok_or_else(|| format!("{key:?} of the return is not a string"))?;
            STANDARD.decode(s).map_err(|e| format!("{key:?} of the return is not base64: {e}"))
        }
    }
}

/// The text of `bytes` in base64, as the agent reads it.
pub fn base64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

/// The id of a `guest-sync-delimited` return: the bare integer.
pub fn sync_id(ret: &Value) -> Result<u64, String> {
    ret.as_u64().ok_or_else(|| "the return of the sync is not an integer".into())
}

/// The pid of a `guest-exec` return, `{"pid": N}`.
pub fn exec_pid(ret: &Value) -> Result<i64, String> {
    int(ret, "pid")
}

/// The handle of a `guest-file-open` return: the bare integer.
pub fn file_handle(ret: &Value) -> Result<i64, String> {
    ret.as_i64().ok_or_else(|| "the return of the open is not an integer".into())
}

/// A command for `guest-exec`. The agent starts `path` with `args` and
/// `env` (each `NAME=VALUE`) and gives `input` on its stdin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exec {
    pub path: String,
    pub args: Vec<String>,
    pub env: Vec<String>,
    /// The bytes on the stdin of the command. `None`: no stdin.
    pub input: Option<Vec<u8>>,
    /// Keep stdout and stderr for [`ExecStatus`]. With no capture the
    /// agent drops them.
    pub capture_output: bool,
}

impl Exec {
    /// `path` with its output kept, no argument, no environment and no
    /// stdin.
    pub fn new(path: impl Into<String>) -> Exec {
        Exec { path: path.into(), capture_output: true, ..Exec::default() }
    }

    /// The arguments of `guest-exec`.
    pub fn arguments(&self) -> Value {
        let mut m = Map::new();
        m.insert("path".into(), Value::String(self.path.clone()));
        if !self.args.is_empty() {
            m.insert("arg".into(), self.args.iter().cloned().map(Value::String).collect());
        }
        if !self.env.is_empty() {
            m.insert("env".into(), self.env.iter().cloned().map(Value::String).collect());
        }
        if let Some(input) = &self.input {
            m.insert("input-data".into(), Value::String(base64(input)));
        }
        m.insert("capture-output".into(), Value::Bool(self.capture_output));
        Value::Object(m)
    }
}

/// The state of a command that `guest-exec` started.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecStatus {
    /// The command ended. The other fields mean nothing before.
    pub exited: bool,
    /// The exit code of a command that exited.
    pub exit_code: Option<i32>,
    /// The signal that ended the command (not on Windows).
    pub signal: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// The agent kept only the first part of the output.
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

/// An optional member that is an `i32`.
fn small(ret: &Value, key: &str) -> Result<Option<i32>, String> {
    match ret.get(key) {
        None => Ok(None),
        Some(v) => v
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| format!("{key:?} of the return is not a 32-bit integer")),
    }
}

impl ExecStatus {
    /// The status of a `guest-exec-status` return.
    pub fn from_return(ret: &Value) -> Result<ExecStatus, String> {
        if !ret.is_object() {
            return Err("the return of the status is not an object".into());
        }
        let exited = match ret.get("exited") {
            Some(v) => v.as_bool().ok_or("\"exited\" of the return is not a boolean")?,
            None => return Err("no \"exited\" in the return".into()),
        };
        Ok(ExecStatus {
            exited,
            exit_code: small(ret, "exitcode")?,
            signal: small(ret, "signal")?,
            stdout: bytes(ret, "out-data")?,
            stderr: bytes(ret, "err-data")?,
            stdout_truncated: flag(ret, "out-truncated")?,
            stderr_truncated: flag(ret, "err-truncated")?,
        })
    }
}

/// The bytes of one `guest-file-read`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chunk {
    pub data: Vec<u8>,
    /// The read reached the end of the file.
    pub eof: bool,
}

impl Chunk {
    /// The chunk of a `guest-file-read` return. Its `count` must be the
    /// length of its bytes.
    pub fn from_return(ret: &Value) -> Result<Chunk, String> {
        let count = int(ret, "count")?;
        let data = bytes(ret, "buf-b64")?;
        if u64::try_from(count).ok() != Some(data.len() as u64) {
            return Err(format!("the read says {count} bytes and holds {}", data.len()));
        }
        Ok(Chunk { data, eof: flag(ret, "eof")? })
    }
}

/// How many bytes one `guest-file-write` wrote.
pub fn written(ret: &Value) -> Result<u64, String> {
    let count = int(ret, "count")?;
    u64::try_from(count).map_err(|_| format!("the write says {count} bytes"))
}

/// The mode of `guest-file-open`, as `fopen(3)` takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    /// `r`
    Read,
    /// `w`: made, or cut to zero.
    Write,
    /// `a`
    Append,
    /// `r+`
    ReadWrite,
}

impl OpenMode {
    pub fn as_str(self) -> &'static str {
        match self {
            OpenMode::Read => "r",
            OpenMode::Write => "w",
            OpenMode::Append => "a",
            OpenMode::ReadWrite => "r+",
        }
    }
}

/// The mode of `guest-shutdown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shutdown {
    /// An ACPI power down: the guest stops its services first.
    Powerdown,
    Halt,
    Reboot,
}

impl Shutdown {
    pub fn as_str(self) -> &'static str {
        match self {
            Shutdown::Powerdown => "powerdown",
            Shutdown::Halt => "halt",
            Shutdown::Reboot => "reboot",
        }
    }
}

#[cfg(test)]
mod tests;
