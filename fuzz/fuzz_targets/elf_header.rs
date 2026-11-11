//! The fuzz target `elf_header`: `podcrates_fuzz::targets::elf_header` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::elf_header(data));
