//! The fuzz target `e2e_handover`: `podcrates_fuzz::targets::e2e_handover` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::e2e_handover(data));
