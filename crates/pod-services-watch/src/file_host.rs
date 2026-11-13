//! The round trip of one service of the kind `file-host` (T-528): an
//! upload by the form of the catalog, the download by its form, a compare
//! of the bytes, and the delete where the host has one. Only the facts of
//! the service drive it, so a trip that holds proves the facts that a road
//! reads (PodUtils/podssh@T-738). No daily watch uploads: the live step of
//! `live.yml` runs it (`tests/file_host_live.rs`), and the tests run it
//! against a stand-in.

use pod_proxy::http::Response;
use pod_services::Service;

use crate::{Finding, Net};

/// The name of the file of each upload: a word with no meaning.
pub const FILE_NAME: &str = "piece.bin";
/// The most characters of an answer that a finding shows.
const SHOWN: usize = 200;

/// What a trip did, one line for each step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trip {
    /// The URL of the answer to the upload.
    pub url: String,
    /// The URL that gave the bytes.
    pub fetched: String,
    /// The host deleted the file.
    pub deleted: bool,
    pub lines: Vec<String>,
}

/// The body of a form (RFC 7578): each field, then the file when there is
/// one, as the field `file.0` with the name [`FILE_NAME`].
pub fn multipart(boundary: &str, fields: &[(&str, &str)], file: Option<(&str, &[u8])>) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    if let Some((name, bytes)) = file {
        let head = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{FILE_NAME}\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n"
        );
        body.extend_from_slice(head.as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

/// The host of an `https://` URL, with no port, lower case.
fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host.split(':').next()?.to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// The path of an `https://` URL, from its first `/`.
fn path_of(url: &str) -> &str {
    let rest = url.strip_prefix("https://").unwrap_or(url);
    rest.find('/').map_or("/", |at| &rest[at..])
}

/// True when `url` is an `https://` URL of a host of the fact `serves`.
pub fn served(service: &Service, url: &str) -> bool {
    let serves = service.texts("serves").unwrap_or_default();
    host_of(url).is_some_and(|host| serves.iter().any(|s| s.eq_ignore_ascii_case(&host)))
}

/// The URL in the answer to an upload, by the fact `answer`: the first
/// `https://` word of a text, or the string at `answer-pointer` of JSON.
pub fn answer_url(service: &Service, body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    match service.text("answer")? {
        "text" => text.split_whitespace().find(|w| w.starts_with("https://")).map(str::to_string),
        "json" => {
            let json: serde_json::Value = serde_json::from_str(&text).ok()?;
            json.pointer(service.text("answer-pointer")?)?.as_str().map(str::to_string)
        }
        _ => None,
    }
}

/// The URL that downloads the file of `answer` with `segment` (the fact
/// `fetch-segment`) as the first segment of its path; none when empty.
pub fn fetch_url(answer: &str, segment: &str) -> String {
    let Some(rest) = answer.strip_prefix("https://").filter(|_| !segment.is_empty()) else {
        return answer.to_string();
    };
    match rest.split_once('/') {
        Some((host, path)) => format!("https://{host}/{segment}/{path}"),
        None => format!("https://{rest}/{segment}"),
    }
}

/// The first link in `page` that starts with `prefix` and ends with the
/// path of `answer`: the page of one file can link to other pages, and a
/// link of another file is not this one's.
pub fn page_link(page: &str, prefix: &str, answer: &str) -> Option<String> {
    let path = path_of(answer);
    let mut from = 0;
    while let Some(at) = page[from..].find(prefix) {
        let start = from + at;
        let end = page[start..].find(['"', '\'', '<', '>', ' ', '\t', '\r', '\n']).map_or(page.len(), |e| start + e);
        let link = &page[start..end];
        if path.len() > 1 && link.ends_with(path) {
            return Some(link.to_string());
        }
        from = end.max(start + 1);
    }
    None
}

/// The finding of a step that `answer` refused: its status and the start
/// of its text, which names the fault of many hosts (litterbox: 412 and a
/// sentence).
fn refused(id: &str, step: &str, answer: &Response) -> Finding {
    Finding::failed(format!("{id}: the {step} answered HTTP {}: {}", answer.status, answer.body_text(SHOWN)))
}

/// The fields of the fact `name` (`name=value`) as pairs.
fn pairs(service: &Service, name: &str) -> Result<Vec<(String, String)>, Finding> {
    let list = service.texts(name).unwrap_or_default();
    list.iter()
        .map(|p| p.split_once('=').map(|(n, v)| (n.to_string(), v.to_string())))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| Finding::failed(format!("{}: a field of {name} is no name=value", service.id())))
}

/// Upload `bytes` to `service` by its form, download them by its form,
/// compare, and delete where the host deletes; `boundary` is the boundary
/// of each form.
pub async fn trip(service: &Service, net: &Net, bytes: &[u8], boundary: &str) -> Result<Trip, Finding> {
    let id = service.id();
    let fact = |name: &str| service.text(name).ok_or_else(|| Finding::failed(format!("{id}: no fact {name}")));
    let upload = service.endpoint("upload").ok_or_else(|| Finding::failed(format!("{id}: no endpoint upload")))?;
    let fields = pairs(service, "fields")?;
    let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
    let body = multipart(boundary, &fields, Some((fact("file-field")?, bytes)));
    let kind = format!("multipart/form-data; boundary={boundary}");
    let request = net.request("POST", &upload.url()).header("Content-Type", kind.clone()).header("Accept", "*/*");
    let answer = net.send(request.body(body)).await?;
    if !answer.is_success() {
        return Err(refused(id, "upload", &answer));
    }
    let url = answer_url(service, &answer.body)
        .ok_or_else(|| Finding::failed(format!("{id}: the answer names no URL: {}", answer.body_text(SHOWN))))?;
    if !served(service, &url) {
        return Err(Finding::failed(format!("{id}: the answer names a URL of another host: {url}")));
    }
    let mut lines = vec![format!("upload: {} bytes, {url}", bytes.len())];
    let token = match service.text("delete") {
        Some("token-form") => Some(answer.header(fact("delete-token-header")?).map(str::to_string)),
        _ => None,
    };
    let fetched = match service.text("fetch-link-prefix") {
        Some(prefix) => {
            let page = net.send(net.request("GET", &url).header("Accept", "text/html").redirects(2)).await?;
            if !page.is_success() {
                return Err(refused(id, "page", &page));
            }
            let link = page_link(&page.text(), prefix, &url).ok_or_else(|| {
                Finding::failed(format!("{id}: the page holds no link {prefix}...: {}", page.body_text(SHOWN)))
            })?;
            lines.push(format!("page: {} bytes, the link {link}", page.body.len()));
            link
        }
        None => fetch_url(&url, fact("fetch-segment")?),
    };
    if !served(service, &fetched) {
        return Err(Finding::failed(format!("{id}: the download is at another host: {fetched}")));
    }
    let request = match fact("fetch-method")? {
        "POST" => net.request("POST", &fetched),
        _ => net.request("GET", &fetched).redirects(2),
    };
    let back = net.send(request.header("Accept", "*/*")).await?;
    if !back.is_success() {
        return Err(refused(id, "download", &back));
    }
    // A redirect can end at another host: such bytes are not the host's.
    if !back.url.is_empty() && !served(service, &back.url) {
        return Err(Finding::failed(format!("{id}: the download ended at another host: {}", back.url)));
    }
    if back.body != bytes {
        let why =
            format!("{} bytes, not the {} sent; they begin: {}", back.body.len(), bytes.len(), back.body_text(80));
        return Err(Finding::failed(format!("{id}: the download gave {why}")));
    }
    lines.push(format!("download: {} bytes, the same", back.body.len()));
    let deleted = match token {
        None => false,
        Some(None) => return Err(Finding::failed(format!("{id}: the answer to the upload holds no token"))),
        Some(Some(token)) => {
            let mut fields = vec![(fact("delete-token-field")?.to_string(), token)];
            fields.extend(pairs(service, "delete-fields")?);
            let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
            let request = net.request("POST", &url).header("Content-Type", kind);
            let answer = net.send(request.body(multipart(boundary, &fields, None))).await?;
            if !answer.is_success() {
                return Err(refused(id, "delete", &answer));
            }
            lines.push("delete: done".to_string());
            true
        }
    };
    Ok(Trip { url, fetched, deleted, lines })
}

#[cfg(test)]
#[path = "file_host_tests.rs"]
mod tests;
