//! The date of a run. The harness reads the clock of the machine. It does
//! not hold a date of its own.

use std::time::{SystemTime, UNIX_EPOCH};

/// The date of this run as `YYYY-MM-DD`, from the clock of the machine.
pub fn today() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    from_unix_days(seconds / 86_400)
}

/// The date of a count of days after 1970-01-01, in the proleptic
/// Gregorian calendar (Howard Hinnant's civil_from_days).
pub fn from_unix_days(days: u64) -> String {
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    format!("{year:04}-{month:02}-{day:02}")
}

/// Whether `text` holds an ISO date (`YYYY-MM-DD`) with a real month and
/// day. The claims check uses it. A year is four digits from 2000.
pub fn is_iso_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !bytes[0..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || !bytes[8..10].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let year: u32 = text[0..4].parse().unwrap_or(0);
    let month: u32 = text[5..7].parse().unwrap_or(0);
    let day: u32 = text[8..10].parse().unwrap_or(0);
    year >= 2000 && (1..=12).contains(&month) && (1..=31).contains(&day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_and_a_leap_day_are_read() {
        assert_eq!(from_unix_days(0), "1970-01-01");
        assert_eq!(from_unix_days(19_782), "2024-02-29");
        assert_eq!(from_unix_days(20_000), "2024-10-04");
    }

    #[test]
    fn a_date_token_is_strict() {
        assert!(is_iso_date("2026-11-20"));
        assert!(!is_iso_date("2026-13-20"));
        assert!(!is_iso_date("26-11-20"));
        assert!(!is_iso_date("2026-11-20T00:00"));
        assert!(!is_iso_date("1999-01-01"));
    }

    #[test]
    fn today_has_the_form_of_a_date() {
        let now = today();
        assert!(is_iso_date(&now), "{now}");
    }
}
