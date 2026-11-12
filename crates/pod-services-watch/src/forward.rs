//! `forward-url`: a session with an SSH forward relay, as podnet's
//! `ssh-relay` opens one: a key made for the run, the user of the service,
//! a remote forward, a shell, and the banner of the shell read with the
//! rule of the URL of the service (`pod_services::forward`). A banner with
//! no URL that the rule accepts is a finding. The forward goes to no local
//! server, and the session ends when the URL comes.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use pod_core::text::one_line;
use pod_services::forward::{Banner, Relay, UrlRule};
use pod_services::{Service, Watch};
use russh::client::{self, AuthResult, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::{Algorithm, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, Disconnect, Preferred, SshId};
use tokio::time::Instant;

use crate::args::{Args, named};
use crate::net::shown;
use crate::{Checked, Finding, Net, Outcome};

/// The bound of one session, from the dial to the URL: podnet's
/// `ssh-relay` waits as long for a URL.
pub const LIMIT: Duration = Duration::from_secs(45);
/// The SSH user of a relay with no fact `user`: such a relay takes any
/// user.
pub const USER: &str = "pod-services-watch";
/// The port of the remote forward, as the documents of the relays write
/// `ssh -R 80:localhost:PORT`.
const PORT: i64 = 80;
/// The address of the remote forward that OpenSSH sends for
/// `-R PORT:HOST:HOSTPORT`.
const ADDRESS: &str = "localhost";
/// The most bytes of each stream of the shell that a check keeps to show
/// in a finding.
const KEPT: usize = 8192;
/// The most characters of a banner with no URL in a finding.
const SHOWN: usize = 200;

/// The client of the session. It takes the host key of each relay: the
/// watch `ssh-host-key` checks the key, and this session sends no secret.
struct Client;

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(&mut self, _key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// `forward-url`: the banner of a session with `endpoint` holds a URL that
/// the rule of the service accepts. The arguments `port` (the remote
/// forward, 80 if none) and `seconds` (the bound, 45 if none) are optional.
pub async fn url(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let endpoint = args.endpoint()?;
    let relay = Relay::of(service).ok_or_else(|| {
        Finding::failed("the service is no forward relay: no fact url-match or url-skip, or no endpoint of plain SSH")
    })?;
    if !relay.endpoints().any(|e| e.name() == endpoint.name()) {
        return Err(Finding::failed(format!("the endpoint {:?} is not plain SSH", endpoint.name())));
    }
    let port = args.opt_int("port")?.unwrap_or(PORT);
    let port = u32::try_from(port)
        .ok()
        .filter(|p| *p <= u32::from(u16::MAX))
        .ok_or_else(|| Finding::failed(format!("the argument \"port\" is no port: {port}")))?;
    let limit = match args.opt_int("seconds")? {
        Some(s) => u64::try_from(s)
            .ok()
            .filter(|s| *s > 0)
            .map(Duration::from_secs)
            .ok_or_else(|| Finding::failed(format!("the argument \"seconds\" is no time: {s}")))?,
        None => LIMIT,
    };
    let user = relay.user().unwrap_or(USER);
    let session = Session {
        at: format!("{} as {user}", endpoint.authority()),
        user,
        port,
        limit,
        deadline: Instant::now() + limit,
    };
    let io = session
        .step("the connection", async { Ok::<_, russh::Error>(net.tcp(endpoint.host(), endpoint.port()).await) })
        .await??;
    let config = client::Config {
        client_id: SshId::Standard("SSH-2.0-pod-services-watch".into()),
        preferred: Preferred::DEFAULT,
        ..client::Config::default()
    };
    let mut handle = session.step("the SSH handshake", client::connect_stream(Arc::new(config), io, Client)).await?;
    let result = session.run(&mut handle, service, relay.url_rule()).await;
    // The close: the relay ends the tunnel with the session.
    let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
    let _ = tokio::time::timeout(net.limit, handle).await;
    result
}

/// One session with a relay.
struct Session<'a> {
    /// The endpoint and the user, for a line.
    at: String,
    user: &'a str,
    port: u32,
    limit: Duration,
    deadline: Instant,
}

impl Session<'_> {
    /// One step of the session, before the deadline.
    async fn step<T>(&self, what: &str, step: impl Future<Output = Result<T, russh::Error>>) -> Result<T, Finding> {
        match tokio::time::timeout_at(self.deadline, step).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(e)) => Err(Finding::failed(format!("{}: {what}: {e}", self.at))),
            Err(_) => Err(Finding::failed(format!("{}: no end of {what} within {:?}", self.at, self.limit))),
        }
    }

    /// The authentication, the forward, the shell and its banner.
    async fn run(&self, handle: &mut Handle<Client>, service: &Service, rule: UrlRule) -> Checked {
        let method = self.authenticate(handle).await?;
        let what = format!("the remote forward of port {}", self.port);
        self.step(&what, handle.tcpip_forward(ADDRESS, self.port)).await?;
        let mut channel = self.step("the session channel", handle.channel_open_session()).await?;
        self.step("the shell", channel.request_shell(true)).await?;
        let mut read = Read { banner: Banner::new(rule), out: Vec::new(), err: Vec::new(), refused: false };
        loop {
            let Ok(message) = tokio::time::timeout_at(self.deadline, channel.wait()).await else {
                return Err(read.no_url(self, service, &format!("no URL within {:?}", self.limit)));
            };
            let url = match message {
                Some(ChannelMsg::Data { data }) => {
                    keep(&mut read.out, &data);
                    read.banner.push(&data).map(str::to_string)
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    keep(&mut read.err, &data);
                    None
                }
                Some(ChannelMsg::Failure) => {
                    read.refused = true;
                    None
                }
                Some(ChannelMsg::Eof | ChannelMsg::Close) | None => match read.banner.finish() {
                    Some(url) => Some(url.to_string()),
                    None => return Err(read.no_url(self, service, "the relay closed the shell")),
                },
                Some(_) => None,
            };
            if let Some(url) = url {
                let mut out = Outcome::default();
                out.saw(format!("{} ({method}): the banner names the tunnel on the host {}", self.at, host_of(&url)));
                return Ok(out);
            }
        }
    }

    /// A key made for the run first, as podnet's leg; then no key, as
    /// OpenSSH tries first; then keyboard-interactive with an empty answer,
    /// as OpenSSH's anonymous login to Serveo goes. The key goes with the
    /// session.
    async fn authenticate(&self, handle: &mut Handle<Client>) -> Result<&'static str, Finding> {
        let key = PrivateKey::random(&mut russh::keys::key::safe_rng(), Algorithm::Ed25519)
            .map_err(|e| Finding::failed(format!("the key of the run: {e}")))?;
        let key = PrivateKeyWithHashAlg::new(Arc::new(key), None);
        if self.step("the authentication", handle.authenticate_publickey(self.user, key)).await?.success() {
            return Ok("a key of the run");
        }
        let none = self.step("the authentication", handle.authenticate_none(self.user)).await?;
        let methods = match none {
            AuthResult::Success => return Ok("no key"),
            AuthResult::Failure { remaining_methods, .. } => {
                remaining_methods.iter().map(<&str>::from).collect::<Vec<_>>().join(", ")
            }
        };
        if methods.split(", ").any(|m| m == "keyboard-interactive") && self.keyboard_interactive(handle).await? {
            return Ok("keyboard-interactive, no answer");
        }
        Err(Finding::failed(format!(
            "{}: the relay takes neither a key of the run, nor no key, nor an empty keyboard-interactive; it offers: \
             {methods}",
            self.at
        )))
    }

    /// An empty answer to each prompt, for a few rounds: a relay that lets
    /// anyone in asks nothing that a user must know.
    async fn keyboard_interactive(&self, handle: &mut Handle<Client>) -> Result<bool, Finding> {
        let what = "the authentication";
        let mut reply =
            self.step(what, handle.authenticate_keyboard_interactive_start(self.user, None::<String>)).await?;
        for _ in 0..4 {
            match reply {
                KeyboardInteractiveAuthResponse::Success => return Ok(true),
                KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
                KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                    let answers = vec![String::new(); prompts.len()];
                    reply = self.step(what, handle.authenticate_keyboard_interactive_respond(answers)).await?;
                }
            }
        }
        Ok(false)
    }
}

/// What a check read of the shell: the banner through the rule, and the
/// first bytes of each stream, to show in a finding.
struct Read {
    banner: Banner,
    out: Vec<u8>,
    err: Vec<u8>,
    /// The relay refused the shell.
    refused: bool,
}

impl Read {
    /// The finding of a banner with no URL that the rule accepts: the fact
    /// of the rule moved when the banner holds other URLs; else the session
    /// failed.
    fn no_url(&self, session: &Session<'_>, service: &Service, why: &str) -> Finding {
        let mut why = why.to_string();
        if self.refused {
            why.push_str("; the relay refused the shell");
        }
        let errors = urls(&self.err);
        if !errors.is_empty() {
            why.push_str(&format!("; its standard error names {}", errors.join(", ")));
        }
        let found = urls(&self.out);
        let fact = service.fact("url-match").or_else(|| service.fact("url-skip"));
        match fact {
            Some(fact) if !found.is_empty() => Finding::moved(
                named(fact),
                fact.value().to_string(),
                format!("a banner with no URL that the rule accepts ({why}): {}", found.join(", ")),
            ),
            _ => {
                let text: String = one_line(&String::from_utf8_lossy(&self.out)).chars().take(SHOWN).collect();
                Finding::failed(format!("{}: {why}, and the banner holds no URL: {text:?}", session.at))
            }
        }
    }
}

/// Keep `data` up to [`KEPT`] bytes.
fn keep(kept: &mut Vec<u8>, data: &[u8]) {
    let room = KEPT.saturating_sub(kept.len());
    kept.extend_from_slice(&data[..data.len().min(room)]);
}

/// Each URL of `text`, as the reader of a banner finds one, with no query:
/// a query can hold a token.
fn urls(text: &[u8]) -> Vec<String> {
    let any = UrlRule::default();
    text.split(|b| *b == b' ' || b.is_ascii_control())
        .filter_map(|word| any.pick(&String::from_utf8_lossy(word)))
        .map(|url| shown(&url).to_string())
        .collect()
}

/// The host of a URL: after the scheme, before a path, a query, a port or
/// a user.
fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    if host.starts_with('[') {
        return host.split_inclusive(']').next().unwrap_or(host);
    }
    host.split(':').next().unwrap_or(host)
}

#[cfg(test)]
mod tests {
    use super::{host_of, urls};

    #[test]
    fn the_host_of_a_url_has_no_port_no_user_and_no_path() {
        assert_eq!(host_of("https://abc.run.pinggy-free.link"), "abc.run.pinggy-free.link");
        assert_eq!(host_of("https://u@abc.lhr.life:8443/x?y"), "abc.lhr.life");
        assert_eq!(host_of("http://[2001:db8::1]:80/"), "[2001:db8::1]");
    }

    #[test]
    fn each_url_of_a_banner_is_shown_with_no_query() {
        let banner = b"Pro: https://dashboard.example.test.\r\n\x1b[4mhttps://a.zone.test/k?token=x\x1b[0m\r\n";
        assert_eq!(urls(banner), vec!["https://dashboard.example.test", "https://a.zone.test/k"]);
    }
}
