//! The fuzz target `relay_bearer`: `podcrates_fuzz::targets::relay_bearer` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::relay_bearer(data));
