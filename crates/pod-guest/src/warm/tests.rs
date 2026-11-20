use super::state::load;
use super::*;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::plan::{MailboxBus, Medium, MediumKind, Options, Virtio};
use crate::{Accel, Mode, qmp, scratch_dir};

/// A plan with a USB mailbox whose files are in the per-run directory
/// `dir`.
pub(super) fn plan_in(dir: &Path) -> Plan {
    Plan {
        emulator: "/bin/sh".into(),
        accel: Accel::Tcg,
        share: "/usr/share/qemu".into(),
        firmware_code: dir.join("code.fd"),
        firmware_vars: dir.join("vars.fd"),
        root: dir.join("run.qcow2"),
        mailbox: dir.join("mailbox.img"),
        serial: dir.join("serial.log"),
        monitor: dir.join("qmp.sock"),
        memory_mib: 512,
        cpus: 1,
        options: Options { mailbox: MailboxBus::Usb, ..Options::default() },
        emu_args: Vec::new(),
    }
}

/// A `query-block` response with the root disk and the mailbox device
/// `wqmbox` with `medium` in it, as QEMU 8.2 writes it.
pub(super) fn block_with(medium: &Path) -> String {
    format!(
        "{{\"return\": [{{\"device\": \"root\", \"inserted\": {{\"file\": \"/run/run.qcow2\"}}, \"qdev\": \
         \"/machine/peripheral-anon/device[0]\"}}, {{\"io-status\": \"ok\", \"device\": \"mbox\", \"locked\": true, \
         \"inserted\": {{\"image\": {{\"filename\": \"x.img\"}}, \"ro\": false, \"file\": {}}}, \"qdev\": \"wqmbox\"}}]}}",
        qmp::json_text(medium.to_str().unwrap())
    )
}

pub(super) fn dir(tag: &str) -> PathBuf {
    scratch_dir(&Settings::new(std::env::temp_dir()), tag).expect("a scratch directory")
}

pub(super) fn request(token: &str) -> Request {
    Request { command: "ver".into(), token: token.into(), timeout: Duration::from_secs(10), mode: Mode::KeepAlive }
}

/// The mailbox of an idle agent in keep-alive mode, as the agent leaves it.
pub(super) fn idle_mailbox() -> Fat16 {
    let mut m = Fat16::new();
    m.put(agent::MARKER, b"pod-guest").unwrap();
    m.put(agent::KEEP, b"").unwrap();
    m.put(agent::READY, b"WQREADY\r\n").unwrap();
    m
}

fn print(qemu: &str) -> Fingerprint {
    Fingerprint {
        qemu: qemu.into(),
        cpu: "tcg".into(),
        firmware: "f".into(),
        base: "sha256:b".into(),
        agent: "a".into(),
        machine: vec!["-M".into(), "q35".into()],
    }
}

/// The head of a qcow2 image whose backing file is `name`.
fn qcow2_naming(name: &str) -> Vec<u8> {
    let mut h = b"QFI\xfb\0\0\0\x03".to_vec();
    h.extend_from_slice(&104u64.to_be_bytes());
    h.extend_from_slice(&(name.len() as u32).to_be_bytes());
    h.resize(104, 0);
    h.extend_from_slice(name.as_bytes());
    h
}

/// The files of a run: the overlay, an idle mailbox, the variable store.
fn run_files(plan: &Plan) {
    std::fs::write(&plan.root, qcow2_naming("/images/base.vhdx")).unwrap();
    std::fs::write(&plan.mailbox, idle_mailbox().image()).unwrap();
    std::fs::write(&plan.firmware_vars, b"vars of the guest").unwrap();
}

/// A QMP stand-in on the socket of `plan`. `answer` gives the response
/// line to each request; the stand-in keeps the name of each request. A
/// `migrate` writes the file of its URI.
fn stand(socket: &Path, answer: impl Fn(&str, usize) -> String + Send + 'static) -> Arc<Mutex<Vec<String>>> {
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket).expect("the QMP socket");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { return };
            let mut out = conn.try_clone().expect("a second handle");
            let _ = out.write_all(b"{\"QMP\": {\"version\": {}}}\n");
            for line in BufReader::new(conn).lines() {
                let Ok(line) = line else { break };
                let name = qmp::json_member(&line, "execute").unwrap_or_default();
                if let Some(file) =
                    qmp::json_member(&line, "uri").and_then(|u| u.strip_prefix("file:").map(PathBuf::from))
                {
                    let _ = std::fs::write(file, b"QEVM stream");
                }
                let n = {
                    let mut l = log.lock().unwrap_or_else(|e| e.into_inner());
                    l.push(name.clone());
                    l.iter().filter(|x| **x == name).count()
                };
                if out.write_all(format!("{}\n", answer(&name, n)).as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    seen
}

const OK: &str = "{\"return\": {}}";

fn names(seen: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Two runs in two directories have one machine line; a change of the
/// memory, the accelerator, a device or an argument of the caller is a
/// new line. A medium keeps its extension, which gives its format.
#[test]
fn the_machine_line_has_no_path_of_a_run_and_each_option() {
    let (a, b) = (Path::new("/run/pod-guest-1-a"), Path::new("/run/pod-guest-2-b"));
    let (mut pa, mut pb) = (plan_in(a), plan_in(b));
    for (p, d) in [(&mut pa, a), (&mut pb, b)] {
        p.options.vnc = Some(d.join("vnc.sock"));
        p.options.media.push(Medium { kind: MediumKind::Usb, path: d.join("setup.vhdx"), readonly: true, boot: false });
    }
    let line = machine_line(&pa);
    assert_eq!(line, machine_line(&pb));
    assert!(!line.iter().any(|x| x.contains("/run/")), "{line:?}");
    assert!(line.iter().any(|x| x.contains("format=vhdx") && x.contains("MEDIUM0.vhdx")), "{line:?}");
    assert!(line.iter().any(|x| x == "unix:VNC"), "{line:?}");
    let base = machine_line(&pa);
    let mut changed = pa.clone();
    changed.memory_mib = 1024;
    assert_ne!(machine_line(&changed), base);
    let mut changed = pa.clone();
    changed.accel = Accel::Kvm;
    assert_ne!(machine_line(&changed), base);
    let mut changed = pa.clone();
    changed.options.virtio.push(Virtio::Rng);
    assert_ne!(machine_line(&changed), base);
    let mut changed = pa;
    changed.emu_args = vec!["-smbios".into(), "type=1".into()];
    assert_ne!(machine_line(&changed), base);
}

/// The text names each input on its own line; a value with a new line
/// cannot make a second line.
#[test]
fn the_text_of_a_fingerprint_has_one_line_for_each_input() {
    let mut f = print("QEMU emulator version 11.0.0");
    f.base = "sha256:b\nmachine \"-x\"".into();
    let text = f.text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], FORMAT);
    assert_eq!(lines[1], "qemu \"QEMU emulator version 11.0.0\"");
    assert_eq!(lines[4], "base \"sha256:b\\u000amachine \\\"-x\\\"\"");
    assert_eq!(lines[6..], ["machine \"-M\"", "machine \"q35\""]);
    assert_eq!(lines.len(), 8);
    assert_ne!(print("QEMU 10.2.1").text(), print("QEMU 11.0.0").text());
}

/// The CPU of the host is the features, not the clock or the microcode,
/// and only the first processor.
#[test]
fn the_cpu_of_the_host_is_its_features() {
    let info = "processor\t: 0\nvendor_id\t: GenuineIntel\nmodel\t\t: 85\ncpu MHz\t\t: 2100.0\nmicrocode\t: 0x1\nflags\t\t: fpu vmx avx2\n\nprocessor\t: 1\nflags\t\t: fpu\n";
    let cpu = host_cpu(info);
    assert!(cpu.starts_with("host ") && cpu.len() == 5 + 64, "{cpu}");
    assert_eq!(host_cpu(&info.replace("2100.0", "3000.0").replace("0x1", "0x2")), cpu);
    assert_ne!(host_cpu(&info.replace("avx2", "avx512f")), cpu);
    assert_eq!(host_cpu(&info.replace("flags\t\t: fpu\n", "flags\t\t: none\n")), cpu, "the second processor");
}

/// The fingerprint reads the version of the emulator and the firmware,
/// and refuses an empty digest of the base.
#[test]
fn the_fingerprint_reads_the_emulator_and_the_firmware() {
    pod_testkit::need_tool!("/bin/echo");
    let d = dir("warm-print");
    let mut plan = plan_in(&d);
    // The `echo` of coreutils answers `--version` as an emulator does.
    plan.emulator = "/bin/echo".into();
    std::fs::write(&plan.firmware_code, b"code").unwrap();
    let f = fingerprint(&plan, "sha256:base", agent::AGENT_CMD).unwrap();
    if f.qemu == "--version" {
        let _ = std::fs::remove_dir_all(&d);
        pod_testkit::host::skip_line("this echo has no --version");
        return;
    }
    assert!(f.qemu.contains("echo"), "{f:?}");
    assert_eq!((f.cpu.as_str(), f.base.as_str()), ("tcg", "sha256:base"));
    assert_eq!(f.firmware, "5694d08a2e53ffcae0c3103e5ad6f6076abd960eb1f8a56577040bc1028f702b", "sha256 of `code`");
    assert_eq!(f.machine, machine_line(&plan));
    assert_eq!(fingerprint(&plan, " ", agent::AGENT_CMD).unwrap_err().class, Class::Usage);
    std::fs::write(&plan.firmware_code, b"new code").unwrap();
    assert_ne!(fingerprint(&plan, "sha256:base", agent::AGENT_CMD).unwrap().firmware, f.firmware);
    plan.emulator = d.join("no-such-emulator");
    assert!(fingerprint(&plan, "sha256:base", agent::AGENT_CMD).is_err());
    let _ = std::fs::remove_dir_all(&d);
}

/// The restore line is the machine line, then `-incoming` and the stream.
#[test]
fn the_restore_line_is_the_plan_and_then_the_stream() {
    let plan = plan_in(Path::new("/run/pod-guest-1-a"));
    let a = incoming_argv(&plan, Path::new("/var/state/validationos")).unwrap();
    assert_eq!(a[..a.len() - 2], crate::argv(&plan)[..]);
    assert_eq!(a[a.len() - 2..], ["-incoming", "file:/var/state/validationos/state.bin"]);
    let e = incoming_argv(&plan, Path::new("/var/state/a,b")).unwrap_err();
    assert_eq!(e.class, Class::Usage, "a comma ends the path of a file URI: {e}");
}

/// A state holds an idle agent in keep-alive mode, never a command.
#[test]
fn only_an_idle_agent_in_keep_alive_mode_is_saved() {
    assert!(idle(&idle_mailbox()).is_ok());
    let mut m = idle_mailbox();
    m.remove(agent::READY).unwrap();
    assert!(idle(&m).unwrap_err().to_string().contains("WQREADY"));
    let mut m = idle_mailbox();
    m.remove(agent::KEEP).unwrap();
    assert!(idle(&m).unwrap_err().to_string().contains("keep-alive"));
    let mut m = idle_mailbox();
    m.put(agent::GO, b"tok").unwrap();
    let e = idle(&m).unwrap_err();
    assert_eq!(e.class, Class::Usage);
    assert!(e.to_string().contains("a command waits or runs"), "{e}");
}

/// The save stops the guest, writes the stream and the files, names the
/// directory at the end, and lets the guest go on. The mailbox of the
/// state is the medium in the guest, which after a swap is the other file.
#[test]
fn a_save_writes_each_file_and_lets_the_guest_go_on() {
    pod_testkit::need_loopback!();
    let d = dir("warm-save");
    let plan = plan_in(&d);
    run_files(&plan);
    let medium = crate::swap::other_file(&plan);
    let mut inside = idle_mailbox();
    inside.set_volume_id(0x1234_5679);
    std::fs::write(&medium, inside.image()).unwrap();
    let mut old = idle_mailbox();
    old.put(agent::GO, b"tok-old").unwrap();
    std::fs::write(&plan.mailbox, old.image()).unwrap();
    let block = block_with(&medium);
    let seen = stand(&plan.monitor, move |name, n| match (name, n) {
        ("query-migrate", 1) => OK.into(),
        ("query-migrate", 2) => "{\"return\": {\"status\": \"active\"}}".into(),
        ("query-migrate", _) => "{\"return\": {\"status\": \"completed\", \"total-time\": 7}}".into(),
        ("query-block", _) => block.clone(),
        _ => OK.into(),
    });
    let state = d.join("state");
    std::fs::create_dir(&state).unwrap();
    std::fs::write(state.join("old"), b"the state before").unwrap();
    let f = print("QEMU 11.0.0");
    save(&plan, &f, &state, Duration::from_secs(30)).unwrap();
    let names = names(&seen);
    assert_eq!(
        names[..6],
        ["qmp_capabilities", "query-migrate", "stop", "query-block", "migrate-set-parameters", "migrate"]
    );
    assert_eq!(names.last().map(String::as_str), Some("cont"));
    assert_eq!(std::fs::read_to_string(state.join(FINGERPRINT)).unwrap(), f.text());
    assert_eq!(std::fs::read(state.join(STATE)).unwrap(), b"QEVM stream");
    assert_eq!(std::fs::read(state.join(ROOT)).unwrap(), std::fs::read(&plan.root).unwrap());
    assert_eq!(std::fs::read(state.join(VARS)).unwrap(), b"vars of the guest");
    let kept = Fat16::from_image(std::fs::read(state.join(MAILBOX)).unwrap()).unwrap();
    assert_eq!(kept.volume_id(), inside.volume_id(), "the state keeps the medium in the guest");
    assert!(idle(&kept).is_ok());
    assert!(!state.join("old").exists(), "the new state replaces the old one");
    assert!(!d.join("state.part").exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// A save that cannot be made leaves the state before it, and the guest
/// goes on: a device that the emulator cannot migrate, a command on the
/// mailbox, a failed migration, a base with a relative name, no medium in
/// the mailbox device.
#[test]
fn a_failed_save_keeps_the_state_before_and_the_guest_goes_on() {
    pod_testkit::need_loopback!();
    let blocked = "{\"return\": {\"blocked-reasons\": [\"non-migratable device: 0000:00:02.0/nvme\"]}}";
    let failed = "{\"return\": {\"status\": \"failed\", \"error-desc\": \"no space\"}}";
    let cases: [(&str, &str, Class, &str); 5] = [
        ("blocked", blocked, Class::Unavailable, "nvme"),
        ("command", OK, Class::Usage, "a command waits"),
        ("failed", failed, Class::Protocol, "no space"),
        ("relative", OK, Class::Usage, "relative path"),
        ("nomedium", OK, Class::Protocol, "no medium of the mailbox"),
    ];
    for (case, migrate, class, text) in cases {
        let d = dir(&format!("warm-fail-{case}"));
        let plan = plan_in(&d);
        run_files(&plan);
        if case == "command" {
            let mut m = idle_mailbox();
            m.put(agent::GO, b"tok").unwrap();
            std::fs::write(&plan.mailbox, m.image()).unwrap();
        }
        if case == "relative" {
            std::fs::write(&plan.root, qcow2_naming("base.vhdx")).unwrap();
        }
        let done = "{\"return\": {\"status\": \"completed\"}}".to_string();
        let first = if case == "blocked" { blocked } else { OK }.to_string();
        let later = if case == "relative" { done } else { migrate.to_string() };
        let block = if case == "nomedium" { "{\"return\": []}".to_string() } else { block_with(&plan.mailbox) };
        let seen = stand(&plan.monitor, move |name, n| match (name, n) {
            ("query-migrate", 1) => first.clone(),
            ("query-migrate", _) => later.clone(),
            ("query-block", _) => block.clone(),
            _ => OK.into(),
        });
        let state = d.join("state");
        std::fs::create_dir(&state).unwrap();
        std::fs::write(state.join(FINGERPRINT), b"the state before").unwrap();
        let e = save(&plan, &print("QEMU 11.0.0"), &state, Duration::from_secs(30)).unwrap_err();
        assert_eq!(e.class, class, "{case}: {e}");
        assert!(e.to_string().contains(text), "{case}: {e}");
        assert_eq!(std::fs::read(state.join(FINGERPRINT)).unwrap(), b"the state before", "{case}");
        assert!(!d.join("state.part").exists(), "{case}");
        let names = names(&seen);
        if case == "blocked" {
            assert!(!names.iter().any(|n| n == "stop"), "{case}: the guest was not stopped: {names:?}");
        } else {
            assert_eq!(names.last().map(String::as_str), Some("cont"), "{case}: {names:?}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}

/// A child that lives until the test kills it.
pub(super) struct Alive(pub Child);

impl Alive {
    pub fn start() -> Alive {
        Alive(Command::new("sleep").arg("600").spawn().expect("sleep starts"))
    }
}

impl Drop for Alive {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The load waits while the emulator is `inmigrate`, then sends `cont`
/// to a paused guest. Another status, or an emulator that exits, is a
/// failed load.
#[test]
fn the_load_waits_for_the_stream_and_then_lets_the_guest_go_on() {
    pod_testkit::need_loopback!();
    pod_testkit::need_tool!("sleep");
    let d = dir("warm-load");
    let plan = plan_in(&d);
    let status = |s: &str| format!("{{\"return\": {{\"status\": \"{s}\", \"running\": false}}}}");
    let seen = stand(&plan.monitor, move |name, n| match (name, n) {
        ("query-status", 1 | 2) => status("inmigrate"),
        ("query-status", _) => status("paused"),
        _ => OK.into(),
    });
    let mut child = Alive::start();
    load(&plan, &mut child.0, Duration::from_secs(30)).unwrap();
    assert_eq!(names(&seen), ["qmp_capabilities", "query-status", "query-status", "query-status", "cont"]);

    let _seen =
        stand(&plan.monitor, move |name, _| if name == "query-status" { status("shutdown") } else { OK.into() });
    let e = load(&plan, &mut child.0, Duration::from_secs(30)).unwrap_err();
    assert_eq!(e.class, Class::Protocol, "{e}");
    assert!(e.to_string().contains("shutdown"), "{e}");

    // No socket: only the exit of the emulator ends this load.
    std::fs::remove_file(&plan.monitor).unwrap();
    let mut gone = Command::new("/bin/sh").args(["-c", "exit 1"]).spawn().expect("sh starts");
    let e = load(&plan, &mut gone, Duration::from_secs(30)).unwrap_err();
    assert_eq!(e.class, Class::Unavailable, "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

/// The qcow2 check reads the name of the backing file.
#[test]
fn a_saved_overlay_names_its_base_by_an_absolute_path() {
    let d = dir("warm-backing");
    let f = d.join("o.qcow2");
    for (name, ok) in [("/images/base.vhdx", true), ("base.vhdx", false), ("../base.img", false), ("", true)] {
        std::fs::write(&f, qcow2_naming(name)).unwrap();
        assert_eq!(state::absolute_backing(&f).is_ok(), ok, "{name:?}");
    }
    std::fs::write(&f, b"not an image at all").unwrap();
    assert_eq!(state::absolute_backing(&f).unwrap_err().class, Class::Protocol);
    let _ = std::fs::remove_dir_all(&d);
}

#[path = "tests_start.rs"]
mod start_tests;

/// The time a test waits for an event of a stand-in.
pub(super) fn until(what: &str, ok: impl Fn() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(start.elapsed() < pod_testkit::host::scaled(Duration::from_secs(10)), "{what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
