//! The fuzz target `hold_sockaddr`: `podcrates_fuzz::targets::hold_sockaddr` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::hold_sockaddr(data));
