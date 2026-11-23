//! Each split behaviour has a simulator test and a named failure when the
//! path reassembles or requires a valid checksum.

use pod_sidecar::{Behaviour, Date, Policy, Simulator, SplitConfig, SplitError};

fn date() -> Date {
    Date::parse("2026-11-12").unwrap()
}

#[test]
fn a_split_at_an_offset_makes_two_segments_and_keeps_the_stream() {
    let simulator = Simulator::new(42);
    let run = simulator.split(Behaviour::SplitAt { offset: 3 }).unwrap();
    assert_eq!(run.frames.len(), run.baseline.len() + 1);
    assert_eq!(run.frames[0].seq.wrapping_add(run.frames[0].payload.len() as u32), run.frames[1].seq);
    assert_eq!(run.delivered, b"hello, sidecar!");
    assert_eq!(run.state.as_str(), "ESTABLISHED");
}

#[test]
fn a_disorder_swaps_the_first_two_segments_and_tcp_puts_them_back() {
    let simulator = Simulator::new(42);
    let run = simulator.split(Behaviour::Disorder).unwrap();
    assert_eq!(run.frames[0], run.baseline[1]);
    assert_eq!(run.frames[1], run.baseline[0]);
    assert_eq!(run.delivered, b"hello, sidecar!");
    assert_eq!(run.state.as_str(), "ESTABLISHED");
}

#[test]
fn a_fake_ttl_is_set_and_a_path_that_honours_it_defeats_the_behaviour() {
    let simulator = Simulator::new(42);
    let run = simulator.split(Behaviour::FakeTtl { ttl: 1 }).unwrap();
    assert_eq!(run.ttl, Some(1));
    let strict = Simulator::new(42).with_policy(Policy::strict());
    assert_eq!(strict.split(Behaviour::FakeTtl { ttl: 1 }), Err(SplitError::TimeExceeded { behaviour: "fake-ttl" }));
    let high = Simulator::new(42).with_policy(Policy::strict());
    assert_eq!(high.split(Behaviour::FakeTtl { ttl: 8 }).unwrap().ttl, Some(8));
}

#[test]
fn a_tampered_checksum_is_invalid_and_a_valid_checksum_requirement_defeats_it() {
    let simulator = Simulator::new(42);
    let run = simulator.split(Behaviour::TamperChecksum { mask: 0x0102 }).unwrap();
    assert!(!run.checksum_valid);
    assert!(!run.frames[0].checksum_valid());
    assert!(run.delivered.len() < run.baseline.iter().map(|s| s.payload.len()).sum::<usize>());
    let strict = Simulator::new(42).with_policy(Policy::strict());
    assert_eq!(
        strict.split(Behaviour::TamperChecksum { mask: 0x0102 }),
        Err(SplitError::ChecksumRejected { behaviour: "tamper-a-checksum" })
    );
}

#[test]
fn a_padded_first_segment_is_longer_and_reassembly_defeats_it() {
    let simulator = Simulator::new(42);
    let run = simulator.split(Behaviour::PadFirst { bytes: 4 }).unwrap();
    assert_eq!(run.padding, 4);
    assert_eq!(run.frames[0].payload.len(), run.baseline[0].payload.len() + 4);
    assert_eq!(run.delivered.len(), run.baseline.iter().map(|s| s.payload.len()).sum::<usize>() + 4);
    let strict = Simulator::new(42).with_policy(Policy::strict());
    assert_eq!(
        strict.split(Behaviour::PadFirst { bytes: 4 }),
        Err(SplitError::Reassembled { behaviour: "pad-the-first-segment" })
    );
}

#[test]
fn reassembly_defeats_a_split_and_a_disorder() {
    let strict = Simulator::new(42).with_policy(Policy::strict());
    assert_eq!(
        strict.split(Behaviour::SplitAt { offset: 3 }),
        Err(SplitError::Reassembled { behaviour: "split-at-an-offset" })
    );
    assert_eq!(strict.split(Behaviour::Disorder), Err(SplitError::Reassembled { behaviour: "disorder" }));
}

#[test]
fn one_file_format_carries_each_behaviour() {
    let file = r#"
date = "2026-11-12"
network = "lab-a"
found_by = "hand"

[[behaviours]]
kind = "split-at"
offset = 3

[[behaviours]]
kind = "disorder"

[[behaviours]]
kind = "fake-ttl"
ttl = 1

[[behaviours]]
kind = "tamper-checksum"
mask = 258

[[behaviours]]
kind = "pad-first"
bytes = 4
"#;
    let config = SplitConfig::from_toml(file).unwrap();
    assert_eq!(config.date, date());
    assert_eq!(config.behaviours.len(), 5);
    assert_eq!(config.behaviours[0], Behaviour::SplitAt { offset: 3 });
    assert_eq!(config.behaviours[3], Behaviour::TamperChecksum { mask: 258 });
    let runs = Simulator::new(42).split_all(&config).unwrap();
    assert_eq!(runs.len(), 5);
}

#[test]
fn a_split_file_with_no_date_is_refused() {
    let file = "network = \"lab-a\"\nfound_by = \"hand\"\n[[behaviours]]\nkind = \"disorder\"\n";
    assert_eq!(SplitConfig::from_toml(file), Err(SplitError::NoDate));
    let file = "date = \"2026-11-12\"\nnetwork = \"lab-a\"\nfound_by = \"hand\"\nbehaviours = []\n";
    assert!(matches!(SplitConfig::from_toml(file), Err(SplitError::Config(_))));
    let file = "date = \"2026-11-12\"\nnetwork = \"lab-a\"\nfound_by = \"hand\"\n[[behaviours]]\nkind = \"shuffle\"\n";
    assert!(matches!(SplitConfig::from_toml(file), Err(SplitError::Config(_))));
}

#[test]
fn each_behaviour_has_its_own_name() {
    let names: Vec<&str> = Behaviour::ALL.iter().map(|b| b.name()).collect();
    assert_eq!(names.len(), 5);
    assert!(names.contains(&"split-at-an-offset"));
    assert!(names.contains(&"disorder"));
    assert!(names.contains(&"fake-ttl"));
    assert!(names.contains(&"tamper-a-checksum"));
    assert!(names.contains(&"pad-the-first-segment"));
}
