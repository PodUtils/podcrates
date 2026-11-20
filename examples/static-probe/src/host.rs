//! The crates of the host that are Linux only: on another target each of
//! them is empty, and its line is none.

/// pod-hostprobe is Linux only: on another target the crate is empty.
#[cfg(target_os = "linux")]
pub(crate) fn hostprobe() -> Option<String> {
    use pod_hostprobe::{Findings, report, select::Selection};
    // The reader of the report links in: an empty set reads back.
    let empty = Findings::empty();
    let text = report::write(&empty, &Selection::choose(&empty));
    Some(format!(
        "pod-hostprobe: pid {}, ELF machine 0x{:x}, {} probes, {} free inodes at /, report {}",
        pod_hostprobe::sys::getpid(),
        pod_hostprobe::binfmt::SELF_MACHINE,
        pod_hostprobe::probes::PROBES.len(),
        pod_hostprobe::space::read("/").map_or_else(|e| e.to_string(), |h| h.free.inodes.to_string()),
        report::read(&text).map_or_else(|e| e.to_string(), |_| "reads back".to_string())
    ))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn hostprobe() -> Option<String> {
    None
}

/// pod-supervise is Linux only: on another target the crate is empty.
#[cfg(target_os = "linux")]
pub(crate) fn supervise() -> Option<String> {
    let s = pod_supervise::Settings::new(std::env::temp_dir().join("static-probe-supervise"));
    Some(format!(
        "pod-supervise: name {}, {} records, a hold of no record: {}",
        pod_supervise::generated_name(&s, &pod_supervise::new_id()),
        pod_supervise::table::read::<pod_supervise::table::Any>(&s)
            .map_or_else(|e| e.to_string(), |t| t.containers.len().to_string()),
        pod_supervise::foreground::<pod_supervise::table::Any>(&s, "static-probe-none", 1)
            .map_or_else(|e| format!("{:?}", e.class), |h| h.id().to_string())
    ))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn supervise() -> Option<String> {
    None
}

/// pod-guest is Linux only: on another target the crate is empty.
#[cfg(target_os = "linux")]
pub(crate) fn guest() -> Option<String> {
    let s = pod_guest::Settings::new(std::env::temp_dir());
    let request = pod_guest::Request {
        command: "ver".into(),
        token: pod_guest::agent::nonce().unwrap_or_else(|e| e.to_string()),
        timeout: std::time::Duration::from_secs(1),
        mode: pod_guest::Mode::Once,
    };
    let files = pod_guest::mailbox(&s, pod_guest::agent::AGENT_CMD, &request)
        .map_or_else(|e| e.to_string(), |m| m.list().len().to_string());
    // An empty stream: the store and its digest link in.
    let dest = std::env::temp_dir().join(format!("static-probe-guest-{}.img", std::process::id()));
    let ceiling = pod_guest::Ceiling { max_bytes: 1, fsize: u64::MAX };
    let mut origin = pod_guest::fetch::Stream::new(std::io::empty(), Some(0));
    let digest = pod_guest::fetch::store(&mut origin, &dest, None, ceiling, 1).unwrap_or_else(|e| e.to_string());
    let _ = std::fs::remove_file(&dest);
    // An image of 96 zero bytes that the optimizer cannot see: the squashfs
    // reader and its xz decoder link in.
    let image = std::io::Cursor::new(std::hint::black_box([0u8; 96]));
    let modloop = pod_guest::modloop::modules(image, "probe", ["tun"])
        .map_or_else(|e| e.to_string(), |set| set.size().to_string());
    Some(format!(
        "pod-guest: a mailbox of {files} files, {} arguments of the UEFI line, sha256 of nothing {digest}, {modloop}",
        pod_guest::argv(&pod_guest::Plan {
            emulator: "qemu-system-x86_64".into(),
            accel: pod_guest::accel_for(Some(pod_hostprobe::machine::Profile::Tcg)).unwrap_or(pod_guest::Accel::Tcg),
            share: "/usr/share/qemu".into(),
            firmware_code: "code.fd".into(),
            firmware_vars: "vars.fd".into(),
            root: "run.qcow2".into(),
            mailbox: "mailbox.img".into(),
            serial: "serial.log".into(),
            monitor: "qmp.sock".into(),
            memory_mib: 512,
            cpus: 1,
            options: Default::default(),
            emu_args: Vec::new(),
        })
        .len()
    ))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn guest() -> Option<String> {
    None
}

/// pod-enter is Linux only: on another target the crate is empty.
#[cfg(target_os = "linux")]
pub(crate) fn enter() -> Option<String> {
    let names = pod_enter::Names::DEFAULT;
    let argv = pod_enter::Plan::argv_for(Some(&["/bin/sh".to_string()]), Some(&["-c".to_string()]), &[], None);
    let env = pod_enter::Plan::env_for(&names, &[], &[]);
    // The reader of ELF files, on this binary; the memfd probe forks once.
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    let elf =
        pod_enter::abi::Elf::read(&exe).map_or_else(|e| e.to_string(), |e| format!("ELF machine 0x{:x}", e.machine));
    let memfd = pod_enter::memfd::probe::answer(&names, pod_enter::memfd::memfd_exec());
    Some(format!(
        "pod-enter: argv {argv:?}, {} variables, {elf}, memfd: {}, a namespace selection enters {}",
        env.len(),
        memfd.detail,
        pod_enter::entered_word(pod_hostprobe::select::Rung::Namespace)
    ))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn enter() -> Option<String> {
    None
}

/// pod-confine is Linux only: on another target the crate is empty. The
/// probe of the ABI forks one child; the compile builds the deny filter.
#[cfg(target_os = "linux")]
pub(crate) fn confine() -> Option<String> {
    let abi = pod_confine::check().map_or_else(|e| e.to_string(), |abi| format!("Landlock ABI {abi}"));
    let filter = pod_confine::deny::program(&pod_confine::Deny::defaults())
        .map_or_else(|e| e.to_string(), |p| format!("{} BPF instructions", p.len()));
    let spec = pod_confine::Spec::new().allow("/", pod_confine::Access::READ).compile(pod_confine::MIN_ABI);
    Some(format!("pod-confine: {abi}, a deny filter of {filter}, a compile of one rule: {}", spec.is_ok()))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn confine() -> Option<String> {
    None
}

/// pod-beneath is Linux only: on another target the crate is empty. One
/// open of `/tmp` under `/` through the C library.
#[cfg(target_os = "linux")]
pub(crate) fn beneath() -> Option<String> {
    let mut buf = [0u8; 64];
    let r = pod_beneath::open_under_base(&pod_beneath::Libc, b"/", b"/tmp", pod_beneath::O_PATH, &mut buf);
    Some(format!("pod-beneath: /tmp under / gives {}", r.map_or_else(|r| r.to_string(), |fd| format!("fd {fd}"))))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn beneath() -> Option<String> {
    None
}
