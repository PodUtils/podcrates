//! The lines of a frame in a pane, and the largest chunk whose `DATA`
//! frame stays within the rows of the pane. tmux, when its client falls
//! behind, paints only the last screen: the head of a frame taller than the
//! pane is never painted, and the frame never reaches the other end.

use super::{CHUNK_FLOOR, CHUNK_MAX, SPARE_ROWS};

/// The bytes of `#KIND:` and of `!<crc>`.
const HEAD: usize = 6;
const TAIL: usize = 9;
/// The bytes of the offset before the file's bytes in `DATA`.
const OFFSET: usize = 8;
/// [`super::wire::frame`] breaks no line shorter than this.
const NARROWEST: usize = 16;

/// The lines of the frame of a payload of `payload` bytes, broken at
/// `width` bytes as [`super::wire::frame`] breaks it: the head and the
/// base64 fill each line, and the CRC goes on a new line when it does not
/// fit on the last one. The tests prove [`chunk`], its inverse, with it.
#[cfg(test)]
pub(super) fn lines(payload: usize, width: usize) -> usize {
    let text = HEAD + (4 * payload).div_ceil(3);
    (text + TAIL).div_ceil(width.max(NARROWEST))
}

/// The largest chunk of `DATA` whose frame, broken at `cols - 1` bytes,
/// takes at most `rows - SPARE_ROWS` lines, in `CHUNK_FLOOR..=CHUNK_MAX`.
/// A size of 0 is not known: no limit.
pub(super) fn chunk(cols: usize, rows: usize) -> usize {
    if cols == 0 || rows == 0 {
        return CHUNK_MAX;
    }
    let width = (cols - 1).max(NARROWEST);
    let room = rows.saturating_sub(SPARE_ROWS).max(1) * width;
    // `lines` inverted: 3 bytes of payload are 4 bytes of base64.
    let payload = room.saturating_sub(HEAD + TAIL) * 3 / 4;
    payload.saturating_sub(OFFSET).clamp(CHUNK_FLOOR, CHUNK_MAX)
}
