//! The fuzz target `session_handover`: `podcrates_fuzz::targets::session_handover` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::session_handover(data));
