use super::*;

#[test]
fn the_dos_machine_is_seabios_ide_and_text() {
    let a = argv(
        Accel::Tcg,
        Path::new("root.qcow2"),
        Path::new("mbox.img"),
        Path::new("serial.log"),
        Path::new("mon.sock"),
        512,
        &[],
    );
    let has = |s: &str| a.iter().any(|x| x == s);
    assert_eq!(
        a.first().map(String::as_str),
        Some("-M"),
        "argv carries flags only, never the program: the caller names the emulator"
    );
    assert!(has("pc") && has("tcg") && has("max"));
    assert!(a.iter().any(|x| x.contains("if=ide") && x.contains("root.qcow2")));
    assert!(a.iter().any(|x| x.contains("if=ide") && x.contains("cache=writethrough")));
    assert!(!a.iter().any(|x| x.contains("pflash")), "no OVMF on the DOS path");
    assert!(!a.iter().any(|x| x == "nvme"), "no NVMe on the DOS path");
    let nic = a.iter().position(|x| x == "-nic").expect("names -nic");
    assert_eq!(a[nic + 1], "none");
    assert!(has("-no-reboot"));
}

#[test]
fn every_wrapper_character_has_a_key() {
    for ch in format!("command /c ver > {MBOX_DRIVE}\\WQOUT.TXT").chars() {
        assert!(qmp::key(ch).is_some(), "no key for {ch:?}");
    }
    for ch in "echo %errorlevel% 0123456789abcdef > D:\\WQCODE.TXT".chars() {
        assert!(qmp::key(ch).is_some(), "no key for {ch:?}");
    }
}

#[test]
fn a_cmd_slash_c_prefix_drops_and_an_empty_command_is_ver() {
    let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(batch_from_argv(&v(&["cmd", "/c", "ver"])), "ver");
    assert_eq!(batch_from_argv(&v(&["cmd", "/c", "dir", "/zzz"])), "dir /zzz");
    assert_eq!(batch_from_argv(&v(&[])), "ver");
}

#[test]
fn the_dos_mailbox_carries_both_batch_spellings_and_the_token() {
    let dir = std::env::temp_dir().join(format!("pod-guest-dosmb-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mbox = dir.join("mbox.img");
    let root = dir.join("root.qcow2");
    let plan = Plan {
        emulator: "qemu-system-x86_64".into(),
        accel: Accel::Tcg,
        share: "/usr/share/qemu".into(),
        firmware_code: "/code.fd".into(),
        firmware_vars: dir.join("vars.fd"),
        root: root.clone(),
        mailbox: mbox.clone(),
        serial: dir.join("serial.log"),
        monitor: dir.join("qmp.sock"),
        memory_mib: 512,
        cpus: 2,
        emu_args: Vec::new(),
    };
    let mut s = Settings::new(std::env::temp_dir());
    s.marker = b"guest-dos".to_vec();
    // The overlay needs qemu-img. With no qemu-img here, the stage refuses
    // and names it, and writes no half of a run.
    match stage(&s, Path::new("qemu-img"), Path::new("base.img"), &plan, "ver", "tok") {
        Ok(()) => {
            let bytes = std::fs::read(&mbox).unwrap();
            let back = Fat16::from_image(bytes).expect("a mailbox");
            assert_eq!(back.get(agent::MARKER).unwrap(), b"guest-dos");
            assert!(back.get(agent::CMD).is_some());
            assert!(back.get(BAT).is_some());
            assert_eq!(back.get(agent::GO).unwrap(), b"tok");
            assert!(root.is_file(), "the overlay was created");
        }
        Err(e) => {
            assert!(format!("{e}").contains("qemu-img"), "{e}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A line with a character that has no key is refused before the first
/// key: a part of a line is another command. The refusal comes before a
/// connection, so a closed monitor is enough.
#[test]
fn a_line_with_no_key_for_a_character_types_nothing() {
    let dir = pod_testkit::host::socket_dir("pod-guest-dosline");
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("qmp.sock");
    let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        use std::io::{BufRead, Write};
        s.write_all(b"{\"QMP\": {}}\n").unwrap();
        let mut r = std::io::BufReader::new(s.try_clone().unwrap());
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            if r.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            lines.push(line);
            s.write_all(b"{\"return\": {}}\n").unwrap();
        }
        lines
    });
    let mut m = Monitor::connect(&sock, Duration::from_secs(5)).unwrap();
    let e = type_line(&mut m, "ver \u{e9}").unwrap_err();
    assert_eq!(e.class, Class::Usage);
    drop(m);
    let lines = server.join().unwrap();
    assert_eq!(lines.len(), 1, "only the capabilities, no key: {lines:?}");
    assert!(lines[0].contains("qmp_capabilities"));
    let _ = std::fs::remove_dir_all(&dir);
}
