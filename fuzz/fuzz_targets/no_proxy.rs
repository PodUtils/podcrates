//! The fuzz target `no_proxy`: `podcrates_fuzz::targets::no_proxy` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::no_proxy(data));
