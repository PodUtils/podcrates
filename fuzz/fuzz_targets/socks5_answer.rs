//! The fuzz target `socks5_answer`: `podcrates_fuzz::targets::socks5_answer` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::socks5_answer(data));
