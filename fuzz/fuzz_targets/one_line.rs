//! The fuzz target `one_line`: `podcrates_fuzz::targets::one_line` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::one_line(data));
