//! The fuzz target `qga_answer`: `podcrates_fuzz::targets::qga_answer` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::qga_answer(data));
