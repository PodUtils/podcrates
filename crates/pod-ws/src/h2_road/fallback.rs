//! How a session of [`connect`](super::connect) was carried, and why a
//! session took the upgrade of HTTP/1.1: the words of each road for a line
//! of `-v`, and the reason that the caller reads.

use super::REMEMBERED;

/// How a session of [`connect`](super::connect) is carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Road {
    /// A stream of an HTTP/2 connection; `shared` when another session took
    /// the connection before.
    Stream { shared: bool },
    /// The upgrade of HTTP/1.1, and why.
    Http1(Fallback),
}

/// Why a session took the upgrade of HTTP/1.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fallback {
    /// TLS chose `http/1.1`, or no protocol: the upgrade on that connection.
    NotChosen,
    /// HTTP/2's SETTINGS offered no extended CONNECT: a new connection.
    NotOffered,
    /// HTTP/2 did not carry the CONNECT, a reset or no answer to its PING:
    /// a new connection.
    Carried(String),
    /// The pool found one of these at this host within [`REMEMBERED`],
    /// so it asked for no HTTP/2.
    Remembered(Box<Fallback>),
    /// The CONNECT was answered 4xx: a proxy that refuses the extended
    /// CONNECT, so the upgrade of HTTP/1.1 takes the session on a new
    /// connection.
    Refused(u16),
    /// The caller's TLS configuration, which offers no ALPN.
    Caller,
}

impl Fallback {
    /// Why, in words.
    pub fn reason(&self) -> String {
        match self {
            Fallback::NotChosen => "the relay's TLS chose HTTP/1.1, not h2".into(),
            Fallback::NotOffered => "the relay's SETTINGS of HTTP/2 offer no extended CONNECT (RFC 8441)".into(),
            Fallback::Carried(why) => format!("HTTP/2 did not carry the CONNECT: {why}"),
            Fallback::Remembered(why) => {
                format!("{}, as this process found within {} minutes", why.reason(), REMEMBERED.as_secs() / 60)
            }
            Fallback::Refused(status) => format!(
                "the CONNECT was answered HTTP {status}: a proxy that refuses the extended CONNECT (RFC 8441) takes \
                 the upgrade of HTTP/1.1"
            ),
            Fallback::Caller => "the caller's TLS configuration offers no ALPN".into(),
        }
    }
}

impl Road {
    /// The road in the words of a line of `-v`, after the relay host's name;
    /// none for the upgrade on the one connection, as before the road.
    pub fn words(&self) -> Option<String> {
        Some(match self {
            Road::Stream { shared: false } => "over HTTP/2 (RFC 8441), on a connection that it opened".into(),
            Road::Stream { shared: true } => "over HTTP/2 (RFC 8441), on a connection that it shares".into(),
            Road::Http1(Fallback::NotChosen | Fallback::Caller) => return None,
            Road::Http1(why @ (Fallback::NotOffered | Fallback::Carried(_) | Fallback::Refused(_))) => {
                format!("over HTTP/1.1 on a new connection, as {}", why.reason())
            }
            Road::Http1(why) => format!("over HTTP/1.1, as {}", why.reason()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_road_has_its_words() {
        let words = |road: Road| road.words().unwrap_or_default();
        assert!(words(Road::Stream { shared: true }).contains("on a connection that it shares"));
        let new = words(Road::Http1(Fallback::NotOffered));
        assert!(new.starts_with("over HTTP/1.1 on a new connection, as the relay's SETTINGS"), "{new}");
        let again = words(Road::Http1(Fallback::Remembered(Box::new(Fallback::NotOffered))));
        assert!(again.starts_with("over HTTP/1.1, as the relay's SETTINGS"), "{again}");
        assert!(again.ends_with("as this process found within 10 minutes"), "{again}");
        assert_eq!(Road::Http1(Fallback::NotChosen).words(), None);
        assert_eq!(Fallback::NotChosen.reason(), "the relay's TLS chose HTTP/1.1, not h2");
    }
}
