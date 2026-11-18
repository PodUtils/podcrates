//! A time zone of the system by its IANA name: the zone file from `$TZDIR`,
//! else from `/usr/share/zoneinfo`, with no C library and no network.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::civil::DAY;
use crate::tzif::{self, Data};
use crate::{Error, ErrorKind};

/// The directory of the zone files when `$TZDIR` is not set.
pub const ZONEINFO: &str = "/usr/share/zoneinfo";

/// A time zone: the UTC offset at each instant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    name: String,
    data: Data,
}

/// The UTC instants of one local time of a zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Local {
    /// The local time occurs once.
    One(i64),
    /// The local time occurs twice (the offset went back): the first
    /// instance, then the second.
    Two(i64, i64),
    /// The local time does not occur (the offset went forward): the
    /// instant of the change, the first valid time after the gap.
    Gap(i64),
}

/// Refuse a name that is not a path under the zone directory.
fn check_name(name: &str) -> Result<(), Error> {
    let fine = |c: char| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '+' | '.');
    let parts_fine = name.split('/').all(|p| !p.is_empty() && p != "." && p != "..");
    if name.is_empty() || name.len() > 255 || !name.chars().all(fine) || !parts_fine {
        return Err(Error::new(ErrorKind::Name, format!("{name:?} is not a zone name (for example Europe/Berlin)")));
    }
    Ok(())
}

impl Zone {
    /// UTC.
    pub fn utc() -> Zone {
        Zone { name: "UTC".into(), data: Data::fixed(0) }
    }

    /// The zone `name` (for example `Europe/Berlin`) from `$TZDIR`, else
    /// from [`ZONEINFO`]. `UTC` and `Etc/UTC` need no file.
    pub fn load(name: &str) -> Result<Zone, Error> {
        let dir =
            std::env::var_os("TZDIR").filter(|d| !d.is_empty()).map_or_else(|| PathBuf::from(ZONEINFO), PathBuf::from);
        Zone::load_from(&dir, name)
    }

    /// The zone `name` from the directory `dir`.
    pub fn load_from(dir: &Path, name: &str) -> Result<Zone, Error> {
        check_name(name)?;
        if matches!(name, "UTC" | "Etc/UTC") {
            return Ok(Zone { name: name.into(), data: Data::fixed(0) });
        }
        if !dir.is_dir() {
            let text = format!("the host has no zone files in {} (install tzdata, or set TZDIR)", dir.display());
            return Err(Error::new(ErrorKind::NoZoneFiles, text));
        }
        let path = dir.join(name);
        let file = std::fs::File::open(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                Error::new(ErrorKind::NotFound, format!("no zone {name} in {}", dir.display()))
            }
            _ => Error::new(ErrorKind::Read, format!("{}: {e}", path.display())),
        })?;
        let mut bytes = Vec::new();
        file.take(tzif::MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::new(ErrorKind::Read, format!("{}: {e}", path.display())))?;
        Zone::from_tzif(name, &bytes).map_err(|e| Error::new(e.kind(), format!("{}: {}", path.display(), e.text())))
    }

    /// The zone `name` from the bytes of its TZif file.
    pub fn from_tzif(name: &str, bytes: &[u8]) -> Result<Zone, Error> {
        let data = tzif::parse(bytes).map_err(|e| Error::new(ErrorKind::Format, e))?;
        Ok(Zone { name: name.into(), data })
    }

    /// The name of the zone.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The UTC offset in seconds (east is positive) at the instant `utc`
    /// (seconds since the epoch).
    pub fn offset_at(&self, utc: i64) -> i32 {
        self.data.offset_at(utc)
    }

    /// The local time (seconds since the epoch, as if the zone were UTC) of
    /// the instant `utc`.
    pub fn local(&self, utc: i64) -> i64 {
        utc.saturating_add(i64::from(self.offset_at(utc)))
    }

    /// The first instant after `utc` at which the offset changes.
    pub fn next_change(&self, utc: i64) -> Option<i64> {
        self.data.next_change(utc)
    }

    /// The UTC instants of the local time `local`.
    pub fn instants(&self, local: i64) -> Local {
        // Each offset of a zone is within a day of UTC: the changes of
        // three days around the local time hold each offset that can give it.
        let (from, to) = (local.saturating_sub(3 * DAY), local.saturating_add(3 * DAY));
        let mut changes = Vec::new();
        let mut at = from;
        while let Some(next) = self.next_change(at).filter(|n| *n <= to) {
            changes.push((next, self.offset_at(next - 1), self.offset_at(next)));
            at = next;
            if changes.len() > 64 {
                break;
            }
        }
        let mut offsets: Vec<i32> = changes.iter().flat_map(|(_, a, b)| [*a, *b]).collect();
        offsets.push(self.offset_at(local));
        offsets.sort_unstable();
        offsets.dedup();
        let mut found: Vec<i64> = offsets
            .iter()
            .map(|o| local.saturating_sub(i64::from(*o)))
            .filter(|u| i64::from(self.offset_at(*u)) == local.saturating_sub(*u))
            .collect();
        found.sort_unstable();
        found.dedup();
        match found[..] {
            [one] => Local::One(one),
            [first, .., last] => Local::Two(first, last),
            [] => {
                // A gap: the change whose jump holds the local time.
                let gap = changes
                    .iter()
                    .find(|(at, before, after)| at + i64::from(*before) <= local && local < at + i64::from(*after));
                // A zone with no such change has each local time once; keep
                // the offset of the instant as the fallback.
                Local::Gap(gap.map_or(local.saturating_sub(i64::from(self.offset_at(local))), |(at, _, _)| *at))
            }
        }
    }
}
