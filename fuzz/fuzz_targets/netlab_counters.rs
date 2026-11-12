//! The fuzz target `netlab_counters`: `podcrates_fuzz::targets::netlab_counters` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::netlab_counters(data));
