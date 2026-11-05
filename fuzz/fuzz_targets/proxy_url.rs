//! The fuzz target `proxy_url`: `podcrates_fuzz::targets::proxy_url` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::proxy_url(data));
