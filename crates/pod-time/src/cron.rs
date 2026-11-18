//! The cron times of `on.schedule` (T-142): five fields (minute, hour, day
//! of the month, month, day of the week) in UTC, as GitHub reads them. A
//! field takes `*`, a value, a range `A-B`, a step `*/N` or `A-B/N`, and
//! lists of these; the month and the day of the week also take their names
//! (`JAN`, `SUN`); a day of the week 7 is Sunday. When both day fields are
//! restricted, a day that matches one of them matches, as POSIX cron.

/// A parsed cron expression: the allowed values of each field.
#[derive(Debug, Clone, PartialEq)]
pub struct Cron {
    minute: Vec<bool>,
    hour: Vec<bool>,
    dom: Vec<bool>,
    month: Vec<bool>,
    dow: Vec<bool>,
    dom_any: bool,
    dow_any: bool,
}

const MONTHS: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
const DAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

fn value(s: &str, names: &[&str], base: u32) -> Option<u32> {
    let up = s.to_ascii_uppercase();
    names.iter().position(|n| *n == up).map(|i| i as u32 + base).or_else(|| s.parse().ok())
}

/// The allowed values of one field, in `lo..=hi`.
fn field(text: &str, lo: u32, hi: u32, names: &[&str], base: u32) -> Result<Vec<bool>, String> {
    let mut out = vec![false; hi as usize + 1];
    let bad = || format!("the cron field {text:?} is not valid ({lo}-{hi})");
    for part in text.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u32>().ok().filter(|s| *s > 0).ok_or_else(bad)?),
            None => (part, 1),
        };
        let (a, b) = match range {
            "*" => (lo, hi),
            r => match r.split_once('-') {
                Some((a, b)) => (value(a, names, base).ok_or_else(bad)?, value(b, names, base).ok_or_else(bad)?),
                // `N/S` runs from N to the end, as cron.
                None => {
                    let v = value(r, names, base).ok_or_else(bad)?;
                    (v, if part.contains('/') { hi } else { v })
                }
            },
        };
        if a < lo || b > hi || a > b {
            return Err(bad());
        }
        for v in (a..=b).step_by(step as usize) {
            out[v as usize] = true;
        }
    }
    Ok(out)
}

impl Cron {
    pub fn parse(text: &str) -> Result<Cron, String> {
        let f: Vec<&str> = text.split_whitespace().collect();
        let [m, h, dom, mon, dow] = f[..] else {
            return Err(format!("the cron {text:?} does not have five fields"));
        };
        let mut dow_v = field(dow, 0, 7, &DAYS, 0)?;
        if dow_v[7] {
            dow_v[0] = true;
        }
        Ok(Cron {
            minute: field(m, 0, 59, &[], 0)?,
            hour: field(h, 0, 23, &[], 0)?,
            dom: field(dom, 1, 31, &[], 0)?,
            month: field(mon, 1, 12, &MONTHS, 1)?,
            dow: dow_v,
            dom_any: dom == "*",
            dow_any: dow == "*",
        })
    }

    fn day(&self, dom: u32, dow: u32) -> bool {
        let (d, w) = (self.dom[dom as usize], self.dow[dow as usize]);
        match (self.dom_any, self.dow_any) {
            (false, false) => d || w,
            _ => d && w,
        }
    }

    /// The first time after `after` (seconds since the epoch, UTC) that
    /// matches, at the start of its minute; `None` in the next 5 years.
    pub fn next(&self, after: i64) -> Option<i64> {
        let start = after.div_euclid(60) * 60 + 60;
        let first_day = start.div_euclid(86_400);
        for day in first_day..first_day + 366 * 5 {
            let (_, month, dom) = civil(day);
            let dow = (day + 4).rem_euclid(7) as u32; // 1970-01-01 is a Thursday.
            if !self.month[month as usize] || !self.day(dom, dow) {
                continue;
            }
            for hour in 0..24u32 {
                if !self.hour[hour as usize] {
                    continue;
                }
                for minute in 0..60u32 {
                    let t = day * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60;
                    if self.minute[minute as usize] && t >= start {
                        return Some(t);
                    }
                }
            }
        }
        None
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Seconds of a UTC date and time.
    fn at(y: i64, mo: u32, d: u32, h: i64, mi: i64) -> i64 {
        let mut days = 0i64;
        // Count from the epoch with `civil` to stay in one algorithm.
        let target = (y, mo, d);
        while civil(days) < target {
            days += 1;
        }
        days * 86_400 + h * 3600 + mi * 60
    }

    #[test]
    fn the_next_time_of_a_cron_expression() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_406), (2025, 11, 14));
        let t = at(2026, 11, 14, 10, 7);
        let next = |c: &str| Cron::parse(c).unwrap().next(t).unwrap();
        assert_eq!(next("*/15 * * * *"), at(2026, 11, 14, 10, 15));
        assert_eq!(next("0 3 * * *"), at(2026, 11, 15, 3, 0));
        // 2026-11-14 is a Saturday: the next Monday is the 16th.
        assert_eq!(next("30 9 * * MON"), at(2026, 11, 16, 9, 30));
        assert_eq!(next("0 0 1 JAN *"), at(2027, 1, 1, 0, 0));
        assert_eq!(next("0 0 29 2 *"), at(2028, 2, 29, 0, 0), "the next leap day");
        // Both day fields: the 1st of the month, or a Sunday (the 15th).
        assert_eq!(next("0 12 1 * 0"), at(2026, 11, 15, 12, 0));
        assert_eq!(next("0 12 1 * 7"), at(2026, 11, 15, 12, 0), "7 is Sunday");
        assert_eq!(next("5/20 10 * * *"), at(2026, 11, 14, 10, 25));
        assert_eq!(next("7 10 * * *"), at(2026, 11, 15, 10, 7), "the time itself is not after it");
        for bad in ["* * * *", "60 * * * *", "* 24 * * *", "*/0 * * * *", "5-1 * * * *", "* * * FOO *"] {
            assert!(Cron::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(Cron::parse("0 0 31 2 *").unwrap().next(t), None, "February has no 31st");
    }
}
