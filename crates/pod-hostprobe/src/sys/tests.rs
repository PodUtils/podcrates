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

/// The four values of `statfs` are those of the kernel on each width: a
/// block size that is a power of two and a count of free inodes that is not
/// above the count of inodes. A 32-bit build that read the narrow struct
/// into the wide buffer broke both.
#[test]
fn statfs_reads_the_wide_struct_on_each_width() {
    let root = CBuf::new("/").expect("a path");
    let fs = statfs(&root).expect("statfs of /");
    let (bsize, files, ffree) = (fs.f_bsize, fs.f_files, fs.f_ffree);
    assert!(bsize > 0 && (bsize as u64).is_power_of_two(), "f_bsize {bsize}");
    assert!(ffree <= files, "f_ffree {ffree}, f_files {files}");
}

/// `f_flags` is the field of the kernel on each width: `/` has
/// `ST_VALID`, and each mount of this process has the `ro`, `nosuid`,
/// `nodev` and `noexec` of its line in `/proc/self/mounts` (`/proc` is
/// `nosuid,nodev,noexec` on most hosts). A buffer read at the wrong offset
/// gives no `ST_VALID` or wrong flags.
#[test]
fn statfs_reads_the_mount_flags() {
    let root = statfs(&CBuf::new("/").expect("a path")).expect("statfs of /");
    // qemu-user copies no f_flags into the guest's struct: zero there is
    // the emulator's answer, and Statfs::flag reads it as None. A kernel
    // always sets ST_VALID, so a native run keeps the full check.
    let set = |v: &str| std::env::var_os(v).is_some();
    if root.f_flags == 0 && crate::interp::detect(&set).is_emulated() {
        pod_testkit::host::skip_line("an emulator runs this test, and it gives no f_flags of statfs(2)");
        return;
    }
    assert_ne!(root.f_flags & ST_VALID, 0, "f_flags {:#x} of / has no ST_VALID", root.f_flags);
    assert!(root.f_flags < 0x1_0000, "f_flags {:#x} of / has bits past the ST_* set", root.f_flags);
    let Ok(mounts) = std::fs::read_to_string("/proc/self/mounts") else {
        eprintln!("skipping, /proc/self/mounts cannot be read on this host");
        return;
    };
    // The last line of a mount point is the mount on top, which statfs reads.
    let mut top: Vec<(String, String)> = Vec::new();
    for l in mounts.lines() {
        let f: Vec<&str> = l.split(' ').collect();
        if let (Some(point), Some(opts)) = (f.get(1), f.get(3))
            && !point.contains('\\')
        {
            top.retain(|(p, _)| p != point);
            top.push((point.to_string(), opts.to_string()));
        }
    }
    let words = [(ST_RDONLY, "ro"), (ST_NOSUID, "nosuid"), (ST_NODEV, "nodev"), (ST_NOEXEC, "noexec")];
    let mut seen = 0u64;
    for (point, opts) in &top {
        let Ok(fs) = statfs(&CBuf::new(point).expect("a path")) else {
            continue;
        };
        let has = |o: &str| opts.split(',').any(|x| x == o);
        for (flag, word) in words {
            assert_eq!(fs.flag(flag), Some(has(word)), "{point}: {word} in {opts:?}, f_flags {:#x}", fs.f_flags);
            if has(word) {
                seen |= flag;
            }
        }
    }
    for (flag, word) in words {
        if seen & flag == 0 {
            eprintln!("skipping the check of {word}: no mount of this host has it");
        }
    }
}

/// The id calls are the 32-bit form on each target. On x86 and arm the plain
/// name is the 16-bit call, and an id above 65535 reads wrong through it.
#[test]
fn the_id_calls_are_the_32_bit_form_on_each_target() {
    let n = [SYS_GETUID, SYS_GETGID, SYS_SETUID, SYS_SETGID, SYS_GETGROUPS, SYS_SETGROUPS];
    // kernel numbers: i386 and arm EABI share these.
    #[cfg(any(target_arch = "x86", target_arch = "arm"))]
    assert_eq!(n, [199, 200, 213, 214, 205, 206], "32-bit id calls");
    #[cfg(target_arch = "x86_64")]
    assert_eq!(n, [102, 104, 105, 106, 115, 116], "x86_64 id calls");
    #[cfg(target_arch = "aarch64")]
    assert_eq!(n, [174, 176, 146, 144, 158, 159], "aarch64 id calls");
    #[cfg(any(target_arch = "x86", target_arch = "arm"))]
    assert_eq!([CHOWN.nr, LCHOWN.nr], [212, 198], "32-bit chown calls");
    #[cfg(any(target_arch = "x86", target_arch = "arm"))]
    assert_eq!([CHOWN.name, LCHOWN.name], ["chown32", "lchown32"]);
    assert!(n.iter().all(|&x| x > 0));
}

fn status_ids(key: &str) -> Vec<u32> {
    let s = std::fs::read_to_string("/proc/self/status").expect("status");
    let l = s.lines().find(|l| l.starts_with(key)).expect("line");
    l[key.len()..].split_whitespace().map(|v| v.parse().expect("id")).collect()
}

#[test]
fn the_ids_match_proc_self_status() {
    let uid = unsafe { syscall6(SYS_GETUID, 0, 0, 0, 0, 0, 0) };
    let gid = unsafe { syscall6(SYS_GETGID, 0, 0, 0, 0, 0, 0) };
    assert_eq!(uid as u32, status_ids("Uid:")[0]);
    assert_eq!(gid as u32, status_ids("Gid:")[0]);
    let mut got: Vec<u32> = getgroups().expect("getgroups").into_iter().map(|g| g as u32).collect();
    let mut want = status_ids("Groups:");
    got.sort_unstable();
    want.sort_unstable();
    assert_eq!(got, want);
}

/// A child with an id above 65535 reads it whole. Needs root; skipped
/// otherwise.
#[test]
fn an_id_above_65535_reads_whole() {
    use std::os::unix::process::CommandExt;
    if unsafe { syscall6(SYS_GETUID, 0, 0, 0, 0, 0, 0) } != 0 {
        return;
    }
    const HIGH: u32 = 100_000;
    let mut cmd = std::process::Command::new("true");
    unsafe {
        cmd.pre_exec(|| {
            let list = [HIGH];
            let set = syscall6(SYS_SETGROUPS, 1, list.as_ptr() as u64, 0, 0, 0, 0)
                | syscall6(SYS_SETGID, HIGH as u64, 0, 0, 0, 0, 0)
                | syscall6(SYS_SETUID, HIGH as u64, 0, 0, 0, 0, 0);
            if set != 0 {
                return Err(std::io::Error::from_raw_os_error(4243));
            }
            let uid = syscall6(SYS_GETUID, 0, 0, 0, 0, 0, 0);
            let gid = syscall6(SYS_GETGID, 0, 0, 0, 0, 0, 0);
            let mut buf = [0u32; 4];
            let n = syscall6(SYS_GETGROUPS, 4, buf.as_mut_ptr() as u64, 0, 0, 0, 0);
            if uid == HIGH as i64 && gid == HIGH as i64 && n == 1 && buf[0] == HIGH {
                Ok(())
            } else {
                Err(std::io::Error::from_raw_os_error(4242))
            }
        });
    }
    if let Err(e) = cmd.spawn().map(|mut c| c.wait()) {
        assert_ne!(e.raw_os_error(), Some(4242), "an id above 65535 read wrong");
        assert_ne!(e.raw_os_error(), Some(4243), "an id above 65535 was not set");
    }
}

/// The open flags are numbers of the kernel that differ by architecture. A
/// wrong value opens a directory as a plain file or follows a link, so the
/// test makes the kernel say what each flag does.
#[test]
fn the_open_flags_mean_what_the_kernel_means_on_each_target() {
    let dir = std::env::temp_dir().join(format!("pod-hostprobe-flags-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("d")).unwrap();
    std::fs::write(dir.join("f"), b"x").unwrap();
    std::os::unix::fs::symlink(dir.join("d"), dir.join("l")).unwrap();
    let path = |n: &str| CBuf::new(dir.join(n).to_str().unwrap()).unwrap();

    let fd = open(&path("d"), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0).expect("a directory opens");
    let _ = close(fd as i64);
    assert_eq!(open(&path("f"), O_RDONLY | O_DIRECTORY, 0).unwrap_err(), Errno(20), "O_DIRECTORY refuses a file");
    assert_eq!(open(&path("l"), O_RDONLY | O_NOFOLLOW, 0).unwrap_err(), Errno(40), "O_NOFOLLOW refuses a link");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `O_NONBLOCK` is a number of the kernel that differs by architecture. With
/// a wrong value, the open of a fifo with no writer waits for ever, so the
/// test opens one in a thread and waits for the answer with a time limit.
#[test]
fn an_open_with_o_nonblock_does_not_wait_for_a_fifo_writer() {
    let dir = std::env::temp_dir().join(format!("pod-hostprobe-nonblock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fifo = CBuf::new(dir.join("fifo").to_str().unwrap()).unwrap();
    mknodat(AT_FDCWD as i64, &fifo, S_IFIFO | 0o600, 0).expect("an unprivileged process makes a fifo");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = open(&fifo, O_RDONLY | O_NONBLOCK | O_CLOEXEC, 0);
        if let Ok(fd) = r {
            let _ = close(fd);
        }
        let _ = tx.send(r.is_ok());
    });
    let opened = rx.recv_timeout(pod_testkit::host::scaled(std::time::Duration::from_secs(10)));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(opened, Ok(true), "the open of a fifo with O_NONBLOCK waited or failed");
}
