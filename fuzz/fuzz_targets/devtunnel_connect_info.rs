//! The fuzz target `devtunnel_connect_info`: `podcrates_fuzz::targets::devtunnel_connect_info` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::devtunnel_connect_info(data));
