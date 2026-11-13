//! The fuzz target `link_kcp`: `podcrates_fuzz::targets::link_kcp` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::link_kcp(data));
