//! The readers of the wire that the fuzz targets of `fuzz/` call (feature
//! `fuzzing`): a packet of a Wisp server, this client's answer to the INFO
//! of a server, and a message of IAP's relay. No consumer turns it on.

pub use crate::iap::frame::{MAX_DATA, Message, ack as iap_ack, data as iap_data};
pub use crate::wisp::packet::{Info, Packet, decode, encode};
use crate::wisp::{Opened, answer};

/// The stream id and the packet in `data`, as the client reads a message of
/// a Wisp server.
pub fn wisp_packet(data: &[u8]) -> Result<(u32, Packet), String> {
    decode(data)
}

/// This client's INFO and what the opening settles, in answer to a
/// server's `info`.
pub fn wisp_answer(info: &Info) -> Result<(Info, Opened), String> {
    answer(info, None)
}

/// The message of IAP's relay in one WebSocket message, as the carrier
/// reads it.
pub fn iap_message(data: &[u8]) -> Result<Message, String> {
    crate::iap::frame::decode(data).map_err(|e| e.to_string())
}
