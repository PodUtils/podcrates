//! The fuzz target `tzif_zone`: `podcrates_fuzz::targets::tzif_zone` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::tzif_zone(data));
