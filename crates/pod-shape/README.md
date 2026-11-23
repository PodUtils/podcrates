# pod-shape

The shape of the record layer as data: padding profiles for four kinds of
traffic, a deterministic shaper, first-flight sizing, a pacer, entropy
control and a report of the declared shape against the observed one. New
code for issue #493 (T-300 in podnet's record layer). The crate prints
nothing, reads no environment variable and keeps no process state.

A record layer that pads each record to one fixed bucket has a signature.
This crate gives each session the distribution and the rules of the traffic
that it imitates, so that the padding follows the protocol.

## API

| Item | What |
| --- | --- |
| `Profile`, `Profile::load(name)`, `Profile::all()`, `Profile::parse(name, text)` | a profile as data: its name, its protocol, its bucket distribution, its padding rule, its first-flight target, its pace, its entropy band and its source date and version. `NAMES` names the four files under `profiles/` |
| `Bucket { size, weight }` | one body size and its relative weight. The sizes are `BUCKETS`: 0, 128, 512, 1024 and 2048 |
| `Rule` | `Always`, `First(n)` or `Below(n)`: when a record pads |
| `Profile::first_flight(measured_rate_bps)` | the opening write: two seconds of the measured rate, bounded by the target of the profile |
| `Shaper::new(profile, seed)`, `select_bucket()`, `shape(payload_len)` | a bucket for each record from a seed, and the body of a payload. `shape` applies the rule and counts `padded_out` and `body_out` |
| `Shaped { body, padded }` | the body length of one record, and whether it padded |
| `Shaper::padded_fraction()` | the padded bytes over the body bytes |
| `Pacer::new(rate_bps, jitter, seed)`, `Pacer::from_profile(profile, measured_rate_bps)`, `delay(bytes)`, `keepalive()` | the delay of a record at the rate, and a keepalive moved by the jitter of the profile |
| `Entropy::new(band, seed)`, `fill(len)`, `check(bytes)`, `nonce(now)`, `accept(nonce, now)` | a payload inside the band with one printable run per 64 KiB, and a nonce salted with a coarse timestamp, accepted once inside four minutes |
| `printable_runs(bytes, minimum)` | the count of the maximal printable runs of at least `minimum` bytes |
| `Shape`, `Shape::of(samples, padded)`, `Shape::of_profile(profile)` | the distribution of the body sizes, and the padded fraction where a transfer measured it |
| `ShapeReport::compare(declared, observed)`, `State` | the declared shape beside the observed one. The state is `Holds`, `Fails` or `Unseen` (the words of `pod-lab`). A divergence is a warning in words, not an error |
| `PaddedFraction { padded, total }`, `fraction()`, `percent()` | the padded bytes over the total bytes |

## Use

```rust
use pod_shape::{Profile, Shaper, State, Shape, ShapeReport, PaddedFraction};

let profile = Profile::load("page-load")?;
let mut shaper = Shaper::new(profile.clone(), seed);
let shaped = shaper.shape(payload.len());
// pod-e2e: records.seal_with(now, kind, 0, stream, &payload, shaped.body)?;

let declared = Shape::of_profile(&profile);
let observed = Shape::of(&samples, PaddedFraction::of(shaper.padded_out(), shaper.body_out()));
let report = ShapeReport::compare(declared, Some(observed));
if report.state == State::Fails {
    for warning in &report.warnings {
        eprintln!("{warning}");
    }
}
```

`pod-e2e` gives `Records::seal_with(now, kind, flags, stream, payload,
pad_to)` for the body that the shaper chose. `Records::stats()` gives
`records`, `body_out` and `padded_out`. `Settings::pad` and `seal` keep
their behaviour.

## The profiles

Each file is data for one kind of traffic:

| Profile | Protocol | Rule | First flight |
| --- | --- | --- | --- |
| `page-load` | HTTPS | the first 8 records | 16 KiB |
| `video-call` | RTP | each record | 8 KiB |
| `terminal` | SSH | a payload below 1024 bytes | 4 KiB |
| `file-transfer` | SFTP | each record | 64 KiB |

A profile with no date or no cover version fails to load, with the named
errors `Error::MissingDate` and `Error::MissingCoverVersion`. A bucket
outside the five sizes, a weight of zero, a bad rule, a bad pace and a bad
band fail with their own named error.

## The rules of issue #493

* PAD-1: a profile is a data file named for the traffic that it imitates.
* PAD-2: a profile is a distribution over record sizes plus its padding
  rule: `always`, `first:N` or `below:N`.
* PAD-3: the profile states the rule that decides when to pad. The caller
  applies it before the first write and after each write.
* PAD-4: a body pads to a bucket, not to a random length. The shaper
  chooses a bucket for each record, from a seeded generator. It does not
  repeat a short cycle.
* PAD-5: `PaddedFraction` reports the padded bytes as a fraction of the
  total bytes.
* ENT-1: the entropy of each window stays inside the band, and each 64 KiB
  block carries one printable run of the profile. The band is a range, not
  one target value.
* ENT-2: a nonce carries a coarse timestamp and is accepted only inside
  four minutes. A replay is refused.
* FLY-1: the opening write fits two seconds at the measured rate, bounded
  by the target of the profile. A fast link uses the target.
* PACE-1: `Pacer::delay` follows the measured rate, and `keepalive` moves by
  the jitter of the profile (about 20 percent).
* PACE-2: the caller never merges two logical writes. The shaper gives a
  body for each record.
* PACE-3: the session owns the pacing decision, so a forwarded port gets the
  same pacer.
* DECL-1: the caller gives `ShapeReport` the declared shape of the profile
  and the shape that it observed, and prints both with the state.
* DECL-2: a divergence is the warning list of `ShapeReport`, never a failed
  session.

## Tests

`tests/profiles.rs` checks each profile, the histogram of 1000 samples
within five percentage points, the absence of a cycle shorter than 16
records, the first flight at 10 Mbit/s and 64 kbit/s, the pacer at 1, 4 and
20 kbit/s, the entropy band over 1000 windows, the printable run, the
nonce window, the three states of the report and the padded fraction of a
1 MiB transfer at the 512 bucket.
