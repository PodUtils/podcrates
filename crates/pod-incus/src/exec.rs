//! A command in an instance (`POST /1.0/instances/NAME/exec`,
//! `shared/api/instance_exec.go`). With `record-output`, incus writes
//! stdout and stderr to two logs of the instance, and the final operation
//! gives their paths in `metadata.output` (`1`, `2`) and the exit code in
//! `metadata.return`. The crate reads each log (`GET
//! /1.0/instances/NAME/logs/exec-output/FILE`) and deletes it, as the client
//! of incus does. The command with its websockets is [`crate::attach`].
//!
//! incusd 6.0 ends an exec whose command ends with 127 or 126 as a failure
//! ("Command not found", "Command not executable"), and its wait answers an
//! error, not the operation (PodUtils/podvm#653). The crate then reads the
//! operation and takes its `return` as the exit code ([`Client::exec_end`]).

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::client::Client;
use crate::error::{Error, ErrorKind, Result};
use crate::operation::Operation;
use crate::wire::{Body, Request, escape};

/// A command to run in an instance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exec {
    /// The program and its arguments.
    pub command: Vec<String>,
    /// Variables over those of the instance; incus sets `PATH`, `HOME`,
    /// `USER` and `LANG` when they are not given.
    pub environment: BTreeMap<String, String>,
    /// The user and the group of the command; 0 is root.
    pub user: u32,
    pub group: u32,
    /// The work directory; empty: the default of incus.
    pub cwd: String,
}

impl Exec {
    /// `command` as root, with no extra variable.
    pub fn new<S: Into<String>>(command: impl IntoIterator<Item = S>) -> Exec {
        Exec { command: command.into_iter().map(Into::into).collect(), ..Exec::default() }
    }
}

/// The output of a command whose output incus recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    /// `metadata.return`: the exit code; 129 when a VM disconnected.
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// The body of the post (`InstanceExecPost`).
#[derive(Serialize)]
pub(crate) struct ExecPost<'a> {
    command: &'a [String],
    #[serde(rename = "wait-for-websocket")]
    wait_for_websocket: bool,
    interactive: bool,
    environment: &'a BTreeMap<String, String>,
    width: u32,
    height: u32,
    #[serde(rename = "record-output")]
    record_output: bool,
    user: u32,
    group: u32,
    cwd: &'a str,
}

impl<'a> ExecPost<'a> {
    pub(crate) fn of(exec: &'a Exec, record_output: bool) -> ExecPost<'a> {
        ExecPost {
            command: &exec.command,
            wait_for_websocket: !record_output,
            interactive: false,
            environment: &exec.environment,
            width: 0,
            height: 0,
            record_output,
            user: exec.user,
            group: exec.group,
            cwd: &exec.cwd,
        }
    }
}

/// The words of incusd for an exec whose command ended with 127 and 126
/// (`ErrExecCommandNotFound`, `ErrExecCommandNotExecutable`), with the code.
const FAILED_ENDS: [(&str, i32); 2] = [("Command not found", 127), ("Command not executable", 126)];

pub(crate) fn exec_path(name: &str) -> String {
    format!("/1.0/instances/{}/exec", escape(name))
}

/// The exit code of a final operation of exec.
pub(crate) fn exit_code(call: &str, operation: &Operation) -> Result<i32> {
    operation
        .metadata
        .get("return")
        .and_then(Value::as_i64)
        .and_then(|code| i32::try_from(code).ok())
        .ok_or_else(|| Error::protocol(call, format!("the operation {} gives no exit code", operation.id)))
}

impl Client {
    /// Run `exec` in the instance `name` with `record-output`, wait for its
    /// end, and read and delete its two logs.
    pub async fn exec_recorded(&self, name: &str, exec: &Exec) -> Result<ExecOutput> {
        let path = exec_path(name);
        let call = format!("POST {path}");
        if exec.command.is_empty() {
            return Err(Error::usage(&call, "a command needs a program"));
        }
        let request = self.json_request(&call, "POST", &path, &ExecPost::of(exec, true))?;
        let started = self.begin(&call, request).await?;
        let (exit_code, done) = self.exec_end(&call, &started.id).await?;
        // incusd dropped the operation, and with it the paths of its logs.
        let Some(done) = done else { return Ok(ExecOutput { exit_code, stdout: Vec::new(), stderr: Vec::new() }) };
        let output = done.texts("output");
        let logs = format!("/1.0/instances/{}/logs/exec-output/", escape(name));
        let mut streams = [Vec::new(), Vec::new()];
        for (fd, stream) in ["1", "2"].into_iter().zip(streams.iter_mut()) {
            let Some(log) = output.get(fd) else {
                return Err(Error::protocol(&call, format!("the operation {} gives no log of fd {fd}", done.id)));
            };
            let file = log.strip_prefix(&logs).filter(|file| !file.is_empty() && !file.contains(['/', '?', '#']));
            let Some(file) = file else {
                let why = format!("the log of fd {fd} is not under {logs}: {}", crate::error::far_text(log));
                return Err(Error::protocol(&call, why));
            };
            let log = format!("{logs}{}", escape(file));
            *stream = self.raw(&log, &[]).await?.body;
            let delete =
                Request { method: "DELETE", target: self.target(&log, &[]), headers: Vec::new(), body: Body::Empty };
            self.envelope(&format!("DELETE {log}"), delete).await?;
        }
        let [stdout, stderr] = streams;
        Ok(ExecOutput { exit_code, stdout, stderr })
    }

    /// The exit code of the exec of the operation `id`, with the final
    /// operation; `None` for an operation that incusd no longer has. Where
    /// the wait gives the error of an exec that ended with 127 or 126, the
    /// operation is read again for its `return`: the caller gets the exit
    /// code and the logs of a command that ran, as podvm needs. A missing
    /// operation (404) takes the code of the words; another failure of the
    /// read is the error of that read.
    pub(crate) async fn exec_end(&self, call: &str, id: &str) -> Result<(i32, Option<Operation>)> {
        let e = match self.finish(call, id, self.settings().wait).await {
            Ok(done) => return Ok((exit_code(call, &done)?, Some(done))),
            Err(e) => e,
        };
        let by_words = match e.kind() {
            ErrorKind::Incus { status: 400, error, .. } => {
                FAILED_ENDS.iter().find(|(words, _)| error.trim() == *words).map(|(_, code)| *code)
            }
            _ => None,
        };
        let Some(by_words) = by_words else { return Err(e) };
        match self.operation(id).await {
            Ok(operation) => {
                // incusd gives these words only for 127 and 126: a missing
                // or odd `return` takes the code of the words.
                let code = operation.metadata.get("return").and_then(Value::as_i64).filter(|c| (0..=255).contains(c));
                Ok((code.map_or(by_words, |code| code as i32), Some(operation)))
            }
            Err(read) if read.status() == Some(404) => Ok((by_words, None)),
            Err(read) => Err(read),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_post_has_the_fields_of_instance_exec_post() {
        let mut exec = Exec::new(["sh", "-c", "echo hi"]);
        exec.cwd = "/tmp".into();
        exec.user = 1000;
        let json = serde_json::to_value(ExecPost::of(&exec, true)).unwrap();
        let want = serde_json::json!({
            "command": ["sh", "-c", "echo hi"], "wait-for-websocket": false, "interactive": false,
            "environment": {}, "width": 0, "height": 0, "record-output": true,
            "user": 1000, "group": 0, "cwd": "/tmp"
        });
        assert_eq!(json, want);
        let json = serde_json::to_value(ExecPost::of(&exec, false)).unwrap();
        assert_eq!((json["wait-for-websocket"].as_bool(), json["record-output"].as_bool()), (Some(true), Some(false)));
    }
}
