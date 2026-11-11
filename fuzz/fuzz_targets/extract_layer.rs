//! The fuzz target `extract_layer`: `podcrates_fuzz::targets::extract_layer` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::extract_layer(data));
