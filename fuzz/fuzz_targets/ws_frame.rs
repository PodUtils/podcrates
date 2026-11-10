//! The fuzz target `ws_frame`: `podcrates_fuzz::targets::ws_frame` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::ws_frame(data));
