//! The headers of the caller for a host: the credential of a gate in front
//! of the relay, such as the service token of Cloudflare Access
//! (`CF-Access-Client-Id`, `CF-Access-Client-Secret`). The caller gives a
//! function of the host in its [`crate::Way`] ([`Headers`]), and each road
//! writes what that function gives for the road's host, after the relay's
//! token.
//!
//! **Each name and each value is checked before a byte goes out.** A name
//! is an HTTP token and not a name that a road writes itself; a value has no
//! control character. A refusal names the header, never its value.
//!
//! **A value is never shown.** The values stay in memory that is wiped, no
//! `Debug` shows one, and the words of a refusal that quote one back are
//! masked ([`Extra::masked`]). The status of a refusal stays as it came.
//!
//! **A road in cleartext writes them only where the caller allows it**
//! ([`Headers::in_cleartext`]): each hop on such a road reads each header,
//! and a credential of a gate lives longer than a token of one session.

use std::sync::Arc;

pub use zeroize::Zeroizing;

use crate::client::ConnectError;

/// One header of the caller: its name, and its value in memory that is
/// wiped.
pub type Header = (&'static str, Zeroizing<String>);

/// The function of the caller: the headers for a host, as the road names
/// it (a name or an IP address, with no port and no brackets).
pub type ForHost = dyn Fn(&str) -> Vec<Header> + Send + Sync;

/// What replaces a value of a header in the words of a refusal.
pub const MASK: &str = "<not shown>";

/// The shortest start of a value that is masked at the end of a text that
/// was cut, so that a cut quote shows no part of a credential.
const MASK_TAIL: usize = 8;

/// The names that a road writes itself, which a header of the caller does
/// not replace or repeat, in lowercase.
const RESERVED: &[&str] = &[
    "host",
    "connection",
    "upgrade",
    "content-length",
    "transfer-encoding",
    "te",
    "trailer",
    "keep-alive",
    "proxy-connection",
    "user-agent",
    "x-relay-token",
    "x-podssh-proto",
];

/// The starts of the names that a road writes itself, in lowercase.
const RESERVED_STARTS: &[&str] = &["sec-websocket-"];

/// The headers of the caller for each host: none by default.
#[derive(Clone, Default)]
pub struct Headers {
    of: Option<Arc<ForHost>>,
    cleartext: bool,
}

impl std::fmt::Debug for Headers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Headers").field("given", &self.of.is_some()).field("in_cleartext", &self.cleartext).finish()
    }
}

impl Headers {
    /// The headers that `of` gives for each host.
    pub fn new(of: impl Fn(&str) -> Vec<Header> + Send + Sync + 'static) -> Headers {
        Headers::from_arc(Arc::new(of))
    }

    /// [`Headers::new`] of a function that the caller shares.
    pub fn from_arc(of: Arc<ForHost>) -> Headers {
        Headers { of: Some(of), cleartext: false }
    }

    /// The same headers, which a road in cleartext also writes where
    /// `allowed`. Off by default: each hop reads such a road.
    pub fn in_cleartext(mut self, allowed: bool) -> Headers {
        self.cleartext = allowed;
        self
    }

    /// Whether the caller gave no function.
    pub fn is_none(&self) -> bool {
        self.of.is_none()
    }

    /// The headers for `host`, checked.
    pub fn for_host(&self, host: &str) -> Result<Extra, ConnectError> {
        match &self.of {
            None => Ok(Extra::default()),
            Some(of) => Extra::checked(of(host)),
        }
    }

    /// The headers for `host` on a road in cleartext: none unless the
    /// caller allows it.
    pub fn for_cleartext(&self, host: &str) -> Result<Extra, ConnectError> {
        if self.cleartext { self.for_host(host) } else { Ok(Extra::default()) }
    }
}

/// The headers of the caller for one host, checked: what a road writes
/// after the relay's token. Clones share the values.
#[derive(Clone, Default)]
pub struct Extra(Arc<Vec<Header>>);

impl std::fmt::Debug for Extra {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Extra").field(&self.0.iter().map(|(name, _)| *name).collect::<Vec<_>>()).finish()
    }
}

impl Extra {
    /// `headers`, each checked: a name that is an HTTP token (RFC 9110,
    /// section 5.6.2), not a name that a road writes itself, and not twice;
    /// a value with no control character (CR, LF, NUL, another C0, DEL). A
    /// refusal names the header, never its value.
    pub fn checked(headers: Vec<Header>) -> Result<Extra, ConnectError> {
        let refused = |why: String| Err(ConnectError::Config(why));
        for (i, (name, value)) in headers.iter().enumerate() {
            if name.is_empty() || !name.bytes().all(is_tchar) {
                return refused(format!("the caller's header {name:?} is not a name of an HTTP header"));
            }
            let lower = name.to_ascii_lowercase();
            if RESERVED.contains(&lower.as_str()) || RESERVED_STARTS.iter().any(|s| lower.starts_with(s)) {
                return refused(format!("the caller's header {name} is one that the road writes itself"));
            }
            if headers[..i].iter().any(|(other, _)| other.eq_ignore_ascii_case(name)) {
                return refused(format!("the caller gives the header {name} twice"));
            }
            if value.bytes().any(|b| b < 0x20 || b == 0x7f) {
                return refused(format!("the value of the caller's header {name} holds a control character"));
            }
        }
        Ok(Extra(Arc::new(headers)))
    }

    /// Whether there is no header.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Each header, as a request writes it.
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &str)> {
        self.0.iter().map(|(name, value)| (*name, value.as_str()))
    }

    /// `(name, value)`, then each header: the list of a request that writes
    /// its own token or key first.
    pub fn after<'a>(&'a self, name: &'a str, value: &'a str) -> Vec<(&'a str, &'a str)> {
        let mut headers = Vec::with_capacity(self.0.len() + 1);
        headers.push((name, value));
        for (name, value) in self.iter() {
            headers.push((name, value));
        }
        headers
    }

    /// Refused where a header has one of `own`, the names that a road of
    /// another crate writes itself (in any case).
    pub fn refuse_names(&self, own: &[&str]) -> Result<(), ConnectError> {
        match self.0.iter().find(|(name, _)| own.iter().any(|o| o.eq_ignore_ascii_case(name))) {
            Some((name, _)) => {
                Err(ConnectError::Config(format!("the caller's header {name} is one that the road writes itself")))
            }
            None => Ok(()),
        }
    }

    /// Each header as a line of a head of HTTP/1.1, onto `out`.
    pub fn write(&self, out: &mut Vec<u8>) {
        for (name, value) in self.iter() {
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(value.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
    }

    /// `text` with each value replaced by [`MASK`], and the start of a
    /// value at its end too, where a quote was cut in a value.
    pub fn mask(&self, text: &str) -> String {
        let values: Vec<&str> = self.0.iter().map(|(_, v)| v.as_str()).filter(|v| !v.is_empty()).collect();
        let mut out = text.to_string();
        for value in &values {
            if out.contains(value) {
                out = out.replace(value, MASK);
            }
        }
        for value in &values {
            let (body, ellipsis) = match out.strip_suffix('…') {
                Some(body) => (body, "…"),
                None => (out.as_str(), ""),
            };
            let cut = (MASK_TAIL..value.len())
                .rev()
                .find(|&n| value.is_char_boundary(n) && body.ends_with(&value[..n]))
                .map(|n| format!("{}{MASK}{ellipsis}", &body[..body.len() - n]));
            if let Some(cut) = cut {
                out = cut;
            }
        }
        out
    }

    /// `e` with each value masked in the words that a peer or a request
    /// gave; the kind of `e` and the status of a refusal stay.
    pub fn masked(&self, e: ConnectError) -> ConnectError {
        if self.is_empty() {
            return e;
        }
        match e {
            ConnectError::Refused { status, body } => ConnectError::Refused { status, body: self.mask(&body) },
            ConnectError::Upgrade(why) => ConnectError::Upgrade(self.mask(&why)),
            ConnectError::Http(why) => ConnectError::Http(self.mask(&why)),
            ConnectError::Config(why) => ConnectError::Config(self.mask(&why)),
            ConnectError::Ended(why) => ConnectError::Ended(self.mask(&why)),
            other => other,
        }
    }
}

/// A byte of an HTTP token (RFC 9110, section 5.6.2).
fn is_tchar(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

#[cfg(test)]
#[path = "headers_tests.rs"]
mod tests;
