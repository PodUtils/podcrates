//! The fuzz target `trojan_request_accept`: `podcrates_fuzz::targets::trojan_request_accept` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::trojan_request_accept(data));
