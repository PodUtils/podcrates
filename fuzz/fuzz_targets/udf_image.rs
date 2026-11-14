//! The fuzz target `udf_image`: `podcrates_fuzz::targets::udf_image` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::udf_image(data));
