//! The fuzz target \`warp_capsule\`: \`podcrates_fuzz::targets::warp_capsule\` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::warp_capsule(data));
