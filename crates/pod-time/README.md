# pod-time

Time zones and the cron of GitHub Actions' `on.schedule`. GitHub runs a
cron of `on.schedule` at the local time of its `timezone` (an IANA name).
The crate reads the zone files of the system (TZif, RFC 8536) and gives
the next time of a cron as a UTC instant. It uses std alone: no C
library, no network, no table of zones in the binary.

podrunner asked for it (PodUtils/podrunner#291, T-907, call C779). Its
cron (`PodUtils/podrunner:crates/podrunner-local/src/cron.rs`) moves here
by the ruling "Shared code".

## Use

```rust
use pod_time::{Cron, Zone};

let cron = Cron::parse("30 2 * * *")?;
let zone = Zone::load("Europe/Berlin")?;        // $TZDIR, else /usr/share/zoneinfo
let next = cron.next_in(&zone, now_unix_seconds); // a UTC instant, or None
let utc = cron.next(now_unix_seconds);           // a cron in UTC, as with no timezone
```

## API

- `Cron::parse(text)`: five fields (minute, hour, day of the month,
  month, day of the week). A field takes `*`, a value, a range `A-B`, a
  step `*/N` or `A-B/N`, and lists of these; the month and the day of the
  week also take their names (`JAN`, `SUN`); a day of the week 7 is
  Sunday. When both day fields are restricted, a day that matches one of
  them matches (POSIX cron).
- `Cron::next(after)`: the first matching minute after `after` in UTC;
  `None` in the next 5 years.
- `Cron::next_in(zone, after)`: the same in the local time of `zone`, as
  a UTC instant.
- `Zone::load(name)`, `Zone::load_from(dir, name)`, `Zone::from_tzif(name,
  bytes)`, `Zone::utc()`: a zone. `UTC` and `Etc/UTC` need no file.
- `Zone::offset_at(utc)`: the UTC offset in seconds (east is positive).
- `Zone::local(utc)`, `Zone::instants(local)`: an instant to its local
  time, and a local time to its instants (`Local::One`, `Local::Two`,
  `Local::Gap`).
- `Zone::next_change(utc)`: the next instant at which the offset changes.
- `civil`: the proleptic Gregorian calendar of a day since the epoch.

## The rules

- A local time in a gap (the offset went forward) runs once, at the first
  valid time after the gap: the instant of the change. Each time of the
  cron in the gap gives that instant once.
- A local time that occurs twice (the offset went back) runs once, at its
  first instance. These are the rules of cronie and Vixie cron; GitHub
  does not document its rule.
- The zone name is a relative path of the letters, digits and `/_-+.`,
  with no empty part, no `.` and no `..`. A name with a leading `/`, a
  NUL or `..` is refused before a file is opened.
- The zone file is TZif version 1 to 4. The 64-bit data of version 2 and
  later is read, and the 32-bit block is skipped. Before the first
  transition, the first time type holds; after the last transition, the
  POSIX TZ string of the footer holds (RFC 8536, section 3.2), with the
  extensions of RFC 8536 (a rule time from -167 to 167 hours, a quoted
  name).
- A zone with leap seconds (a `right/` zone) is refused: a cron counts
  POSIX seconds.
- A file is read up to 1 MiB. Each count is checked against the length of
  the file before a read, so a short or a hostile file is an error, never
  a panic.

## Errors

`Error` has a kind and one line of text: `Name` (not a zone name),
`NoZoneFiles` (the host has no zone directory: install tzdata, or set
`TZDIR`), `NotFound` (no zone of the name), `Read`, `Format` (not a TZif
file that the crate reads), `Cron` (not a cron).

## Adoption map

| podrunner | pod-time |
| --- | --- |
| `podrunner_local::cron::Cron` | `pod_time::Cron` |
| `podrunner_local::cron::civil` | `pod_time::civil::civil` |
| `Cron::parse` gives `Err(String)` | `Cron::parse` gives `Err(pod_time::Error)` (`to_string()` gives the same text) |
| the cron in UTC alone | `Cron::next_in(&Zone::load(timezone)?, after)` for an item with `timezone` |

## Tests

```sh
cargo test -p pod-time
```

`tests/zone.rs` writes small TZif files: a zone with no daylight time, a
gap, a repeat, an instant after the last transition, a zone of the south,
a table with no footer, each prefix and each changed byte of a file. It
reads `/usr/share/zoneinfo/Europe/Berlin` where the host has it.

## Sources

`SOURCES`: `src/cron.rs` from podrunner at `d72b161`.
