//! The proxy that a URL names: HTTP `CONNECT` (`http://`, or a bare
//! `host:port`) or SOCKS5 (`socks5://` resolves the target's name on this
//! host, `socks5h://` gives the name to the proxy), with the user and the
//! password that it carries, which no message shows. The proxy is on TCP,
//! on a Unix socket (`SCHEME+unix:/PATH`, `SCHEME://unix:/PATH`), or at the
//! stdin and the stdout of a command (`SCHEME+exec:COMMAND`), which only a
//! value that the user gave on purpose can name ([`Proxy::parse_with`]).

mod parse;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

pub use parse::{HTTP_PORT, SOCKS_PORT, percent_decode};

use crate::host::authority;

/// How a proxy tunnels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyKind {
    /// HTTP `CONNECT`: `http://`, or a bare `host:port`.
    Http,
    /// SOCKS5 (RFC 1928). With `remote_dns`, the proxy resolves the name
    /// (`socks5h://`), so that a host with no DNS needs none; else this
    /// host resolves it and sends the address (`socks5://`), as curl does.
    Socks5 { remote_dns: bool },
}

impl ProxyKind {
    /// The scheme of a URL that names a proxy of this kind: `http`,
    /// `socks5` or `socks5h`.
    pub fn scheme(self) -> &'static str {
        match self {
            ProxyKind::Http => "http",
            ProxyKind::Socks5 { remote_dns: false } => "socks5",
            ProxyKind::Socks5 { remote_dns: true } => "socks5h",
        }
    }

    /// The kind as a word of a report: `http` or `socks5`.
    pub fn name(self) -> &'static str {
        match self {
            ProxyKind::Http => "http",
            ProxyKind::Socks5 { .. } => "socks5",
        }
    }

    /// The request that opens the tunnel, as a line that says how a
    /// connection was opened names it.
    pub fn request(self) -> &'static str {
        match self {
            ProxyKind::Http => "CONNECT",
            ProxyKind::Socks5 { .. } => "SOCKS5 CONNECT",
        }
    }
}

/// How the dial reaches the proxy itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Carrier {
    /// TCP to the host and the port of the proxy.
    Tcp,
    /// A Unix socket: `socks5h+unix:/PATH` or `socks5h://unix:/PATH`.
    Unix(PathBuf),
    /// The stdin and the stdout of a command (`sh -c`), one process for each
    /// connection, as the ProxyCommand of SSH: `http+exec:COMMAND`. Only from
    /// a value that the user gave on purpose ([`crate::Origin::User`]).
    Exec(String),
}

/// A proxy that tunnels: HTTP `CONNECT`, or SOCKS5 `CONNECT`.
#[derive(Clone, PartialEq, Eq)]
pub struct Proxy {
    /// The host of the proxy; empty for a proxy on a Unix socket or at a
    /// command.
    pub host: String,
    /// Its port; 0 for a proxy on a Unix socket or at a command.
    pub port: u16,
    pub kind: ProxyKind,
    /// The user and the password, each percent-decoded: HTTP Basic or
    /// Digest, or the user and password of RFC 1929. Never displayed.
    credentials: Option<(String, String)>,
    carrier: Carrier,
}

/// The name of [`Proxy`] before SOCKS5, kept for the crates and the
/// projects that name it.
pub type HttpProxy = Proxy;

/// The Debug form shows no credentials, and of a command one word only
/// (the program, not an assignment): they go to logs and reports.
impl std::fmt::Debug for Proxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Proxy")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("kind", &self.kind)
            .field("carrier", &self.carrier_text())
            .field("credentials", &self.credentials.as_ref().map(|_| "<hidden>"))
            .finish()
    }
}

/// `host:port` of a proxy on TCP, `unix:/PATH`, or `exec:WORD ...` (the
/// first word of a command that is not an assignment and holds no `@`; the
/// rest can hold a secret): the credentials never appear in a message.
impl std::fmt::Display for Proxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.carrier {
            Carrier::Tcp => f.write_str(&authority(&self.host, self.port)),
            _ => f.write_str(&self.carrier_text()),
        }
    }
}

impl Proxy {
    /// The user and the password, for the exchange with the proxy.
    pub(crate) fn credentials(&self) -> Option<(&str, &str)> {
        self.credentials.as_ref().map(|(user, password)| (user.as_str(), password.as_str()))
    }

    /// The protocol of the proxy.
    pub fn kind(&self) -> ProxyKind {
        self.kind
    }

    /// How the dial reaches the proxy.
    pub fn carrier(&self) -> &Carrier {
        &self.carrier
    }

    /// The proxy's own address. Connecting there is direct, never proxied.
    pub fn address(&self) -> (&str, u16) {
        (&self.host, self.port)
    }

    /// True when the proxy URL holds a user and a password.
    pub fn has_credentials(&self) -> bool {
        self.credentials.is_some()
    }

    /// `http`, `socks5` or `socks5h`.
    pub fn scheme(&self) -> &'static str {
        self.kind.scheme()
    }

    /// The kind of the proxy as the dial reaches it: the scheme, with
    /// `+unix` or `+exec` for a proxy that is not on TCP.
    pub fn kind_text(&self) -> String {
        match &self.carrier {
            Carrier::Tcp => self.scheme().to_string(),
            Carrier::Unix(_) => format!("{}+unix", self.scheme()),
            Carrier::Exec(_) => format!("{}+exec", self.scheme()),
        }
    }

    /// The proxy as a line names it, with no credentials:
    /// `socks5h://proxy.example:1080`, `socks5h+unix:/run/w.sock`,
    /// `http+exec:ssh ...`.
    pub fn label(&self) -> String {
        match self.carrier {
            Carrier::Tcp => format!("{}://{}", self.scheme(), authority(&self.host, self.port)),
            _ => format!("{}+{}", self.scheme(), self.carrier_text()),
        }
    }

    /// The word of the proxy in the decisions of an observer: `unix`, `exec`,
    /// `http` or `socks5`.
    pub fn outcome(&self) -> &'static str {
        match (&self.carrier, self.kind) {
            (Carrier::Unix(_), _) => "unix",
            (Carrier::Exec(_), _) => "exec",
            (Carrier::Tcp, kind) => kind.name(),
        }
    }

    fn carrier_text(&self) -> String {
        match &self.carrier {
            Carrier::Tcp => "tcp".into(),
            Carrier::Unix(path) => format!("unix:{}", path.display()),
            Carrier::Exec(command) => match command_word(command) {
                Some(word) => format!("exec:{word} ..."),
                None => "exec:...".into(),
            },
        }
    }

    /// The proxy as a URL at `addr`, its resolved address, with its
    /// credentials escaped again: for a library that takes a URL, as the
    /// iroh road's relay dial does. For a proxy on TCP; never for a message.
    pub fn url_at(&self, addr: std::net::SocketAddr) -> String {
        format!("{}://{}{addr}", self.kind.scheme(), self.userinfo())
    }

    /// The proxy as a URL by its name, with its port always written and its
    /// credentials escaped again: for a library that dials the proxy itself
    /// and reads a missing port its own way, as the tailnet fork does
    /// (8080). A proxy on a Unix socket or at a command has its own form.
    /// Never for a message.
    pub fn url(&self) -> String {
        match &self.carrier {
            Carrier::Tcp => {
                format!("{}://{}{}", self.kind.scheme(), self.userinfo(), authority(&self.host, self.port))
            }
            Carrier::Unix(path) => format!("{}+unix:{}", self.scheme(), path.display()),
            Carrier::Exec(command) => format!("{}+exec:{command}", self.scheme()),
        }
    }

    /// `user:password@`, escaped, or nothing.
    fn userinfo(&self) -> String {
        match &self.credentials {
            Some((user, password)) => {
                format!("{}:{}@", parse::percent_encode(user), parse::percent_encode(password))
            }
            None => String::new(),
        }
    }
}

/// The word of a command that a message shows: the first word that is not
/// an assignment (`TOKEN=VALUE cmd` sets a secret for `cmd`), unless it
/// holds `@`, as a user of a host or of a URL does.
fn command_word(command: &str) -> Option<&str> {
    command.split_whitespace().find(|word| !word.contains('=')).filter(|word| !word.contains('@'))
}
