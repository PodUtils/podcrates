//! The readers of the wire that the fuzz targets of `fuzz/` call (feature
//! `fuzzing`): a relay list and a forward path, a `/relays.json` body and its
//! signed list, and the frames of the reverse road. No consumer turns it on.

pub use crate::pool::parse_pool;
pub use crate::pool::signed::Signed;
pub use crate::relay::{forward_path, parse_relay_list};
pub use crate::reverse::control::parse_inbound;
pub use crate::reverse::framing::legs::decode_node_frame;
