//! The fuzz target `close_reason`: `podcrates_fuzz::targets::close_reason` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::close_reason(data));
