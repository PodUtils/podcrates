//! The fuzz target `connect_status`: `podcrates_fuzz::targets::connect_status` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::connect_status(data));
