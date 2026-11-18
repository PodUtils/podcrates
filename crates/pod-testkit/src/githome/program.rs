//! `pod-fake-githome source`: the program of a `[[source]]` of the kind
//! `program`. Each request line goes to `serve` as
//! `POST /_runner/source/OP` of the session of this program, and its reply
//! line comes back. The program keeps no state of a job: two programs on one
//! `serve` are two hosts of one forge.

use std::io::{self, BufRead, Write};

use pod_job::source::{Reply, SourceCode, SourceError, parse_reply, parse_request, reply_line, request_line};
use serde_json::{Value, json};

use super::SESSION_HEADER;
use crate::stand::call;

/// Answer each request line of `input` on `output` through the fake at
/// `base` (`http://ADDR/`), until `input` ends. The session starts at the
/// first line.
pub fn serve_lines(base: &str, input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let mut session = None;
    for line in input.split(b'\n') {
        let reply = answer(base, &mut session, &line?);
        output.write_all(reply_line(&reply).as_bytes())?;
        output.flush()?;
    }
    Ok(())
}

/// The reply to one line. A line that is not of the wire is refused here,
/// and `serve` records the refusal: it cannot take a line over the limit.
fn answer(base: &str, session: &mut Option<String>, line: &[u8]) -> Reply {
    let req = match parse_request(line) {
        Ok(r) => r,
        Err(SourceError { code, detail }) => {
            if let Some(id) = open(base, session) {
                let told = json!({ "code": code, "detail": detail }).to_string();
                let headers = [(SESSION_HEADER, id.as_str())];
                let _ = call("POST", &format!("{base}_runner/source/refusal"), &headers, told.as_bytes());
            }
            return Reply::Error { code, detail };
        }
    };
    let Some(id) = open(base, session) else {
        return unavailable(base);
    };
    let headers = [(SESSION_HEADER, id.as_str())];
    let url = format!("{base}_runner/source/{}", req.op());
    match call("POST", &url, &headers, request_line(&req).as_bytes()) {
        Ok((200, body)) => parse_reply(&body).unwrap_or_else(|_| unavailable(base)),
        _ => unavailable(base),
    }
}

/// The session of this program; a new one at the first call that reaches
/// `serve`.
fn open<'a>(base: &str, session: &'a mut Option<String>) -> Option<&'a String> {
    if session.is_none() {
        let (status, body) = call("POST", &format!("{base}_runner/source/session"), &[], b"").ok()?;
        let v: Value = serde_json::from_slice(&body).ok()?;
        *session = v["session"].as_u64().filter(|_| status == 201).map(|n| n.to_string());
    }
    session.as_ref()
}

fn unavailable(base: &str) -> Reply {
    Reply::Error { code: SourceCode::Unavailable, detail: format!("the fake githome at {base} gives no reply") }
}
