//! The fuzz target `relay_pool`: `podcrates_fuzz::targets::relay_pool` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::relay_pool(data));
