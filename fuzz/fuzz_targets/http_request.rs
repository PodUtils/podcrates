//! The fuzz target `http_request`: `podcrates_fuzz::targets::http_request` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::http_request(data));
