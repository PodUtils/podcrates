//! The fuzz target `tpm_response`: `podcrates_fuzz::targets::tpm_response` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::tpm_response(data));
