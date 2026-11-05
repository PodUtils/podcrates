//! The fuzz target `http_chunked`: `podcrates_fuzz::targets::http_chunked` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::http_chunked(data));
