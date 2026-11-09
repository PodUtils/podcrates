use super::*;

#[test]
fn the_host_architecture_needs_no_interpreter() {
    assert!(matches!(support_for("amd64", "amd64"), Support::Native));
}

#[test]
fn an_architecture_podvm_cannot_name_says_so_rather_than_guessing() {
    let s = support_for("nosucharch", "amd64");
    match s {
        Support::None { why } => assert!(why.contains("no ELF machine number"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_elf_machine_numbers_are_the_specifications() {
    // `readelf -h` prints 0xb7 for an aarch64 ELF file and 0x3e for
    // x86-64.
    assert_eq!(machine_for("arm64"), Some(0xb7));
    assert_eq!(machine_for("amd64"), Some(0x3e));
    assert_eq!(machine_for("riscv64"), Some(0xf3));
    assert_eq!(machine_for("nope"), None);
}

/// Reads the real `/proc/sys/fs/binfmt_misc` where it is mounted, and
/// checks only what is true in each case. A test that needs a
/// registration would test the configuration of the machine, not the
/// code.
#[test]
fn a_missing_binfmt_directory_is_reported_and_not_a_panic() {
    let s = support_for("arm64", "amd64");
    match s {
        Support::None { why } => {
            assert!(!why.is_empty(), "a refusal with no reason");
        }
        Support::Interpreter(r) | Support::InterpreterNeedsCopyIn(r) => {
            assert!(!r.interpreter.is_empty());
            assert_eq!(r.machine, Some(0xb7));
        }
        Support::Native => panic!("arm64 is not amd64"),
    }
}
