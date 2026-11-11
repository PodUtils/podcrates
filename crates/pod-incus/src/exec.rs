//! A command in an instance (`POST /1.0/instances/NAME/exec`,
//! `shared/api/instance_exec.go`). With `record-output`, incus writes
//! stdout and stderr to two logs of the instance, and the final operation
//! gives their paths in `metadata.output` (`1`, `2`) and the exit code in
//! `metadata.return`. The crate reads each log (`GET
//! /1.0/instances/NAME/logs/exec-output/FILE`) and deletes it, as the client
//! of incus does. The command with its websockets is [`crate::attach`].

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::client::Client;
use crate::error::{Error, Result};
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
        let done = self.finish(&call, &started.id, self.settings().wait).await?;
        let exit_code = exit_code(&call, &done)?;
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
