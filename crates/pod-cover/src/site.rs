//! A static site that a listener serves before it accepts a session, and the
//! routing of a name to its site.
//!
//! [`Site`] holds a tree that was read once: `index.html`, the assets,
//! `robots.txt` and the site's own 404 page. No response names the relay, the
//! contract or a version. The `Server` header is the operator's value, and
//! `nginx` by default.
//!
//! [`Sites`] routes the name of a connection to a site. An unknown name gets
//! the default site's own 404 page, not a page of this crate. A path with a
//! `..` segment, or one that leaves the root by a link, is not served: it
//! gets the 404 page.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use pod_core::http1::{self, MAX_HEAD, RequestError};

/// The most bytes of a site tree that one site holds.
const MAX_BYTES: usize = 64 << 20;

/// The most files of a site tree.
const MAX_FILES: usize = 4096;

/// The most directories below the root.
const MAX_DEPTH: usize = 16;

/// The default `Server` header: a value that a page server sends.
pub const DEFAULT_SERVER: &str = "nginx";

/// The 404 page of a site with no `404.html`.
const DEFAULT_NOT_FOUND: &[u8] = b"<!doctype html>\n<html><head><title>Not Found</title></head>\n<body><h1>Not Found</h1><p>The page you asked for is not here.</p></body></html>\n";

/// One file of a site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub body: Vec<u8>,
    pub content_type: &'static str,
}

/// How a site answers one request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The answer to a HEAD: the same head and no body.
    pub head_only: bool,
}

impl Response {
    /// The bytes of the answer, the status line and the headers first.
    pub fn to_bytes(&self) -> Vec<u8> {
        let phrase = if self.status == 200 { "OK" } else { http1::reason(self.status) };
        let fields: Vec<(&str, &str)> = self.headers.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
        let mut out = match http1::response_head(self.status, phrase, &fields) {
            Ok(head) => head.into_bytes(),
            // Each header of this crate is made of visible bytes, so this
            // arm is a guard, not a path of the site.
            Err(_) => {
                return b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_vec();
            }
        };
        if !self.head_only {
            out.extend_from_slice(&self.body);
        }
        out
    }
}

/// Why a site tree was not read.
#[derive(Debug)]
pub enum SiteError {
    /// The tree has no `index.html`.
    MissingIndex(PathBuf),
    /// A file or a directory of the tree could not be read.
    Io { path: PathBuf, error: std::io::Error },
    /// A file resolves outside the root.
    Escape(PathBuf),
    /// The tree is larger than this crate reads.
    TooLarge,
    /// The tree has more directories below the root than this crate reads.
    TooDeep,
    /// The tree has more files than this crate reads.
    TooManyFiles,
    /// The `Server` value is not a visible header value.
    Server(String),
}

impl fmt::Display for SiteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SiteError::MissingIndex(path) => {
                write!(f, "the site tree {} has no index.html", path.display())
            }
            SiteError::Io { path, error } => write!(f, "the site file {}: {error}", path.display()),
            SiteError::Escape(path) => {
                write!(f, "the site file {} is a link outside the site root", path.display())
            }
            SiteError::TooLarge => write!(f, "the site tree is larger than {MAX_BYTES} bytes"),
            SiteError::TooDeep => write!(f, "the site tree is deeper than {MAX_DEPTH} directories"),
            SiteError::TooManyFiles => write!(f, "the site tree has more than {MAX_FILES} files"),
            SiteError::Server(value) => write!(f, "{value:?} is not a visible value of the Server header"),
        }
    }
}

impl std::error::Error for SiteError {}

/// A static site: the files of a tree, its own 404 page and its `Server`
/// value.
#[derive(Clone, Debug)]
pub struct Site {
    assets: BTreeMap<String, Asset>,
    not_found: Asset,
    server: String,
}

impl Site {
    /// Read the tree at `root`: each file under it, keyed by its path from
    /// the root with a leading `/`. The tree must hold `index.html`. A link
    /// that leaves the root, too many files and too many bytes are refused.
    pub fn load(root: impl AsRef<Path>) -> Result<Site, SiteError> {
        let root = std::fs::canonicalize(root.as_ref())
            .map_err(|error| SiteError::Io { path: root.as_ref().to_path_buf(), error })?;
        if !root.join("index.html").is_file() {
            return Err(SiteError::MissingIndex(root));
        }
        let mut assets = BTreeMap::new();
        let mut bytes = 0usize;
        read_tree(&root, &root, "", 0, &mut assets, &mut bytes)?;
        let not_found = assets
            .get("/404.html")
            .cloned()
            .unwrap_or(Asset { body: DEFAULT_NOT_FOUND.to_vec(), content_type: "text/html; charset=utf-8" });
        Ok(Site { assets, not_found, server: DEFAULT_SERVER.to_string() })
    }

    /// Set the `Server` header. The value must be visible ASCII with no CR
    /// or LF, and it must not name this project.
    pub fn set_server(&mut self, value: impl Into<String>) -> Result<(), SiteError> {
        let value = value.into();
        let visible = !value.is_empty()
            && value.len() <= 64
            && value.bytes().all(|b| (0x20..=0x7e).contains(&b))
            && !names_project(&value);
        if !visible {
            return Err(SiteError::Server(value));
        }
        self.server = value;
        Ok(())
    }

    /// The `Server` header of this site.
    pub fn server(&self) -> &str {
        &self.server
    }

    /// Answer one request head. A method other than GET or HEAD gets 405.
    /// A traversal gets the 404 page.
    pub fn respond(&self, request: &[u8]) -> Response {
        let Some((start, end)) = http1::head_bounds(request) else {
            return self.plain(400, b"Bad request.\n", false);
        };
        if end > MAX_HEAD {
            return self.plain(431, b"Too large.\n", false);
        }
        let request = match http1::parse_request(&request[start..end]) {
            Ok(request) => request,
            Err(e) => return self.refusal(e),
        };
        let head_only = request.method == "HEAD";
        if request.method != "GET" && !head_only {
            let mut response = self.plain(405, b"Method not allowed.\n", false);
            response.headers.push(("Allow".into(), "GET, HEAD".into()));
            return response;
        }
        match resolve(&request.target) {
            Some(key) => match self.assets.get(&key) {
                Some(asset) => self.asset(200, asset, head_only),
                None => match self.assets.get(&format!("{key}/index.html")) {
                    Some(asset) if key != "/index.html" => self.asset(200, asset, head_only),
                    _ => self.not_found(head_only),
                },
            },
            None => self.not_found(head_only),
        }
    }

    /// The site's own 404 page.
    pub fn not_found(&self, head_only: bool) -> Response {
        self.asset(404, &self.not_found, head_only)
    }

    /// The content type that a file name gives.
    pub fn content_type(name: &str) -> &'static str {
        content_type(name)
    }

    fn refusal(&self, error: RequestError) -> Response {
        let status = error.status().unwrap_or(400);
        let phrase = http1::reason(status);
        let body = format!("{}\n", phrase.to_ascii_lowercase());
        self.plain(status, body.as_bytes(), false)
    }

    fn plain(&self, status: u16, body: &[u8], head_only: bool) -> Response {
        self.fields(status, "text/plain; charset=utf-8", body, head_only)
    }

    fn asset(&self, status: u16, asset: &Asset, head_only: bool) -> Response {
        self.fields(status, asset.content_type, &asset.body, head_only)
    }

    fn fields(&self, status: u16, content_type: &str, body: &[u8], head_only: bool) -> Response {
        Response {
            status,
            headers: vec![
                ("Server".into(), self.server.clone()),
                ("Content-Type".into(), content_type.into()),
                ("Content-Length".into(), body.len().to_string()),
                ("X-Content-Type-Options".into(), "nosniff".into()),
                ("Connection".into(), "close".into()),
            ],
            body: body.to_vec(),
            head_only,
        }
    }
}

/// The sites of one listener, by name, and the site of an unknown name.
#[derive(Clone, Debug)]
pub struct Sites {
    pub names: BTreeMap<String, Site>,
    pub default: Site,
}

impl Sites {
    /// The sites with `default` for each name that is not in `names`.
    pub fn new(default: Site) -> Sites {
        Sites { names: BTreeMap::new(), default }
    }

    /// Add `site` for `name`, with ASCII case ignored.
    pub fn insert(&mut self, name: impl Into<String>, site: Site) {
        self.names.insert(name.into().to_ascii_lowercase(), site);
    }

    /// Answer a request on a connection of `name`. An unknown name, or no
    /// name, gets the default site's own 404 page.
    pub fn respond(&self, name: Option<&str>, request: &[u8]) -> Response {
        let site = name.and_then(|name| self.names.get(name).or_else(|| self.names.get(&name.to_ascii_lowercase())));
        match site {
            Some(site) => site.respond(request),
            None => {
                let head_only = http1::head_bounds(request)
                    .and_then(|(start, end)| http1::parse_request(&request[start..end]).ok())
                    .is_some_and(|request| request.method == "HEAD");
                self.default.not_found(head_only)
            }
        }
    }
}

/// Whether `text` names this project, the relay or the contract.
fn names_project(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    ["podutils", "podcrates", "podssh", "podnet", "contract"].iter().any(|word| text.contains(word))
}

/// Read each file under `dir` into `assets`, keyed from `prefix`.
fn read_tree(
    root: &Path,
    dir: &Path,
    prefix: &str,
    depth: usize,
    assets: &mut BTreeMap<String, Asset>,
    bytes: &mut usize,
) -> Result<(), SiteError> {
    if depth > MAX_DEPTH {
        return Err(SiteError::TooDeep);
    }
    let entries = std::fs::read_dir(dir).map_err(|error| SiteError::Io { path: dir.to_path_buf(), error })?;
    for entry in entries {
        let entry = entry.map_err(|error| SiteError::Io { path: dir.to_path_buf(), error })?;
        let path = entry.path();
        // A link is followed, and its target must stay under the root.
        let kind = std::fs::metadata(&path).map_err(|error| SiteError::Io { path: path.clone(), error })?;
        if kind.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            read_tree(root, &path, &format!("{prefix}/{name}"), depth + 1, assets, bytes)?;
        } else if kind.is_file() {
            let real = std::fs::canonicalize(&path).map_err(|error| SiteError::Io { path: path.clone(), error })?;
            if !real.starts_with(root) {
                return Err(SiteError::Escape(path));
            }
            if assets.len() >= MAX_FILES {
                return Err(SiteError::TooManyFiles);
            }
            let body = std::fs::read(&real).map_err(|error| SiteError::Io { path: path.clone(), error })?;
            *bytes = bytes.saturating_add(body.len());
            if *bytes > MAX_BYTES {
                return Err(SiteError::TooLarge);
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let key = format!("{prefix}/{name}");
            let content_type = content_type(&name);
            assets.insert(key, Asset { body, content_type });
        }
    }
    Ok(())
}

/// The asset key of a request target, or `None` when it is not a path of the
/// site: a traversal, a byte that is not UTF-8, a control byte.
fn resolve(target: &str) -> Option<String> {
    let path = target.split(['?', '#']).next().unwrap_or_default();
    if !path.starts_with('/') {
        return None;
    }
    let decoded = percent_decode(path)?;
    let text = std::str::from_utf8(&decoded).ok()?;
    if text.bytes().any(|b| b == b'\\' || b < 0x20 || b == 0x7f) {
        return None;
    }
    let mut parts = Vec::new();
    for part in text.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return Some("/index.html".into());
    }
    Some(format!("/{}", parts.join("/")))
}

/// The bytes of a percent-encoded path. `None` for a cut escape or a byte
/// that is not hexadecimal.
fn percent_decode(path: &str) -> Option<Vec<u8>> {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let high = hex(*bytes.get(i + 1)?)?;
            let low = hex(*bytes.get(i + 2)?)?;
            out.push(high << 4 | low);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(out)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// The content type of a file name, by its extension.
fn content_type(name: &str) -> &'static str {
    let extension = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match extension.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "txt" | "md" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}
