//! The fuzz target `udp_frame`: `podcrates_fuzz::targets::udp_frame` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::udp_frame(data));
