//! The control of a broker: JSON lines on a Unix socket of mode 0600, which
//! only the user of the broker may use. One request on each line, one answer
//! on each line, each answer with `"v": 1`:
//!
//! ```text
//! {"op":"load","job":"J","token":"T","ttl_s":600,"policy":{"schema":1,"mode":"open"}}
//! {"v":1,"ok":true,"op":"load","job":"J","mode":"open","rules":0,"expires_in_s":600}
//! {"op":"drop","job":"J"}
//! {"v":1,"ok":true,"op":"drop","job":"J","dropped":true}
//! {"op":"list"}
//! {"v":1,"ok":true,"op":"list","jobs":[{"job":"J","mode":"open","rules":0,"expires_in_s":590,"open":1}]}
//! {"v":1,"ok":false,"class":"usage","error":"..."}
//! ```
//!
//! An answer never holds a token. The class of a failure is the name of
//! `pod_core::Class`.

use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::broker::Broker;
use crate::jobs::{JobInfo, Load, Token};
use crate::policy::Policy;

/// The longest request line: 1 MiB.
pub const MAX_LINE: usize = 1 << 20;

/// The version of the answers.
pub const VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
enum Request {
    Load { job: String, token: Token, ttl_s: u64, policy: Value },
    Drop { job: String },
    List,
}

fn info(job: &JobInfo) -> Value {
    let mut info = json!({
        "job": job.job,
        "mode": job.mode.word(),
        "rules": job.rules,
        "expires_in_s": job.expires_in.as_secs(),
        "open": job.open,
    });
    // Only when a rule is flagged: the host of each allow rule under an
    // open suffix (T-161).
    if !job.flagged.is_empty() {
        info["flagged"] = json!(job.flagged);
    }
    info
}

fn failed(class: Class, error: &str) -> Value {
    json!({ "v": VERSION, "ok": false, "class": class.name(), "error": error })
}

/// The words of a request that does not parse. The words of serde can quote
/// a value of the request, and a value can be a token: only the names of
/// fields and the place go back.
fn parse_words(e: &serde_json::Error) -> String {
    let text = e.to_string();
    let named = ["unknown field", "missing field", "unknown variant", "EOF while parsing"];
    if named.iter().any(|n| text.contains(n)) && !text.contains("invalid") {
        return format!("not a request of the control: {text}");
    }
    format!("not a request of the control: a value has the wrong type or form (column {})", e.column())
}

impl Broker {
    /// The answer to one line of the control.
    pub fn control_line(&self, line: &str) -> Value {
        let request: Request = match serde_json::from_str(line) {
            Ok(request) => request,
            Err(e) => return failed(Class::Usage, &parse_words(&e)),
        };
        match request {
            Request::Load { job, token, ttl_s, policy } => {
                let loaded = Policy::from_value(policy)
                    .and_then(|policy| self.load(Load { job, token, ttl: Duration::from_secs(ttl_s), policy }));
                match loaded {
                    Ok(job) => {
                        let mut answer = info(&job);
                        answer["v"] = json!(VERSION);
                        answer["ok"] = json!(true);
                        answer["op"] = json!("load");
                        answer.as_object_mut().map(|o| o.remove("open"));
                        answer
                    }
                    Err(e) => failed(e.class, e.text()),
                }
            }
            Request::Drop { job } => {
                let dropped = self.drop_job(&job);
                json!({ "v": VERSION, "ok": true, "op": "drop", "job": job, "dropped": dropped })
            }
            Request::List => {
                let jobs: Vec<Value> = self.list().iter().map(info).collect();
                json!({ "v": VERSION, "ok": true, "op": "list", "jobs": jobs })
            }
        }
    }
}

#[cfg(unix)]
pub use unix::{bind_unix, serve_control};

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::path::Path;
    use std::sync::Arc;

    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::{UnixListener, UnixStream};
    use zeroize::Zeroizing;

    use super::*;

    /// Bind a Unix socket at `path` with the mode `mode` (0600 for the
    /// control). A socket that is there already is taken away first; another
    /// kind of file is an error. Put the socket in a directory of mode 0700:
    /// the mode is set just after the bind.
    pub fn bind_unix(path: &Path, mode: u32) -> std::io::Result<UnixListener> {
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_socket() => std::fs::remove_file(path)?,
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("{} is there and is not a socket", path.display()),
                ));
            }
            Err(_) => {}
        }
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
        Ok(listener)
    }

    /// Serve the control on `listener` ([`bind_unix`] with 0600), one task
    /// for each peer, until the listener fails. A peer of another user is
    /// refused.
    pub async fn serve_control(broker: Arc<Broker>, listener: UnixListener) -> std::io::Result<()> {
        loop {
            let (stream, _) = listener.accept().await?;
            let broker = broker.clone();
            tokio::spawn(async move {
                if let Err(e) = peer(broker, stream).await {
                    tracing::debug!(error = %e, "a peer of the control ended");
                }
            });
        }
    }

    async fn peer(broker: Arc<Broker>, stream: UnixStream) -> std::io::Result<()> {
        // SAFETY: geteuid has no precondition and cannot fail.
        let ours = unsafe { libc::geteuid() };
        let theirs = stream.peer_cred().map(|c| c.uid()).ok();
        let (read, mut write) = stream.into_split();
        if theirs != Some(ours) {
            let answer = failed(Class::Refused, "the control takes the user of the broker only");
            write.write_all(format!("{answer}\n").as_bytes()).await?;
            return Ok(());
        }
        let mut read = BufReader::new(read).take(u64::MAX);
        loop {
            let mut line = Zeroizing::new(Vec::new());
            read.set_limit(MAX_LINE as u64 + 1);
            let n = read.read_until(b'\n', &mut line).await?;
            if n == 0 {
                return Ok(());
            }
            let answer = if line.len() > MAX_LINE {
                let answer = failed(Class::Usage, "a line of the control is 1 MiB at most");
                write.write_all(format!("{answer}\n").as_bytes()).await?;
                return Ok(());
            } else {
                match std::str::from_utf8(&line) {
                    Ok(text) if text.trim().is_empty() => continue,
                    Ok(text) => broker.control_line(text.trim()),
                    Err(_) => failed(Class::Usage, "a line of the control is UTF-8"),
                }
            };
            write.write_all(format!("{answer}\n").as_bytes()).await?;
        }
    }
}

/// A failure of the control as a [`Failure`], for a host that reads an
/// answer.
pub fn answer_failure(answer: &Value) -> Option<Failure> {
    if answer["ok"] == json!(true) {
        return None;
    }
    let class = Class::ALL.into_iter().find(|c| answer["class"] == json!(c.name())).unwrap_or(Class::Protocol);
    Some(Failure::new(Stage::Proxy, class, answer["error"].as_str().unwrap_or("no words")))
}
