//! The fuzz target `wg_initiation`: `podcrates_fuzz::targets::wg_initiation` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::wg_initiation(data));
