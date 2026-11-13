//! The packets of MQTT 3.1.1 (OASIS Standard, 29 October 2014) that a
//! meeting point speaks: `CONNECT`, `CONNACK`, `PUBLISH`, `SUBSCRIBE`,
//! `SUBACK`, `PINGREQ`, `PINGRESP` and `DISCONNECT`. No function here does
//! I/O, so a fuzz target runs the reader on untrusted bytes. The reader
//! refuses a malformed packet (section 4.8): a remaining length of more
//! than four bytes or past the caller's bound, flags that the standard
//! reserves, a string that is no UTF-8, holds U+0000 or runs past its
//! packet, and bytes after the end of a packet of a fixed form.

use std::fmt;

/// The name and the level of the protocol in a `CONNECT` (section 3.1.2).
pub const PROTOCOL_NAME: &str = "MQTT";
pub const LEVEL: u8 = 4;
/// The largest remaining length that four bytes hold (section 2.2.3).
pub const LENGTH_MOST: usize = 268_435_455;
/// A `SUBACK` code of a refused filter (section 3.9.3).
pub const REFUSED: u8 = 0x80;

/// One packet. A `PUBLISH` is written at QoS 0 with no duplicate flag,
/// and with the retain flag of `retain`; one read at QoS 1 or 2 gives its
/// topic, its payload and its retain flag, its packet identifier dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    Connect(Connect),
    /// The session present flag and the return code (0: accepted).
    ConnAck {
        present: bool,
        code: u8,
    },
    /// `retain`: the broker keeps the message for each later subscriber
    /// to the topic, and an empty one clears the topic (section 3.3.1.3).
    Publish {
        topic: String,
        payload: Vec<u8>,
        retain: bool,
    },
    /// The packet identifier and each topic filter with its QoS.
    Subscribe {
        id: u16,
        filters: Vec<(String, u8)>,
    },
    /// The packet identifier and a return code for each filter.
    SubAck {
        id: u16,
        codes: Vec<u8>,
    },
    PingReq,
    PingResp,
    Disconnect,
}

/// What a `CONNECT` carries: no will, which a meeting point does not use.
#[derive(Clone, PartialEq, Eq)]
pub struct Connect {
    pub client_id: String,
    /// Seconds (section 3.1.2.10).
    pub keep_alive: u16,
    /// A clean session: the broker keeps no state of this client.
    pub clean: bool,
    /// The user name and the password.
    pub login: Option<(String, Vec<u8>)>,
}

impl fmt::Debug for Connect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The password is a credential: its presence alone shows.
        let login = self.login.as_ref().map(|(user, _)| (user, "..."));
        f.debug_struct("Connect")
            .field("client_id", &self.client_id)
            .field("keep_alive", &self.keep_alive)
            .field("clean", &self.clean)
            .field("login", &login)
            .finish()
    }
}

/// Why bytes are no packet, or a packet cannot be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Malformed(pub String);

impl fmt::Display for Malformed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Malformed {}

fn malformed<T>(why: impl Into<String>) -> Result<T, Malformed> {
    Err(Malformed(why.into()))
}

/// The name of the type of a packet, for the words of a failure.
pub fn kind_name(kind: u8) -> &'static str {
    const NAMES: [&str; 16] = [
        "reserved (0)",
        "CONNECT",
        "CONNACK",
        "PUBLISH",
        "PUBACK",
        "PUBREC",
        "PUBREL",
        "PUBCOMP",
        "SUBSCRIBE",
        "SUBACK",
        "UNSUBSCRIBE",
        "UNSUBACK",
        "PINGREQ",
        "PINGRESP",
        "DISCONNECT",
        "reserved (15)",
    ];
    NAMES[usize::from(kind & 0x0f)]
}

impl Packet {
    /// The name of the type, as the standard writes it.
    pub fn name(&self) -> &'static str {
        kind_name(match self {
            Packet::Connect(_) => 1,
            Packet::ConnAck { .. } => 2,
            Packet::Publish { .. } => 3,
            Packet::Subscribe { .. } => 8,
            Packet::SubAck { .. } => 9,
            Packet::PingReq => 12,
            Packet::PingResp => 13,
            Packet::Disconnect => 14,
        })
    }
}

/// The remaining length in one to four bytes (section 2.2.3).
pub fn encode_length(mut length: usize, out: &mut Vec<u8>) -> Result<(), Malformed> {
    if length > LENGTH_MOST {
        return malformed(format!("a packet of {length} bytes, over the {LENGTH_MOST} that MQTT carries"));
    }
    loop {
        let mut byte = (length % 128) as u8;
        length /= 128;
        if length > 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if length == 0 {
            return Ok(());
        }
    }
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), Malformed> {
    let Ok(length) = u16::try_from(bytes.len()) else {
        return malformed(format!("a string of {} bytes, over 65535", bytes.len()));
    };
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

/// The bytes of `packet`.
pub fn encode(packet: &Packet) -> Result<Vec<u8>, Malformed> {
    let mut body = Vec::new();
    let first = match packet {
        Packet::Connect(c) => {
            put_bytes(&mut body, PROTOCOL_NAME.as_bytes())?;
            body.push(LEVEL);
            let mut flags = if c.clean { 0x02 } else { 0 };
            if c.login.is_some() {
                flags |= 0xc0;
            }
            body.push(flags);
            body.extend_from_slice(&c.keep_alive.to_be_bytes());
            put_bytes(&mut body, c.client_id.as_bytes())?;
            if let Some((user, password)) = &c.login {
                put_bytes(&mut body, user.as_bytes())?;
                put_bytes(&mut body, password)?;
            }
            0x10
        }
        Packet::ConnAck { present, code } => {
            body.extend_from_slice(&[u8::from(*present), *code]);
            0x20
        }
        Packet::Publish { topic, payload, retain } => {
            put_bytes(&mut body, topic.as_bytes())?;
            body.extend_from_slice(payload);
            0x30 | u8::from(*retain)
        }
        Packet::Subscribe { id, filters } => {
            body.extend_from_slice(&id.to_be_bytes());
            for (filter, qos) in filters {
                put_bytes(&mut body, filter.as_bytes())?;
                body.push(*qos);
            }
            0x82
        }
        Packet::SubAck { id, codes } => {
            body.extend_from_slice(&id.to_be_bytes());
            body.extend_from_slice(codes);
            0x90
        }
        Packet::PingReq => 0xc0,
        Packet::PingResp => 0xd0,
        Packet::Disconnect => 0xe0,
    };
    let mut out = vec![first];
    encode_length(body.len(), &mut out)?;
    out.extend_from_slice(&body);
    Ok(out)
}

/// The first packet of `bytes` and the bytes that it took; `None` while
/// the packet is not whole. A remaining length over `most` is refused as
/// soon as its bytes are read, so that a reader keeps at most `most` bytes
/// of one packet.
pub fn decode(bytes: &[u8], most: usize) -> Result<Option<(Packet, usize)>, Malformed> {
    let Some(&first) = bytes.first() else { return Ok(None) };
    let (mut length, mut factor, mut at) = (0usize, 1usize, 1usize);
    loop {
        let Some(&byte) = bytes.get(at) else { return Ok(None) };
        length += usize::from(byte & 0x7f) * factor;
        at += 1;
        if byte & 0x80 == 0 {
            break;
        }
        if at == 5 {
            return malformed("a remaining length of more than four bytes");
        }
        factor *= 128;
    }
    if length > most {
        return malformed(format!("a packet of {length} bytes, over the bound of {most}"));
    }
    let Some(body) = bytes.get(at..at + length) else { return Ok(None) };
    let packet = read(first, body)?;
    Ok(Some((packet, at + length)))
}

/// The reader of the body of one packet.
struct Body<'a> {
    bytes: &'a [u8],
    kind: &'static str,
}

impl<'a> Body<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Malformed> {
        if self.bytes.len() < n {
            return malformed(format!("a {} that ends inside a field", self.kind));
        }
        let (taken, rest) = self.bytes.split_at(n);
        self.bytes = rest;
        Ok(taken)
    }

    fn byte(&mut self) -> Result<u8, Malformed> {
        Ok(self.take(1)?[0])
    }

    fn two(&mut self) -> Result<u16, Malformed> {
        let pair = self.take(2)?;
        Ok(u16::from_be_bytes([pair[0], pair[1]]))
    }

    fn binary(&mut self) -> Result<&'a [u8], Malformed> {
        let length = self.two()?;
        self.take(usize::from(length))
    }

    /// A string of UTF-8 with no U+0000 (section 1.5.3).
    fn text(&mut self) -> Result<String, Malformed> {
        let kind = self.kind;
        let bytes = self.binary()?;
        match std::str::from_utf8(bytes) {
            Ok(text) if !text.contains('\0') => Ok(text.to_string()),
            _ => malformed(format!("a {kind} with a string that is no UTF-8 or that holds U+0000")),
        }
    }

    fn end(&self) -> Result<(), Malformed> {
        if self.bytes.is_empty() { Ok(()) } else { malformed(format!("a {} with bytes after its end", self.kind)) }
    }
}

fn read(first: u8, bytes: &[u8]) -> Result<Packet, Malformed> {
    let (kind, flags) = (first >> 4, first & 0x0f);
    let mut body = Body { bytes, kind: kind_name(kind) };
    // The flags of each type but PUBLISH are fixed (section 2.2.2).
    let fixed = match kind {
        3 => None,
        8 => Some(0x02),
        1 | 2 | 9 | 12 | 13 | 14 => Some(0),
        _ => return malformed(format!("a packet of the type {}, which a meeting point does not speak", body.kind)),
    };
    if fixed.is_some_and(|f| f != flags) {
        return malformed(format!("a {} with the reserved flags {flags:#06b}", body.kind));
    }
    let packet = match kind {
        1 => Packet::Connect(connect(&mut body)?),
        2 => {
            let (acknowledge, code) = (body.byte()?, body.byte()?);
            if acknowledge & 0xfe != 0 {
                return malformed("a CONNACK with reserved flags set");
            }
            Packet::ConnAck { present: acknowledge & 1 == 1, code }
        }
        3 => {
            let qos = (flags >> 1) & 0x03;
            if qos == 3 {
                return malformed("a PUBLISH at QoS 3");
            }
            let topic = body.text()?;
            if topic.is_empty() || topic.contains(['+', '#']) {
                return malformed("a PUBLISH whose topic is empty or holds a wildcard");
            }
            if qos > 0 {
                body.two()?;
            }
            Packet::Publish { topic, payload: std::mem::take(&mut body.bytes).to_vec(), retain: flags & 1 == 1 }
        }
        8 => {
            let id = body.two()?;
            let mut filters = Vec::new();
            while !body.bytes.is_empty() {
                let filter = body.text()?;
                let qos = body.byte()?;
                if qos > 2 {
                    return malformed("a SUBSCRIBE that asks for a QoS over 2, or sets reserved bits");
                }
                filters.push((filter, qos));
            }
            if filters.is_empty() {
                return malformed("a SUBSCRIBE with no topic filter");
            }
            Packet::Subscribe { id, filters }
        }
        9 => {
            let id = body.two()?;
            let codes = std::mem::take(&mut body.bytes).to_vec();
            if codes.is_empty() || codes.iter().any(|c| *c > 2 && *c != REFUSED) {
                return malformed("a SUBACK with no return code, or with one that the standard does not name");
            }
            Packet::SubAck { id, codes }
        }
        12 => Packet::PingReq,
        13 => Packet::PingResp,
        _ => Packet::Disconnect,
    };
    body.end()?;
    Ok(packet)
}

fn connect(body: &mut Body<'_>) -> Result<Connect, Malformed> {
    if body.binary()? != PROTOCOL_NAME.as_bytes() || body.byte()? != LEVEL {
        return malformed("a CONNECT of another protocol or level than MQTT 3.1.1");
    }
    let flags = body.byte()?;
    if flags & 0x01 != 0 || flags & 0x3c != 0 {
        return malformed("a CONNECT with a will or with the reserved flag, which a meeting point does not take");
    }
    let (has_user, has_password) = (flags & 0x80 != 0, flags & 0x40 != 0);
    if has_password && !has_user {
        return malformed("a CONNECT with a password and no user name");
    }
    let keep_alive = body.two()?;
    let client_id = body.text()?;
    let login = if has_user {
        let user = body.text()?;
        let password = if has_password { body.binary()?.to_vec() } else { Vec::new() };
        Some((user, password))
    } else {
        None
    };
    Ok(Connect { client_id, keep_alive, clean: flags & 0x02 != 0, login })
}

#[cfg(test)]
#[path = "packet_tests.rs"]
mod tests;
