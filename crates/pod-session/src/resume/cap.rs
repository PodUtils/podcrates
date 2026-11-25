//! The bytes of the old leg of a move, against the cap of a relay
//! (PodUtils/podnet#756).
//!
//! The session moves when a leg carried `move_bytes`, both ways
//! (`Watch::count`). The bytes counted before the count that passes the
//! mark are `move_bytes - 1` at most. From that count on, the leg holds
//! (`pump::Hold`) until the new leg greets, and it still carries:
//!
//! - the far end's `DATA` in flight: its window of payload at most, as a
//!   held leg acknowledges no new byte, with 13 bytes of head for each
//!   record. The window bounds the payload, not the heads: a far end whose
//!   application writes one byte at a time sends 14 bytes for each byte of
//!   its window. A resend can send one record of `MAX_DATA` bytes over a
//!   smaller window;
//! - the one record that this side's writer took before the hold: a `DATA`
//!   of `MAX_DATA` bytes at most, as a resend makes them;
//! - the far end's `ACK`s of this side's bytes in flight: one for each
//!   [`ACK_EVERY`] of the window, and one more;
//! - the handshake of the leg, which the relay counts and the watch does
//!   not;
//! - the small records while the leg holds, and a last `CLOSE` of the far
//!   end.
//!
//! The bound counts the bytes of the records. A road that adds bytes to each
//! frame (an id, a head of its own) adds them to each record.
//!
//! podnet's study gave `move_bytes + PAIR_WINDOW + 2 * CHUNK + 64`. The code
//! gives more: the heads of the far end's records, the handshake and the
//! small records are not in that sum. This side adds one record, not two
//! chunks.

use pod_core::{Class, Failure, Stage};

use crate::Settings;
use crate::link::ACK_EVERY;
use crate::record::{HEADER, MAGIC, MAX_BODY, MAX_FEATURE_NAME, MAX_FEATURES, MAX_REASON};
use crate::secret::{ID_LEN, KEY_LEN};

/// The bytes of a `DATA` record besides its payload: the head and the
/// offset. An `ACK` has as many.
const DATA_HEAD: u64 = HEADER as u64 + 8;
/// The largest record: a `DATA` of `MAX_DATA` bytes, as a resend makes
/// them.
const RECORD: u64 = (HEADER + MAX_BODY) as u64;
/// A `GREETING` at most: the head, the magic, the version, the role, the
/// nonce, and the most feature names. An `OPEN` has the same, and its kind
/// and key.
const HELLO: u64 = (HEADER + MAGIC.len() + 2 + KEY_LEN + 1 + MAX_FEATURES * (1 + MAX_FEATURE_NAME)) as u64;
/// The handshake of a leg at most: a `GREETING`, an `OPEN` of a new
/// session, its `ACCEPT`, and a `PROOF`.
const HANDSHAKE: u64 =
    2 * HELLO + (1 + KEY_LEN) as u64 + (HEADER + 1 + ID_LEN + KEY_LEN) as u64 + (HEADER + 8 + KEY_LEN) as u64;
/// The small records of both ends while the leg holds: an `ACK` of the far
/// end each 200 ms at most, a `PING` and a `PONG` each way each 10 s, an
/// `ACK` and a `PONG` of this side that were on their way at the mark, and
/// the `RETIRE`. Less than 80 bytes a second: 8 KiB is a hold of 100 s, more
/// than a connect and the greeting of the new leg (30 s at most).
const HOLD_RECORDS: u64 = 8 << 10;
/// A `CLOSE` of the far end, with the longest reason.
const LAST_WORD: u64 = (HEADER + MAX_REASON) as u64;

/// The most bytes that the old leg of a move carries, both ways, past
/// `move_bytes - 1`, when the far end sends at most `window` bytes with no
/// acknowledgement (the window of the pair's road, or the replay buffer's
/// capacity with no window).
pub fn after_mark(window: usize) -> u64 {
    let window = window.max(1) as u64;
    let far = window.saturating_mul(DATA_HEAD + 1).max(RECORD);
    let acks = (window / ACK_EVERY + 1) * DATA_HEAD;
    far.saturating_add(RECORD + acks + HANDSHAKE + HOLD_RECORDS + LAST_WORD)
}

/// The largest `move_bytes` whose old leg stays under a relay's `cap`, both
/// ways: the leg carries `cap - 1` bytes at most, so a relay that ends a leg
/// at `cap` never ends it. `None` when no move fits: a smaller window
/// fits a smaller cap. `window` is as for [`after_mark`].
pub fn move_under(cap: u64, window: usize) -> Option<u64> {
    cap.checked_sub(after_mark(window)).filter(|most| *most > 0)
}

impl Settings {
    /// These settings for a relay that ends a leg at `cap` bytes, both ways;
    /// a failure of the class `Usage` when `move_bytes` is above
    /// [`move_under`] of the cap and the window: [`Settings::window`], else
    /// [`Settings::pair_window`], as a relay with a cap is on the pair's
    /// road. Settings that never move (`move_bytes` 0, or no resume) pass.
    pub fn under_cap(self, cap: u64) -> Result<Settings, Failure> {
        if self.move_bytes == 0 || !self.resumes {
            return Ok(self);
        }
        let window = self.window.unwrap_or(self.pair_window);
        let text = match move_under(cap, window) {
            Some(most) if self.move_bytes <= most => return Ok(self),
            Some(most) => format!(
                "a move at {} bytes lets the old leg reach the relay's cap of {cap} bytes: the largest move under it is {most} bytes",
                self.move_bytes
            ),
            None => format!(
                "no move fits the relay's cap of {cap} bytes with a window of {window} bytes: use a smaller window"
            ),
        };
        Err(Failure::new(Stage::Session, Class::Usage, text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{Acceptance, Hello, Opening, Record, Role};
    use crate::replay::PAIR_WINDOW;
    use crate::resume::MOVE_BYTES;
    use crate::secret::{Nonce, Proof, SessionId};

    const KIB: usize = 1 << 10;
    const MIB: u64 = 1 << 20;

    /// The relay's cap of 64 MiB and the window of the pair's road: the
    /// default move fits, with room to spare.
    #[test]
    fn the_default_move_fits_the_cap_of_the_relay() {
        let most = move_under(64 * MIB, PAIR_WINDOW).expect("a move under 64 MiB");
        assert!(MOVE_BYTES <= most, "the default move {MOVE_BYTES} is under {most}");
        assert_eq!(most + after_mark(PAIR_WINDOW), 64 * MIB);
        assert!(Settings::default().under_cap(64 * MIB).is_ok());
        assert!(Settings::default().on_the_pair().under_cap(64 * MIB).is_ok());
    }

    /// A cap of 1 MiB: the window of the pair's road holds more than the cap
    /// in the heads of records of one byte, so no move fits; a window of 64
    /// KiB fits. At the edge, a move of 0 bytes is no move.
    #[test]
    fn a_cap_of_1_mib_takes_a_smaller_window() {
        assert_eq!(move_under(MIB, PAIR_WINDOW), None, "14 bytes for each byte of 768 KiB pass 1 MiB");
        let most = move_under(MIB, 64 * KIB).expect("a move under 1 MiB with a window of 64 KiB");
        assert_eq!(most + after_mark(64 * KIB), MIB);
        let after = after_mark(64 * KIB);
        assert_eq!(move_under(after, 64 * KIB), None);
        assert_eq!(move_under(after + 1, 64 * KIB), Some(1));
        assert_eq!(move_under(0, 0), None);
        let mut before = u64::MAX;
        for window in (0..=64).map(|n| n * 16 * KIB) {
            let most = move_under(64 * MIB, window).unwrap_or(0);
            assert!(most <= before, "a larger window {window} gives no larger move");
            before = most;
        }
        assert_eq!(move_under(u64::MAX, usize::MAX), None, "no overflow");
    }

    /// The largest handshake that the records can carry fits its bound.
    #[test]
    fn the_handshake_of_a_leg_fits_its_bound() {
        let features: Vec<String> = (0..MAX_FEATURES).map(|n| format!("{n:a<32}")).collect();
        let hello = Hello { version: 1, role: Role::CLIENT, nonce: Nonce([1; KEY_LEN]), features };
        let open = Record::Open { hello: hello.clone(), opening: Opening::New { public: [2; KEY_LEN] } };
        let records = [
            Record::Greeting(hello),
            open,
            Record::Accept(Acceptance::New { id: SessionId([3; ID_LEN]), public: [4; KEY_LEN] }),
            Record::Proof { offset: u64::MAX, proof: Proof([5; KEY_LEN]) },
        ];
        let bytes: usize = records.iter().map(|r| r.to_bytes().expect("a record of the table").len()).sum();
        assert_eq!(bytes as u64, HANDSHAKE, "the bound is the largest handshake");
    }

    /// A move above the bound is a failure of the class `Usage`; a move at
    /// the bound, and settings that never move, pass.
    #[test]
    fn a_move_over_the_cap_is_a_usage_failure() {
        let on_the_pair = Settings { pair_window: 64 * KIB, ..Settings::default() };
        let most = move_under(MIB, 64 * KIB).unwrap();
        let at = Settings { move_bytes: most, ..on_the_pair };
        assert_eq!(at.under_cap(MIB).expect("a move at the bound").move_bytes, most);
        let over = Settings { move_bytes: most + 1, ..on_the_pair }.under_cap(MIB).expect_err("a move over the bound");
        assert_eq!((over.stage, over.class), (Stage::Session, Class::Usage), "{over}");
        assert!(over.text().contains(&format!("the largest move under it is {most} bytes")), "{over}");
        let none = Settings::default().under_cap(MIB).expect_err("no move under 1 MiB with 768 KiB");
        assert_eq!(none.class, Class::Usage);
        assert!(none.text().contains("use a smaller window"), "{none}");
        let window = Settings { window: Some(64 * KIB), move_bytes: most, ..Settings::default() };
        assert!(window.under_cap(MIB).is_ok(), "the window of the settings first");
        assert!(Settings { move_bytes: 0, ..Settings::default() }.under_cap(MIB).is_ok(), "no move");
        assert!(Settings { resumes: false, ..Settings::default() }.under_cap(MIB).is_ok(), "no resume");
    }
}
