//! The POSIX TZ string of the footer of a TZif file (RFC 8536, section
//! 3.3): a standard offset, and an optional daylight offset with the two
//! rules of its start and its end. The extensions of RFC 8536 hold: a rule
//! time from -167 to 167 hours, and a name in angle brackets.

use crate::civil::{DAY, days_from_civil, is_leap, month_days, weekday};

/// The reach of a rule in seconds from the epoch.
const REACH: i64 = 1 << 40;

/// The rule of a TZ string. Each offset is in seconds east of UTC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rule {
    std: i32,
    dst: Option<Dst>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Dst {
    offset: i32,
    start: (When, i32),
    end: (When, i32),
}

/// The day of a rule in a year.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum When {
    /// `Jn`: day 1 to 365; the 29th of February is never counted.
    Julian(u16),
    /// `n`: day 0 to 365; the 29th of February is counted.
    Day(u16),
    /// `Mm.w.d`: the weekday `d` (0 is Sunday) of week `w` (5 is the last)
    /// of month `m`.
    Month { month: u32, week: u32, day: u32 },
}

/// A cursor over the bytes of a TZ string.
struct Text<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Text<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn eat(&mut self, byte: u8) -> bool {
        let found = self.peek() == Some(byte);
        if found {
            self.at += 1;
        }
        found
    }

    fn number(&mut self, max: u32) -> Result<u32, String> {
        let start = self.at;
        let mut value = 0u32;
        while let Some(digit @ b'0'..=b'9') = self.peek() {
            value = value.saturating_mul(10).saturating_add(u32::from(digit - b'0'));
            self.at += 1;
        }
        if self.at == start || self.at - start > 3 || value > max {
            return Err(format!("a number at byte {start} is not from 0 to {max}"));
        }
        Ok(value)
    }

    /// A zone name: three or more letters, or `<...>` with letters,
    /// digits, `+` and `-`.
    fn name(&mut self) -> Result<(), String> {
        let start = self.at;
        if self.eat(b'<') {
            while matches!(self.peek(), Some(b) if b.is_ascii_alphanumeric() || b == b'+' || b == b'-') {
                self.at += 1;
            }
            if self.at - start - 1 < 3 || !self.eat(b'>') {
                return Err(format!("the quoted name at byte {start} is not valid"));
            }
        } else {
            while matches!(self.peek(), Some(b) if b.is_ascii_alphabetic()) {
                self.at += 1;
            }
            if self.at - start < 3 {
                return Err(format!("the name at byte {start} has fewer than three letters"));
            }
        }
        Ok(())
    }

    /// `[+-]hh[:mm[:ss]]` in seconds, with `hours` as the largest hour.
    fn time(&mut self, hours: u32) -> Result<i32, String> {
        let negative = self.eat(b'-');
        if !negative {
            self.eat(b'+');
        }
        let mut seconds = self.number(hours)? * 3600;
        if self.eat(b':') {
            seconds += self.number(59)? * 60;
            if self.eat(b':') {
                seconds += self.number(59)?;
            }
        }
        // At most 167 hours: the value fits an i32.
        let seconds = seconds as i32;
        Ok(if negative { -seconds } else { seconds })
    }

    fn when(&mut self) -> Result<(When, i32), String> {
        let when = if self.eat(b'J') {
            let n = self.number(365)?;
            if n == 0 {
                return Err("a Julian day is from 1 to 365".into());
            }
            When::Julian(n as u16)
        } else if self.eat(b'M') {
            let month = self.number(12)?;
            let week = if self.eat(b'.') { self.number(5)? } else { 0 };
            let day = if self.eat(b'.') { self.number(6)? } else { 7 };
            if month == 0 || week == 0 || day > 6 {
                return Err("a rule Mm.w.d needs a month 1-12, a week 1-5 and a day 0-6".into());
            }
            When::Month { month, week, day }
        } else {
            When::Day(self.number(365)? as u16)
        };
        // The default time of a change is 02:00 local.
        let time = if self.eat(b'/') { self.time(167)? } else { 7200 };
        Ok((when, time))
    }
}

/// Parse a TZ string. An empty string is not a rule.
pub(crate) fn parse(text: &str) -> Result<Rule, String> {
    let mut t = Text { bytes: text.as_bytes(), at: 0 };
    t.name()?;
    // POSIX counts an offset west of UTC as positive.
    let std = -t.time(24)?;
    if t.peek().is_none() {
        return Ok(Rule { std, dst: None });
    }
    t.name()?;
    let offset = match t.peek() {
        Some(b'+' | b'-' | b'0'..=b'9') => -t.time(24)?,
        _ => std + 3600,
    };
    if !t.eat(b',') {
        return Err("a daylight time with no rule of its start and end".into());
    }
    let start = t.when()?;
    if !t.eat(b',') {
        return Err("a daylight time with no rule of its end".into());
    }
    let end = t.when()?;
    if t.peek().is_some() {
        return Err(format!("text after the rule at byte {}", t.at));
    }
    Ok(Rule { std, dst: Some(Dst { offset, start, end }) })
}

/// The day since the epoch of `when` in `year`.
fn day_of(year: i64, when: When) -> i64 {
    let jan1 = days_from_civil(year, 1, 1);
    match when {
        When::Julian(n) => jan1 + i64::from(n) - 1 + i64::from(is_leap(year) && n >= 60),
        When::Day(n) => jan1 + i64::from(n),
        When::Month { month, week, day } => {
            let first = days_from_civil(year, month, 1);
            let mut date = 1 + (day + 7 - weekday(first)) % 7 + (week - 1) * 7;
            while date > month_days(year, month) {
                date -= 7;
            }
            first + i64::from(date) - 1
        }
    }
}

impl Rule {
    /// The changes of the years around `utc`, sorted: the instant and the
    /// offset from then on. At one instant the start of daylight time
    /// comes after the end of the year before, so a zone in daylight time
    /// all year stays in it.
    fn changes(&self, utc: i64) -> Vec<(i64, i32)> {
        let Some(dst) = &self.dst else { return Vec::new() };
        // About 34,000 years each way: the products below fit an i64.
        let utc = utc.clamp(-REACH, REACH);
        let year = crate::civil::civil(utc.div_euclid(DAY)).0;
        let mut all = Vec::with_capacity(6);
        for y in year - 1..=year + 1 {
            let start = day_of(y, dst.start.0) * DAY + i64::from(dst.start.1) - i64::from(self.std);
            let end = day_of(y, dst.end.0) * DAY + i64::from(dst.end.1) - i64::from(dst.offset);
            all.push((start, 1, dst.offset));
            all.push((end, 0, self.std));
        }
        all.sort_unstable();
        all.into_iter().map(|(at, _, offset)| (at, offset)).collect()
    }

    /// The offset at the instant `utc`.
    pub(crate) fn offset_at(&self, utc: i64) -> i32 {
        let changes = self.changes(utc);
        changes.iter().rev().find(|(at, _)| *at <= utc).map_or(self.std, |(_, offset)| *offset)
    }

    /// The first instant after `utc` at which the offset changes.
    pub(crate) fn next_change(&self, utc: i64) -> Option<i64> {
        let now = self.offset_at(utc);
        // The changes of the next year come with a later instant.
        for probe in [utc, utc.saturating_add(366 * DAY)] {
            for (at, _) in self.changes(probe).into_iter().filter(|(at, _)| *at > utc) {
                if self.offset_at(at) != now {
                    return Some(at);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_forms_of_a_tz_string() {
        let fixed = parse("<+0530>-5:30").unwrap();
        assert_eq!(fixed, Rule { std: 19_800, dst: None });
        assert_eq!(parse("EST5").unwrap().std, -18_000);
        let us = parse("EST5EDT,M3.2.0,M11.1.0").unwrap();
        let dst = us.dst.unwrap();
        assert_eq!((dst.offset, dst.start.1, dst.end.1), (-14_400, 7200, 7200));
        let all_year = parse("EST5EDT,0/0,J365/25").unwrap();
        // A zone in daylight time all year.
        for day in [0, 100, 364, 365, 366] {
            let t = days_from_civil(2026, 1, 1) * DAY + day * DAY + 3600;
            assert_eq!(all_year.offset_at(t), -14_400, "day {day}");
        }
        assert_eq!(all_year.next_change(days_from_civil(2026, 6, 1) * DAY), None);
        for bad in ["", "AB1", "EST", "EST5EDT", "EST5EDT,M3.2.0", "EST5EDT,M13.2.0,M11.1.0", "<+05>-5x", "EST99"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_day_of_each_rule() {
        // 2026-03-08 is the second Sunday of March; 2026-10-25 the last
        // Sunday of October.
        let second = When::Month { month: 3, week: 2, day: 0 };
        assert_eq!(day_of(2026, second), days_from_civil(2026, 3, 8));
        let last = When::Month { month: 10, week: 5, day: 0 };
        assert_eq!(day_of(2026, last), days_from_civil(2026, 10, 25));
        assert_eq!(day_of(2028, When::Julian(60)), days_from_civil(2028, 3, 1), "J60 is March 1st");
        assert_eq!(day_of(2028, When::Day(59)), days_from_civil(2028, 2, 29));
    }
}
