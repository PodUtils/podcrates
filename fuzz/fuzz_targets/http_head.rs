//! The fuzz target `http_head`: `podcrates_fuzz::targets::http_head` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::http_head(data));
