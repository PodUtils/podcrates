//! Single choke-point STUN response validator implementing rules V1 through V13.

use std::net::SocketAddr;

use pod_turn::stun::{
    ALTERNATE_DOMAIN, ALTERNATE_SERVER, BINDING_ERROR, BINDING_SUCCESS, CACHE_TIMEOUT, CHANGED_ADDRESS, ERROR_CODE,
    FINGERPRINT, MAPPED_ADDRESS, MESSAGE_INTEGRITY, MESSAGE_INTEGRITY_SHA256, PADDING, SOFTWARE, SOURCE_ADDRESS,
    TransactionId, UNKNOWN_ATTRIBUTES, XOR_MAPPED_ADDRESS, decode, fingerprint_ok, read_address,
};

use super::addr::canonical_addr;
use crate::report::Flag;

/// Outcome of validating a received datagram against an in-flight STUN request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidateOutcome {
    /// Datagram silently ignored (malformed, wrong TID, stray request, bad FINGERPRINT).
    Dropped(&'static str),
    /// Validated STUN Binding Success from the exact expected source.
    Ok(Validated),
    /// Validated STUN Binding Error with ERROR-CODE (never treated as success).
    Error { code: u16, reason: String, unknown_attrs: Vec<u16>, alternate_server: Option<SocketAddr> },
    /// Validated STUN Binding Success arrived from an unexpected UDP source (for example,
    /// a server that silently ignored CHANGE-REQUEST and replied from its primary socket).
    WrongSource { from: SocketAddr, expected: SocketAddr, mapped: SocketAddr },
    /// Validated STUN Binding Success lacked a usable mapped address or had an unknown
    /// comprehension-required attribute.
    ProtocolFault(&'static str),
}

/// Verified STUN Binding Success payload. Constructed only by [`validate_response`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validated {
    mapped: SocketAddr,
    xor_mapped: Option<SocketAddr>,
    plain_mapped: Option<SocketAddr>,
    other_address: Option<SocketAddr>,
    response_origin: Option<SocketAddr>,
    padding_len: Option<usize>,
    flags: Vec<Flag>,
}

impl Validated {
    /// Preferred mapped endpoint (XOR-MAPPED-ADDRESS > 0x8020 > MAPPED-ADDRESS), canonicalised.
    pub fn mapped(&self) -> SocketAddr {
        self.mapped
    }

    /// XOR-MAPPED-ADDRESS (0x0020 or 0x8020) when present, canonicalised.
    pub fn xor_mapped(&self) -> Option<SocketAddr> {
        self.xor_mapped
    }

    /// Plain MAPPED-ADDRESS (0x0001) when present, canonicalised.
    pub fn plain_mapped(&self) -> Option<SocketAddr> {
        self.plain_mapped
    }

    /// OTHER-ADDRESS (0x802C), falling back to legacy CHANGED-ADDRESS (0x0005), canonicalised.
    pub fn other_address(&self) -> Option<SocketAddr> {
        self.other_address
    }

    /// RESPONSE-ORIGIN (0x802B), falling back to legacy SOURCE-ADDRESS (0x0004), canonicalised.
    pub fn response_origin(&self) -> Option<SocketAddr> {
        self.response_origin
    }

    /// Length of echoed PADDING attribute if present.
    pub fn padding_len(&self) -> Option<usize> {
        self.padding_len
    }

    /// Diagnostic flags recorded during validation (ALG, 0x8020, OriginNeSrc, MappedOnly).
    pub fn flags(&self) -> &[Flag] {
        &self.flags
    }
}

/// Known comprehension-required attributes (`< 0x8000`) allowed in a Binding Success response.
fn is_known_required(attr: u16) -> bool {
    matches!(
        attr,
        MAPPED_ADDRESS
            | SOURCE_ADDRESS
            | CHANGED_ADDRESS
            | MESSAGE_INTEGRITY
            | ERROR_CODE
            | UNKNOWN_ATTRIBUTES
            | MESSAGE_INTEGRITY_SHA256
            | XOR_MAPPED_ADDRESS
            | PADDING
    )
}

/// Validate `datagram` received from `udp_src` against expected transaction IDs and `expected_src`.
pub fn validate_response(
    datagram: &[u8],
    udp_src: SocketAddr,
    expected_src: SocketAddr,
    expected_tid: &TransactionId,
) -> ValidateOutcome {
    // V1, V2, V3: header length, top-two-bits zero, magic cookie, length%4, attribute framing.
    let Ok(msg) = decode(datagram) else {
        return ValidateOutcome::Dropped("malformed-stun");
    };
    // V4: transaction ID match.
    if msg.id != *expected_tid {
        return ValidateOutcome::Dropped("tid-mismatch");
    }
    // V5: only Binding Success (0x0101) or Binding Error (0x0111).
    if msg.kind != BINDING_SUCCESS && msg.kind != BINDING_ERROR {
        return ValidateOutcome::Dropped("not-binding-response");
    }
    // V6: if FINGERPRINT is present, it must be the last attribute and pass CRC-32.
    if msg.attribute(FINGERPRINT).is_some() && !fingerprint_ok(datagram) {
        return ValidateOutcome::Dropped("bad-fingerprint");
    }
    let udp_src = canonical_addr(udp_src);
    let expected_src = canonical_addr(expected_src);

    // V8: Binding Error must carry ERROR-CODE and is never treated as success.
    if msg.kind == BINDING_ERROR {
        let Some(Ok((code, reason))) = msg.error() else {
            return ValidateOutcome::Dropped("error-without-code");
        };
        let unknown_attrs = msg.unknown_attributes().and_then(Result::ok).unwrap_or_default();
        let alternate_server =
            msg.attribute(ALTERNATE_SERVER).and_then(|v| read_address(v, None).ok()).map(canonical_addr);
        return ValidateOutcome::Error { code, reason, unknown_attrs, alternate_server };
    }

    // V9: reject unknown comprehension-required attributes (< 0x8000) in Binding Success.
    for &(attr, _) in &msg.attributes {
        if attr < 0x8000 && !is_known_required(attr) {
            return ValidateOutcome::ProtocolFault("unknown-required-attribute");
        }
    }

    // V7 & V11 & V12: extract XOR (0x0020), alt XOR (0x8020) and plain (0x0001).
    let mut flags = Vec::new();
    let mut saw_xor_vals: Vec<SocketAddr> = Vec::new();
    for &(attr, val) in &msg.attributes {
        if (attr == XOR_MAPPED_ADDRESS || attr == pod_turn::stun::XOR_MAPPED_ADDRESS_ALT)
            && let Ok(addr) = read_address(val, Some(&msg.id))
        {
            let canon = canonical_addr(addr);
            if saw_xor_vals.iter().any(|&prev| prev != canon) && !flags.contains(&Flag::AlgSuspected) {
                flags.push(Flag::AlgSuspected);
            }
            saw_xor_vals.push(canon);
        }
    }
    let xor_std = match msg.xor_mapped() {
        Some(Ok(a)) => Some(canonical_addr(a)),
        Some(Err(_)) => return ValidateOutcome::ProtocolFault("bad-xor-mapped-address"),
        None => None,
    };
    let xor_alt = match msg.xor_mapped_alt() {
        Some(Ok(a)) => Some(canonical_addr(a)),
        Some(Err(_)) => return ValidateOutcome::ProtocolFault("bad-xor-mapped-alt"),
        None => None,
    };
    if let (Some(a), Some(b)) = (xor_std, xor_alt)
        && a != b
        && !flags.contains(&Flag::AlgSuspected)
    {
        flags.push(Flag::AlgSuspected);
    }
    let plain_mapped = match msg.mapped_plain() {
        Some(Ok(a)) => Some(canonical_addr(a)),
        Some(Err(_)) => return ValidateOutcome::ProtocolFault("bad-mapped-address"),
        None => None,
    };
    let xor_mapped = match (xor_std, xor_alt) {
        (Some(a), _) => Some(a),
        (None, Some(a)) => {
            flags.push(Flag::XorAlt8020);
            Some(a)
        }
        (None, None) => None,
    };
    let mapped = match (xor_mapped, plain_mapped) {
        (Some(x), Some(p)) => {
            if x != p && !flags.contains(&Flag::AlgSuspected) {
                flags.push(Flag::AlgSuspected);
            }
            x
        }
        (Some(x), None) => x,
        (None, Some(p)) => {
            flags.push(Flag::MappedOnly);
            p
        }
        (None, None) => return ValidateOutcome::ProtocolFault("missing-mapped-address"),
    };

    // V10: strict UDP source check (catches servers that ignore CHANGE-REQUEST).
    if udp_src != expected_src {
        return ValidateOutcome::WrongSource { from: udp_src, expected: expected_src, mapped };
    }

    // V13: OTHER-ADDRESS (fallback CHANGED-ADDRESS) and RESPONSE-ORIGIN (fallback SOURCE-ADDRESS).
    let other_address = msg
        .other_address()
        .and_then(Result::ok)
        .or_else(|| msg.changed_address().and_then(Result::ok))
        .map(canonical_addr);
    let response_origin = msg
        .response_origin()
        .and_then(Result::ok)
        .or_else(|| msg.source_address().and_then(Result::ok))
        .map(canonical_addr);
    if let Some(origin) = response_origin
        && origin != udp_src
    {
        flags.push(Flag::OriginNeSrc);
    }
    let padding_len = msg.attribute(PADDING).map(<[u8]>::len);
    let _ = (ALTERNATE_DOMAIN, CACHE_TIMEOUT, SOFTWARE);

    ValidateOutcome::Ok(Validated {
        mapped,
        xor_mapped,
        plain_mapped,
        other_address,
        response_origin,
        padding_len,
        flags,
    })
}
