//! `Settings::emulator`: the machine legs run the emulator at the path of the
//! caller, and name it. The emulator is a `sh` script, and `PATH` is empty, so
//! a leg that ignores the setting finds no emulator. A probe child of this
//! binary runs each leg: no libtest.

#[cfg(target_os = "linux")]
fn main() {
    use pod_hostprobe::child::Spawner;
    use pod_hostprobe::probes::EMULATOR_VARIABLE;
    use pod_hostprobe::verdict::{Outcome, Verdict};
    use pod_hostprobe::{Settings, run_child};
    use std::os::unix::fs::PermissionsExt;

    const VERSION: &str = "qemu-system-x86_64 --version";
    const ACCEL: &str = "qemu-system-x86_64 -accel help";

    let settings = Settings::default();
    let args: Vec<String> = std::env::args().collect();
    if args.get(1) == Some(&settings.child_flag) {
        let name = args.get(2).map_or("", String::as_str);
        std::process::exit(run_child(name, &mut std::io::stdout()));
    }

    let dir = std::env::temp_dir().join(format!("hostprobe-emulator-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temp dir");
    let fake = dir.join("fake-qemu");
    std::fs::write(
        &fake,
        "#!/bin/sh\ncase \"$1\" in\n--version) echo 'QEMU emulator version 9.9.9 (fake)';;\n\
         -accel) echo 'Accelerators supported in QEMU binary:'; echo tcg; echo kvm;;\nesac\n",
    )
    .expect("the fake emulator");
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).expect("mode");
    let fake_text = fake.to_str().expect("a UTF-8 path").to_string();

    // No emulator on PATH: only the path of the settings can find one. The
    // variable of the caller must not turn the setting on.
    // SAFETY: this binary has one thread here.
    unsafe {
        std::env::set_var("PATH", "");
        std::env::set_var(EMULATOR_VARIABLE, &fake_text);
    }
    let leg = |s: &Settings, row: &str| -> Outcome { Spawner::new(s).run_probe(row, 0) };

    let with = Settings::default().with_emulator(&fake);
    let v = leg(&with, VERSION);
    assert_eq!(v.verdict, Verdict::Ok, "version leg: {}", v.reason);
    assert!(v.reason.contains("QEMU emulator version 9.9.9"), "{}", v.reason);
    assert!(v.reason.contains(&fake_text), "the leg does not name the path: {}", v.reason);
    let a = leg(&with, ACCEL);
    assert_eq!(a.verdict, Verdict::Ok, "accel leg: {}", a.reason);
    assert!(a.reason.contains("tcg kvm"), "{}", a.reason);
    assert!(a.reason.contains(&fake_text), "the leg does not name the path: {}", a.reason);

    // The default: PATH only, and the old words.
    for (row, tail) in [(VERSION, "the emulator leg could not run"), (ACCEL, "the accelerator list could not be read")]
    {
        let o = leg(&Settings::default(), row);
        assert_eq!(o.verdict, Verdict::Skip, "{row}: {}", o.reason);
        assert_eq!(o.reason, format!("no qemu-system-x86_64 on PATH, so {tail}"), "{row}");
    }

    // A path with no file: the words name the path, not PATH.
    let gone = Settings::default().with_emulator(dir.join("absent"));
    let o = leg(&gone, VERSION);
    assert_eq!(o.verdict, Verdict::Skip, "{}", o.reason);
    assert!(o.reason.starts_with(&format!("no emulator at {}/absent", dir.display())), "{}", o.reason);

    let _ = std::fs::remove_dir_all(&dir);
    println!("emulator: the legs run the path of the settings, name it, and keep the PATH words by default");
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("emulator: skipping, pod-hostprobe is Linux only");
}
