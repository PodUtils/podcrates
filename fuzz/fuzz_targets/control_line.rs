//! The fuzz target `control_line`: `podcrates_fuzz::targets::control_line` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::control_line(data));
