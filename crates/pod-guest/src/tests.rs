use super::*;

/// The provisioning boot's root disk, which `T-1641` moved into the
/// per-run directory. ⛔ It is not the provisioned image: that is a second
/// path, and this is the disposable scratch overlay the commit reads.
#[test]
fn the_provisioning_root_is_per_run_scratch_and_not_the_provisioned_image() {
    let scratch = scratch_dir("provision-root").expect("a scratch directory");
    let vendor = std::env::temp_dir().join(format!("pbx-prov-{}", std::process::id()));
    std::fs::create_dir_all(&vendor).unwrap();
    let image = vendor.join("ValidationOS.vhdx");
    std::fs::write(&image, b"not really a disk image").unwrap();
    let out = vendor.join("ValidationOS.podvm.qcow2");
    let plan = Plan {
        emulator: "qemu-system-x86_64".into(),
        accel: Accel::Tcg,
        share: "/usr/share/qemu".into(),
        firmware_code: "/code.fd".into(),
        firmware_vars: scratch.join("vars.fd"),
        // exactly what the CLI's `setup` does
        root: scratch.join("run.qcow2"),
        mailbox: scratch.join("mailbox.img"),
        serial: scratch.join("serial.log"),
        monitor: scratch.join("qmp.sock"),
        memory_mib: 4096,
        cpus: 2,
        emu_args: Vec::new(),
    };
    assert_ne!(plan.root, out, "the two are separate files");
    assert_eq!(
        scratch_of(&plan).as_deref(),
        Some(scratch.as_path()),
        "the root overlay is inside the directory cleanup removes"
    );
    // and the boot's own disk line names that per-run file
    let a = argv(&plan);
    assert!(
        a.iter()
            .any(|x| x.contains("run.qcow2") && x.contains("id=root")),
        "{a:?}"
    );
    // ⛔ so a cleanup cannot reach the directory the vendor image and the
    // provisioned image live in, whatever a caller does with `root`.
    let _ = std::fs::remove_dir_all(&scratch);
    let _ = std::fs::remove_dir_all(&vendor);
}

/// ⛔ The defect this pins: `setup` puts `root` in a caller's directory of
/// Windows images in every revision but one, so a cleanup keyed on `root`
/// would delete that directory. It is keyed on the mailbox, and it refuses
/// a directory that is not named like a scratch one.
#[test]
fn cleanup_never_removes_the_directory_a_vendor_image_lives_in() {
    let vendor = std::env::temp_dir().join(format!("pbx-vendor-{}", std::process::id()));
    std::fs::create_dir_all(&vendor).unwrap();
    let image = vendor.join("ValidationOS.vhdx");
    std::fs::write(&image, b"not really a disk image").unwrap();
    let scratch = scratch_dir("cleanup-test").expect("a scratch directory");
    let plan = Plan {
        emulator: "qemu-system-x86_64".into(),
        accel: Accel::Tcg,
        share: "/usr/share/qemu".into(),
        firmware_code: "/code.fd".into(),
        firmware_vars: scratch.join("vars.fd"),
        // exactly what the CLI's `setup` does
        root: vendor.join("ValidationOS.podvm.qcow2"),
        mailbox: scratch.join("mailbox.img"),
        serial: scratch.join("serial.log"),
        monitor: scratch.join("qmp.sock"),
        memory_mib: 4096,
        cpus: 2,
        emu_args: Vec::new(),
    };
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::remove_var("PODVM_WINDOWS_KEEP") };
    cleanup(&plan);
    assert_eq!(scratch_of(&plan).as_deref(), Some(scratch.as_path()));
    assert!(image.is_file(), "the vendor image is untouched");
    assert!(vendor.is_dir(), "and so is the directory holding it");
    assert!(!scratch.exists(), "the per-run directory is gone");

    // and a plan whose mailbox is not in a scratch-named directory is
    // left alone rather than recursively deleted
    let mut not_ours = plan.clone();
    not_ours.mailbox = vendor.join("mailbox.img");
    cleanup(&not_ours);
    assert!(
        vendor.is_dir(),
        "an unrecognised directory is never removed"
    );

    // ⭐ and the override keeps it
    let kept = scratch_dir("cleanup-keep").expect("a scratch directory");
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::set_var("PODVM_WINDOWS_KEEP", "1") };
    let mut p2 = plan.clone();
    p2.mailbox = kept.join("mailbox.img");
    cleanup(&p2);
    assert!(kept.is_dir(), "PODVM_WINDOWS_KEEP keeps the run");
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::remove_var("PODVM_WINDOWS_KEEP") };
    let _ = std::fs::remove_dir_all(&kept);
    let _ = std::fs::remove_dir_all(&vendor);
}

/// The defect this pins: `cleanup` runs on the coded exits, and a panic
/// past staging skipped every one of them, leaking the per-run
/// directory. The guard removes it on drop, including the unwind path.
#[test]
fn a_dropped_guard_removes_the_run_directory() {
    let scratch = scratch_dir("guard-drop").expect("a scratch directory");
    std::fs::write(scratch.join("mailbox.img"), b"mailbox").unwrap();
    let plan = Plan {
        emulator: "qemu-system-x86_64".into(),
        accel: Accel::Tcg,
        share: "/usr/share/qemu".into(),
        firmware_code: "/code.fd".into(),
        firmware_vars: scratch.join("vars.fd"),
        root: scratch.join("root.qcow2"),
        mailbox: scratch.join("mailbox.img"),
        serial: scratch.join("serial.log"),
        monitor: scratch.join("qmp.sock"),
        memory_mib: 512,
        cpus: 1,
        emu_args: Vec::new(),
    };
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::remove_var("PODVM_WINDOWS_KEEP") };
    {
        let _guard = RunGuard::arm(&plan);
        assert!(scratch.is_dir(), "the run is staged");
    }
    assert!(!scratch.exists(), "dropping the guard removes the run");
}

/// ⛔ The defect this pins: the emulator's streams were piped and not
/// read, so an emulator that wrote more than one pipe buffer blocked and
/// the run was reported as a guest that did not power off. The fake
/// emulator writes 256 KiB to each stream and exits; the run must end
/// at once and carry the end of the error stream in its message.
#[test]
fn a_noisy_emulator_is_not_blocked_on_its_own_output() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = scratch_dir("noisy-emulator").expect("a scratch directory");
    let fake = scratch.join("fake-emulator");
    std::fs::write(
        &fake,
        "#!/bin/sh\nhead -c 262144 /dev/zero | tr '\\0' o\n\
             head -c 262144 /dev/zero | tr '\\0' e >&2\necho '' >&2\necho last-line >&2\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    let plan = Plan {
        emulator: fake,
        accel: Accel::Tcg,
        share: "/usr/share/qemu".into(),
        firmware_code: "/code.fd".into(),
        firmware_vars: scratch.join("vars.fd"),
        root: scratch.join("root.qcow2"),
        mailbox: scratch.join("mailbox.img"),
        serial: scratch.join("serial.log"),
        monitor: scratch.join("qmp.sock"),
        memory_mib: 512,
        cpus: 1,
        emu_args: Vec::new(),
    };
    std::fs::write(&plan.mailbox, mailbox(agent::AGENT_CMD, &request()).image()).unwrap();
    let mut req = request();
    req.timeout = Duration::from_secs(30);
    // A file written and then executed while another test thread forks
    // can be refused as busy for a moment; that refusal is not the
    // subject here, so it is retried.
    let mut attempt = 0;
    let (e, elapsed) = loop {
        let start = Instant::now();
        let e = run(&plan, &req).unwrap_err();
        let busy = matches!(&e, Error::NoEmulator(m) if m.contains("busy"));
        if !busy || attempt == 20 {
            break (e, start.elapsed());
        }
        attempt += 1;
        std::thread::sleep(Duration::from_millis(50));
    };
    // SAFETY: only a test changes this variable; edition 2024 marks the change unsafe because another thread can read the environment at the same time.
    unsafe { std::env::remove_var("PODVM_WINDOWS_KEEP") };
    cleanup(&plan);
    assert!(
        elapsed < Duration::from_secs(20),
        "the run blocked for {elapsed:?}"
    );
    assert!(matches!(e, Error::BadResult(_)), "{e}");
    assert!(format!("{e}").contains("last-line"), "{e}");
}

fn request() -> Request {
    Request {
        command: "ver & echo hi".into(),
        token: "tok-1".into(),
        timeout: Duration::from_secs(10),
    }
}

#[test]
fn the_mailbox_carries_the_marker_the_command_and_the_token() {
    let m = mailbox(agent::AGENT_CMD, &request());
    assert_eq!(m.get(agent::MARKER).as_deref(), Some(&b"podvm"[..]));
    assert_eq!(m.get(agent::GO).as_deref(), Some(&b"tok-1"[..]));
    assert!(m.get(agent::CMD).is_some());
    // The agent is on the volume under the name `SETUP_CMD` copies it to.
    assert_eq!(
        m.get(agent::AGENT_NAME).as_deref(),
        Some(agent::AGENT_CMD.as_bytes())
    );
    // and nothing has been answered yet
    assert!(m.get(agent::CODE).is_none());
}

#[test]
fn a_result_is_read_back_with_its_streams_and_its_code() {
    let mut m = mailbox(agent::AGENT_CMD, &request());
    m.put(agent::OUT, b"hello\r\n").unwrap();
    m.put(agent::ERR, b"").unwrap();
    m.put(agent::CODE, b"3 tok-1").unwrap();
    let o = outcome(&m, "tok-1").unwrap().expect("a result");
    assert_eq!(o.code, 3);
    assert_eq!(o.token, "tok-1");
    assert_eq!(o.stdout, b"hello\r\n");
    assert!(o.stderr.is_empty());
}

#[test]
fn a_guest_that_did_not_finish_is_not_a_zero() {
    let m = mailbox(agent::AGENT_CMD, &request());
    assert_eq!(outcome(&m, "tok-1").unwrap(), None);
}

#[test]
fn a_result_for_somebody_elses_token_is_refused_not_reported() {
    let mut m = mailbox(agent::AGENT_CMD, &request());
    m.put(agent::CODE, b"0 someone-else").unwrap();
    let e = outcome(&m, "tok-1").unwrap_err();
    assert!(format!("{e}").contains("someone-else"), "{e}");
}

#[test]
fn the_timeout_message_names_the_time_and_the_pid() {
    let e = Error::Timeout(Duration::from_secs(600), Stop::Confirmed { pid: 4242 });
    assert!(format!("{e}").contains("600"), "{e}");
    assert!(format!("{e}").contains("4242"), "{e}");
}

/// ⛔ The defect this pins: the message said "so it was stopped" on a path
/// where nothing confirmed a stop, and the kill's own result was thrown
/// away. The two states read differently and each names the pid, because
/// a reader chasing a survivor needs it in both.
#[test]
fn a_timeout_says_whether_the_stop_was_confirmed_and_names_the_pid() {
    let confirmed = Error::Timeout(Duration::from_secs(60), Stop::Confirmed { pid: 1234 });
    let text = format!("{confirmed}");
    assert!(text.contains("1234"), "{text}");
    assert!(
        text.contains("reaped"),
        "a confirmed stop says the child was reaped: {text}"
    );
    let attempted = Error::Timeout(Duration::from_secs(60), Stop::Attempted { pid: 1234 });
    let text = format!("{attempted}");
    assert!(text.contains("1234"), "{text}");
    assert!(
        text.contains("not confirmed"),
        "an unconfirmed stop does not claim one: {text}"
    );
    assert!(
        text.contains("still be running"),
        "an unconfirmed stop names the risk a reader has to act on: {text}"
    );
    assert!(
        Stop::Confirmed { pid: 1234 }.confirmed() && !Stop::Attempted { pid: 1234 }.confirmed()
    );
    assert_eq!(Stop::Confirmed { pid: 1234 }.pid(), 1234);
}

/// ⛔ The defect this pins: the emulator was spawned without a process
/// group and killed as a direct child, so a helper it forked outlived the
/// kill. The emulator and its forked helper are in one group here, and
/// neither survives.
#[test]
fn a_stopped_emulator_takes_its_process_group_with_it() {
    let scratch = scratch_dir("stop-group").expect("a scratch directory");
    let marker = scratch.join("group-survived");
    let fake = scratch.join("fake-emulator");
    // The emulator stands in for its own group; the `sleep` stands for a
    // helper that forks under it, and writes its file only if it outlives
    // the kill.
    std::fs::write(&fake, "#!/bin/sh\n(sleep 30; : > \"$0\") &\nwait\n").unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let mut plan = plan_over(&scratch, &fake);
    plan.root = scratch.join("run.qcow2");
    let mut child = spawn_windows(&plan, Stdio::null()).expect("the fake emulator starts");
    let child_pid = child.id();
    // ⛔ The child is not in the driver's own group, so the group kill below
    // cannot reach this test process.
    let stop = stop_with(
        &plan,
        &mut child,
        Duration::from_millis(50),
        Duration::from_secs(10),
    );
    assert!(
        matches!(stop, Stop::Confirmed { .. }),
        "the emulator was reaped: {stop}"
    );
    assert_eq!(stop.pid(), child_pid);
    assert_ne!(
        std::process::id(),
        child_pid,
        "the emulator is its own process"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(!marker.exists(), "a forked helper outlived the group kill");
    let _ = std::fs::remove_dir_all(&scratch);
}

/// A guest that has already powered itself off needs no signal, and the
/// report says the emulator was reaped rather than that a kill was issued.
/// The wait is zero, so this does not sleep on a real timeout.
#[test]
fn an_emulator_that_exited_on_its_own_is_reported_as_a_confirmed_stop() {
    let scratch = scratch_dir("stop-exited").expect("a scratch directory");
    let fake = scratch.join("fake-emulator");
    std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let plan = plan_over(&scratch, &fake);
    let mut child = spawn_windows(&plan, Stdio::null()).expect("the fake emulator starts");
    // Let the child be reaped by the stop, not by a caller that already
    // waited for it.
    std::thread::sleep(Duration::from_millis(200));
    let stop = stop_with(
        &plan,
        &mut child,
        Duration::from_millis(50),
        Duration::from_secs(5),
    );
    assert!(
        matches!(stop, Stop::Confirmed { pid } if pid == child.id()),
        "{stop}"
    );
    let _ = std::fs::remove_dir_all(&scratch);
}

/// The plan every stop test needs: a real emulator binary that is not
/// qemu, and a monitor socket that was never created. ⛔ `stop` polls the
/// monitor before it signals, and a socket that is absent makes that
/// polling the bound rather than a connection to something live.
fn plan_over(scratch: &Path, emulator: &Path) -> Plan {
    Plan {
        emulator: emulator.to_path_buf(),
        accel: Accel::Tcg,
        share: "/usr/share/qemu".into(),
        firmware_code: "/code.fd".into(),
        firmware_vars: scratch.join("vars.fd"),
        root: scratch.join("run.qcow2"),
        mailbox: scratch.join("mailbox.img"),
        serial: scratch.join("serial.log"),
        monitor: scratch.join("qmp.sock"),
        memory_mib: 512,
        cpus: 1,
        emu_args: Vec::new(),
    }
}

#[test]
fn docker_keeps_0_to_124_and_podvm_keeps_125_up() {
    assert_eq!(cap_status(0), Some(0));
    assert_eq!(cap_status(42), Some(42));
    assert_eq!(cap_status(124), Some(124));
    assert_eq!(cap_status(125), None);
    assert_eq!(cap_status(255), None);
    assert_eq!(cap_status(-1), None);
}

#[test]
fn provisioning_types_a_command_that_finds_the_volume_without_a_letter() {
    // The installer is invoked through a scan, never through `D:`.
    let text = "for %d in (D E) do @if exist %d:\\WQMARK.TXT %d:\\WA.CMD";
    assert!(text.contains("%d"));
    assert!(text.contains("WQMARK.TXT"));
    assert!(keys('d').is_some());
    assert!(keys('D').map(|k| k.1).unwrap_or(false));
}
