//! The fuzz target `wg_transport`: `podcrates_fuzz::targets::wg_transport` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::wg_transport(data));
