//! The fuzz target `ws_upgrade`: `podcrates_fuzz::targets::ws_upgrade` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::ws_upgrade(data));
