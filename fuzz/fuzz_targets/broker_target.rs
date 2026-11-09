//! The fuzz target `broker_target`: `podcrates_fuzz::targets::broker_target` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::broker_target(data));
