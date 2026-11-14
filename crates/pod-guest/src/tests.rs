use super::*;

/// The settings of the tests: per-run directories in the temporary
/// directory, with the default prefix.
fn settings() -> Settings {
    Settings::new(std::env::temp_dir())
}

/// The plan of a stop test: an emulator that is not qemu, and a monitor
/// socket that does not exist. `stop` polls the monitor before the signal,
/// and an absent socket makes the poll end at once.
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

fn request() -> Request {
    Request {
        command: "ver & echo hi".into(),
        token: "tok-1".into(),
        timeout: Duration::from_secs(10),
        mode: agent::Mode::Once,
    }
}

/// True when `pid` is gone or a zombie that no process reaped yet.
fn gone(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat.rsplit_once(')').map(|(_, rest)| rest.trim_start().starts_with('Z')).unwrap_or(false),
    }
}

/// The root disk of the provision boot is in the per-run directory. It is
/// not the provisioned image: that is a second path, and this is the
/// overlay that the commit reads.
#[test]
fn the_provisioning_root_is_per_run_scratch_and_not_the_provisioned_image() {
    let s = settings();
    let scratch = scratch_dir(&s, "provision-root").expect("a scratch directory");
    let vendor = std::env::temp_dir().join(format!("guest-test-prov-{}", std::process::id()));
    std::fs::create_dir_all(&vendor).unwrap();
    let image = vendor.join("ValidationOS.vhdx");
    std::fs::write(&image, b"not really a disk image").unwrap();
    let out = vendor.join("ValidationOS.guest.qcow2");
    let mut plan = plan_over(&scratch, Path::new("qemu-system-x86_64"));
    plan.memory_mib = 4096;
    plan.cpus = 2;
    assert_ne!(plan.root, out, "the two are separate files");
    assert_eq!(
        scratch_of(&plan).as_deref(),
        Some(scratch.as_path()),
        "the root overlay is inside the directory cleanup removes"
    );
    // The disk line of the boot names that file of the run.
    let a = argv(&plan);
    assert!(a.iter().any(|x| x.contains("run.qcow2") && x.contains("id=root")), "{a:?}");
    let _ = std::fs::remove_dir_all(&scratch);
    let _ = std::fs::remove_dir_all(&vendor);
}

/// A caller can put `root` in its own directory of images; a cleanup keyed
/// on `root` removes that directory. The cleanup is keyed on the mailbox,
/// and it refuses a directory whose name does not have the prefix.
#[test]
fn cleanup_never_removes_the_directory_a_vendor_image_lives_in() {
    let mut s = settings();
    let vendor = std::env::temp_dir().join(format!("guest-test-vendor-{}", std::process::id()));
    std::fs::create_dir_all(&vendor).unwrap();
    let image = vendor.join("ValidationOS.vhdx");
    std::fs::write(&image, b"not really a disk image").unwrap();
    let scratch = scratch_dir(&s, "cleanup-test").expect("a scratch directory");
    let mut plan = plan_over(&scratch, Path::new("qemu-system-x86_64"));
    plan.root = vendor.join("ValidationOS.guest.qcow2");
    cleanup(&s, &plan);
    assert_eq!(scratch_of(&plan).as_deref(), Some(scratch.as_path()));
    assert!(image.is_file(), "the vendor image is untouched");
    assert!(vendor.is_dir(), "and so is the directory holding it");
    assert!(!scratch.exists(), "the per-run directory is gone");

    // A plan whose mailbox is not in a directory with the prefix stays.
    // The name of `vendor` does not start with the prefix.
    let mut not_ours = plan.clone();
    not_ours.mailbox = vendor.join("mailbox.img");
    cleanup(&s, &not_ours);
    assert!(vendor.is_dir(), "an unrecognised directory is never removed");

    // `keep` keeps it.
    let kept = scratch_dir(&s, "cleanup-keep").expect("a scratch directory");
    s.keep = true;
    let mut p2 = plan.clone();
    p2.mailbox = kept.join("mailbox.img");
    cleanup(&s, &p2);
    assert!(kept.is_dir(), "keep keeps the run");
    let _ = std::fs::remove_dir_all(&kept);
    let _ = std::fs::remove_dir_all(&vendor);
}

/// `cleanup` runs on the coded exits, and a panic after the stage skips
/// each of them. The guard removes the directory when it is dropped, also
/// on the unwind path.
#[test]
fn a_dropped_guard_removes_the_run_directory() {
    let s = settings();
    let scratch = scratch_dir(&s, "guard-drop").expect("a scratch directory");
    std::fs::write(scratch.join("mailbox.img"), b"mailbox").unwrap();
    let plan = plan_over(&scratch, Path::new("qemu-system-x86_64"));
    {
        let _guard = RunGuard::arm(&s, &plan);
        assert!(scratch.is_dir(), "the run is staged");
    }
    assert!(!scratch.exists(), "dropping the guard removes the run");
}

/// The per-run directory has the prefix, the pid and the tag in its name,
/// and the mode 0700: a process that connects to the QMP socket can type
/// into the guest.
#[test]
fn a_scratch_directory_is_named_for_the_run_and_closed_to_others() {
    use std::os::unix::fs::PermissionsExt;
    let mut s = settings();
    s.scratch_prefix = "pod-guest-mode-".into();
    let dir = scratch_dir(&s, "mode").unwrap();
    let name = dir.file_name().unwrap().to_string_lossy().to_string();
    assert_eq!(name, format!("pod-guest-mode-{}-mode", std::process::id()));
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    // A second call gives the same directory.
    assert_eq!(scratch_dir(&s, "mode").unwrap(), dir);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The streams of the emulator were pipes that nobody read, so an emulator
/// that wrote more than one pipe buffer blocked, and the run read as a
/// guest that did not power off. The fake emulator writes 256 KiB to each
/// stream and exits; the run must end at once, with the end of the error
/// stream in its text.
#[test]
fn a_noisy_emulator_is_not_blocked_on_its_own_output() {
    use std::os::unix::fs::PermissionsExt;
    pod_testkit::need_tool!("/bin/sh");
    pod_testkit::need_tool!("head");
    pod_testkit::need_tool!("tr");
    let s = settings();
    let scratch = scratch_dir(&s, "noisy-emulator").expect("a scratch directory");
    let fake = scratch.join("fake-emulator");
    std::fs::write(
        &fake,
        "#!/bin/sh\nhead -c 262144 /dev/zero | tr '\\0' o\n\
             head -c 262144 /dev/zero | tr '\\0' e >&2\necho '' >&2\necho last-line >&2\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    let plan = plan_over(&scratch, &fake);
    std::fs::write(&plan.mailbox, mailbox(&s, agent::AGENT_CMD, &request()).unwrap().image()).unwrap();
    let mut req = request();
    req.timeout = Duration::from_secs(30);
    // A file that was written and then runs while another test thread
    // forks can be refused as busy (ETXTBSY) for a short time. That is not
    // the subject here, so the test tries again.
    let mut attempt = 0;
    let (e, elapsed) = loop {
        let start = Instant::now();
        let e = run(&s, &plan, &req).unwrap_err();
        let busy = e.io_cause().map(|io| io.kind()) == Some(std::io::ErrorKind::ExecutableFileBusy);
        if !busy || attempt == 20 {
            break (e, start.elapsed());
        }
        attempt += 1;
        std::thread::sleep(Duration::from_millis(50));
    };
    cleanup(&s, &plan);
    assert!(elapsed < pod_testkit::host::scaled(Duration::from_secs(20)), "the run blocked for {elapsed:?}");
    assert_eq!(e.class, Class::Protocol, "{e}");
    assert!(format!("{e}").contains("last-line"), "{e}");
}

#[test]
fn the_mailbox_carries_the_marker_the_command_and_the_token() {
    let mut s = settings();
    s.marker = b"podvm".to_vec();
    s.oem = "PODVM".into();
    let m = mailbox(&s, agent::AGENT_CMD, &request()).unwrap();
    assert_eq!(m.get(agent::MARKER).as_deref(), Some(&b"podvm"[..]));
    assert_eq!(m.get(agent::GO).as_deref(), Some(&b"tok-1"[..]));
    assert!(m.get(agent::CMD).is_some());
    // The agent is on the volume under the name that `SETUP_CMD` copies.
    assert_eq!(m.get(agent::AGENT_NAME).as_deref(), Some(agent::AGENT_CMD.as_bytes()));
    // No result yet.
    assert!(m.get(agent::CODE).is_none());
    let b = fat16::PART_LBA * fat16::SECTOR;
    assert_eq!(&m.image()[b + 3..b + 11], b"PODVM   ", "the OEM name of the settings");
}

/// The keep-alive mode is a file on the mailbox: the installed agent is
/// the same for each mode.
#[test]
fn the_keep_alive_mode_is_a_file_on_the_mailbox_and_a_run_refuses_it() {
    assert!(mailbox(&settings(), agent::AGENT_CMD, &request()).unwrap().get(agent::KEEP).is_none());
    let mut keep = request();
    keep.mode = agent::Mode::KeepAlive;
    assert_eq!(mailbox(&settings(), agent::AGENT_CMD, &keep).unwrap().get(agent::KEEP), Some(Vec::new()));
    // The guest of this mode does not power off: a run would end at its
    // limit, so it is refused before the boot.
    let dir = std::env::temp_dir().join(format!("pod-guest-keep-{}", std::process::id()));
    let e = run(&settings(), &plan_over(&dir, Path::new("/nonexistent/qemu")), &keep).unwrap_err();
    assert_eq!(e.class, Class::Usage, "{e}");
    assert!(e.to_string().contains("keep-alive"), "{e}");
}

#[test]
fn a_result_is_read_back_with_its_streams_and_its_code() {
    let mut m = mailbox(&settings(), agent::AGENT_CMD, &request()).unwrap();
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
    let m = mailbox(&settings(), agent::AGENT_CMD, &request()).unwrap();
    assert_eq!(outcome(&m, "tok-1").unwrap(), None);
}

#[test]
fn a_result_for_somebody_elses_token_is_refused_not_reported() {
    let mut m = mailbox(&settings(), agent::AGENT_CMD, &request()).unwrap();
    m.put(agent::CODE, b"0 someone-else").unwrap();
    let e = outcome(&m, "tok-1").unwrap_err();
    assert!(format!("{e}").contains("someone-else"), "{e}");
    assert_eq!(e.class, Class::Protocol);
}

#[test]
fn the_timeout_message_names_the_time_and_the_pid() {
    let e = Timeout { after: Duration::from_secs(600), stop: Stop::Confirmed { pid: 4242 } }.failure();
    assert!(format!("{e}").contains("600"), "{e}");
    assert!(format!("{e}").contains("4242"), "{e}");
    assert_eq!(e.class, Class::Timeout);
    let t = timeout_of(&e).expect("the cause is the timeout");
    assert_eq!(t.stop, Stop::Confirmed { pid: 4242 });
    assert!(timeout_of(&failure(Class::Timeout, "another wait")).is_none());
}

/// The text said "so it was stopped" where nothing confirmed a stop. The
/// two states read differently and each names the pid: a reader who looks
/// for a survivor needs it in both.
#[test]
fn a_timeout_says_whether_the_stop_was_confirmed_and_names_the_pid() {
    let confirmed = Timeout { after: Duration::from_secs(60), stop: Stop::Confirmed { pid: 1234 } }.failure();
    let text = format!("{confirmed}");
    assert!(text.contains("1234"), "{text}");
    assert!(text.contains("reaped"), "a confirmed stop says the child was reaped: {text}");
    let attempted = Timeout { after: Duration::from_secs(60), stop: Stop::Attempted { pid: 1234 } }.failure();
    let text = format!("{attempted}");
    assert!(text.contains("1234"), "{text}");
    assert!(text.contains("not confirmed"), "an unconfirmed stop does not claim one: {text}");
    assert!(text.contains("still be running"), "an unconfirmed stop names the risk a reader has to act on: {text}");
    assert!(Stop::Confirmed { pid: 1234 }.confirmed() && !Stop::Attempted { pid: 1234 }.confirmed());
    assert_eq!(Stop::Confirmed { pid: 1234 }.pid(), 1234);
}

/// The emulator was started with no process group of its own and killed as
/// a direct child, so a helper that it forked lived after the kill. Here
/// the emulator and its helper are in one group, and neither lives.
#[test]
fn a_stopped_emulator_takes_its_process_group_with_it() {
    pod_testkit::need_tool!("/bin/sh");
    pod_testkit::need_tool!("sleep");
    let s = settings();
    let scratch = scratch_dir(&s, "stop-group").expect("a scratch directory");
    let pidfile = scratch.join("helper.pid");
    let script = scratch.join("fake-emulator");
    // The shell stands for the emulator; the `sleep` stands for a helper
    // that it forks. The shell reads the script, so no file is executed
    // that a test thread just wrote (ETXTBSY).
    std::fs::write(&script, format!("sleep 30 &\necho $! > {}\nwait\n", pidfile.display())).unwrap();
    let plan = plan_over(&scratch, Path::new("/bin/sh"));
    let mut child = emulator::spawn_emulator(&plan, &[script.display().to_string()], Stdio::null())
        .expect("the fake emulator starts");
    let child_pid = child.id();
    let start = Instant::now();
    let helper = loop {
        if let Some(pid) = std::fs::read_to_string(&pidfile).ok().and_then(|t| t.trim().parse::<u32>().ok()) {
            break pid;
        }
        assert!(start.elapsed() < pod_testkit::host::scaled(Duration::from_secs(10)), "the helper did not start");
        std::thread::sleep(Duration::from_millis(20));
    };
    // The child is not in the group of the test, so the group kill cannot
    // reach this process.
    let stop = emulator::stop_with(&plan, &mut child, Duration::from_millis(50), Duration::from_secs(10));
    assert!(matches!(stop, Stop::Confirmed { .. }), "the emulator was reaped: {stop}");
    assert_eq!(stop.pid(), child_pid);
    assert_ne!(std::process::id(), child_pid, "the emulator is its own process");
    let start = Instant::now();
    while !gone(helper) {
        assert!(
            start.elapsed() < pod_testkit::host::scaled(Duration::from_secs(5)),
            "a forked helper outlived the group kill"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = std::fs::remove_dir_all(&scratch);
}

/// A guest that powered itself off needs no signal, and the report says
/// that the emulator was reaped.
#[test]
fn an_emulator_that_exited_on_its_own_is_reported_as_a_confirmed_stop() {
    pod_testkit::need_tool!("/bin/sh");
    let s = settings();
    let scratch = scratch_dir(&s, "stop-exited").expect("a scratch directory");
    let plan = plan_over(&scratch, Path::new("/bin/sh"));
    let mut child = emulator::spawn_emulator(&plan, &["-c".into(), "exit 0".into()], Stdio::null())
        .expect("the fake emulator starts");
    // The stop reaps the child, not a caller that waited for it before.
    std::thread::sleep(Duration::from_millis(200));
    let stop = emulator::stop_with(&plan, &mut child, Duration::from_millis(50), Duration::from_secs(5));
    assert!(matches!(stop, Stop::Confirmed { pid } if pid == child.id()), "{stop}");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn provisioning_types_a_command_that_finds_the_volume_without_a_letter() {
    // The installer runs through a scan, never through `D:`.
    let text = "for %d in (D E) do @if exist %d:\\WQMARK.TXT %d:\\WA.CMD";
    assert!(text.contains("%d"));
    assert!(text.contains("WQMARK.TXT"));
    assert!(text.chars().all(|c| qmp::key(c).is_some()));
    assert!(qmp::key('d').is_some());
    assert!(qmp::key('D').map(|k| k.1).unwrap_or(false));
}

/// A real emulator with no machine (`-M none`) under `accel`: the QMP
/// handshake and a request work, and the stop leaves no emulator. Each
/// wait has a bound.
fn real_emulator_answers_qmp(accel: &str, tag: &str) {
    let s = settings();
    let scratch = scratch_dir(&s, tag).expect("a scratch directory");
    let plan = plan_over(&scratch, Path::new("qemu-system-x86_64"));
    let args: Vec<String> = ["-M", "none", "-accel", accel, "-nodefaults", "-display", "none", "-qmp"]
        .iter()
        .map(|a| a.to_string())
        .chain([format!("unix:{},server=on,wait=off", plan.monitor.display())])
        .collect();
    let mut child = emulator::spawn_emulator(&plan, &args, Stdio::null()).expect("the emulator starts");
    let asked = qmp::Monitor::connect(&plan.monitor, Duration::from_secs(30)).and_then(|mut m| m.call("query-status"));
    let pid = child.id();
    let stop = emulator::stop_emulator(&plan, &mut child);
    cleanup(&s, &plan);
    asked.expect("the emulator answers QMP");
    assert!(stop.confirmed(), "{stop}");
    assert!(gone(pid), "no emulator stays after the run");
}

/// TCG. It skips where no `qemu-system-x86_64` is on `PATH`.
#[test]
fn a_real_emulator_answers_qmp_and_leaves_no_process() {
    pod_testkit::need_tool!("qemu-system-x86_64");
    real_emulator_answers_qmp("tcg", "qemu-qmp");
}

/// KVM. It skips where the user cannot open `/dev/kvm` for read and write.
#[test]
fn a_real_emulator_under_kvm_answers_qmp_and_leaves_no_process() {
    pod_testkit::need_tool!("qemu-system-x86_64");
    if std::fs::OpenOptions::new().read(true).write(true).open("/dev/kvm").is_err() {
        eprintln!("skipping, /dev/kvm is not open for read and write");
        return;
    }
    real_emulator_answers_qmp("kvm", "qemu-kvm");
}
