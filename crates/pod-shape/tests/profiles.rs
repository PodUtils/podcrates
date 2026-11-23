//! The profiles of issue #493: their data, their distribution, the first
//! flight, the pace, the entropy band and the report of a shape.

use pod_shape::{
    Band, Entropy, Error, NAMES, Pacer, PaddedFraction, Profile, Shape, ShapeReport, Shaper, State, printable_runs,
};

/// The tolerance of one bucket against the declared histogram, as an
/// absolute fraction of the records. The test states it here.
const TOLERANCE: f64 = 0.05;

fn profile(name: &str) -> Profile {
    Profile::load(name).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn each_profile_loads_and_a_profile_without_a_date_is_refused() {
    for name in NAMES {
        let profile = profile(name);
        assert_eq!(profile.name, name);
        assert!(!profile.buckets.is_empty(), "{name}");
        assert!(profile.first_flight.is_some(), "{name}");
        assert!(profile.pace.is_some(), "{name}");
        assert!(profile.entropy.is_some(), "{name}");
        assert!(!profile.source.date.is_empty(), "{name}");
    }
    let text = r#"
name = "no-date"
protocol = "https"
rule = "always"
buckets = [{ size = 512, weight = 1 }]
source = { cover_version = "1" }
"#;
    let error = Profile::parse("no-date", text).unwrap_err();
    assert!(matches!(error, Error::MissingDate { .. }), "{error}");
    assert!(error.to_string().contains("no source date"), "{error}");
}

#[test]
fn a_thousand_samples_follow_the_declared_histogram_and_no_short_cycle_repeats() {
    for name in NAMES {
        let profile = profile(name);
        let declared = Shape::of_profile(&profile);
        let mut shaper = Shaper::new(profile.clone(), 0x5eed_0493);
        let samples: Vec<usize> = (0..1000).map(|_| shaper.select_bucket()).collect();
        let observed = Shape::of(&samples, PaddedFraction::default());
        let mut total = 0.0;
        for (size, want) in &declared.buckets {
            total += want;
            let got = observed.bucket(*size);
            assert!(
                (want - got).abs() <= TOLERANCE,
                "{name}: bucket {size} declared {want:.3}, observed {got:.3}, tolerance {TOLERANCE}"
            );
        }
        assert!((total - 1.0).abs() < 1e-9, "{name}: the weights make {total}");
        let mut run = 1;
        let mut longest = 1;
        for pair in samples.windows(2) {
            run = if pair[0] == pair[1] { run + 1 } else { 1 };
            longest = longest.max(run);
        }
        assert!(longest < 16, "{name}: {longest} records of one size");
        for period in 1..16 {
            let repeats = (period..samples.len()).all(|at| samples[at] == samples[at - period]);
            assert!(!repeats, "{name}: a cycle of {period} records");
        }
        // The seed decides the sequence.
        let mut again = Shaper::new(profile, 0x5eed_0493);
        let repeat: Vec<usize> = (0..100).map(|_| again.select_bucket()).collect();
        assert_eq!(repeat, samples[..100], "{name}: the same seed gives the same bucket");
    }
}

#[test]
fn the_first_flight_fits_the_rate_and_is_bounded_by_the_target() {
    for name in NAMES {
        let profile = profile(name);
        let target = profile.first_flight.unwrap();
        assert_eq!(profile.first_flight(10_000_000), target, "{name}: a fast link uses the target");
        let slow = profile.first_flight(64_000);
        assert!(slow > 0, "{name}");
        assert!(slow <= target, "{name}");
        assert!((slow as f64 * 8.0) / 64_000.0 <= 2.0, "{name}: {slow} bytes do not fit two seconds");
    }
    // The file transfer asks for 64 KiB. Two seconds of 64 kbit/s hold 16 KB.
    let transfer = profile("file-transfer");
    assert_eq!(transfer.first_flight(64_000), 16_000);
}

#[test]
fn the_pacer_delay_follows_the_rate_and_the_keepalive_stays_inside_its_jitter() {
    for rate in [1_000u64, 4_000, 20_000] {
        let pacer = Pacer::new(rate, 20, 49);
        for bytes in [1usize, 100, 1_000] {
            let got = pacer.delay(bytes).as_secs_f64();
            let want = bytes as f64 * 8.0 / rate as f64;
            assert!(
                (got - want).abs() / want <= 0.05,
                "at {rate} bits/s, {bytes} bytes: {got} s, where {want} s is within 5 percent"
            );
        }
    }
    let mut pacer = Pacer::new(20_000, 20, 7);
    let mean = pacer.keepalive_mean().as_secs_f64();
    for _ in 0..1_000 {
        let got = pacer.keepalive().as_secs_f64();
        assert!(
            (got - mean).abs() <= 0.20 * mean + 0.001,
            "a keepalive of {got} s, outside 20 percent of {mean} s plus 1 ms"
        );
    }
}

#[test]
fn the_entropy_band_holds_over_a_thousand_windows_and_the_run_is_one_per_64_kib() {
    let band = Band { min_bits: 3.0, max_bits: 7.0, printable_run: 16 };
    let mut source = Entropy::new(band, 493).unwrap();
    let bytes = source.fill(1_000 * 4096);
    source.check(&bytes).unwrap();
    for block in bytes.chunks(65_536) {
        assert_eq!(printable_runs(block, 16), 1, "one printable run in a block of {}", block.len());
    }
    // A nonce is accepted once inside its window, and refused outside it.
    let now = 1_700_000_000u64;
    let nonce = source.nonce(now);
    source.accept(nonce, now).unwrap();
    assert!(matches!(source.accept(nonce, now + 8), Err(Error::Replay)));
    let old = source.nonce(now - 300);
    assert!(matches!(source.accept(old, now), Err(Error::NonceExpired { .. })));
    let future = source.nonce(now + 300);
    assert!(matches!(source.accept(future, now), Err(Error::NonceFuture)));
}

#[test]
fn the_report_holds_on_the_declared_shape_and_is_unseen_with_no_sample() {
    let profile = profile("page-load");
    let mut declared = Shape::of_profile(&profile);
    declared.padded = Some(0.10);
    let mut shaper = Shaper::new(profile, 77);
    let samples: Vec<usize> = (0..1_000).map(|_| shaper.select_bucket()).collect();
    let observed = Shape::of(&samples, PaddedFraction::of(10, 100));
    let report = ShapeReport::compare(declared.clone(), Some(observed));
    assert_eq!(report.state, State::Holds, "{:?}", report.warnings);
    assert!(report.warnings.is_empty());
    assert!(report.observed.is_some());

    let unseen = ShapeReport::compare(declared.clone(), None);
    assert_eq!(unseen.state, State::Unseen);
    assert!(unseen.observed.is_none());
    assert!(unseen.warnings.is_empty());

    let perturbed = Shape { buckets: vec![(128, 0.9), (512, 0.1)], padded: Some(0.9) };
    let report = ShapeReport::compare(declared, Some(perturbed));
    assert_eq!(report.state, State::Fails);
    assert!(!report.warnings.is_empty());
    assert!(report.warnings.iter().any(|w| w.starts_with("bucket 2048")), "{:?}", report.warnings);
    assert!(report.warnings.iter().any(|w| w.starts_with("padded fraction")), "{:?}", report.warnings);
}

#[test]
fn a_megabyte_at_the_512_bucket_stays_under_a_quarter_of_padded_bytes() {
    let text = r#"
name = "bucket-512"
protocol = "test"
rule = "always"
first_flight = 512
buckets = [{ size = 512, weight = 1 }]
pace = { rate = 1000000, jitter = 20, keepalive = 30 }
entropy = { min = 3.0, max = 7.0, printable_run = 16 }
source = { date = "2026-11-12", cover_version = "1" }
"#;
    let profile = Profile::parse("bucket-512", text).unwrap();
    let mut shaper = Shaper::new(profile, 11);
    let mut left = 1 << 20;
    while left > 0 {
        let chunk = left.min(448);
        let shaped = shaper.shape(chunk);
        assert_eq!(shaped.body, 512, "the chosen bucket holds the chunk");
        left -= chunk;
    }
    let fraction = shaper.padded_fraction();
    assert!(fraction.total >= 1 << 20);
    assert!(fraction.fraction() < 0.25, "{:.3} of {} bytes", fraction.fraction(), fraction.total);
    assert!(fraction.percent() < 25.0);
}

#[test]
fn the_rule_of_a_profile_says_when_a_record_pads() {
    let page = profile("page-load");
    let mut shaper = Shaper::new(page, 1);
    for at in 0..8 {
        assert!(shaper.shape(100).padded, "record {at} of the first flight");
    }
    assert!(!shaper.shape(100).padded, "a record after the first flight");

    let terminal = profile("terminal");
    let mut shaper = Shaper::new(terminal, 1);
    assert!(shaper.shape(100).padded, "a payload below the threshold");
    let before = shaper.body_out();
    let shaped = shaper.shape(2_000);
    assert!(!shaped.padded, "a payload at or above the threshold");
    assert_eq!(shaper.body_out(), before + shaped.body as u64);
    assert_eq!(shaper.records(), 2);
}
