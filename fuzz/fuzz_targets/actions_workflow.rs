//! The fuzz target `actions_workflow`: `podcrates_fuzz::targets::actions_workflow` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::actions_workflow(data));
