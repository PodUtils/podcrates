//! The modes of one HTTP/3 path (PodUtils/podnet#486, QUIC-2): CONNECT-IP
//! (RFC 9484), CONNECT-UDP (RFC 9298) with its URI template, and a stream
//! CONNECT (RFC 9220) with the protocol of the caller.

use super::wire::{put_prefixed, put_string};

/// The URI template of CONNECT-UDP (RFC 9298, section 3): the client
/// expands it with the host and the port of the target.
pub const UDP_TEMPLATE: &str = "/.well-known/masque/udp/{target_host}/{target_port}/";

/// What a MASQUE request over HTTP/3 carries: the three modes of one path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// IP packets (RFC 9484), with `cf-connect-ip`.
    Ip,
    /// The payloads of one UDP target (RFC 9298), with `connect-udp` and
    /// the URI template.
    Udp { host: String, port: u16 },
    /// One stream (RFC 9220) with the protocol of the caller, as a
    /// WebTransport-like client uses it.
    Stream { protocol: String },
}

impl Mode {
    /// The `:protocol` of the request.
    pub fn protocol(&self) -> &str {
        match self {
            Mode::Ip => "cf-connect-ip",
            Mode::Udp { .. } => "connect-udp",
            Mode::Stream { protocol } => protocol,
        }
    }

    /// The `:path` of the request: `/` for IP packets and a stream, the
    /// expanded URI template for a UDP target.
    pub fn path(&self) -> String {
        match self {
            Mode::Ip | Mode::Stream { .. } => "/".to_string(),
            Mode::Udp { host, port } => udp_path(host, *port),
        }
    }

    /// Whether the payloads of the request go in HTTP datagrams, with the
    /// Context ID of the request.
    pub fn datagrams(&self) -> bool {
        !matches!(self, Mode::Stream { .. })
    }
}

/// The path of the template of CONNECT-UDP for `host` and `port`, with an
/// IPv6 host in no brackets: RFC 9298 takes the host and the port as the
/// template variables.
pub fn udp_path(host: &str, port: u16) -> String {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    format!("/.well-known/masque/udp/{host}/{port}/")
}

/// The field section of the extended CONNECT (RFC 9220) of CONNECT-IP, with
/// the `:protocol` of the caller ([`connect_headers`] with [`Mode::Ip`]).
pub fn connect_ip_headers(authority: &str, protocol: &str) -> Vec<u8> {
    let mode = match protocol {
        "cf-connect-ip" => Mode::Ip,
        other => Mode::Stream { protocol: other.to_string() },
    };
    connect_headers(authority, &mode)
}

/// The field section of a request in `mode` to `authority`. QPACK with the
/// static table only, no Huffman code.
pub fn connect_headers(authority: &str, mode: &Mode) -> Vec<u8> {
    // Required Insert Count 0, Base 0; :method CONNECT (static 15) and
    // :scheme https (static 23).
    let mut out = vec![0x00, 0x00, 0xc0 | 15, 0xc0 | 23];
    // :path (static name 1): the static "/" where the mode takes it.
    let path = mode.path();
    if path == "/" {
        out.push(0xc0 | 1);
    } else {
        put_prefixed(&mut out, 0x50, 4, 1);
        put_string(&mut out, 0x00, 7, &path);
    }
    // :authority (static name 0), a literal value.
    put_prefixed(&mut out, 0x50, 4, 0);
    put_string(&mut out, 0x00, 7, authority);
    // :protocol and the capsule protocol (RFC 9297).
    for (name, value) in [(":protocol", mode.protocol()), ("capsule-protocol", "?1")] {
        put_string(&mut out, 0x20, 3, name);
        put_string(&mut out, 0x00, 7, value);
    }
    // A hint of the WARP edge; a proxy of another provider ignores it.
    if *mode == Mode::Ip {
        put_string(&mut out, 0x20, 3, "pq-enabled");
        put_string(&mut out, 0x00, 7, "false");
    }
    out
}
