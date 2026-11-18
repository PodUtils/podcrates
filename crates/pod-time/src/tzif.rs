//! The reader of a TZif file (RFC 8536): versions 1 to 4, the 64-bit
//! data of version 2 and later, and the POSIX TZ string of the footer.
//! Each count is checked against the length of the file before a read, so
//! a short or a hostile file is an error, never a panic.

use crate::posix;

/// The largest TZif file that the reader takes. A real zone is under
/// 4 KiB.
pub(crate) const MAX_BYTES: usize = 1 << 20;

/// The transitions of a zone and the rule after the last one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Data {
    /// The instants of the transitions, ascending.
    times: Vec<i64>,
    /// The offset (seconds east of UTC) from each transition on.
    offsets: Vec<i32>,
    /// The offset before the first transition: the first time type.
    first: i32,
    footer: Option<posix::Rule>,
}

/// The six counts of a header.
struct Counts {
    isut: usize,
    isstd: usize,
    leap: usize,
    time: usize,
    types: usize,
    chars: usize,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|end| *end <= self.bytes.len());
        let end = end.ok_or_else(|| format!("the file ends at byte {} in a block of {n} bytes", self.bytes.len()))?;
        let part = &self.bytes[self.at..end];
        self.at = end;
        Ok(part)
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().expect("four bytes")))
    }

    fn header(&mut self) -> Result<(u8, Counts), String> {
        if self.take(4)? != b"TZif" {
            return Err("the file is not TZif (no magic)".into());
        }
        let version = self.take(1)?[0];
        if !matches!(version, 0 | b'2' | b'3' | b'4') {
            return Err(format!("TZif version {version:#04x} is not known"));
        }
        self.take(15)?;
        let mut next = || self.u32().map(|n| n as usize);
        let counts =
            Counts { isut: next()?, isstd: next()?, leap: next()?, time: next()?, types: next()?, chars: next()? };
        Ok((version, counts))
    }
}

impl Counts {
    /// The length of the data block, with `time` bytes for an instant.
    fn block(&self, time: usize) -> Option<usize> {
        let parts = [
            self.time.checked_mul(time + 1)?,
            self.types.checked_mul(6)?,
            self.chars,
            self.leap.checked_mul(time + 4)?,
            self.isstd,
            self.isut,
        ];
        parts.into_iter().try_fold(0usize, |sum, n| sum.checked_add(n))
    }

    fn check(&self) -> Result<(), String> {
        if self.types == 0 || self.types > 256 {
            return Err(format!("{} time types (1 to 256)", self.types));
        }
        if (self.isut != 0 && self.isut != self.types) || (self.isstd != 0 && self.isstd != self.types) {
            return Err("the counts of the indicators do not match the time types".into());
        }
        if self.leap != 0 {
            return Err("a zone with leap seconds (a right/ zone) is not supported".into());
        }
        Ok(())
    }
}

/// Read the data block of `counts`, with `time` bytes for an instant.
fn block(r: &mut Reader<'_>, counts: &Counts, time: usize) -> Result<Data, String> {
    counts.check()?;
    let need = counts.block(time).ok_or("the counts overflow")?;
    if r.bytes.len() - r.at < need {
        return Err(format!("the file is shorter than its counts ({need} bytes of data)"));
    }
    let times: Vec<i64> = r
        .take(counts.time * time)?
        .chunks_exact(time)
        .map(|c| match c.len() {
            4 => i64::from(i32::from_be_bytes(c.try_into().expect("four bytes"))),
            _ => i64::from_be_bytes(c.try_into().expect("eight bytes")),
        })
        .collect();
    if times.windows(2).any(|w| w[0] >= w[1]) {
        return Err("the transition times are not ascending".into());
    }
    let indexes = r.take(counts.time)?.to_vec();
    let mut types = Vec::with_capacity(counts.types);
    for t in r.take(counts.types * 6)?.as_chunks::<6>().0 {
        let offset = i32::from_be_bytes(t[0..4].try_into().expect("four bytes"));
        if offset == i32::MIN || t[4] > 1 || usize::from(t[5]) >= counts.chars.max(1) {
            return Err(format!("a time type is not valid: {t:?}"));
        }
        types.push(offset);
    }
    let offsets = indexes
        .iter()
        .map(|i| types.get(usize::from(*i)).copied().ok_or(format!("a transition names time type {i}")))
        .collect::<Result<Vec<_>, _>>()?;
    r.take(counts.chars + counts.isstd + counts.isut)?;
    Ok(Data { times, offsets, first: types[0], footer: None })
}

/// Parse a TZif file.
pub(crate) fn parse(bytes: &[u8]) -> Result<Data, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("the file has more than {MAX_BYTES} bytes"));
    }
    let mut r = Reader { bytes, at: 0 };
    let (version, counts) = r.header()?;
    if version == 0 {
        return block(&mut r, &counts, 4);
    }
    // Version 2 and later: skip the 32-bit block, read the 64-bit one.
    let skip = counts.block(4).ok_or("the counts overflow")?;
    r.take(skip)?;
    let (again, counts) = r.header()?;
    if again != version {
        return Err("the second header has another version".into());
    }
    let mut data = block(&mut r, &counts, 8)?;
    if r.take(1)? != b"\n" {
        return Err("the footer does not start with a newline".into());
    }
    let rest = &bytes[r.at..];
    let end = rest.iter().position(|b| *b == b'\n').ok_or("the footer does not end with a newline")?;
    let text = std::str::from_utf8(&rest[..end]).map_err(|_| "the footer is not ASCII")?;
    if !text.is_empty() {
        data.footer = Some(posix::parse(text).map_err(|e| format!("the TZ string {text:?}: {e}"))?);
    }
    Ok(data)
}

impl Data {
    /// A zone of one fixed offset.
    pub(crate) fn fixed(offset: i32) -> Data {
        Data { times: Vec::new(), offsets: Vec::new(), first: offset, footer: None }
    }

    /// The offset at the instant `utc`.
    pub(crate) fn offset_at(&self, utc: i64) -> i32 {
        let n = self.times.partition_point(|t| *t <= utc);
        match (&self.footer, n) {
            // RFC 8536, section 3.2: with no transition, the footer gives
            // each instant; before the first transition, the first type.
            (Some(rule), _) if self.times.is_empty() => rule.offset_at(utc),
            (_, 0) => self.first,
            (Some(rule), n) if n == self.times.len() => rule.offset_at(utc),
            (_, n) => self.offsets[n - 1],
        }
    }

    /// The first instant after `utc` at which the offset changes.
    pub(crate) fn next_change(&self, utc: i64) -> Option<i64> {
        let mut before = self.offset_at(utc);
        for (at, offset) in self.times.iter().zip(&self.offsets).skip(self.times.partition_point(|t| *t <= utc)) {
            if *offset != before {
                return Some(*at);
            }
            before = *offset;
        }
        let last = self.times.last().copied().unwrap_or(i64::MIN).max(utc);
        let next = self.footer.as_ref()?.next_change(last)?;
        // The footer starts at the last transition: a first change of it
        // that equals the table's last offset is no change.
        if self.offset_at(next) != self.offset_at(utc) { Some(next) } else { self.next_change(next) }
    }
}
