//! Sans-IO engine inputs, outputs, events, socket role IDs and hairpin probe datagrams.

use std::net::{Ipv6Addr, SocketAddr};

use crate::report::{QuestionId, Report, SockId};
use crate::seams::BindSpec;

/// Fixed logical socket role identifiers (enforcing strict socket isolation).
pub const SOCK_M: SockId = SockId(0);
pub const SOCK_F: SockId = SockId(1);
pub const SOCK_M6: SockId = SockId(2);
pub const SOCK_P: SockId = SockId(3);
pub const SOCK_ALLOC_BASE: u16 = 16;
pub const SOCK_DEEP_GAP: SockId = SockId(48);
pub const SOCK_LIFE_Y: SockId = SockId(64);
pub const SOCK_LIFE_RUNG_BASE: u16 = 65;

/// 4-byte magic prefix for two-socket hairpin reflection datagrams.
pub const HAIRPIN_MAGIC: [u8; 4] = *b"PNPH";

/// Encode a 20-byte hairpin probe datagram (`PNPH` + 16-byte random txid).
pub fn encode_hairpin(txid: &[u8; 16]) -> [u8; 20] {
    let mut out = [0u8; 20];
    out[..4].copy_from_slice(&HAIRPIN_MAGIC);
    out[4..20].copy_from_slice(txid);
    out
}

/// Decode a hairpin probe datagram, returning its 16-byte txid.
pub fn decode_hairpin(bytes: &[u8]) -> Option<[u8; 16]> {
    if bytes.len() != 20 || bytes[..4] != HAIRPIN_MAGIC {
        return None;
    }
    let mut id = [0u8; 16];
    id.copy_from_slice(&bytes[4..20]);
    Some(id)
}

/// Non-blocking I/O instruction emitted by [`super::ProbeMachine::poll_output`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    Bind { sock: SockId, spec: BindSpec },
    Send { sock: SockId, dest: SocketAddr, bytes: Vec<u8> },
    Close { sock: SockId },
    QueryDns64 { domain: String },
    HttpGet { id: u32, url: String },
    HttpSoap { id: u32, url: String, action: String, body: String },
    CheckOnline,
}

/// External event fed into [`super::ProbeMachine::handle_input`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Bound { sock: SockId, local: SocketAddr },
    BindFailed { sock: SockId, reason: String },
    Datagram { sock: SockId, from: SocketAddr, bytes: Vec<u8> },
    Dns64Answer { aaaa: Vec<Ipv6Addr>, has_a: bool, resolver_is_doh: bool, failed: bool },
    HttpReply { id: u32, body: Result<String, String> },
    OnlineResult { ok: bool },
    Cancel,
}

/// Progress or completion event emitted by [`super::ProbeMachine::poll_event`].
#[derive(Clone, Debug, PartialEq)]
pub enum ProbeEvent {
    Partial(QuestionId),
    Final(Box<Report>),
}
