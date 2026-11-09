//! The fuzz target `gsr_message`: `podcrates_fuzz::targets::gsr_message` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::gsr_message(data));
