//! The zones of small TZif files that the tests write: a zone with no
//! daylight time, a gap, a repeat, an instant after the last transition
//! (the footer), a zone of the south, a table with no footer; the names
//! that the crate refuses; the files that are not valid.

use pod_time::civil::{DAY, days_from_civil};
use pod_time::{Cron, ErrorKind, Local, Zone};

/// A UTC instant.
fn at(y: i64, mo: u32, d: u32, h: i64, mi: i64) -> i64 {
    days_from_civil(y, mo, d) * DAY + h * 3600 + mi * 60
}

/// A TZif file: `version` (0 or b'2'), the transitions (instant, type), the
/// time types (offset, daylight), and the footer for version 2.
fn tzif(version: u8, transitions: &[(i64, u8)], types: &[(i32, bool)], footer: &str) -> Vec<u8> {
    fn header(out: &mut Vec<u8>, version: u8, counts: [usize; 6]) {
        out.extend_from_slice(b"TZif");
        out.push(version);
        out.extend_from_slice(&[0; 15]);
        for n in counts {
            out.extend_from_slice(&(n as u32).to_be_bytes());
        }
    }
    let chars = b"ZONE\0";
    let block = |out: &mut Vec<u8>, wide: bool| {
        for (t, _) in transitions {
            if wide {
                out.extend_from_slice(&t.to_be_bytes());
            } else {
                out.extend_from_slice(&(*t as i32).to_be_bytes());
            }
        }
        out.extend(transitions.iter().map(|(_, i)| *i));
        for (offset, dst) in types {
            out.extend_from_slice(&offset.to_be_bytes());
            out.extend_from_slice(&[u8::from(*dst), 0]);
        }
        out.extend_from_slice(chars);
    };
    let counts = [0, 0, 0, transitions.len(), types.len(), chars.len()];
    let mut out = Vec::new();
    header(&mut out, version, counts);
    block(&mut out, false);
    if version != 0 {
        header(&mut out, version, counts);
        block(&mut out, true);
        out.push(b'\n');
        out.extend_from_slice(footer.as_bytes());
        out.push(b'\n');
    }
    out
}

/// A zone of Central Europe: its table ends in 2020; the footer gives the
/// years after.
fn berlin() -> Zone {
    Zone::from_tzif("Test/Berlin", &berlin_bytes()).expect("the zone")
}

#[test]
fn a_zone_with_no_daylight_time_has_one_offset() {
    let zone = Zone::from_tzif("Test/Kolkata", &tzif(b'2', &[], &[(19_800, false)], "<+0530>-5:30")).unwrap();
    for t in [i64::MIN, 0, at(2026, 6, 1, 0, 0), i64::MAX] {
        assert_eq!(zone.offset_at(t), 19_800);
    }
    assert_eq!(zone.next_change(0), None);
    // 09:00 local is 03:30 UTC.
    let cron = Cron::parse("0 9 * * *").unwrap();
    assert_eq!(cron.next_in(&zone, at(2026, 11, 18, 0, 0)), Some(at(2026, 11, 18, 3, 30)));
    assert_eq!(Zone::utc().offset_at(0), 0);
}

#[test]
fn the_table_then_the_footer_give_each_offset() {
    let zone = berlin();
    assert_eq!(zone.offset_at(at(2019, 7, 1, 0, 0)), 3600, "before the first transition: the first type");
    assert_eq!(zone.offset_at(at(2020, 7, 1, 0, 0)), 7200);
    assert_eq!(zone.offset_at(at(2020, 12, 1, 0, 0)), 3600);
    // After the last transition, the footer: 2026-03-29 01:00 UTC and
    // 2026-10-25 01:00 UTC.
    assert_eq!(zone.offset_at(at(2026, 3, 29, 0, 59)), 3600);
    assert_eq!(zone.offset_at(at(2026, 3, 29, 1, 0)), 7200);
    assert_eq!(zone.offset_at(at(2026, 10, 25, 0, 59)), 7200);
    assert_eq!(zone.offset_at(at(2026, 10, 25, 1, 0)), 3600);
    assert_eq!(zone.next_change(at(2026, 1, 1, 0, 0)), Some(at(2026, 3, 29, 1, 0)));
    assert_eq!(zone.next_change(at(2020, 7, 1, 0, 0)), Some(at(2020, 10, 25, 1, 0)));
    assert_eq!(zone.next_change(at(2020, 11, 1, 0, 0)), Some(at(2021, 3, 28, 1, 0)), "from the table to the footer");
}

#[test]
fn a_local_time_in_a_gap_runs_once_at_the_end_of_the_gap() {
    let zone = berlin();
    // 02:30 on 2026-03-29 does not occur: the clock goes from 02:00 to 03:00.
    let local = at(2026, 3, 29, 2, 30);
    assert_eq!(zone.instants(local), Local::Gap(at(2026, 3, 29, 1, 0)));
    let cron = Cron::parse("30 2 * * *").unwrap();
    let first = cron.next_in(&zone, at(2026, 3, 28, 12, 0)).unwrap();
    assert_eq!(first, at(2026, 3, 29, 1, 0), "the first valid time after the gap");
    assert_eq!(cron.next_in(&zone, first), Some(at(2026, 3, 30, 0, 30)), "02:30 CEST of the next day");
    // Each time of a cron in the gap runs once, at its end.
    let each = Cron::parse("*/15 2 * * *").unwrap();
    let first = each.next_in(&zone, at(2026, 3, 29, 0, 0)).unwrap();
    assert_eq!(first, at(2026, 3, 29, 1, 0));
    assert_eq!(each.next_in(&zone, first), Some(at(2026, 3, 30, 0, 0)));
}

#[test]
fn a_local_time_that_occurs_twice_runs_once_at_its_first_instance() {
    let zone = berlin();
    let local = at(2026, 10, 25, 2, 30);
    assert_eq!(zone.instants(local), Local::Two(at(2026, 10, 25, 0, 30), at(2026, 10, 25, 1, 30)));
    let cron = Cron::parse("30 2 * * *").unwrap();
    let first = cron.next_in(&zone, at(2026, 10, 24, 12, 0)).unwrap();
    assert_eq!(first, at(2026, 10, 25, 0, 30));
    assert_eq!(cron.next_in(&zone, first), Some(at(2026, 10, 26, 1, 30)), "not the second instance");
    // A start inside the second pass of the hour does not run it again.
    assert_eq!(cron.next_in(&zone, at(2026, 10, 25, 1, 10)), Some(at(2026, 10, 26, 1, 30)));
    assert_eq!(zone.instants(at(2026, 7, 1, 12, 0)), Local::One(at(2026, 7, 1, 10, 0)));
}

#[test]
fn a_zone_of_the_south_has_daylight_time_across_the_new_year() {
    let bytes = tzif(b'2', &[], &[(36_000, false)], "AEST-10AEDT,M10.1.0,M4.1.0/3");
    let zone = Zone::from_tzif("Test/Sydney", &bytes).unwrap();
    assert_eq!(zone.offset_at(at(2026, 1, 15, 0, 0)), 39_600);
    assert_eq!(zone.offset_at(at(2026, 7, 15, 0, 0)), 36_000);
    assert_eq!(zone.offset_at(at(2026, 12, 31, 20, 0)), 39_600);
}

#[test]
fn a_table_with_no_footer_keeps_its_last_offset() {
    let transitions = [(at(2000, 1, 1, 0, 0), 1)];
    let zone = Zone::from_tzif("Test/Old", &tzif(0, &transitions, &[(0, false), (-18_000, false)], "")).unwrap();
    assert_eq!(zone.offset_at(at(1999, 1, 1, 0, 0)), 0);
    assert_eq!(zone.offset_at(at(2040, 1, 1, 0, 0)), -18_000);
    assert_eq!(zone.next_change(at(1999, 1, 1, 0, 0)), Some(at(2000, 1, 1, 0, 0)));
    assert_eq!(zone.next_change(at(2001, 1, 1, 0, 0)), None);
}

#[test]
fn a_name_outside_the_zone_directory_is_refused() {
    let dir = std::env::temp_dir();
    for name in ["", "../etc/passwd", "/etc/localtime", "Europe//Berlin", "a\0b", "Europe/./Berlin", "x y"] {
        let e = Zone::load_from(&dir, name).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::Name, "{name:?}: {e}");
    }
    let missing = dir.join("pod-time-no-such-directory");
    let e = Zone::load_from(&missing, "Europe/Berlin").unwrap_err();
    assert_eq!(e.kind(), ErrorKind::NoZoneFiles, "{e}");
    assert!(e.text().contains("TZDIR"), "{e}");
    assert_eq!(Zone::load_from(&missing, "UTC").unwrap().offset_at(0), 0, "UTC needs no file");
}

#[test]
fn a_zone_file_from_a_directory() {
    let dir = std::env::temp_dir().join(format!("pod-time-zones-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("Test")).unwrap();
    std::fs::write(dir.join("Test/Kolkata"), tzif(b'2', &[], &[(19_800, false)], "IST-5:30")).unwrap();
    let zone = Zone::load_from(&dir, "Test/Kolkata").unwrap();
    assert_eq!((zone.name(), zone.offset_at(0)), ("Test/Kolkata", 19_800));
    assert_eq!(Zone::load_from(&dir, "Test/Nowhere").unwrap_err().kind(), ErrorKind::NotFound);
    std::fs::write(dir.join("Test/Bad"), b"not a zone").unwrap();
    assert_eq!(Zone::load_from(&dir, "Test/Bad").unwrap_err().kind(), ErrorKind::Format);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_that_is_not_valid_is_an_error_and_never_a_panic() {
    let good = berlin_bytes();
    // Each prefix of a valid file.
    for n in 0..good.len() {
        assert!(Zone::from_tzif("cut", &good[..n]).is_err(), "a prefix of {n} bytes");
    }
    // Each byte of the header and of the data, changed.
    for i in 0..good.len() {
        let mut bent = good.clone();
        bent[i] ^= 0xa5;
        if let Ok(zone) = Zone::from_tzif("bent", &bent) {
            let _ = (zone.offset_at(0), zone.next_change(0), zone.instants(at(2026, 3, 29, 2, 30)));
        }
    }
    let mut leap = good.clone();
    // The leap count of the second header is not zero: a right/ zone.
    let second = good.windows(4).rposition(|w| w == b"TZif").unwrap();
    leap[second + 20 + 11] = 1;
    let e = Zone::from_tzif("leap", &leap).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Format);
    let bad_footer = tzif(b'2', &[], &[(0, false)], "EST5EDT");
    assert!(Zone::from_tzif("footer", &bad_footer).is_err());
}

fn berlin_bytes() -> Vec<u8> {
    let transitions = [(at(2020, 3, 29, 1, 0), 1), (at(2020, 10, 25, 1, 0), 0)];
    tzif(b'2', &transitions, &[(3600, false), (7200, true)], "CET-1CEST,M3.5.0,M10.5.0/3")
}

/// The zone files of the host, where it has them: two known offsets.
#[test]
fn the_zone_files_of_the_host_where_it_has_them() {
    let dir = std::path::Path::new(pod_time::ZONEINFO);
    if !dir.join("Europe/Berlin").is_file() {
        eprintln!("skipped: the host has no {}/Europe/Berlin", dir.display());
        return;
    }
    let zone = Zone::load_from(dir, "Europe/Berlin").unwrap();
    assert_eq!(zone.offset_at(at(2026, 1, 15, 12, 0)), 3600);
    assert_eq!(zone.offset_at(at(2026, 7, 15, 12, 0)), 7200);
    assert_eq!(zone.next_change(at(2026, 1, 1, 0, 0)), Some(at(2026, 3, 29, 1, 0)));
}
