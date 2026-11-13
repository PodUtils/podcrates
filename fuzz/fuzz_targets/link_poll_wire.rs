//! The fuzz target `link_poll_wire`: `podcrates_fuzz::targets::link_poll_wire` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::link_poll_wire(data));
