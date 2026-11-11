//! The fuzz target `wg_cookie`: `podcrates_fuzz::targets::wg_cookie` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::wg_cookie(data));
