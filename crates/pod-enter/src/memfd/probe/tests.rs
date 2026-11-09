use super::*;

/// Each measurement with its sysctl, mapped. Shared by the tests below.
fn cases() -> Vec<MemfdExec> {
    use MemfdExec as M;
    vec![
        M::Exec { sysctl: None },
        M::Exec { sysctl: Some(0) },
        M::Exec { sysctl: Some(1) },
        M::NoMemfd {
            sysctl: Some(2),
            errno: sys::EACCES,
        },
        M::NoMemfd {
            sysctl: Some(2),
            errno: sys::EPERM,
        },
        M::NoMemfd {
            sysctl: Some(2),
            errno: sys::EINVAL,
        },
        M::NoMemfd {
            sysctl: None,
            errno: sys::EPERM,
        },
        M::NoExec {
            sysctl: Some(2),
            errno: sys::EACCES,
        },
        M::NoExec {
            sysctl: Some(1),
            errno: sys::EACCES,
        },
        M::NoExec {
            sysctl: Some(0),
            errno: sys::EACCES,
        },
        M::NoExec {
            sysctl: None,
            errno: sys::EACCES,
        },
        M::Unmeasured {
            step: "clone(2) for the probe child",
            why: "EAGAIN (11)".to_string(),
        },
    ]
}

/// T-1688: health agrees with the fix list. A denial carries exactly one
/// fix, a hold and an unmeasured probe carry none, and only `Exec` runs.
#[test]
fn a_fix_comes_with_each_denial_and_with_nothing_else() {
    for m in cases() {
        let a = answer(&m);
        assert_eq!(
            a.fix.is_some(),
            a.standing == Standing::Denied,
            "{m:?} -> {a:?}"
        );
        assert_eq!(m.runs(), a.standing == Standing::Holds, "{m:?}");
    }
}

/// T-1688: value 2 is named as the cause, with the sysctl fix, where the
/// kernel refuses with `EACCES` or `EPERM`. Each other denial is "denied,
/// cause unknown" and its fix names no sysctl.
#[test]
fn the_mapping_names_the_sysctl_only_where_it_is_the_cause() {
    for m in cases() {
        let a = answer(&m);
        let by_sysctl = matches!(
            m,
            MemfdExec::NoMemfd { sysctl: Some(2), errno } if errno == sys::EACCES || errno == sys::EPERM
        ) || matches!(
            m,
            MemfdExec::NoExec {
                sysctl: Some(2),
                ..
            }
        );
        if by_sysctl {
            assert!(a.detail.starts_with("denied by vm.memfd_noexec=2"), "{a:?}");
            let fix = a.fix.as_deref().unwrap();
            assert!(fix.contains("set vm.memfd_noexec to 0"), "{fix}");
            assert!(fix.contains("another rung"), "{fix}");
        } else if a.standing == Standing::Denied {
            assert!(a.detail.starts_with("denied, cause unknown"), "{a:?}");
            assert!(!a.fix.as_deref().unwrap().contains("memfd_noexec"), "{a:?}");
        }
    }
}

/// T-1688: the words for each sysctl value, pinned.
#[test]
fn the_mapping_words_each_value() {
    let one = answer(&MemfdExec::Exec { sysctl: Some(1) });
    assert!(one.detail.contains("vm.memfd_noexec=1"), "{one:?}");
    assert!(one.detail.contains("podvm passes MFD_EXEC"), "{one:?}");
    let zero = answer(&MemfdExec::Exec { sysctl: Some(0) });
    assert!(zero.detail.ends_with("(vm.memfd_noexec=0)"), "{zero:?}");
    let none = answer(&MemfdExec::Exec { sysctl: None });
    assert!(none.detail.contains("not readable"), "{none:?}");
    let exec1 = answer(&MemfdExec::NoExec {
        sysctl: Some(1),
        errno: sys::EACCES,
    });
    assert!(exec1.detail.contains("EACCES (13)"), "{exec1:?}");
    assert!(
        exec1.detail.contains("which podvm meets with MFD_EXEC"),
        "{exec1:?}"
    );
    let exec0 = answer(&MemfdExec::NoExec {
        sysctl: Some(0),
        errno: sys::EACCES,
    });
    assert!(exec0.detail.contains("which allows exec"), "{exec0:?}");
    let two = answer(&MemfdExec::NoMemfd {
        sysctl: Some(2),
        errno: sys::EACCES,
    });
    assert!(
        two.detail.contains("memfd_create with MFD_EXEC fails"),
        "{two:?}"
    );
    assert!(
        two.sentence().contains("; fix: set vm.memfd_noexec to 0"),
        "{two:?}"
    );
    let un = answer(&MemfdExec::Unmeasured {
        step: "s",
        why: "w".to_string(),
    });
    assert_eq!(un.detail, "not measured: s failed: w");
    assert_eq!(un.sentence(), un.detail);
}

/// Only an exit with the trampoline's own code is `Exec`: a 0, a 127 or
/// a signal is a child that something else ended.
#[test]
fn only_the_trampoline_code_reads_as_exec() {
    assert_eq!(
        classify_status(TRAMPOLINE_EXIT << 8, Some(0)),
        MemfdExec::Exec { sysctl: Some(0) }
    );
    for status in [0, EXEC_FAILED << 8, 9, 31] {
        assert!(
            matches!(classify_status(status, None), MemfdExec::Unmeasured { .. }),
            "status {status:#x}"
        );
    }
}

/// The trampoline is the payload shape the memfd rung takes: an ELF with
/// no `PT_INTERP`, for this machine, entered inside its one segment.
#[test]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn the_trampoline_is_a_static_elf_for_this_machine() {
    super::super::eligible(&TRAMPOLINE).expect("the trampoline is eligible");
    let t = &TRAMPOLINE;
    assert_eq!(t.len(), elf::LEN);
    assert_eq!(u16::from_le_bytes([t[18], t[19]]), elf::MACHINE);
    let entry = u64::from_le_bytes(t[24..32].try_into().unwrap());
    let vaddr = u64::from_le_bytes(t[80..88].try_into().unwrap());
    let filesz = u64::from_le_bytes(t[96..104].try_into().unwrap());
    assert_eq!(filesz, t.len() as u64);
    assert_eq!(entry, vaddr + elf::CODE_AT as u64);
    assert!(entry < vaddr + filesz);
}

/// T-1688: on this host the sysctl is read and the probe answers from it.
/// Value 0, 1 or no sysctl: the trampoline runs (podvm passes MFD_EXEC
/// under 1). Value 2: `memfd_create` is refused, named as the sysctl.
#[test]
fn the_probe_answers_from_this_host() {
    let m = memfd_exec();
    match read_sysctl() {
        Some(2) => assert_eq!(
            answer(m).standing,
            Standing::Denied,
            "vm.memfd_noexec=2 denies: {m:?}"
        ),
        sysctl => assert_eq!(*m, MemfdExec::Exec { sysctl }, "the trampoline runs here"),
    }
    // Cached: a second read is the same answer, not a second measurement.
    assert!(std::ptr::eq(m, memfd_exec()));
}

/// T-1688: the denial path, made on demand. A memfd made with
/// `MFD_NOEXEC_SEAL` is what the kernel makes under `vm.memfd_noexec=1`
/// for a caller without `MFD_EXEC`: `execveat` fails with `EACCES`, and
/// the probe maps that to `NoExec`, which denies.
#[test]
fn a_memfd_sealed_against_exec_reads_as_no_exec() {
    let name = sys::CBuf::new("podvm-noexec").expect("literal has no NUL");
    let flags = super::super::MFD_CLOEXEC | super::super::MFD_NOEXEC_SEAL;
    let fd = unsafe { sys::sys(sys::SYS_MEMFD_CREATE, [name.ptr(), flags, 0, 0, 0, 0]) }
        .expect("memfd_create with MFD_NOEXEC_SEAL runs on Linux 6.3 or later");
    unsafe { super::super::write_full(fd, &TRAMPOLINE) }.expect("the trampoline is written");
    let m = measure_fd(fd, Some(0));
    let _ = sys::close(fd);
    assert_eq!(
        m,
        MemfdExec::NoExec {
            sysctl: Some(0),
            errno: sys::EACCES
        }
    );
    assert_eq!(answer(&m).standing, Standing::Denied);
    // The same bytes in a memfd made the way podvm makes one do run, so
    // the seal and nothing else made the difference. Under value 2 podvm
    // cannot make one, which `the_probe_answers_from_this_host` pins.
    if RUNS_CODE && read_sysctl() != Some(2) {
        let fd = super::super::stage_raw(&TRAMPOLINE).expect("the trampoline stages");
        let ran = measure_fd(fd, Some(0));
        let _ = sys::close(fd);
        assert_eq!(ran, MemfdExec::Exec { sysctl: Some(0) });
    }
}

/// A descriptor that is not open is a denial with the kernel's errno, not
/// an exec: the child reports `execveat`'s errno through the pipe.
#[test]
fn a_closed_descriptor_reads_as_ebadf() {
    let m = measure_fd(1 << 20, None);
    assert_eq!(
        m,
        MemfdExec::NoExec {
            sysctl: None,
            errno: sys::EBADF
        }
    );
}

#[test]
fn the_flags_match_linux_memfd_h() {
    assert_eq!(super::super::MFD_NOEXEC_SEAL, 0x0008, "MFD_NOEXEC_SEAL");
    assert_eq!(super::super::MFD_EXEC, 0x0010, "MFD_EXEC");
}
