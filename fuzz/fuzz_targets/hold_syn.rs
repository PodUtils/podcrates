//! The fuzz target `hold_syn`: `podcrates_fuzz::targets::hold_syn` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::hold_syn(data));
