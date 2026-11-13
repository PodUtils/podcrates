//! The watches of `pod-services`: each `[[watch]]` of a data file names a
//! watch kind and its arguments, and each kind is one implementation of
//! [`Watch`] here. A check reads the live service through the way out of
//! [`Net`] (the dial and the proxy of `pod-proxy`, the TLS of `pod-tls`,
//! the DNS over HTTPS of `pod-dns`) and gives an [`Outcome`]: what it saw,
//! and each [`Finding`]: a fact that moved, with its old and its new value,
//! or a check that could not hold.
//!
//! A new watch kind is one implementation and its line in [`kinds`]. The
//! library reads no variable and no file and prints nothing: the runner
//! (`src/main.rs`) builds the [`Net`] and prints one line for each watch.

pub mod answer;
pub mod args;
mod crate_file;
mod dns;
mod forward;
mod github;
mod heard;
mod http;
mod net;
mod oci;
mod proxy;
mod rdap;
mod report;
pub mod schema;
mod sha;
mod ssh;
mod stun;
mod tls;
mod ws;
mod xhttp;

use std::future::Future;
use std::pin::Pin;

use pod_services::Service;

pub use net::{Bases, Net};
pub use report::Report;

/// The future of one check.
pub type Check<'a> = Pin<Box<dyn Future<Output = Outcome> + Send + 'a>>;

/// One watch kind: the check of one `[[watch]]` of a service against the
/// live service.
pub trait Watch: Send + Sync {
    /// The word of the kind in the data files (`http-json`).
    fn kind(&self) -> &'static str;

    /// Check `watch` of `service` through `net`.
    fn check<'a>(&'a self, service: &'a Service, watch: &'a pod_services::Watch, net: &'a Net) -> Check<'a>;
}

/// What a check found that is not as the file says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// The value of `fact` (a fact, or an endpoint as `endpoint NAME`) is
    /// `was` in the file, and the live service gives `now`.
    Moved { fact: String, was: String, now: String },
    /// The check could not hold: an answer of another kind, or a fault
    /// after the first byte of the far end.
    Failed(String),
    /// The far host gave no answer: a connect that timed out, no route, a
    /// connect that it refused or reset, or a TLS handshake that ended
    /// before its first byte. The way out ([`Net`]) gives this class where
    /// the failure is made. A scheduled run holds the first one (call C153,
    /// [`answer`]): a host that does not answer the addresses of one cloud
    /// can answer the next day from another.
    NoAnswer(String),
}

impl Finding {
    pub fn moved(fact: impl Into<String>, was: impl Into<String>, now: impl Into<String>) -> Finding {
        Finding::Moved { fact: fact.into(), was: was.into(), now: now.into() }
    }

    pub fn failed(why: impl Into<String>) -> Finding {
        Finding::Failed(why.into())
    }

    pub fn no_answer(why: impl Into<String>) -> Finding {
        Finding::NoAnswer(why.into())
    }

    /// The same finding with `prefix` before its words: a failure stays of
    /// its class.
    pub fn prefixed(self, prefix: &str) -> Finding {
        match self {
            Finding::Failed(why) => Finding::Failed(format!("{prefix}{why}")),
            Finding::NoAnswer(why) => Finding::NoAnswer(format!("{prefix}{why}")),
            moved => moved,
        }
    }
}

/// The result of one check: what it saw, and each finding. No finding: the
/// file holds. A check that cannot speak a form of the service (a
/// transport of a proxy link) is skipped, with why: no failure, and no
/// pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub seen: Vec<String>,
    pub findings: Vec<Finding>,
    pub skipped: Option<String>,
}

impl Outcome {
    /// True when the check found nothing.
    pub fn holds(&self) -> bool {
        self.findings.is_empty()
    }

    /// True when the check found something, and each finding is
    /// [`Finding::NoAnswer`].
    pub fn no_answer(&self) -> bool {
        !self.findings.is_empty() && self.findings.iter().all(|f| matches!(f, Finding::NoAnswer(_)))
    }

    /// One more line of what the check saw.
    pub fn saw(&mut self, line: impl Into<String>) {
        self.seen.push(line.into());
    }

    /// One more finding.
    pub fn found(&mut self, finding: Finding) {
        self.findings.push(finding);
    }

    /// The check did not run, for `why`.
    pub fn skip(&mut self, why: impl Into<String>) {
        self.skipped = Some(why.into());
    }
}

impl From<Finding> for Outcome {
    fn from(finding: Finding) -> Outcome {
        Outcome { seen: Vec::new(), findings: vec![finding], skipped: None }
    }
}

/// The result of a check that can stop at its first failure.
pub type Checked = Result<Outcome, Finding>;

/// One watch kind from an async function of its check.
macro_rules! kind {
    ($name:ident, $word:literal, $check:path) => {
        struct $name;
        impl Watch for $name {
            fn kind(&self) -> &'static str {
                $word
            }
            fn check<'a>(&'a self, service: &'a Service, watch: &'a pod_services::Watch, net: &'a Net) -> Check<'a> {
                Box::pin(async move { $check(service, watch, net).await.unwrap_or_else(Outcome::from) })
            }
        }
    };
}

kind!(HttpJson, "http-json", http::json);
kind!(HttpStatus, "http-status", http::status);
kind!(HttpText, "http-text", http::text);
kind!(GithubSshKeys, "github-ssh-keys", http::github_ssh_keys);
kind!(HttpSha256, "http-sha256", sha::check);
kind!(TlsName, "tls-name", tls::name);
kind!(TlsNames, "tls-names", tls::names);
kind!(DnsSrv, "dns-srv", dns::srv);
kind!(DnsAddresses, "dns-addresses", dns::addresses);
kind!(DohAnswer, "doh-answer", dns::doh_answer);
kind!(StunBinding, "stun-binding", stun::binding);
kind!(WebsocketUpgrade, "websocket-upgrade", ws::upgrade);
kind!(ForwardBanner, "forward-banner", ws::forward_banner);
kind!(ForwardUrl, "forward-url", forward::url);
kind!(ProxyBanner, "proxy-banner", proxy::banner);
kind!(SshBanner, "ssh-banner", ssh::banner);
kind!(SshHostKey, "ssh-host-key", ssh::host_key);
kind!(GithubRelease, "github-release", github::release);
kind!(GithubFiles, "github-files", github::files);
kind!(GithubFile, "github-file", github::file);
kind!(TlsRoots, "tls-roots", github::roots);
kind!(CrateFile, "crate-file", crate_file::check);
kind!(Rdap, "rdap", rdap::check);
kind!(OciPing, "oci-ping", oci::ping);
kind!(OciDigest, "oci-digest", oci::digest);

/// Each watch kind of this crate.
pub fn kinds() -> &'static [&'static dyn Watch] {
    &[
        &HttpJson,
        &HttpStatus,
        &HttpText,
        &GithubSshKeys,
        &HttpSha256,
        &TlsName,
        &TlsNames,
        &DnsSrv,
        &DnsAddresses,
        &DohAnswer,
        &StunBinding,
        &WebsocketUpgrade,
        &ForwardBanner,
        &ForwardUrl,
        &ProxyBanner,
        &SshBanner,
        &SshHostKey,
        &GithubRelease,
        &GithubFiles,
        &GithubFile,
        &TlsRoots,
        &CrateFile,
        &Rdap,
        &OciPing,
        &OciDigest,
    ]
}

/// The watch kind of `word`, or none.
pub fn find(word: &str) -> Option<&'static dyn Watch> {
    kinds().iter().copied().find(|w| w.kind() == word)
}

/// Check `watch` of `service`: a kind that this crate does not know is a
/// failure, never a pass; so is a fault of the arguments or of the facts
/// of the service's kind, before the network.
pub async fn run(service: &Service, watch: &pod_services::Watch, net: &Net) -> Report {
    let faults = schema::faults(service, watch, |id| net.services.get(id).is_some());
    let kind_faults = schema::service_faults(service);
    let outcome = match find(watch.kind()) {
        Some(_) if !faults.is_empty() => Finding::failed(format!("the arguments: {}", faults.join("; "))).into(),
        Some(_) if !kind_faults.is_empty() => {
            Finding::failed(format!("the facts of the kind: {}", kind_faults.join("; "))).into()
        }
        Some(kind) => kind.check(service, watch, net).await,
        None => Finding::failed(format!("pod-services-watch has no watch kind {:?}", watch.kind())).into(),
    };
    Report::new(service, watch, outcome)
}
