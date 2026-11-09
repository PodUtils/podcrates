//! `TODO/image.md` T-0201: the OCI distribution endpoints podvm actually uses,
//! over HTTPS and nothing else.
//!
//! ⛔ **No plain-HTTP fallback, at any layer.** tcp/80 egress is broken on the
//! runtime podvm targets, so a fallback that exists to improve reliability
//! makes the failure **untimed and undiagnosable**: the request hangs instead
//! of failing. Every URL this module builds is `https://`, and
//! [`crate::reference::Reference::parse`] refuses an `http://` reference before
//! it reaches here.
//!
//! ⛔ **No secret is ever printed.** `docs/security/secrets.md`. The bearer
//! token is held in memory, never logged, never put in an error, and every URL
//! that reaches a message goes through [`redact`] first.
//!
//! Blocking, per the entry's Decision: podvm has no reason to be async. The
//! transport is `pod-proxy`'s blocking face ([`crate::net`]): its runtime of
//! one thread is private, and podvm's own code stays synchronous.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::time::Duration;

use serde::Deserialize;

use crate::digest::{Digest, Verifier};
use crate::error::{Error, Result};
use crate::net::{Body, Limits, Net, Refusal};
use crate::oci;

/// A sink a cut-short transfer can start over on.
///
/// ⭐ [`TODO/image.md`](../../../TODO/image.md) T-0214. `blob` owns the retry, so
/// it has to own "throw away what arrived": a caller that reset a sink it did
/// not fill is a second place to get the same thing wrong, and the verifier
/// already owns the question of whether every byte arrived.
pub trait Restart {
    /// Discard everything written so far and be ready to receive byte zero.
    fn restart(&mut self) -> std::io::Result<()>;
}

/// ⚠ Bounds a stalled read, not a large transfer. A blob is streamed, so an
/// overall deadline would fail a slow but healthy download; this fails a
/// connection that has stopped producing bytes.
const READ_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// The dial and the head of the answer within the connect and the read
/// bounds together; each wait for the bytes of a body within the read bound.
const LIMITS: Limits = Limits {
    head: CONNECT_TIMEOUT.saturating_add(READ_TIMEOUT),
    idle: READ_TIMEOUT,
};

/// `RULES.md` section 8: bounded retry, never an unbounded one.
const ATTEMPTS: u32 = 3;
const BACKOFF_BASE: Duration = Duration::from_millis(500);
/// ⛔ A cap, honoured even when the server asks for longer.
/// `docs/conventions/forbidden-patterns.md`: retrying a rate limit without
/// honouring its stated delay, and without a cap, is a spiral that makes the
/// limit worse. Both halves are here.
const BACKOFF_CAP: Duration = Duration::from_secs(30);

/// A registry caps a manifest well below this. The bound exists so a registry
/// that streams forever cannot exhaust memory: `docs/conventions/forbidden-patterns.md`
/// forbids buffering a whole body with no ceiling.
const MANIFEST_CEILING: u64 = 32 * 1024 * 1024;

/// A tags listing is names, not bytes: a thousand names fit in kilobytes,
/// and a megabyte ceiling leaves two orders of headroom before a runaway
/// body is refused rather than buffered.
const TAGS_CEILING: u64 = 1024 * 1024;

/// A token answer, and the error body of a registry, are read up to this.
const TOKEN_CEILING: u64 = 10 * 1024 * 1024;

/// A `null` tags list reads as empty: some registries send one for a
/// repository with no tags rather than `[]`.
fn null_to_default<'de, D>(d: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Vec<String>>::deserialize(d)?.unwrap_or_default())
}

/// The `rel="next"` target of a `Link` header, if one is present. A path
/// (what registries send for tags pagination), not a URL.
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

/// What a fetched manifest carries. ⭐ `bytes` is what the digest was computed
/// over, and the store writes those bytes verbatim: re-serialising a parsed
/// document produces a different digest and breaks the parity M1 is accepted on.
pub struct FetchedManifest {
    pub bytes: Vec<u8>,
    pub digest: Digest,
    pub media_type: String,
}

pub struct Client {
    /// ⚠ The transport (`pod-proxy`), or why it could not start. Each request
    /// takes the trust that the policy gives its endpoint, so a verified and
    /// an unverified request never share a connection: `pod-proxy` keeps no
    /// pool, and each request has its own.
    net: std::result::Result<Net, String>,
    pub policy: crate::transport::Policy,
    /// Endpoints whose HTTPS connect failed and which the policy permits over
    /// HTTP. ⛔ Only ever populated for a host the caller named insecure.
    http_endpoints: std::collections::BTreeSet<String>,
    /// One token per `<endpoint>|<scope>`. ⚠ Keyed on the scope as well as the
    /// host: a token minted for one repository does not authorise another, and
    /// a cache keyed on the host alone would send it anyway and read the 403 as
    /// a permission problem.
    tokens: HashMap<String, String>,
    pub roots: crate::tls::Roots,
}

impl Client {
    pub fn new() -> Client {
        Client::with_policy(crate::transport::Policy::default())
    }

    pub fn with_policy(policy: crate::transport::Policy) -> Client {
        let (config, roots) = crate::tls::client_config();
        // ⚠ The environment's proxy, when it sets one. An intercepting proxy
        // is the normal case in the environments podvm is built for, and
        // ignoring it produces a connect timeout rather than a refusal.
        // ⛔ A loopback registry is never reached through a proxy, and
        // `NO_PROXY` is honoured: `crate::net` and `pod-proxy` decide.
        Client {
            net: Net::from_environment(config),
            policy,
            http_endpoints: std::collections::BTreeSet::new(),
            tokens: HashMap::new(),
            roots,
        }
    }

    /// The transport, or the refusal that it could not start.
    fn net(&mut self) -> std::result::Result<&mut Net, Refusal> {
        self.net
            .as_mut()
            .map_err(|why| Refusal::Refused(why.clone()))
    }

    /// The base URL for an endpoint, `https://` unless this one has already
    /// been found to need `http://`.
    ///
    /// ⚠ The fallback is remembered per endpoint for the life of the client, so
    /// one failed HTTPS connect costs one timeout rather than one per blob.
    fn base(&self, endpoint: &str) -> String {
        let scheme = if self.http_endpoints.contains(endpoint) {
            "http"
        } else {
            "https"
        };
        format!("{scheme}://{endpoint}")
    }

    /// `GET /v2/<repository>/manifests/<selector>`.
    pub fn manifest(
        &mut self,
        endpoint: &str,
        repository: &str,
        selector: &str,
    ) -> Result<FetchedManifest> {
        // ⚠ A PATH, not a URL: `get` chooses the scheme from the endpoint's
        // transport policy, so nothing above it hard-codes `https://`.
        let path = format!("/v2/{repository}/manifests/{selector}");
        let scope = format!("repository:{repository}:pull");
        let resp = self.get(&path, endpoint, &scope, Some(oci::ACCEPT))?;
        let media_type = resp
            .header("Content-Type")
            .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());
        let declared = resp.header("Docker-Content-Digest").map(str::to_string);

        let mut bytes = Vec::new();
        // ⛔ `take` before `read_to_end`, so the ceiling bounds what is read
        // rather than being checked after the memory is already spent.
        resp.take(MANIFEST_CEILING + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Http {
                what: format!("GET {}", redact(&path)),
                detail: format!("reading the manifest body: {e}"),
            })?;
        if bytes.len() as u64 > MANIFEST_CEILING {
            return Err(Error::Http {
                what: format!("GET {}", redact(&path)),
                detail: format!("the manifest exceeds {MANIFEST_CEILING} bytes and was not read"),
            });
        }

        // ⭐ The digest podvm reports is COMPUTED over the bytes that arrived.
        // The header is checked against it and never substituted for it: this
        // is the value M1's acceptance compares with docker.
        let digest = Digest::of(&bytes);
        if let Some(declared) = declared {
            let declared = Digest::parse(&declared)?;
            if declared != digest {
                return Err(Error::DigestMismatch {
                    what: format!("manifest {selector} from {endpoint}/{repository}"),
                    want: declared.to_string(),
                    got: digest.to_string(),
                });
            }
        }
        Ok(FetchedManifest {
            bytes,
            digest,
            media_type: media_type.unwrap_or_default(),
        })
    }

    /// `GET /v2/<repository>/tags/list`, following `rel="next"` links.
    ///
    /// TODO/cli.md T-1331: `pull -a` fetches every offered tag, and the
    /// offer is this listing. One page asks for a thousand tags, so most
    /// registries answer once; the link walk covers the rest. The walk is
    /// bounded: a registry that links forever is a hang, and a runtime
    /// whose audience is automated may not wait unbounded.
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
            let mut bytes = Vec::new();
            resp.take(TAGS_CEILING + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| Error::Http {
                    what: format!("GET {}", redact(&path)),
                    detail: format!("reading the tags body: {e}"),
                })?;
            if bytes.len() as u64 > TAGS_CEILING {
                return Err(Error::Http {
                    what: format!("GET {}", redact(&path)),
                    detail: format!(
                        "the tags listing exceeds {TAGS_CEILING} bytes and was not read"
                    ),
                });
            }
            let listing: Listing = serde_json::from_slice(&bytes).map_err(|e| Error::Http {
                what: format!("GET {}", redact(&path)),
                detail: format!("the tags listing is not JSON: {e}"),
            })?;
            tags.extend(listing.tags);
            let Some(next) = link.and_then(|l| next_link(&l)) else {
                return Ok(tags);
            };
            // Registries send a path; an absolute URL is cut back to one
            // rather than appended to the base a second time.
            path = match next.find("/v2/") {
                Some(i) if next.starts_with("http") => next[i..].to_string(),
                _ => next,
            };
        }
        Err(Error::Http {
            what: format!("GET /v2/{repository}/tags/list"),
            detail: format!(
                "the listing paginates past {MAX_PAGES} pages and was not followed further"
            ),
        })
    }

    /// `GET /v2/<repository>/blobs/<digest>`, streamed into `sink` and verified
    /// against the descriptor as it is written.
    ///
    /// ⛔ Verify before use, not after download. The corpus at
    /// `references/qaidvoid__onelf/tree/crates/onelf-rt/src/main.rs:149-157`
    /// checks a payload's hash before an in-memory exec; a layer about to be
    /// extracted as root is the same case. The caller receives the sink back
    /// only when both the digest and the byte count matched.
    /// ⭐ **[`TODO/image.md`](../../../TODO/image.md) T-0214. The retry is around
    /// the WHOLE transfer and not around the request that starts it.**
    ///
    /// ⛔ [`Registry::get`] retries the request; a body that dies after the
    /// first byte is past it. Measured on 2026-09-09 by
    /// `experiments/240-distro-sweep.sh`: the `fedora` row reported `no-pull`
    /// twice, both times with `response body closed before all bytes were read`
    /// after **2,097,153** bytes -- two mebibytes and one byte, which is a proxy
    /// cutting a transfer rather than a registry ending one -- and both times
    /// the identical pull succeeded on the next attempt.
    ///
    /// ⛔ **A partial body is never committed and never counted**: the sink is
    /// restarted and a fresh [`Verifier`] hashes the next attempt, so the digest
    /// is always over one whole transfer.
    ///
    /// ⛔ **A `Range` request is NOT the mechanism.** Resuming at an offset means
    /// trusting that the prefix already on disk is the prefix of the blob podvm
    /// asked for, which is exactly what the digest exists to establish and
    /// cannot establish until the last byte.
    ///
    /// ⚠ Only a TRANSPORT failure is retried. A digest or size mismatch is a
    /// registry serving different bytes, and asking it again is a spiral.
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
        let mut last: Option<Error> = None;
        for attempt in 0..ATTEMPTS {
            if attempt > 0 {
                // ⛔ ANNOUNCED, one line. podvm's audience is automated, and a
                // pull that quietly took three tries is a link degrading with
                // nothing to show for it.
                let _ = writeln!(
                    out,
                    "{}: the body was cut short; retrying it, attempt {} of \
                     {ATTEMPTS}",
                    want.short(),
                    attempt + 1
                );
                std::thread::sleep(backoff(attempt, None));
                sink.restart()
                    .map_err(|e| Error::io(format!("restarting blob {want}"), e))?;
            }
            let resp = self.get(&path, endpoint, &scope, None)?;
            match Client::drain(resp, &mut sink, want, size, &path) {
                Ok(()) => return Ok(sink),
                // ⚠ Anything that is not a transport failure is the registry's
                // answer and not the link's.
                Err(e @ Error::Http { .. }) => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.expect("ATTEMPTS is at least 1 and the loop only exits on Ok or Err"))
    }

    /// One whole body, hashed and counted as it is written.
    ///
    /// ⛔ Reads are clamped so the staged file never passes the cap, which
    /// is the declared size bounded further by the file-size ceiling: a
    /// lying origin that streams past its descriptor is refused mid-stream
    /// instead of writing until the disk fills, and a body that would cross
    /// `RLIMIT_FSIZE` is refused instead of dying with SIGXFSZ. The refusal
    /// repeats the size shape `Verifier::finish` reports at the end.
    fn drain<W: Write>(
        resp: Body,
        sink: &mut W,
        want: &Digest,
        size: Option<u64>,
        path: &str,
    ) -> Result<()> {
        let mut verifier = Verifier::new(sink);
        let mut reader = resp;
        let ceiling = crate::space::file_ceiling().unwrap_or(u64::MAX);
        let cap = size.unwrap_or(u64::MAX).min(ceiling);
        let bound = if size.is_some_and(|s| s <= ceiling) {
            format!("the descriptor declares {} byte(s)", size.unwrap_or(0))
        } else {
            format!("the file-size ceiling is {ceiling} byte(s)")
        };
        let mut buf = vec![0u8; 128 * 1024];
        loop {
            let room = cap.saturating_sub(verifier.written());
            // One byte past the room proves the origin is still sending:
            // read it, refuse on it, and never write it. At exactly the
            // cap a final read answers EOF and the loop exits below.
            let take = if room == u64::MAX {
                buf.len()
            } else {
                (room + 1).min(buf.len() as u64) as usize
            };
            let n = reader.read(&mut buf[..take]).map_err(|e| Error::Http {
                what: format!("GET {}", redact(path)),
                detail: format!(
                    "reading the blob body after {} byte(s): {e}",
                    verifier.written()
                ),
            })?;
            if n == 0 {
                break;
            }
            if n as u64 > room {
                return Err(Error::Http {
                    what: format!("blob {want}"),
                    detail: format!(
                        "{bound} and {} byte(s) arrived with more sending; \
                         the partial was discarded",
                        verifier.written()
                    ),
                });
            }
            verifier
                .write_all(&buf[..n])
                .map_err(|e| Error::io(format!("blob {want}"), e))?;
        }
        verifier.finish(&format!("blob {want}"), want, size)?;
        Ok(())
    }
}

mod http;

impl Default for Client {
    fn default() -> Client {
        Client::new()
    }
}

mod wire;
pub use wire::*;

#[cfg(test)]
mod tests;
