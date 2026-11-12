//! The fuzz target `actions_expr`: `podcrates_fuzz::targets::actions_expr` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::actions_expr(data));
