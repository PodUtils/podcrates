use super::*;

/// The worst failure available to this module is a wrong number or a
/// wrong ABI: it would report a verdict for a syscall nobody named, and it
/// would look exactly like a measurement. Two cross-checks against facts
/// this process already knows by another route.
///
/// Neither proves the whole table. The table comes from the `syscalls` and
/// `linux-raw-sys` crates, which take it from the source of the kernel.
#[test]
fn the_trap_and_the_abi_agree_with_what_this_process_already_knows() {
    let raw = unsafe { syscall6(SYS_GETPID, 0, 0, 0, 0, 0, 0) };
    assert_eq!(raw, std::process::id() as i64, "SYS_GETPID or the syscall ABI is wrong");
}

#[test]
fn a_negative_return_decodes_as_the_errno_and_not_as_a_value() {
    // close(-1) is EBADF on every Linux. If `split` had the window wrong,
    // this would come back as a huge positive value instead.
    assert_eq!(close(-1), Err(EBADF));
    assert_eq!(split(-1), Err(EPERM));
    assert_eq!(split(-4095), Err(Errno(4095)));
    // Outside the window is a VALUE, including a large negative one from
    // a syscall that returns a signed quantity.
    assert_eq!(split(-4096), Ok(-4096));
    assert_eq!(split(0), Ok(0));
}

#[test]
fn a_path_with_an_interior_nul_is_refused_rather_than_truncated() {
    assert!(CBuf::new("/tmp/a\0b").is_none());
    assert!(CBuf::new("/tmp/ab").is_some());
}

/// A short buffer is a kernel write past the end of it, and the shape is
/// **not the same on two architectures**: 144 bytes on x86_64 and 128 on
/// aarch64.
///
/// Both numbers below were **measured, not read from a header**: the
/// x86_64 one on a host, the aarch64 one with the same `size_of` built for
/// aarch64 and run under `qemu-aarch64`.
///
/// An architecture with no number here is not skipped: it asserts that the
/// buffer is at least as large as the view read from it. The crate gets
/// that by construction, because the buffer IS the struct that the crate
/// generates from the headers of the kernel.
#[test]
fn the_stat_buffer_is_the_size_the_kernel_writes() {
    let n = core::mem::size_of::<KernelStat>();
    #[cfg(target_arch = "x86_64")]
    assert_eq!(n, 144, "x86_64 struct stat");
    #[cfg(target_arch = "aarch64")]
    assert_eq!(n, 128, "aarch64 struct stat");
    assert!(n >= core::mem::size_of::<Stat>(), "the kernel's buffer is smaller than the view this crate reads from it");
}

#[test]
fn a_char_device_reads_as_one_and_its_numbers_come_back() {
    // /dev/null is char 1:3 on every Linux, which is also DEV_1_3 above.
    let st = stat(&CBuf::new("/dev/null").unwrap()).expect("/dev/null");
    assert!(st.is_chardev(), "mode {:o}", st.st_mode);
    assert_eq!(st.kind(), "chardev");
    assert_eq!((st.rdev_major(), st.rdev_minor()), (1, 3));
    // The (device, inode) guard of an entry reads these two fields.
    // A zero here is a field the kernel never filled, and the guard
    // would compare nothing.
    assert_ne!(st.st_dev, 0, "the device /dev/null is on");
    assert_ne!(st.st_ino, 0, "the file /dev/null is");
}

#[test]
fn a_directory_is_not_mistaken_for_a_device() {
    let st = stat(&CBuf::new("/tmp").unwrap()).expect("/tmp");
    assert_eq!(st.kind(), "directory");
    assert!(!st.is_chardev());
}

#[test]
fn an_unknown_errno_names_its_number_rather_than_guessing_a_name() {
    assert_eq!(Errno(1).name(), "EPERM");
    assert_eq!(Errno(4093).name(), "E4093");
}

/// **[`shed_after_fork`]'s positive control, and it carries its own
/// negative one in the same test.**
///
/// A measurement found that the hook changes nothing about the race of the
/// store lock. **An absence is not a zero**: a hook that never ran gives
/// the same nothing, so the measurement is worth only as much as the proof
/// that the hook fires.
///
/// It asks the CHILD rather than the parent, because the parent cannot
/// see the child's descriptor table. The pipe is made without `O_CLOEXEC`,
/// so the write end survives the `execve` and only the hook can take it
/// away, and `/bin/sh` then says whether the number is still open in it.
/// The same spawn with no hook is run first, and it has to answer `open`;
/// without that leg an answer of `shed` could mean the fd never reached the
/// child at all.
#[test]
fn a_spawn_through_the_hook_sheds_a_registered_fd_and_one_without_it_does_not() {
    pod_testkit::need_tool!("/bin/sh");
    fn ask_the_child(w: i64, hook: bool) -> String {
        // The number is the parent's, and a `fork` copies the table, so
        // it is the child's number as well.
        let script = format!("if [ -e /proc/self/fd/{w} ]; then echo open; else echo shed; fi");
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.arg("-c").arg(script).stdout(std::process::Stdio::piped());
        if hook {
            shed_after_fork(&mut cmd);
        }
        let out = cmd.output().expect("/bin/sh");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    let mut fds = [0i32; 2];
    pipe2(&mut fds, 0).expect("pipe2");
    let (r, w) = (fds[0] as i64, fds[1] as i64);

    // The negative leg first, and it runs before anything is registered,
    // so it measures the descriptor and not the table.
    let without = ask_the_child(w, false);
    assert!(close_in_children(w), "no free slot to register the control's own fd in");
    let with = ask_the_child(w, true);
    stop_closing_in_children(w);
    let _ = close(w);
    let _ = close(r);

    assert_eq!(
        without, "open",
        "a spawn with no hook did not carry the fd into its child, so the \
             other leg proves nothing"
    );
    assert_eq!(
        with, "shed",
        "the hook is installed and the fd reached the child anyway, so \
             every measurement that rests on this hook rests on nothing"
    );
}
