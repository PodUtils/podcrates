//! The fuzz target `quic_probe_answer`: `podcrates_fuzz::targets::quic_probe_answer` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::quic_probe_answer(data));
