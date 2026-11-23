//! NAT behaviour discovery attributes (RFC 5780) and legacy STUN address
//! readers (RFC 3489) on top of [`super`]. Pure Rust, no I/O, no async.

use std::fmt;
use std::net::SocketAddr;

use super::{
    BINDING_REQUEST, BINDING_SUCCESS, MAPPED_ADDRESS, Message, SOFTWARE, TransactionId, UNKNOWN_ATTRIBUTES,
    XOR_MAPPED_ADDRESS, message, read_address, write_address,
};

/// RFC 5780 section 7.2 / RFC 3489 section 11.2.4: request a reply from another IP or port.
pub const CHANGE_REQUEST: u16 = 0x0003;
/// RFC 3489 section 11.2.5: legacy response origin address.
pub const SOURCE_ADDRESS: u16 = 0x0004;
/// RFC 3489 section 11.2.6: legacy alternate server address.
pub const CHANGED_ADDRESS: u16 = 0x0005;
/// RFC 5780 section 7.6: padding bytes for path MTU discovery.
pub const PADDING: u16 = 0x0026;
/// RFC 5780 section 7.5: ask the server to reply to another port on the client IP.
pub const RESPONSE_PORT: u16 = 0x0027;
/// Non-standard legacy XOR-MAPPED-ADDRESS type seen on older servers.
pub const XOR_MAPPED_ADDRESS_ALT: u16 = 0x8020;
/// RFC 5780 section 9.1: registered attribute with no wire format.
pub const CACHE_TIMEOUT: u16 = 0x8027;
/// RFC 5780 section 7.3: the address and port from which the server sent the reply.
pub const RESPONSE_ORIGIN: u16 = 0x802b;
/// RFC 5780 section 7.4: the server's alternate IP and alternate port.
pub const OTHER_ADDRESS: u16 = 0x802c;

/// CHANGE-REQUEST flag: reply from the alternate IP address.
pub const CHANGE_IP: u8 = 0x04;
/// CHANGE-REQUEST flag: reply from the alternate UDP port.
pub const CHANGE_PORT: u8 = 0x02;

/// Maximum SOFTWARE length accepted by [`NatRequestBuilder`].
const MAX_SOFTWARE_LEN: usize = 32;

/// Why [`NatRequestBuilder::build`] refused to build a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NatBuildError {
    /// RFC 5780 section 7.6 forbids PADDING and RESPONSE-PORT in one request.
    ResponsePortWithPadding,
    /// CHANGE-REQUEST carries bits outside `CHANGE_IP | CHANGE_PORT`.
    InvalidChangeFlags(u8),
    /// SOFTWARE is empty or longer than 32 bytes.
    InvalidSoftwareLen(usize),
    /// PADDING exceeds the maximum 16-bit STUN message body limit.
    PaddingTooLarge(usize),
}

impl fmt::Display for NatBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResponsePortWithPadding => f.write_str("RESPONSE-PORT and PADDING must not appear together"),
            Self::InvalidChangeFlags(bits) => write!(f, "invalid CHANGE-REQUEST bits: 0x{bits:02x}"),
            Self::InvalidSoftwareLen(len) => write!(f, "SOFTWARE length {len} is outside 1..={MAX_SOFTWARE_LEN}"),
            Self::PaddingTooLarge(len) => write!(f, "PADDING length {len} exceeds 65532 bytes"),
        }
    }
}

impl std::error::Error for NatBuildError {}

/// Builder for a STUN Binding request with RFC 5780 attributes.
#[derive(Debug, Clone)]
pub struct NatRequestBuilder {
    id: TransactionId,
    change_flags: Option<u8>,
    response_port: Option<u16>,
    padding: Option<usize>,
    software: Option<String>,
}

impl NatRequestBuilder {
    /// Start a Binding request with `id`.
    pub fn binding(id: TransactionId) -> Self {
        Self { id, change_flags: None, response_port: None, padding: None, software: None }
    }

    /// Attach a CHANGE-REQUEST attribute with `flags` (`CHANGE_IP` and/or `CHANGE_PORT`).
    pub fn change_request(mut self, flags: u8) -> Self {
        self.change_flags = Some(flags);
        self
    }

    /// Attach a RESPONSE-PORT attribute for `port`.
    pub fn response_port(mut self, port: u16) -> Self {
        self.response_port = Some(port);
        self
    }

    /// Attach a PADDING attribute of `len` zero bytes.
    pub fn padding(mut self, len: usize) -> Self {
        self.padding = Some(len);
        self
    }

    /// Attach a SOFTWARE attribute.
    pub fn software(mut self, text: impl Into<String>) -> Self {
        self.software = Some(text.into());
        self
    }

    /// Validate all attributes and encode the Binding request.
    pub fn build(self) -> Result<Vec<u8>, NatBuildError> {
        if self.response_port.is_some() && self.padding.is_some() {
            return Err(NatBuildError::ResponsePortWithPadding);
        }
        let change_buf;
        let rp_buf;
        let pad_buf;
        let mut attrs: Vec<(u16, &[u8])> = Vec::new();
        if let Some(flags) = self.change_flags {
            if flags & !(CHANGE_IP | CHANGE_PORT) != 0 {
                return Err(NatBuildError::InvalidChangeFlags(flags));
            }
            change_buf = [0, 0, 0, flags];
            attrs.push((CHANGE_REQUEST, &change_buf));
        }
        if let Some(port) = self.response_port {
            let [hi, lo] = port.to_be_bytes();
            rp_buf = [hi, lo, 0, 0];
            attrs.push((RESPONSE_PORT, &rp_buf));
        }
        if let Some(len) = self.padding {
            // Compute existing attributes' framed size (each TLV is 4 + len + pad-to-4) so the
            // total STUN body length never exceeds 65_532 (the largest multiple of 4 <= u16::MAX).
            let used: usize = attrs.iter().map(|(_, v)| 4 + v.len() + ((4 - v.len() % 4) % 4)).sum();
            let sw_used = self.software.as_ref().map_or(0, |s| 4 + s.len() + ((4 - s.len() % 4) % 4));
            let max_pad = 65_532usize.saturating_sub(used + sw_used + 4);
            if len > max_pad {
                return Err(NatBuildError::PaddingTooLarge(len));
            }
            pad_buf = vec![0u8; len];
            attrs.push((PADDING, &pad_buf));
        }
        if let Some(ref sw) = self.software {
            if sw.is_empty() || sw.len() > MAX_SOFTWARE_LEN {
                return Err(NatBuildError::InvalidSoftwareLen(sw.len()));
            }
            attrs.push((SOFTWARE, sw.as_bytes()));
        }
        Ok(message(BINDING_REQUEST, &self.id, &attrs))
    }
}

/// Encode a Binding success response carrying optional RFC 5780 attributes.
pub fn binding_success_nat(
    id: &TransactionId,
    xor_mapped: Option<SocketAddr>,
    plain_mapped: Option<SocketAddr>,
    other: Option<SocketAddr>,
    origin: Option<SocketAddr>,
    padding_len: Option<usize>,
) -> Vec<u8> {
    let xor_buf = xor_mapped.map(|a| write_address(a, Some(id)));
    let plain_buf = plain_mapped.map(|a| write_address(a, None));
    let other_buf = other.map(|a| write_address(a, None));
    let origin_buf = origin.map(|a| write_address(a, None));
    let mut attrs: Vec<(u16, &[u8])> = Vec::new();
    if let Some(ref b) = xor_buf {
        attrs.push((XOR_MAPPED_ADDRESS, b));
    }
    if let Some(ref b) = plain_buf {
        attrs.push((MAPPED_ADDRESS, b));
    }
    if let Some(ref b) = other_buf {
        attrs.push((OTHER_ADDRESS, b));
    }
    if let Some(ref b) = origin_buf {
        attrs.push((RESPONSE_ORIGIN, b));
    }
    let used: usize = attrs.iter().map(|(_, v)| 4 + v.len() + ((4 - v.len() % 4) % 4)).sum();
    let max_pad = 65_532usize.saturating_sub(used + 4);
    let pad_buf = padding_len.map(|n| vec![0u8; n.min(max_pad)]);
    if let Some(ref b) = pad_buf {
        attrs.push((PADDING, b));
    }
    message(BINDING_SUCCESS, id, &attrs)
}

impl Message<'_> {
    /// XOR-MAPPED-ADDRESS (0x0020) alone.
    pub fn xor_mapped(&self) -> Option<Result<SocketAddr, String>> {
        self.attribute(XOR_MAPPED_ADDRESS).map(|v| read_address(v, Some(&self.id)))
    }

    /// Non-standard XOR-MAPPED-ADDRESS (0x8020) alone.
    pub fn xor_mapped_alt(&self) -> Option<Result<SocketAddr, String>> {
        self.attribute(XOR_MAPPED_ADDRESS_ALT).map(|v| read_address(v, Some(&self.id)))
    }

    /// Plain MAPPED-ADDRESS (0x0001) alone, for ALG comparison against XOR-MAPPED-ADDRESS.
    pub fn mapped_plain(&self) -> Option<Result<SocketAddr, String>> {
        self.attribute(MAPPED_ADDRESS).map(|v| read_address(v, None))
    }

    /// OTHER-ADDRESS (0x802C) alone.
    pub fn other_address(&self) -> Option<Result<SocketAddr, String>> {
        self.attribute(OTHER_ADDRESS).map(|v| read_address(v, None))
    }

    /// Legacy CHANGED-ADDRESS (0x0005) alone.
    pub fn changed_address(&self) -> Option<Result<SocketAddr, String>> {
        self.attribute(CHANGED_ADDRESS).map(|v| read_address(v, None))
    }

    /// RESPONSE-ORIGIN (0x802B) alone.
    pub fn response_origin(&self) -> Option<Result<SocketAddr, String>> {
        self.attribute(RESPONSE_ORIGIN).map(|v| read_address(v, None))
    }

    /// Legacy SOURCE-ADDRESS (0x0004) alone.
    pub fn source_address(&self) -> Option<Result<SocketAddr, String>> {
        self.attribute(SOURCE_ADDRESS).map(|v| read_address(v, None))
    }

    /// CHANGE-REQUEST flags byte (`CHANGE_IP` and/or `CHANGE_PORT`).
    pub fn change_request(&self) -> Option<Result<u8, String>> {
        let v = self.attribute(CHANGE_REQUEST)?;
        if v.len() != 4 {
            return Some(Err(format!("CHANGE-REQUEST length {} is not 4", v.len())));
        }
        Some(Ok(v[3]))
    }

    /// RESPONSE-PORT value (RFC 5780 section 7.5).
    pub fn response_port(&self) -> Option<Result<u16, String>> {
        let v = self.attribute(RESPONSE_PORT)?;
        if v.len() != 4 {
            return Some(Err(format!("RESPONSE-PORT length {} is not 4", v.len())));
        }
        Some(Ok(u16::from_be_bytes([v[0], v[1]])))
    }

    /// UNKNOWN-ATTRIBUTES list from a 420 response.
    pub fn unknown_attributes(&self) -> Option<Result<Vec<u16>, String>> {
        let v = self.attribute(UNKNOWN_ATTRIBUTES)?;
        if v.len() % 2 != 0 {
            return Some(Err(format!("UNKNOWN-ATTRIBUTES length {} is odd", v.len())));
        }
        Some(Ok(v.as_chunks::<2>().0.iter().map(|&c| u16::from_be_bytes(c)).collect()))
    }
}

#[cfg(test)]
#[path = "stun_nat_tests.rs"]
mod tests;
