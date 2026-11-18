//! The proleptic Gregorian calendar of a day since the epoch
//! (1970-01-01), both ways, with no table and no I/O.

/// Seconds in one day.
pub const DAY: i64 = 86_400;

/// The year, the month (1-12) and the day (1-31) of a day since the epoch.
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The day since the epoch of a year, a month (1-12) and a day (1-31).
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Whether `year` has a 29th of February.
pub fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// The count of days of `month` (1-12) in `year`.
pub fn month_days(year: i64, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The day of the week of a day since the epoch: 0 is Sunday.
pub fn weekday(days: i64) -> u32 {
    // 1970-01-01 is a Thursday.
    (days + 4).rem_euclid(7) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_day_of_four_centuries_goes_both_ways() {
        for days in -146_097..=146_097 {
            let (y, m, d) = civil(days);
            assert_eq!(days_from_civil(y, m, d), days, "{y}-{m}-{d}");
            assert!((1..=month_days(y, m)).contains(&d), "{y}-{m}-{d}");
        }
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2000, 2, 29), 11_016);
        assert_eq!(weekday(days_from_civil(2026, 11, 18)), 3, "a Wednesday");
        assert!(is_leap(2000) && !is_leap(1900) && is_leap(2028) && !is_leap(2026));
    }
}
