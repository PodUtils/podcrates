//! The far ends of the package clients of a job (T-980): a simple index of
//! pip, a registry of npm, the Resource Manager API of the Google Cloud CLI
//! and the API of Hex. Each answers 401 to a request with no
//! `Authorization`, and names the value that it got where the client
//! prints it.

use std::path::Path;
use std::sync::Arc;

use crate::tls_common::{Answer, ok};

/// The value of `Authorization` in `head`, the head of a request.
pub fn auth_of(head: &str) -> Option<String> {
    head.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(": ")?;
        name.eq_ignore_ascii_case("authorization").then(|| value.to_string())
    })
}

/// 401 with a Basic challenge.
pub fn unauthorized() -> Vec<u8> {
    b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"far\"\r\nContent-Length: 0\r\n\r\n".to_vec()
}

fn not_found() -> Vec<u8> {
    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec()
}

/// An answer by `route` of the request line and the value of
/// `Authorization`; `None` is 404.
fn authorized(route: fn(&str, &str) -> Option<Vec<u8>>) -> Answer {
    Arc::new(move |head: &str, _body: &[u8]| {
        let Some(auth) = auth_of(head) else { return unauthorized() };
        let line = head.lines().next().unwrap_or_default();
        route(line, &auth).unwrap_or_else(not_found)
    })
}

/// The package `foo` 1.0.0 of a simple index (PEP 503) at `/simple/`.
pub fn pip(_dir: &Path) -> Answer {
    authorized(|line, auth| {
        line.starts_with("GET /simple/foo/ ").then(|| {
            let page = format!(
                "<!DOCTYPE html><html><body><!-- echo={auth} -->\n\
                 <a href=\"/files/foo-1.0.0.tar.gz#sha256={}\">foo-1.0.0.tar.gz</a>\n</body></html>\n",
                "0".repeat(64)
            );
            ok(&format!("Content-Type: text/html\r\nX-Seen: {auth}\r\n"), page.as_bytes())
        })
    })
}

/// The packument of `foo` 1.0.0 of a registry of npm, whose description
/// is the value that the far end got.
pub fn npm(_dir: &Path) -> Answer {
    authorized(|line, auth| {
        line.starts_with("GET /foo ").then(|| {
            let auth = auth.replace('\\', "\\\\").replace('"', "\\\"");
            let version = format!(r#"{{"name":"foo","version":"1.0.0","description":"{auth}","dist":{{"tarball":"https://api.example.com/foo/-/foo-1.0.0.tgz","shasum":"{}"}}}}"#, "0".repeat(40));
            let body = format!(
                r#"{{"name":"foo","description":"{auth}","dist-tags":{{"latest":"1.0.0"}},"versions":{{"1.0.0":{version}}}}}"#
            );
            ok("Content-Type: application/json\r\n", body.as_bytes())
        })
    })
}

/// The project `job-project` of the Resource Manager API v1, whose name is
/// the value that the far end got.
pub fn gcloud(_dir: &Path) -> Answer {
    authorized(|line, auth| {
        line.starts_with("GET /v1/projects/job-project").then(|| {
            let auth = auth.replace('\\', "\\\\").replace('"', "\\\"");
            let body = format!(
                r#"{{"projectId":"job-project","projectNumber":"1","lifecycleState":"ACTIVE","name":"{auth}"}}"#
            );
            ok("Content-Type: application/json; charset=UTF-8\r\n", body.as_bytes())
        })
    })
}

/// The user of the key of the Hex API, as the external term format: a
/// map with one binary key and value. The name is fixed: the TLS end puts
/// the placeholder back in a body, and a length in the term would not
/// change with it.
pub fn hex(_dir: &Path) -> Answer {
    authorized(|line, _auth| {
        line.starts_with("GET /api/users/me ").then(|| {
            let mut term = vec![131, 116, 0, 0, 0, 1];
            for text in ["username", "job-user"] {
                term.push(109);
                term.extend_from_slice(&(text.len() as u32).to_be_bytes());
                term.extend_from_slice(text.as_bytes());
            }
            ok("Content-Type: application/vnd.hex+erlang\r\n", &term)
        })
    })
}
