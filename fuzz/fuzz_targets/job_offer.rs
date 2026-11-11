//! The fuzz target `job_offer`: `podcrates_fuzz::targets::job_offer` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::job_offer(data));
