//! The fuzz target `mqtt_packet`: `podcrates_fuzz::targets::mqtt_packet` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::mqtt_packet(data));
