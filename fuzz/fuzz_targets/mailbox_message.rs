//! The fuzz target `mailbox_message`: `podcrates_fuzz::targets::mailbox_message` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::mailbox_message(data));
