//! Simulated RFC 5780 4-socket STUN server and broken/quirky server variants.

use std::net::SocketAddr;

use pod_turn::stun::{
    BINDING_ERROR, BINDING_REQUEST, BINDING_SUCCESS, CHANGE_IP, CHANGE_PORT, CHANGE_REQUEST, ERROR_CODE, OTHER_ADDRESS,
    RESPONSE_ORIGIN, UNKNOWN_ATTRIBUTES, XOR_MAPPED_ADDRESS_ALT, binding_success_nat, decode, message, write_address,
};

/// Behaviour mode of a simulated STUN server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerMode {
    /// Full RFC 5780 server on a 4-socket square (PP/PA/AP/AA), sending both XOR and plain mapped.
    Rfc5780,
    /// Plain RFC 8489 server (like Cloudflare / Google): sends `XOR-MAPPED-ADDRESS` only, no `OTHER-ADDRESS`.
    PlainXorOnly,
    /// Broken server: advertises `OTHER-ADDRESS`, but silently ignores `CHANGE-REQUEST` and replies from PP.
    IgnoresChangeRequest,
    /// Advertises `OTHER-ADDRESS`, but returns 420 Unknown Attributes for `CHANGE-REQUEST`.
    RefusesChangeRequest420,
    /// Honours `CHANGE-REQUEST`, but ignores `RESPONSE-PORT` and replies to the requesting socket port.
    IgnoresResponsePort,
    /// Emits non-standard `0x8020` instead of `0x0020`.
    AltXor8020,
    /// Alternate socket replies are silently firewalled (tests positive control guards).
    FirewalledAlt,
    /// Answers `CHANGE-REQUEST 0x06` from the wrong source (PP), but honours `0x02` (tests N-10).
    WrongSource06Honours02,
}

/// Simulated STUN server holding up to 4 socket addresses (PP, PA, AP, AA).
#[derive(Clone, Debug)]
pub struct SimStunServer {
    pub name: String,
    pub pp: SocketAddr,
    pub aa: Option<SocketAddr>,
    pub mode: ServerMode,
}

impl SimStunServer {
    /// Full RFC 5780 server with primary `(ip1, p1)` and alternate `(ip2, p2)`.
    pub fn rfc5780(name: impl Into<String>, pp: SocketAddr, aa: SocketAddr) -> Self {
        Self { name: name.into(), pp, aa: Some(aa), mode: ServerMode::Rfc5780 }
    }

    /// Plain STUN server with one address (no `OTHER-ADDRESS`, XOR-only).
    pub fn plain(name: impl Into<String>, pp: SocketAddr) -> Self {
        Self { name: name.into(), pp, aa: None, mode: ServerMode::PlainXorOnly }
    }

    pub fn pa(&self) -> Option<SocketAddr> {
        self.aa.map(|aa| SocketAddr::new(self.pp.ip(), aa.port()))
    }

    pub fn ap(&self) -> Option<SocketAddr> {
        self.aa.map(|aa| SocketAddr::new(aa.ip(), self.pp.port()))
    }

    /// True if `addr` is one of this server's listening sockets.
    pub fn owns(&self, addr: SocketAddr) -> bool {
        addr == self.pp || self.aa == Some(addr) || self.pa() == Some(addr) || self.ap() == Some(addr)
    }

    /// Handle an incoming STUN datagram received on `arrived_at` from `client_ext`.
    /// Returns `Some((reply_src, reply_dst, bytes))` or `None` if dropped.
    pub fn handle(
        &self,
        arrived_at: SocketAddr,
        client_ext: SocketAddr,
        bytes: &[u8],
    ) -> Option<(SocketAddr, SocketAddr, Vec<u8>)> {
        let msg = decode(bytes).ok()?;
        if msg.kind != BINDING_REQUEST {
            return None;
        }
        let change_flags = msg.change_request().and_then(Result::ok).unwrap_or(0);
        let resp_port = msg.response_port().and_then(Result::ok);

        if change_flags != 0 && self.mode == ServerMode::RefusesChangeRequest420 {
            let err_val = [0, 0, 4, 20, b'U', b'n', b'k', b'n'];
            let unk = CHANGE_REQUEST.to_be_bytes();
            let body = message(BINDING_ERROR, &msg.id, &[(ERROR_CODE, &err_val), (UNKNOWN_ATTRIBUTES, &unk)]);
            return Some((arrived_at, client_ext, body));
        }

        // Compute OTHER-ADDRESS as opposite of `arrived_at`.
        let other = match self.mode {
            ServerMode::PlainXorOnly => None,
            _ => self.aa.map(|aa| {
                let other_ip = if arrived_at.ip() == self.pp.ip() { aa.ip() } else { self.pp.ip() };
                let other_port = if arrived_at.port() == self.pp.port() { aa.port() } else { self.pp.port() };
                SocketAddr::new(other_ip, other_port)
            }),
        };

        // Determine reply source socket.
        let reply_src = if change_flags == 0 {
            arrived_at
        } else {
            match self.mode {
                ServerMode::PlainXorOnly | ServerMode::IgnoresChangeRequest => arrived_at,
                ServerMode::FirewalledAlt => return None,
                ServerMode::WrongSource06Honours02 => {
                    if (change_flags & CHANGE_IP) != 0 {
                        arrived_at
                    } else if let Some(oth) = other {
                        SocketAddr::new(arrived_at.ip(), oth.port())
                    } else {
                        arrived_at
                    }
                }
                _ => match other {
                    Some(oth) => {
                        let ip = if (change_flags & CHANGE_IP) != 0 { oth.ip() } else { arrived_at.ip() };
                        let port = if (change_flags & CHANGE_PORT) != 0 { oth.port() } else { arrived_at.port() };
                        SocketAddr::new(ip, port)
                    }
                    None => arrived_at,
                },
            }
        };

        // Determine destination port (RESPONSE-PORT support).
        let reply_dst = match (resp_port, self.mode) {
            (Some(p), ServerMode::IgnoresResponsePort | ServerMode::PlainXorOnly) => {
                let _ = p;
                client_ext
            }
            (Some(p), _) => SocketAddr::new(client_ext.ip(), p),
            (None, _) => client_ext,
        };

        let body = match self.mode {
            ServerMode::PlainXorOnly => binding_success_nat(&msg.id, Some(client_ext), None, None, None, None),
            ServerMode::AltXor8020 => {
                let alt = write_address(client_ext, Some(&msg.id));
                let mut attrs: Vec<(u16, Vec<u8>)> = vec![(XOR_MAPPED_ADDRESS_ALT, alt)];
                if let Some(o) = other {
                    attrs.push((OTHER_ADDRESS, write_address(o, None)));
                }
                attrs.push((RESPONSE_ORIGIN, write_address(reply_src, None)));
                let slices: Vec<(u16, &[u8])> = attrs.iter().map(|(k, v)| (*k, v.as_slice())).collect();
                message(BINDING_SUCCESS, &msg.id, &slices)
            }
            _ => binding_success_nat(&msg.id, Some(client_ext), Some(client_ext), other, Some(reply_src), None),
        };
        Some((reply_src, reply_dst, body))
    }
}
