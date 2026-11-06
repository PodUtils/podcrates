//! The fuzz target `dns_wire`: `podcrates_fuzz::targets::dns_wire` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::dns_wire(data));
