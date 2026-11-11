//! A background operation of incus (`shared/api/operation.go`): a call
//! that takes time answers 202 with the operation, and the client waits for
//! its final state with `GET /1.0/operations/ID/wait?timeout=N` (N in
//! seconds, -1 for no limit). At the limit, incus answers the state of the
//! moment: the crate makes that a [`ErrorKind::Timeout`]. A status code of
//! 200 or more is final: 200 success, 400 failure (`err` says why), 401
//! canceled.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::client::Client;
use crate::envelope::nullable;
use crate::error::{Error, ErrorKind, Result, far_text};
use crate::wire::{Body, Request, escape};

/// The status codes of incus (`doc/rest-api.md`, "Status codes").
pub mod status {
    pub const CREATED: u16 = 100;
    pub const STARTED: u16 = 101;
    pub const STOPPED: u16 = 102;
    pub const RUNNING: u16 = 103;
    pub const CANCELING: u16 = 104;
    pub const PENDING: u16 = 105;
    pub const STARTING: u16 = 106;
    pub const STOPPING: u16 = 107;
    pub const ABORTING: u16 = 108;
    pub const FREEZING: u16 = 109;
    pub const FROZEN: u16 = 110;
    pub const THAWED: u16 = 111;
    pub const ERROR: u16 = 112;
    pub const READY: u16 = 113;
    pub const SUCCESS: u16 = 200;
    pub const FAILURE: u16 = 400;
    pub const CANCELED: u16 = 401;
}

/// The slack past the limit of a wait that the crate gives incus to answer.
const WAIT_SLACK: Duration = Duration::from_secs(30);

/// A background operation.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Operation {
    #[serde(default)]
    pub id: String,
    /// `task`, `websocket` or `token`.
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub status_code: u16,
    /// The URLs of the resources that it changes, by kind.
    #[serde(default, deserialize_with = "nullable")]
    pub resources: BTreeMap<String, Vec<String>>,
    /// What the operation gives: the `fds` and the `return` of exec, the
    /// `fingerprint` of an image.
    #[serde(default, deserialize_with = "nullable")]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub may_cancel: bool,
    #[serde(default)]
    pub err: String,
    #[serde(default)]
    pub location: String,
}

impl Operation {
    /// The state is final: success, failure or canceled.
    pub fn is_final(&self) -> bool {
        self.status_code >= status::SUCCESS
    }

    pub fn succeeded(&self) -> bool {
        self.status_code == status::SUCCESS
    }

    /// A text of the metadata.
    pub fn text(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).and_then(Value::as_str)
    }

    /// The map of texts of the metadata at `key` (the `fds` of exec).
    pub fn texts(&self, key: &str) -> BTreeMap<String, String> {
        let Some(Value::Object(map)) = self.metadata.get(key) else { return BTreeMap::new() };
        map.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect()
    }
}

impl Client {
    /// `GET /1.0/operations/ID`: the state of the operation now.
    pub async fn operation(&self, id: &str) -> Result<Operation> {
        let path = format!("/1.0/operations/{}", escape(id));
        self.get(&path, &[]).await
    }

    /// `GET /1.0/operations/ID/wait`: the final state of the operation, a
    /// failure too. `limit` is rounded up to whole seconds; `None` waits
    /// with no limit. At the limit: [`ErrorKind::Timeout`].
    pub async fn wait(&self, id: &str, limit: Option<Duration>) -> Result<Operation> {
        let call = format!("GET /1.0/operations/{}/wait", escape(id));
        self.wait_for(&call, id, limit).await
    }

    /// [`Client::wait`], and the error of an operation that failed or was
    /// canceled.
    pub async fn wait_success(&self, id: &str, limit: Option<Duration>) -> Result<Operation> {
        let call = format!("GET /1.0/operations/{}/wait", escape(id));
        self.finish(&call, id, limit).await
    }

    /// `DELETE /1.0/operations/ID`: cancel the operation, where it can be.
    pub async fn cancel(&self, id: &str) -> Result<()> {
        let path = format!("/1.0/operations/{}", escape(id));
        let call = format!("DELETE {path}");
        let request =
            Request { method: "DELETE", target: self.target(&path, &[]), headers: Vec::new(), body: Body::Empty };
        self.envelope(&call, request).await.map(drop)
    }

    /// The final state of `id`, with the errors named by `call`.
    pub(crate) async fn wait_for(&self, call: &str, id: &str, limit: Option<Duration>) -> Result<Operation> {
        let path = format!("/1.0/operations/{}/wait", escape(id));
        // incus reads the limit as a Go int of seconds.
        let seconds = limit.map(|limit| (limit.as_secs() + u64::from(limit.subsec_nanos() > 0)).min(i32::MAX as u64));
        let timeout = seconds.map_or_else(|| "-1".to_string(), |s| s.to_string());
        let request = Request {
            method: "GET",
            target: self.target(&path, &[("timeout", &timeout)]),
            headers: Vec::new(),
            body: Body::Empty,
        };
        let reply = match seconds {
            None => self.envelope(call, request).await?,
            Some(seconds) => {
                let bound = Duration::from_secs(seconds).saturating_add(WAIT_SLACK);
                match tokio::time::timeout(bound, self.envelope(call, request)).await {
                    Ok(reply) => reply?,
                    Err(_) => {
                        let status = format!("not final after {} s with no answer of incus", bound.as_secs());
                        return Err(Error::new(call, ErrorKind::Timeout { id: id.to_string(), status }));
                    }
                }
            }
        };
        let operation: Operation = crate::envelope::sync(call, reply)?;
        tracing::debug!(call, id, status = operation.status_code, "incus operation");
        if !operation.is_final() {
            let status = format!("{} ({})", far_text(&operation.status), operation.status_code);
            return Err(Error::new(call, ErrorKind::Timeout { id: id.to_string(), status }));
        }
        Ok(operation)
    }

    /// [`Client::wait_for`], and the error of a final state that is not a
    /// success, named by `call`.
    pub(crate) async fn finish(&self, call: &str, id: &str, limit: Option<Duration>) -> Result<Operation> {
        let operation = self.wait_for(call, id, limit).await?;
        if operation.succeeded() {
            return Ok(operation);
        }
        let err = if operation.err.is_empty() { far_text(&operation.status) } else { far_text(&operation.err) };
        let id = if operation.id.is_empty() { id } else { &operation.id };
        Err(Error::new(call, ErrorKind::Operation { id: id.to_string(), code: operation.status_code, err }))
    }
}
