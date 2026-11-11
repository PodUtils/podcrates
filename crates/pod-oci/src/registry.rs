//! The endpoints of the OCI distribution API that the client uses, over
//! HTTPS, and over plain HTTP only for a registry that the caller named
//! insecure.
//!
//! No plain-HTTP fallback at a layer of its own: on hosts that drop
//! outbound tcp/80, a fallback hangs and does not fail. Each URL that this
//! module makes is `https://` until the policy of the caller allows HTTP for
//! its endpoint ([`crate::transport::Policy::after_connect_failure`]).
//!
//! No secret is printed. The bearer token stays in memory, never in a log
//! or an error, and each URL in a message goes through [`redact`] first.
//!
//! Blocking: the transport is the blocking face of `pod-proxy`, whose
//! runtime of one thread is private, so the code of the caller stays
//! synchronous.

use std::collections::{BTreeSet, HashMap};
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Class, Events, Failure, NoEvents, Stage};
use pod_proxy::blocking;
use pod_tls::Trust;
use serde::Deserialize;

use crate::credentials::{Logins, NoLogins};
use crate::digest::{Digest, Verifier};
use crate::error::{Result, refusal};
use crate::net::{Body, Limits, Net, Refusal};
use crate::spec;
use crate::transport::Policy;

/// A sink that a cut transfer can start again on.
///
/// [`Client::blob`] owns the new try, so it owns "discard what arrived" too:
/// a caller that resets a sink that it did not fill is a second place for
/// the same defect, and the [`Verifier`] owns the question of whether each
/// byte arrived.
pub trait Restart {
    /// Discard each byte written so far, and be ready for byte zero.
    fn restart(&mut self) -> std::io::Result<()>;
}

impl Restart for Vec<u8> {
    fn restart(&mut self) -> std::io::Result<()> {
        self.clear();
        Ok(())
    }
}

/// A file starts again at byte zero: truncated and rewound. A truncate
/// alone leaves the offset where the cut try left it, so the next byte
/// would land after a hole.
impl Restart for std::fs::File {
    fn restart(&mut self) -> std::io::Result<()> {
        use std::io::Seek;
        self.set_len(0)?;
        self.seek(std::io::SeekFrom::Start(0))?;
        Ok(())
    }
}

/// A limit of a stalled read, not of a large transfer. A blob is streamed:
/// a limit of the whole would fail a slow download that works; this limit
/// fails a connection that stopped.
const READ_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// The dial and the head of the answer within the connect and the read
/// limits together; each wait for the bytes of a body within the read
/// limit.
const LIMITS: Limits = Limits { head: CONNECT_TIMEOUT.saturating_add(READ_TIMEOUT), idle: READ_TIMEOUT };

/// A bounded retry, never an unbounded one.
const ATTEMPTS: u32 = 3;
const BACKOFF_BASE: Duration = Duration::from_millis(500);
/// A cap, also when the server asks for longer. A retry of a rate limit
/// with no respect for its delay and no cap makes the limit worse.
const BACKOFF_CAP: Duration = Duration::from_secs(30);

/// A registry caps a manifest well below this. The cap stops a registry
/// that streams with no end from using all the memory.
pub const MANIFEST_CEILING: u64 = 32 * 1024 * 1024;

/// A tags list holds names, not bytes: a thousand names fit in kilobytes.
pub const TAGS_CEILING: u64 = 1024 * 1024;

/// The read of a token answer, and of the error body of a registry, stops
/// here.
const TOKEN_CEILING: u64 = 10 * 1024 * 1024;

/// A `null` tags list is empty: some registries send one for a repository
/// with no tags, not `[]`.
fn null_to_default<'de, D>(d: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Vec<String>>::deserialize(d)?.unwrap_or_default())
}

/// The `rel="next"` target of a `Link` header, when there is one. A path
/// (what registries send for the pages of tags), not a URL.
fn next_link(header: &str) -> Option<String> {
    for part in header.split(',') {
        let mut halves = part.splitn(2, ';');
        let target = halves.next()?.trim();
        let params = halves.next().unwrap_or("");
        if params.contains("rel=\"next\"") || params.contains("rel='next'") {
            let path = target.strip_prefix('<')?.strip_suffix('>')?.trim();
            if path.is_empty() {
                return None;
            }
            return Some(path.to_string());
        }
    }
    None
}

/// What a fetched manifest holds. `bytes` are the bytes of the digest, and a
/// store writes them as they are: a parsed document written again has
/// another digest.
pub struct FetchedManifest {
    pub bytes: Vec<u8>,
    pub digest: Digest,
    pub media_type: String,
}

/// A client of the registries of one caller.
///
/// The caller gives the HTTP client of `pod-proxy` (its proxy settings, its
/// TLS context, its user agent), the trust of a checked request, and, with
/// the builders, the policy, the trust of a registry named insecure, the
/// logins, the sink of events and the file-size ceiling.
pub struct Client {
    net: Net,
    pub policy: Policy,
    logins: Arc<dyn Logins>,
    events: Arc<dyn Events>,
    file_ceiling: Option<u64>,
    /// The endpoints whose HTTPS connect failed and which the policy lets
    /// use HTTP. Only a host that the caller named insecure gets here.
    http_endpoints: BTreeSet<String>,
    /// One token for each `<endpoint>|<scope>`. The scope is in the key: a
    /// token for one repository does not authorise another, and a key of
    /// the host alone would send it and read the 403 as a refused
    /// permission.
    tokens: HashMap<String, String>,
}

impl Client {
    /// A client over `http`, whose checked requests use `verified`. The
    /// default policy checks each certificate and allows no HTTP; there is
    /// no login, no event and no file-size ceiling.
    pub fn new(http: blocking::Client, verified: Trust) -> Client {
        Client {
            net: Net { client: http, verified, unverified: None },
            policy: Policy::default(),
            logins: Arc::new(NoLogins),
            events: Arc::new(NoEvents),
            file_ceiling: None,
            http_endpoints: BTreeSet::new(),
            tokens: HashMap::new(),
        }
    }

    /// The registries that the client may reach with no check or over HTTP.
    pub fn with_policy(mut self, policy: Policy) -> Client {
        self.policy = policy;
        self
    }

    /// The trust of a registry whose certificate the policy does not check:
    /// the caller's own configuration. With none, such a request fails with
    /// `Class::Config`.
    pub fn with_insecure(mut self, unverified: Trust) -> Client {
        self.net.unverified = Some(unverified);
        self
    }

    /// Where the logins come from (a `Basic` challenge, a token realm).
    pub fn with_logins(mut self, logins: Arc<dyn Logins>) -> Client {
        self.logins = logins;
        self
    }

    /// The sink of the lines for the user: `oci.plain-http` when a registry
    /// named insecure goes to HTTP after its HTTPS connect failed.
    pub fn with_events(mut self, events: Arc<dyn Events>) -> Client {
        self.events = events;
        self
    }

    /// The largest file that the process can write (`RLIMIT_FSIZE`): a blob
    /// past it gets a refusal before the signal `SIGXFSZ` ends the process.
    pub fn with_file_ceiling(mut self, ceiling: Option<u64>) -> Client {
        self.file_ceiling = ceiling;
        self
    }

    /// The base URL of an endpoint: `https://`, or `http://` once this
    /// endpoint was found to need it. The choice stays for the life of the
    /// client: one failed HTTPS connect costs one time limit, not one for
    /// each blob.
    fn base(&self, endpoint: &str) -> String {
        let scheme = if self.http_endpoints.contains(endpoint) { "http" } else { "https" };
        format!("{scheme}://{endpoint}")
    }

    /// `GET /v2/<repository>/manifests/<selector>`. A missing manifest is
    /// `Class::NotFound`; a manifest past [`MANIFEST_CEILING`] is
    /// `Class::Policy`; a digest that is not the declared one is
    /// [`crate::Refusal::Digest`].
    pub fn manifest(&mut self, endpoint: &str, repository: &str, selector: &str) -> Result<FetchedManifest> {
        // A path, not a URL: `get` chooses the scheme from the policy of the
        // endpoint.
        let path = format!("/v2/{repository}/manifests/{selector}");
        let scope = format!("repository:{repository}:pull");
        let resp = self.get(&path, endpoint, &scope, Some(spec::ACCEPT))?;
        let media_type = resp.header("Content-Type").map(|s| s.split(';').next().unwrap_or(s).trim().to_string());
        let declared = resp.header("Docker-Content-Digest").map(str::to_string);
        let what = format!("GET {}", redact(&path));
        let bytes = read_capped(resp, MANIFEST_CEILING, &what, "the manifest")?;

        // The digest of the client is computed over the bytes that arrived.
        // The header is checked against it and never used in its place.
        let digest = Digest::of(&bytes);
        if let Some(declared) = declared {
            let declared = Digest::parse(&declared).map_err(|f| {
                http(Class::Protocol, &what, format!("the Docker-Content-Digest header is not a digest: {f}"))
            })?;
            if declared != digest {
                return Err(crate::Refusal::Digest {
                    what: format!("manifest {selector} from {endpoint}/{repository}"),
                    want: declared.to_string(),
                    got: digest.to_string(),
                }
                .failure());
            }
        }
        Ok(FetchedManifest { bytes, digest, media_type: media_type.unwrap_or_default() })
    }

    /// `GET /v2/<repository>/tags/list`, with each `rel="next"` link.
    ///
    /// One page asks for a thousand tags, so most registries answer once;
    /// the walk of the links covers the rest. The walk is bounded: a
    /// registry that links with no end is a hang, and `Class::Policy`.
    pub fn tags(&mut self, endpoint: &str, repository: &str) -> Result<Vec<String>> {
        #[derive(Deserialize)]
        struct Listing {
            #[serde(default, deserialize_with = "null_to_default")]
            tags: Vec<String>,
        }
        const PAGE_SIZE: u32 = 1000;
        const MAX_PAGES: u32 = 100;
        let scope = format!("repository:{repository}:pull");
        let mut path = format!("/v2/{repository}/tags/list?n={PAGE_SIZE}");
        let mut tags = Vec::new();
        for _ in 0..MAX_PAGES {
            let resp = self.get(&path, endpoint, &scope, None)?;
            let link = resp.header("Link").map(str::to_string);
            let what = format!("GET {}", redact(&path));
            let bytes = read_capped(resp, TAGS_CEILING, &what, "the tags listing")?;
            let listing: Listing = serde_json::from_slice(&bytes)
                .map_err(|e| http(Class::Protocol, &what, format!("the tags listing is not JSON: {e}")))?;
            tags.extend(listing.tags);
            let Some(next) = link.and_then(|l| next_link(&l)) else {
                return Ok(tags);
            };
            // Registries send a path; an absolute URL becomes a path again,
            // not a second base before it.
            path = match next.find("/v2/") {
                Some(i) if next.starts_with("http") => next[i..].to_string(),
                _ => next,
            };
        }
        Err(http(
            Class::Policy,
            &format!("GET /v2/{repository}/tags/list"),
            format!("the listing paginates past {MAX_PAGES} pages and was not followed further"),
        ))
    }

    /// `GET /v2/<repository>/blobs/<digest>`, streamed into `sink` and
    /// checked against the descriptor while it is written. The caller gets
    /// the sink back only when the digest and the count of bytes agree.
    ///
    /// The retry is around the whole transfer, not around the request that
    /// starts it. The GET tries the request again; a body that ends
    /// after its first byte is past it, and a proxy that cuts a transfer at a
    /// round size is a known case. A cut body is never kept or counted: the
    /// sink starts again and a new [`Verifier`] hashes the next try, so the
    /// digest is of one whole transfer. No `Range` request: a resume trusts
    /// that the bytes on disk are the start of the blob, which only the
    /// digest proves, at the last byte.
    ///
    /// Only a cut transfer is tried again. A wrong digest is the registry
    /// that serves other bytes, and a new ask makes it worse. Each new try is
    /// one line on `out`, as an automated caller cannot see a slow link.
    pub fn blob<W: Write + Restart>(
        &mut self,
        endpoint: &str,
        repository: &str,
        want: &Digest,
        size: Option<u64>,
        mut sink: W,
        out: &mut dyn Write,
    ) -> Result<W> {
        let path = format!("/v2/{repository}/blobs/{want}");
        let scope = format!("repository:{repository}:pull");
        let mut last: Option<Failure> = None;
        for attempt in 0..ATTEMPTS {
            if attempt > 0 {
                let _ = writeln!(
                    out,
                    "{}: the body was cut short; retrying it, attempt {} of {ATTEMPTS}",
                    want.short(),
                    attempt + 1
                );
                std::thread::sleep(backoff(attempt, None));
                sink.restart().map_err(|e| crate::error::io(format!("restarting blob {want}"), e))?;
            }
            let resp = self.get(&path, endpoint, &scope, None)?;
            match Client::drain(resp, &mut sink, want, size, self.file_ceiling, &path) {
                Ok(()) => return Ok(sink),
                Err(Cut::Again(f)) => last = Some(f),
                Err(Cut::Stop(f)) => return Err(f),
            }
        }
        Err(last.unwrap_or_else(|| http(Class::Unavailable, &format!("blob {want}"), "no attempt was made")))
    }

    /// One whole body, hashed and counted while it is written.
    ///
    /// The reads stop at the cap: the declared size, and the file-size
    /// ceiling below it. An origin that streams past its descriptor gets a
    /// refusal in the stream, not a write until the disk is full; a body
    /// past `RLIMIT_FSIZE` gets a refusal, not the end by `SIGXFSZ`.
    fn drain<W: Write>(
        resp: Body,
        sink: &mut W,
        want: &Digest,
        size: Option<u64>,
        ceiling: Option<u64>,
        path: &str,
    ) -> std::result::Result<(), Cut> {
        let mut verifier = Verifier::new(sink);
        let mut reader = resp;
        let ceiling = ceiling.unwrap_or(u64::MAX);
        let cap = size.unwrap_or(u64::MAX).min(ceiling);
        let (bound, class) = if size.is_some_and(|s| s <= ceiling) {
            (format!("the descriptor declares {} byte(s)", size.unwrap_or(0)), Class::Protocol)
        } else {
            (format!("the file-size ceiling is {ceiling} byte(s)"), Class::Policy)
        };
        let mut buf = vec![0u8; 128 * 1024];
        loop {
            let room = cap.saturating_sub(verifier.written());
            // One byte past the room proves that the origin still sends:
            // read it, refuse on it, never write it. At the cap, a last read
            // gives the end and the loop stops.
            let take = usize::try_from(room.saturating_add(1)).unwrap_or(usize::MAX).min(buf.len());
            let n = reader.read(&mut buf[..take]).map_err(|e| {
                let what = format!("GET {}", redact(path));
                let text = format!("{what}: reading the blob body after {} byte(s): {e}", verifier.written());
                Cut::Again(Failure::caused(Stage::Connect, Class::of_io(e.kind()), text, e))
            })?;
            if n == 0 {
                break;
            }
            if n as u64 > room {
                return Err(Cut::Again(http(
                    class,
                    &format!("blob {want}"),
                    format!(
                        "{bound} and {} byte(s) arrived with more sending; the partial was discarded",
                        verifier.written()
                    ),
                )));
            }
            verifier.write_all(&buf[..n]).map_err(|e| Cut::Stop(crate::error::io(format!("blob {want}"), e)))?;
        }
        verifier
            .finish(&format!("blob {want}"), want, size)
            .map(|_| ())
            .map_err(|f| if refusal(&f).is_some() { Cut::Stop(f) } else { Cut::Again(f) })
    }
}

/// How a transfer of a blob ended with no blob.
enum Cut {
    /// The transfer was cut or the bytes were too many: a new try can work.
    Again(Failure),
    /// The bytes are wrong, or the sink failed: a new try does not help.
    Stop(Failure),
}

impl std::fmt::Display for Cut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Cut::Again(e) | Cut::Stop(e) => write!(f, "{e}"),
        }
    }
}

/// Read a body to its end, at most `ceiling` bytes. The `take` comes before
/// the read, so the cap limits what is read, not a check after the memory
/// is spent. A body past the cap is `Class::Policy`.
fn read_capped(resp: Body, ceiling: u64, what: &str, name: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    resp.take(ceiling + 1).read_to_end(&mut bytes).map_err(|e| {
        let text = format!("{what}: reading {name} body: {e}");
        Failure::caused(Stage::Connect, Class::of_io(e.kind()), text, e)
    })?;
    if bytes.len() as u64 > ceiling {
        return Err(http(Class::Policy, what, format!("{name} exceeds {ceiling} bytes and was not read")));
    }
    Ok(bytes)
}

/// A failure of a request to a registry, at [`Stage::Registry`]: `what`
/// names the request, `detail` the cause.
fn http(class: Class, what: &str, detail: impl std::fmt::Display) -> Failure {
    Failure::new(Stage::Registry, class, format!("{what}: {detail}"))
}

mod http;
mod wire;
use wire::backoff;
pub use wire::redact;

/// The parameters of a `WWW-Authenticate` challenge of a registry, for the
/// fuzz target `oci_challenge` (T-173): the parse of the client, private
/// here.
#[cfg(any(fuzzing, feature = "fuzzing"))]
pub fn fuzz_parse_challenge(challenge: &str) -> std::collections::HashMap<String, String> {
    wire::parse_challenge(challenge)
}

#[cfg(test)]
mod tests;
