//! The fuzz target `flatten_layers`: `podcrates_fuzz::targets::flatten_layers` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::flatten_layers(data));
