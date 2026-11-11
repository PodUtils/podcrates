//! The fuzz target `relay_list`: `podcrates_fuzz::targets::relay_list` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::relay_list(data));
