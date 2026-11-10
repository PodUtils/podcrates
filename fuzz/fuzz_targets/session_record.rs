//! The fuzz target `session_record`: `podcrates_fuzz::targets::session_record` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::session_record(data));
