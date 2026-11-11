//! The fuzz target `oci_manifest`: `podcrates_fuzz::targets::oci_manifest` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::oci_manifest(data));
