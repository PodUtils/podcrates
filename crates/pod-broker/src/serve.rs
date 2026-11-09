//! One connection of a job: the head of its request, its token, its target,
//! the checks, the one resolve, the dial of a checked address, and the
//! tunnel or the plain request. Each connection writes one audit line.
//!
//! The answers (the words of PodUtils/podvm#184): 400 a malformed or an
//! ambiguous request, 407 no token or a token of no job, 421 a CONNECT
//! target and a `Host` that differ, 403 a refusal of a deny or of the
//! policy, 502 the far end or the upstream failed, 504 the far end did not
//! answer in time.

use std::net::SocketAddr;
use std::time::Duration;

use base64::Engine as _;
use pod_core::{BoxedIo, Class, Failure, Stage, Target};
use pod_proxy::http::request::{Request, established, read_request, refusal as refusal_bytes};
use tokio::io::AsyncWriteExt;
use tokio::time::Instant;
use zeroize::Zeroizing;

use crate::audit::Line;
use crate::broker::Broker;
use crate::deny::{DenyList, check_port};
use crate::forward::{exchange, head, request_framing};
use crate::jobs::Lease;
use crate::policy::{PATH_NEEDS_TLS_END, Verdict};
use crate::prefixed::Prefixed;
use crate::target::{Authority, Host, HttpTarget, parse_absolute, parse_authority};
use crate::tunnel::{Counts, Tunnel};

/// A refusal: its status, its rule and its class.
#[derive(Debug)]
struct Refusal {
    status: u16,
    rule: String,
    stage: Stage,
    class: Class,
    words: String,
}

fn refuse(status: u16, rule: impl Into<String>, class: Class, words: impl Into<String>) -> Refusal {
    Refusal { status, rule: rule.into(), stage: Stage::Proxy, class, words: words.into() }
}

fn of_failure(f: &Failure, rule: &str) -> Refusal {
    let status = if f.class == Class::Timeout { 504 } else { 502 };
    Refusal { status, rule: rule.into(), stage: f.stage, class: f.class, words: f.chain() }
}

/// What the audit line of a connection says.
struct Note {
    started: Instant,
    job: Option<String>,
    kind: &'static str,
    target: Option<Authority>,
    addr: Option<SocketAddr>,
}

impl Note {
    fn line(&self, level: &'static str) -> Line {
        let mut line = Line::new(level, "broker.connection");
        line.set("job", self.job.clone()).set("kind", self.kind);
        if let Some(target) = &self.target {
            line.set("host", target.host.to_string()).set("port", target.port);
        }
        line.set("addr", self.addr.map(|a| a.to_string()));
        line
    }
}

/// The checked way to a target: the addresses that passed each deny.
struct Checked {
    addrs: Vec<SocketAddr>,
    rule: String,
}

/// The next step after the checks.
enum Next {
    Tunnel { authority: Authority },
    Forward { target: HttpTarget, framing: pod_proxy::http::Framing },
}

impl Broker {
    /// Serve one connection of a job (from [`Broker::serve_tcp`],
    /// [`Broker::serve_unix`], or a listener of the host).
    pub async fn handle(&self, mut io: BoxedIo) {
        let mut note = Note { started: Instant::now(), job: None, kind: "request", target: None, addr: None };
        let read = tokio::time::timeout(self.settings.head_timeout, read_request(&mut io, Vec::new())).await;
        let (request, rest) = match read {
            Ok(Ok(read)) => read,
            Ok(Err(e)) => {
                let refusal = refuse(e.status().unwrap_or(400), "request", Class::Usage, e.to_string());
                return self.refused(&mut io, &note, refusal, e.status().is_some()).await;
            }
            Err(_) => {
                let refusal = refuse(408, "request:timeout", Class::Timeout, "no request in time");
                return self.refused(&mut io, &note, refusal, true).await;
            }
        };
        let lease = match self.lease(&request) {
            Ok(lease) => lease,
            Err(refusal) => return self.refused(&mut io, &note, refusal, true).await,
        };
        note.job = Some(lease.job.clone());
        let next = match self.next(&request, &mut note) {
            Ok(next) => next,
            Err(refusal) => return self.refused(&mut io, &note, refusal, true).await,
        };
        let path = match &next {
            Next::Forward { target, .. } => Some(target.path.as_str()),
            Next::Tunnel { .. } => None,
        };
        let authority = note.target.clone().expect("a target after the checks");
        let checked = match self.check(&lease, &authority, path, &mut note).await {
            Ok(checked) => checked,
            Err(refusal) => return self.refused(&mut io, &note, refusal, true).await,
        };
        let upstream = match self.connect(&authority, &checked, &mut note).await {
            Ok(upstream) => upstream,
            Err(refusal) => return self.refused(&mut io, &note, refusal, true).await,
        };
        let counts = Counts::default();
        let result = match next {
            Next::Tunnel { authority } => {
                if let Err(e) = io.write_all(&established()).await {
                    return self.ended(&note, &checked.rule, 200, &counts, Some(Failure::io(Stage::Proxy, e)));
                }
                let client: BoxedIo = Box::new(Prefixed::new(rest, io));
                let tunnel = Tunnel {
                    job: &lease.job,
                    authority: &authority,
                    addr: note.addr,
                    policy: &lease.policy,
                    rule: &checked.rule,
                    counts: &counts,
                };
                tokio::select! {
                    r = self.end.run(tunnel, client, upstream) => r.err(),
                    () = lease.ended() => Some(Failure::new(Stage::Proxy, Class::Revoked, "the job ended")),
                }
            }
            Next::Forward { target, framing } => {
                let client: BoxedIo = Box::new(Prefixed::new(rest, io));
                let head = head(&request, &target);
                tokio::select! {
                    r = exchange(client, upstream, head, framing, &counts) => r.err().map(|e| Failure::io(Stage::Origin, e)),
                    () = lease.ended() => Some(Failure::new(Stage::Proxy, Class::Revoked, "the job ended")),
                }
            }
        };
        let status = if note.kind == "connect" { 200 } else { 0 };
        self.ended(&note, &checked.rule, status, &counts, result);
    }

    /// The job of the request's `Proxy-Authorization`: `Bearer TOKEN`, or
    /// `Basic` with the token as the password and the job id (or nothing) as
    /// the user.
    fn lease(&self, request: &Request) -> Result<Lease, Refusal> {
        let no = |words: &str| refuse(407, "auth", Class::Refused, words);
        let values: Vec<&str> =
            request.headers.iter().filter(|(n, _)| n == "proxy-authorization").map(|(_, v)| v.as_str()).collect();
        let [value] = values[..] else { return Err(no("give one Proxy-Authorization with the job token")) };
        let (scheme, credential) = value.split_once(' ').unwrap_or((value, ""));
        let credential = credential.trim();
        let (user, token) = if scheme.eq_ignore_ascii_case("bearer") {
            (None, Zeroizing::new(credential.to_string()))
        } else if scheme.eq_ignore_ascii_case("basic") {
            let raw =
                base64::engine::general_purpose::STANDARD.decode(credential).map_err(|_| no("a bad Basic value"))?;
            let raw = Zeroizing::new(raw);
            let text = std::str::from_utf8(&raw).map_err(|_| no("a bad Basic value"))?;
            let (user, pass) = text.split_once(':').ok_or_else(|| no("a bad Basic value"))?;
            (Some(user.to_string()), Zeroizing::new(pass.to_string()))
        } else {
            return Err(no("the scheme is Basic or Bearer"));
        };
        let lease = self.jobs.find(&token).ok_or_else(|| no("the token is the token of no job"))?;
        match user {
            Some(user) if !user.is_empty() && user != lease.job => Err(no("the user is not the job of the token")),
            _ => Ok(lease),
        }
    }

    /// The target of the request, checked against its `Host` (421).
    fn next(&self, request: &Request, note: &mut Note) -> Result<Next, Refusal> {
        let ambiguous = |words: String| refuse(400, "target", Class::Usage, words);
        let hosts: Vec<&str> = request.headers.iter().filter(|(n, _)| n == "host").map(|(_, v)| v.as_str()).collect();
        if hosts.len() > 1 {
            return Err(ambiguous("a request has one Host".into()));
        }
        let (authority, next) = if request.method == "CONNECT" {
            note.kind = "connect";
            let authority = parse_authority(&request.target, None).map_err(|e| ambiguous(e.0))?;
            (authority.clone(), Next::Tunnel { authority })
        } else {
            note.kind = "http";
            let target = parse_absolute(&request.target).map_err(|e| ambiguous(e.0))?;
            let framing = request_framing(request).map_err(ambiguous)?;
            (target.authority.clone(), Next::Forward { target, framing })
        };
        note.target = Some(authority.clone());
        if let Some(host) = hosts.first() {
            let named = parse_authority(host, Some(authority.port)).map_err(|e| ambiguous(e.0))?;
            if named != authority {
                return Err(refuse(421, "host-mismatch", Class::Usage, "the Host is not the target of the request"));
            }
        }
        Ok(next)
    }

    fn denies<'a>(&'a self, lease: &'a Lease) -> impl Iterator<Item = &'a DenyList> {
        self.denies.iter().chain(std::iter::once(lease.policy.deny()))
    }

    /// The denies of the port and the host, the policy, then the one resolve
    /// and the denies of each address.
    async fn check(
        &self,
        lease: &Lease,
        target: &Authority,
        path: Option<&str>,
        note: &mut Note,
    ) -> Result<Checked, Refusal> {
        let denied = |rule: &str| refuse(403, rule, Class::Policy, format!("denied by the rule {rule}"));
        if let Some(rule) = check_port(target.port) {
            return Err(denied(&rule));
        }
        if let Some(rule) = self.denies(lease).find_map(|d| d.check_host(&target.host)) {
            return Err(denied(rule));
        }
        let rule = match lease.policy.verdict(target, path) {
            Verdict::Allow(rule) => rule,
            Verdict::PathsOnly if self.end.reads_requests() => PATH_NEEDS_TLS_END.to_string(),
            Verdict::PathsOnly => return Err(denied(PATH_NEEDS_TLS_END)),
            Verdict::Deny(rule) => return Err(denied(&rule)),
        };
        let addrs = match &target.host {
            Host::Ip(ip) => vec![SocketAddr::new(*ip, target.port)],
            Host::Name(name) => self.resolve_once(name, target.port).await?,
        };
        for addr in &addrs {
            if let Some(rule) = self.denies(lease).find_map(|d| d.check_ip(addr.ip())) {
                note.addr = Some(*addr);
                return Err(denied(rule));
            }
        }
        Ok(Checked { addrs, rule })
    }

    /// The addresses of `name`, from one call of the resolver: the broker
    /// dials only these.
    async fn resolve_once(&self, name: &str, port: u16) -> Result<Vec<SocketAddr>, Refusal> {
        let wait = self.settings.resolve_timeout;
        let deadline = Instant::now() + wait;
        let found = tokio::time::timeout(wait, self.resolve.resolve(name, port, deadline)).await;
        let addrs = match found {
            Ok(Ok(addrs)) => addrs,
            Ok(Err(f)) => return Err(of_failure(&f, "resolve")),
            Err(_) => return Err(refuse(504, "resolve", Class::Timeout, format!("no address of {name} in time"))),
        };
        if addrs.is_empty() {
            return Err(Refusal {
                status: 502,
                rule: "resolve".into(),
                stage: Stage::Dns,
                class: Class::NotFound,
                words: format!("{name} has no address"),
            });
        }
        Ok(addrs.into_iter().map(|a| SocketAddr::new(crate::deny::plain(a.ip()), port)).collect())
    }

    /// The first checked address that connects, through the way out.
    async fn connect(&self, target: &Authority, checked: &Checked, note: &mut Note) -> Result<BoxedIo, Refusal> {
        let wait = self.settings.connect_timeout;
        let deadline = Instant::now() + wait;
        let mut last = None;
        for addr in &checked.addrs {
            note.addr = Some(*addr);
            let left = deadline.saturating_duration_since(Instant::now());
            let dialed = tokio::time::timeout(left, self.dial.dial(&Target::Addr(*addr), deadline)).await;
            match dialed {
                Ok(Ok(connected)) => return Ok(connected.io),
                Ok(Err(f)) => last = Some(of_failure(&f, "upstream")),
                Err(_) => {
                    last = Some(refuse(504, "upstream", Class::Timeout, format!("{target} did not answer in time")));
                    break;
                }
            }
        }
        Err(last.unwrap_or_else(|| refuse(502, "upstream", Class::Unavailable, "no address")))
    }

    /// Write the line of `refusal`, then answer it when the client can read
    /// it: a client that reads the answer finds the line there.
    async fn refused(&self, io: &mut BoxedIo, note: &Note, refusal: Refusal, answer: bool) {
        let mut line = note.line("warn");
        line.set("decision", "deny").set("rule", refusal.rule.as_str()).set("status", refusal.status);
        line.set("ms", millis(note.started.elapsed()));
        line.failed(refusal.stage, refusal.class);
        tracing::debug!(status = refusal.status, words = %pod_core::text::one_line(&refusal.words), "refused");
        self.audit.line(&line);
        if answer {
            let headers: &[(&str, &str)] =
                if refusal.status == 407 { &[("Proxy-Authenticate", "Basic realm=\"pod-broker\"")] } else { &[] };
            let words = if refusal.status >= 500 { "the far end failed" } else { refusal.words.as_str() };
            let _ = io.write_all(&refusal_bytes(refusal.status, words, headers)).await;
            let _ = io.shutdown().await;
        }
    }

    /// The line of a connection that the checks allowed.
    fn ended(&self, note: &Note, rule: &str, status: u16, counts: &Counts, failure: Option<Failure>) {
        let mut line = note.line("info");
        line.set("decision", "allow").set("rule", rule);
        if status != 0 {
            line.set("status", status);
        }
        line.set("up", counts.up()).set("down", counts.down()).set("ms", millis(note.started.elapsed()));
        if let Some(f) = failure {
            line.set("end", if f.class == Class::Revoked { "job-ended" } else { "failed" });
            line.failed(f.stage, f.class);
        }
        self.audit.line(&line);
    }
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}
