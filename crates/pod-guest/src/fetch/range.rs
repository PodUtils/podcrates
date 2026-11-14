//! The `Content-Range` of an answer (RFC 9110, section 14.4).

/// One `Content-Range`: `bytes START-END/TOTAL`, `bytes START-END/*`, or
/// `bytes */TOTAL` (an answer `416`, with no range).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// The offset of the first byte; 0 for `*/TOTAL`.
    pub start: u64,
    /// The offset of the last byte, inclusive; `None` for `*/TOTAL`.
    pub end: Option<u64>,
    /// The size of the whole object; `None` for `/*`.
    pub total: Option<u64>,
}

/// Parse a `Content-Range`. `None`: another unit, a range whose end is
/// before its start, or a value that is not of the grammar. The unit has
/// any case; the numbers are ASCII digits alone, with no sign.
pub fn parse_content_range(value: &str) -> Option<Span> {
    let value = value.trim();
    let (unit, rest) = value.split_once(' ')?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return None;
    }
    let (range, total) = rest.trim_start().split_once('/')?;
    let total = if total == "*" { None } else { Some(number(total)?) };
    if range == "*" {
        return Some(Span { start: 0, end: None, total: Some(total?) });
    }
    let (start, end) = range.split_once('-')?;
    let (start, end) = (number(start)?, number(end)?);
    if end < start {
        return None;
    }
    Some(Span { start, end: Some(end), total })
}

fn number(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}
