//! The fuzz target `link_arq`: `podcrates_fuzz::targets::link_arq` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::link_arq(data));
