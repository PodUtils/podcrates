//! The fuzz target `tpm_keyfile`: `podcrates_fuzz::targets::tpm_keyfile` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::tpm_keyfile(data));
