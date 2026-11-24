//! This host's clock, in the words of a TLS failure. A certificate
//! that is expired, or not valid yet, at each host usually means that this
//! host's clock is wrong: certificates are checked against it, so the hint
//! gives its reading. And HTTP's dates both ways, and a time to the minute
//! for a person, so that each crate reads and writes them with one calendar.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];

/// A source of the time, which a test can plant: the monotonic time for a
/// bound or an age, and the wall clock for a date. One trait in place of
/// the copies of the consumers (the owner's ruling "Shared code").
pub trait Clock: Send + Sync {
    /// The monotonic time now.
    fn now(&self) -> Instant;
    /// The wall clock now.
    fn wall(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// The clock of the system.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// A clock that moves only when a test moves it: both times are their start
/// and the sum of each [`ManualClock::advance`].
#[derive(Debug)]
pub struct ManualClock {
    start: Instant,
    wall: SystemTime,
    moved: Mutex<Duration>,
}

impl ManualClock {
    /// A clock at the time of its make.
    pub fn new() -> ManualClock {
        ManualClock::at(SystemTime::now())
    }

    /// A clock whose wall clock starts at `wall`.
    pub fn at(wall: SystemTime) -> ManualClock {
        ManualClock { start: Instant::now(), wall, moved: Mutex::new(Duration::ZERO) }
    }

    /// Move both times on by `by`.
    pub fn advance(&self, by: Duration) {
        let mut moved = self.moved.lock().unwrap_or_else(|e| e.into_inner());
        *moved += by;
    }

    fn moved(&self) -> Duration {
        *self.moved.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for ManualClock {
    fn default() -> ManualClock {
        ManualClock::new()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.start + self.moved()
    }
    fn wall(&self) -> SystemTime {
        self.wall + self.moved()
    }
}

/// The words for a certificate that this host's clock refused, with the
/// clock's reading `now`.
pub fn hint(now: SystemTime) -> String {
    // A clock before the epoch is given as it reads, as that is the reason
    // of the refusal: the second that holds `now`, rounded down.
    let secs = match now.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64) - i64::from(e.duration().subsec_nanos() > 0),
    };
    format!("certificates are checked against this host's clock, which reads {}", http_date(secs))
}

/// An IMF-fixdate (RFC 9110, `Thu, 08 Oct 2026 06:29:49 GMT`) for seconds
/// since the Unix epoch.
pub fn http_date(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{}, {d:02} {} {y} {:02}:{:02}:{:02} GMT",
        DAYS[days.rem_euclid(7) as usize],
        MONTHS[(m - 1) as usize],
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Seconds since the Unix epoch for an IMF-fixdate, or `None` for anything
/// else: the obsolete forms are sent by none of the servers that a crate
/// reads a date of (the relay's `Date`, an API's `Retry-After`).
pub fn parse_http_date(text: &str) -> Option<i64> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let [_, day, month, year, time, "GMT"] = parts.as_slice() else {
        return None;
    };
    let day: i64 = day.parse().ok().filter(|d| (1..=31).contains(d))?;
    let month = MONTHS.iter().position(|m| m == month)? as i64 + 1;
    // Four digits, as RFC 9110 writes a year: a peer's longer one would
    // overflow the count of seconds.
    let year: i64 = year.parse().ok().filter(|y| (1970..=9999).contains(y))?;
    let mut hms = time.split(':').map(|n| n.parse::<i64>().ok());
    let (h, m, s) = (hms.next()??, hms.next()??, hms.next()??);
    if hms.next().is_some() || h > 23 || m > 59 || s > 60 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + h * 3600 + m * 60 + s)
}

/// The seconds of a `Retry-After` value (RFC 9110): a number of seconds,
/// or the seconds from `now_secs` (since the Unix epoch) to its date; 0 for
/// a date in the past. `None` for anything else.
pub fn retry_after_secs(value: &str, now_secs: i64) -> Option<u64> {
    let value = value.trim();
    if let Ok(secs) = value.parse::<u64>() {
        return Some(secs);
    }
    let at = parse_http_date(value)?;
    Some(u64::try_from(at.saturating_sub(now_secs)).unwrap_or(0))
}

/// The wait of a `Retry-After` value at the wall time `now`, by the rules of
/// [`retry_after_secs`]; a `now` before the epoch counts as the epoch.
pub fn retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    let now_secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    retry_after_secs(value, i64::try_from(now_secs).unwrap_or(i64::MAX)).map(Duration::from_secs)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// algorithm), the inverse of [`civil_from_days`].
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The year, month and day of a count of days since 1970-01-01, proleptic
/// Gregorian (Howard Hinnant's algorithm).
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// A UTC time to the minute for a person, `2026-11-05 09:30 UTC`, for
/// seconds since the Unix epoch (the expiry of a token or of a pair).
pub fn utc_minutes(secs: u64) -> String {
    let secs = secs as i64;
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rest = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rest / 3600, rest % 3600 / 60)
}

/// An RFC 3339 UTC time to the second, `2026-11-05T09:30:00Z`, for a time
/// (a time before the epoch gives the epoch). The records of `pod-supervise`
/// hold their times in this form.
pub fn rfc3339(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rest = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A manual clock moves both of its times by each advance, and only
    /// then; the clock of the system moves on its own.
    #[test]
    fn a_manual_clock_moves_only_when_it_is_moved() {
        let wall = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let c = ManualClock::at(wall);
        let (t0, w0) = (c.now(), c.wall());
        assert_eq!(w0, wall);
        assert_eq!((c.now(), c.wall()), (t0, w0));
        c.advance(Duration::from_secs(90));
        assert_eq!(c.now() - t0, Duration::from_secs(90));
        assert_eq!(c.wall(), wall + Duration::from_secs(90));
        let s: &dyn Clock = &SystemClock;
        assert!(s.now() <= Instant::now());
    }
    use std::time::Duration;

    #[test]
    fn a_retry_after_is_seconds_or_a_date() {
        let now = parse_http_date("Tue, 24 Nov 2026 08:00:00 GMT").unwrap();
        assert_eq!(retry_after_secs(" 120 ", now), Some(120));
        assert_eq!(retry_after_secs("Tue, 24 Nov 2026 08:00:30 GMT", now), Some(30));
        assert_eq!(retry_after_secs("Tue, 24 Nov 2026 07:59:00 GMT", now), Some(0), "a date in the past");
        assert_eq!(retry_after_secs("soon", now), None);
        assert_eq!(retry_after_secs("-5", now), None);
        let wall = UNIX_EPOCH + Duration::from_secs(now as u64);
        assert_eq!(retry_after("Tue, 24 Nov 2026 08:00:30 GMT", wall), Some(Duration::from_secs(30)));
        assert_eq!(retry_after("7", wall), Some(Duration::from_secs(7)));
        assert_eq!(retry_after("soon", wall), None);
    }

    // The values of Python's datetime.fromtimestamp(t, timezone.utc).
    #[test]
    fn an_rfc3339_time_is_written_to_the_second() {
        assert_eq!(rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(UNIX_EPOCH + Duration::from_secs(1_791_440_989)), "2026-10-08T06:29:49Z");
        assert_eq!(rfc3339(UNIX_EPOCH + Duration::from_secs(951_868_799)), "2000-02-29T23:59:59Z");
        assert_eq!(rfc3339(UNIX_EPOCH - Duration::from_secs(5)), "1970-01-01T00:00:00Z");
    }

    // The values of Python's email.utils.formatdate(usegmt=True).
    #[test]
    fn a_date_is_written_as_python_writes_it() {
        assert_eq!(http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(http_date(1_791_440_989), "Thu, 08 Oct 2026 06:29:49 GMT");
        assert_eq!(http_date(951_868_799), "Tue, 29 Feb 2000 23:59:59 GMT");
    }

    // Expected values from Python's email.utils.parsedate and calendar.timegm.
    #[test]
    fn http_dates_parse_to_the_epoch_seconds_python_gives() {
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(parse_http_date("Thu, 08 Oct 2026 06:29:49 GMT"), Some(1_791_440_989));
        assert_eq!(parse_http_date("Tue, 29 Feb 2000 23:59:59 GMT"), Some(951_868_799));
        assert_eq!(parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"), Some(784_111_777));
    }

    #[test]
    fn anything_but_an_imf_fixdate_is_refused() {
        for bad in [
            "",
            "120",
            "Sunday, 06-Nov-94 08:49:37 GMT",
            "Sun Nov  6 08:49:37 1994",
            "Sun, 06 Nov 1994 08:49:37 UTC",
            "Sun, 06 Foo 1994 08:49:37 GMT",
            "Sun, 32 Nov 1994 08:49:37 GMT",
            "Sun, 06 Nov 1994 24:00:00 GMT",
            "Sun, 06 Nov 1994 08:49 GMT",
            "Sat, 31 Oct 10000 15:19:00 GMT",
            "Sat, 31 Oct 9223372036854775807 15:19:00 GMT",
        ] {
            assert_eq!(parse_http_date(bad), None, "{bad}");
        }
    }

    #[test]
    fn writing_is_the_inverse_of_reading() {
        for secs in [0, 784_111_777, 951_868_799, 1_791_440_989, 4_102_444_800] {
            let text = http_date(secs);
            assert_eq!(parse_http_date(&text), Some(secs), "{text}");
        }
        for days in [-1, 0, 20_652, 47_482] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }

    #[test]
    fn the_hint_gives_the_clock_that_it_was_given() {
        let said = hint(UNIX_EPOCH + Duration::from_secs(1_791_440_989));
        assert_eq!(
            said,
            "certificates are checked against this host's clock, which reads Thu, 08 Oct 2026 06:29:49 GMT"
        );
    }

    #[test]
    fn the_hint_gives_a_clock_before_the_epoch() {
        let day = std::time::Duration::from_secs(86_400);
        let text = hint(UNIX_EPOCH - day);
        assert!(text.ends_with("Wed, 31 Dec 1969 00:00:00 GMT"), "{text}");
        let text = hint(UNIX_EPOCH - std::time::Duration::from_millis(500));
        assert!(text.ends_with("Wed, 31 Dec 1969 23:59:59 GMT"), "{text}");
    }

    #[test]
    fn a_time_to_the_minute_is_in_utc() {
        assert_eq!(utc_minutes(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc_minutes(1_793_871_059), "2026-11-05 09:30 UTC");
        assert_eq!(utc_minutes(951_782_400), "2000-02-29 00:00 UTC");
    }
}
