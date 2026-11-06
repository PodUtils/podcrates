//! The fuzz target `doh_json`: `podcrates_fuzz::targets::doh_json` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::doh_json(data));
