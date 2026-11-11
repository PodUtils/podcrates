//! The fuzz target `squashfs_image`: `podcrates_fuzz::targets::squashfs_image` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::squashfs_image(data));
