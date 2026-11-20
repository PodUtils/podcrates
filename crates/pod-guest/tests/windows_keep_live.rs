//! The keep-alive mode on a real Windows guest (T-602), under KVM:
//!
//! 1. install: Windows Server 2019 (Server Core) from the evaluation ISO,
//!    with the answer file of `tests/fixtures/windows/` and the agent and
//!    the installer of the crate on a second CD-ROM. The guest powers off
//!    at the end of setup, with the agent as a boot task.
//! 2. swap: a boot with a USB mailbox. The first command is on the mailbox
//!    before the boot; two more come on new media through
//!    `blockdev-change-medium` with `force` (`swap::Swap::next`): Windows
//!    locks the medium that it has mounted. The test prints the lock before
//!    each swap and checks each outcome.
//! 3. warm start (T-628): a save of the idle guest of step 2
//!    (`warm::save`), and after the control of step 4 a restore with a
//!    fourth command on a new medium (`warm::start`). It runs only where
//!    the emulator saves the plan (QEMU 11.1: QEMU 8.2 to 11.0 refuse the
//!    NVMe root disk); else the test prints the refusal and goes on.
//! 4. controls: a command that the host writes into the image of the
//!    mounted mailbox, on the USB mailbox of step 2 and on an NVMe mailbox
//!    (the line of a run before T-602). The test prints if each one
//!    reached the agent; it does not fail on them.
//! 5. cold boots (T-640): six runs of `pod_guest::run` on the installed
//!    disk, each a cold boot with its own overlay, variable store, NVMe
//!    mailbox, command and limit, as podvm runs a provisioned image. A run
//!    that times out keeps its screen, its serial log and its mailbox in
//!    `WORK/evidence`. The test prints the count that reached their
//!    outcome, and fails at its end when one did not.
//!
//! The variables: `POD_GUEST_WINDOWS_ISO` (the ISO, which the workflow
//! checks against the pin of the service `microsoft-windows-eval`),
//! `POD_GUEST_OVMF_CODE`, `POD_GUEST_OVMF_VARS`, `POD_GUEST_WORK` (a
//! directory with 30 GB free). It needs `/dev/kvm`, `qemu-img` and
//! `xorriso`. `guest-windows.yml` runs it by hand.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use pod_core::Class;
use pod_guest::plan::{MailboxBus, Options};
use pod_guest::swap::{self, Swap};
use pod_guest::warm::{self, Start, Warm};
use pod_guest::{Accel, Fat16, Mode, Outcome, Plan, Request, Settings, agent, emulator, qmp};

const ANSWER: &str = include_str!("fixtures/windows/autounattend.xml");
/// The limit of the install, of the boot to the first result, and of one
/// command after a swap.
const INSTALL: Duration = Duration::from_secs(80 * 60);
const BOOT: Duration = Duration::from_secs(20 * 60);
const COMMAND: Duration = Duration::from_secs(5 * 60);
/// How long a control waits for the agent to take its command.
const CONTROL: Duration = Duration::from_secs(150);
/// The limit of the save and of the load of 4 GiB of RAM.
const SAVE: Duration = Duration::from_secs(10 * 60);
/// The limit of one cold boot of step 5, as the runs of podvm.
const COLD: Duration = Duration::from_secs(10 * 60);

struct Env {
    iso: PathBuf,
    code: PathBuf,
    vars: PathBuf,
    work: PathBuf,
    start: Instant,
}

impl Env {
    fn read() -> Env {
        let var = |name: &str| PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is not set")));
        Env {
            iso: var("POD_GUEST_WINDOWS_ISO"),
            code: var("POD_GUEST_OVMF_CODE"),
            vars: var("POD_GUEST_OVMF_VARS"),
            work: var("POD_GUEST_WORK"),
            start: Instant::now(),
        }
    }

    fn say(&self, text: &str) {
        eprintln!("[{:>5}s] {text}", self.start.elapsed().as_secs());
    }
}

/// An emulator that is stopped when the value is dropped.
struct Guest<'a> {
    plan: &'a Plan,
    child: Child,
}

impl Drop for Guest<'_> {
    fn drop(&mut self) {
        emulator::stop_emulator(self.plan, &mut self.child);
    }
}

fn plan(env: &Env, dir: &Path, mailbox: MailboxBus) -> Plan {
    std::fs::create_dir_all(dir).unwrap();
    Plan {
        emulator: "qemu-system-x86_64".into(),
        accel: Accel::Kvm,
        share: "/usr/share/qemu".into(),
        firmware_code: env.code.clone(),
        firmware_vars: dir.join("vars.fd"),
        root: dir.join("run.qcow2"),
        mailbox: dir.join("mailbox.img"),
        serial: dir.join("serial.log"),
        monitor: dir.join("qmp.sock"),
        memory_mib: 4096,
        cpus: 4,
        options: Options { mailbox, hyperv: true, ..Options::default() },
        emu_args: Vec::new(),
    }
}

fn spawn(plan: &Plan, args: &[String], log: &Path) -> Child {
    let _ = std::fs::remove_file(&plan.monitor);
    let err = std::fs::File::create(log).unwrap();
    emulator::spawn_emulator(plan, args, Stdio::from(err)).expect("the emulator starts")
}

fn tool(program: &str, args: &[&str]) {
    let out = Command::new(program).args(args).output().unwrap_or_else(|e| panic!("{program}: {e}"));
    assert!(out.status.success(), "{program} {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Step 1: the installed disk `win.qcow2` and its variable store.
fn install(env: &Env) -> (PathBuf, PathBuf) {
    let dir = env.work.join("install");
    let p = plan(env, &dir, MailboxBus::Nvme);
    let answer = dir.join("answer");
    std::fs::create_dir_all(&answer).unwrap();
    std::fs::write(answer.join("autounattend.xml"), ANSWER).unwrap();
    std::fs::write(answer.join(agent::MARKER), b"pod-guest").unwrap();
    std::fs::write(answer.join(agent::AGENT_NAME), agent::AGENT_CMD).unwrap();
    std::fs::write(answer.join(agent::SETUP_NAME), agent::SETUP_CMD).unwrap();
    let iso = dir.join("answer.iso");
    tool("xorriso", &["-as", "mkisofs", "-quiet", "-J", "-R", "-V", "WQANSWER", "-o", s(&iso), s(&answer)]);
    let disk = dir.join("win.qcow2");
    tool("qemu-img", &["create", "-q", "-f", "qcow2", s(&disk), "40G"]);
    std::fs::copy(&env.vars, &p.firmware_vars).unwrap();
    // Its own line: no mailbox disk, so the only disk is disk 0 of the
    // answer file; the reboots of setup are allowed.
    let mut a: Vec<String> = Vec::new();
    for x in [
        "-M", "q35", "-accel", "kvm", "-cpu", "host", "-smp", "4", "-m", "4096", "-vga", "std", "-display", "none",
        "-nic", "none",
    ] {
        a.push(x.into());
    }
    a.push("-drive".into());
    a.push(format!("if=pflash,format=raw,readonly=on,file={}", s(&env.code)));
    a.push("-drive".into());
    a.push(format!("if=pflash,format=raw,file={}", s(&p.firmware_vars)));
    a.push("-drive".into());
    a.push(format!("if=none,id=root,file={},format=qcow2", s(&disk)));
    a.push("-device".into());
    // The CD boots first and the disk next, as `pod_guest::argv` gives them
    // (`Medium::boot`): OVMF deletes each boot entry outside the boot order
    // of QEMU, also the entry of Windows Boot Manager that setup writes.
    // After a reboot of setup, the prompt of the CD times out and the
    // firmware boots the disk.
    a.push("nvme,drive=root,serial=wqroot,bootindex=1".into());
    for (i, file) in [&env.iso, &iso].into_iter().enumerate() {
        a.push("-drive".into());
        a.push(format!("if=none,id=cd{i},media=cdrom,readonly=on,format=raw,file={}", s(file)));
        a.push("-device".into());
        a.push(format!("ide-cd,drive=cd{i},bus=ide.{i}{}", if i == 0 { ",bootindex=0" } else { "" }));
    }
    a.push("-serial".into());
    a.push(format!("file:{}", s(&p.serial)));
    a.push("-qmp".into());
    a.push(format!("unix:{},server=on,wait=off", s(&p.monitor)));
    env.say("install: the emulator starts from the ISO");
    let mut g = Guest { plan: &p, child: spawn(&p, &a, &dir.join("emulator.log")) };
    let mut m = qmp::Monitor::connect(&p.monitor, Duration::from_secs(30)).expect("the QMP socket");
    // "Press any key to boot from CD or DVD": Enter, two times each second.
    let keys = Instant::now();
    while keys.elapsed() < Duration::from_secs(60) {
        let _ = m.keys(&["ret"]);
        std::thread::sleep(Duration::from_millis(500));
    }
    env.say("install: the keys of the boot prompt are sent; setup runs");
    let start = Instant::now();
    let mut shots = 0;
    loop {
        if let Some(status) = g.child.try_wait().unwrap() {
            env.say(&format!("install: the guest powered off ({status})"));
            break;
        }
        assert!(start.elapsed() < INSTALL, "the install did not end within {}s", INSTALL.as_secs());
        std::thread::sleep(Duration::from_secs(5));
        if start.elapsed().as_secs() / 120 >= shots {
            shots += 1;
            let _ = m.screendump(&env.work.join(format!("install-{shots:03}.ppm")));
            env.say("install: setup runs");
        }
    }
    drop(m);
    drop(g);
    (disk, p.firmware_vars.clone())
}

fn s(p: &Path) -> &str {
    p.to_str().expect("a UTF-8 path")
}

fn request(command: &str, token: &str, mode: Mode) -> Request {
    Request { command: command.into(), token: token.into(), timeout: BOOT, mode }
}

fn read(file: &Path) -> Option<Fat16> {
    Fat16::from_image(std::fs::read(file).ok()?)
}

/// Wait until the result of `token` is settled on `file`.
fn settled(env: &Env, guest: &mut Guest, file: &Path, token: &str, limit: Duration) -> Option<Outcome> {
    let start = Instant::now();
    let mut ready = false;
    while start.elapsed() < limit {
        if let Some(status) = guest.child.try_wait().unwrap() {
            env.say(&format!("the emulator exited ({status})"));
            return None;
        }
        if let Some(m) = read(file) {
            if !ready && agent::ready(&m) {
                ready = true;
                env.say(&format!("the agent wrote {} on {}", agent::READY, file.display()));
            }
            if let Some(o) = swap::settled(&m, token) {
                return Some(o);
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    None
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

/// Write the command of `request` into the image of a mounted mailbox, as
/// a run before T-602 did, and wait for its result. True when the agent
/// took it.
fn control(env: &Env, guest: &mut Guest, file: &Path, request: &Request, what: &str) -> bool {
    let mut m = read(file).expect("the mailbox");
    pod_guest::next_command(&mut m, request).unwrap();
    std::fs::write(file, m.image()).unwrap();
    env.say(&format!("control ({what}): the host wrote the token {} into the mounted image", request.token));
    let start = Instant::now();
    while start.elapsed() < CONTROL {
        if read(file).is_some_and(|m| pod_guest::poll_outcome(&m, &request.token).is_some()) {
            return true;
        }
        if guest.child.try_wait().unwrap().is_some() {
            return false;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    false
}

/// A boot of the installed disk with a mailbox on `bus` and the first
/// command of `first` on it.
fn boot<'a>(env: &Env, p: &'a Plan, disk: &Path, vars: &Path, first: &Request) -> Guest<'a> {
    std::fs::copy(vars, &p.firmware_vars).unwrap();
    let st = Settings::new(&env.work);
    pod_guest::stage(&st, Path::new("qemu-img"), disk, p, agent::AGENT_CMD, first).expect("the stage");
    p.options.check().unwrap();
    let log = p.serial.with_file_name("emulator.log");
    Guest { plan: p, child: spawn(p, &pod_guest::argv(p), &log) }
}

#[test]
#[ignore = "a Windows install under KVM; guest-windows.yml runs it"]
fn a_windows_guest_takes_each_next_command_on_a_new_medium() {
    let env = Env::read();
    let (disk, vars) = install(&env);
    let (reached_cold, boots) = cold_boots(&env, &disk, &vars);

    let p = plan(&env, &env.work.join("swap"), MailboxBus::Usb);
    let first = request("echo pod-guest-one& exit /b 3", "tok-one", Mode::KeepAlive);
    env.say("swap: boot with a USB mailbox and the first command");
    let mut g = boot(&env, &p, &disk, &vars, &first);
    let mut sw = Swap::new(&p, "tok-one").unwrap();
    let one = settled(&env, &mut g, sw.current(), "tok-one", BOOT).expect("the result of the first command");
    env.say(&format!("swap: first command: code {}, stdout {:?}", one.code, text(&one.stdout)));
    assert_eq!(one.code, 3);
    assert!(text(&one.stdout).contains("pod-guest-one"));

    let st = Settings::new(&env.work);
    let mut m = qmp::Monitor::connect(&p.monitor, Duration::from_secs(30)).expect("the QMP socket");
    let mut outcomes = Vec::new();
    for (word, token) in [("two", "tok-two"), ("three", "tok-three")] {
        let next = request(&format!("echo pod-guest-{word}& ver"), token, Mode::KeepAlive);
        let block = m.ask("{\"execute\":\"query-block\"}").unwrap_or_default();
        env.say(&format!("swap: the guest locks the medium: {:?}", swap::mailbox_locked(&block)));
        sw.next(&mut m, &st, agent::AGENT_CMD, &next).expect("the swap");
        env.say(&format!("swap: medium {} is in the guest with the token {token}", sw.current().display()));
        let o = settled(&env, &mut g, sw.current(), token, COMMAND).unwrap_or_else(|| panic!("no result of {token}"));
        env.say(&format!("swap: code {}, stdout {:?}", o.code, text(&o.stdout)));
        outcomes.push((word, o));
    }
    for (word, o) in &outcomes {
        assert_eq!(o.code, 0, "{word}: {o:?}");
        assert!(text(&o.stdout).contains(&format!("pod-guest-{word}")), "{word}: {o:?}");
        assert!(text(&o.stdout).contains("Microsoft Windows"), "{word}: {o:?}");
    }
    env.say("RESULT: the swap gave two next commands to the agent, and both outcomes are right");
    // The save connects to QMP itself, and QEMU serves one client.
    drop(m);
    let saved = save(&env, &mut g, &sw);

    let ctl = request("echo pod-guest-control", "tok-control-usb", Mode::KeepAlive);
    let reached = control(&env, &mut g, sw.current(), &ctl, "USB mailbox, no swap");
    env.say(&format!("RESULT: a write into the mounted USB mailbox reached the agent: {}", yes(reached)));
    drop(g);
    if let Some(w) = saved {
        restore(&env, &p, &disk, &w);
    }

    let p = plan(&env, &env.work.join("nvme"), MailboxBus::Nvme);
    let first = request("echo pod-guest-nvme", "tok-nvme", Mode::KeepAlive);
    env.say("control: boot with an NVMe mailbox and the first command");
    let mut g = boot(&env, &p, &disk, &vars, &first);
    let o = settled(&env, &mut g, &p.mailbox, "tok-nvme", BOOT).expect("the result of the NVMe boot");
    assert!(text(&o.stdout).contains("pod-guest-nvme"), "{o:?}");
    let ctl = request("echo pod-guest-control", "tok-control-nvme", Mode::KeepAlive);
    let reached = control(&env, &mut g, &p.mailbox, &ctl, "NVMe mailbox");
    env.say(&format!("RESULT: a write into the mounted NVMe mailbox reached the agent: {}", yes(reached)));
    drop(g);
    assert_eq!(reached_cold, boots, "cold boots that did not reach their outcome; see WORK/evidence");
}

/// Step 5: cold boots of `pod_guest::run` on the installed disk. Returns
/// the count that reached their outcome, and the count of boots.
fn cold_boots(env: &Env, disk: &Path, vars: &Path) -> (usize, usize) {
    // The host argv of each run, as podvm gives it, the exit code, and a
    // text of stdout. The first three are the runs of PodUtils/podvm#380.
    let runs: [(&[&str], i32, &str); 6] = [
        (&["exit", "42"], 42, ""),
        (&["cmd", "/c", "exit", "42"], 42, ""),
        (&["echo", "out", "&", "exit", "7"], 7, "out"),
        (&["ver"], 0, "Microsoft Windows"),
        (&["echo", "pod-guest-cold&", "exit", "/b", "3"], 3, "pod-guest-cold"),
        (&["exit", "0"], 0, ""),
    ];
    let mut st = Settings::new(&env.work);
    st.evidence = Some(env.work.join("evidence"));
    let mut reached = 0;
    for (i, (argv, code, out)) in runs.iter().enumerate() {
        let argv: Vec<String> = argv.iter().map(|a| a.to_string()).collect();
        let command = agent::command_from_argv(&argv);
        let token = agent::nonce().expect("a token");
        let req = Request { command: command.clone(), token, timeout: COLD, mode: Mode::Once };
        let dir = pod_guest::scratch_dir(&st, &format!("cold-{i}")).expect("a per-run directory");
        let p = plan(env, &dir, MailboxBus::Nvme);
        std::fs::copy(vars, &p.firmware_vars).unwrap();
        pod_guest::stage(&st, Path::new("qemu-img"), disk, &p, agent::AGENT_CMD, &req).expect("the stage");
        let start = Instant::now();
        let got = pod_guest::run(&st, &p, &req);
        let secs = start.elapsed().as_secs();
        match got {
            Ok(o) if o.code == *code && text(&o.stdout).contains(out) => {
                reached += 1;
                env.say(&format!("cold {i}: {command:?}: code {} in {secs}s", o.code));
            }
            Ok(o) => env.say(&format!("cold {i}: {command:?}: a wrong outcome in {secs}s: {o:?}")),
            Err(e) => env.say(&format!("cold {i}: {command:?}: no outcome in {secs}s: {e}")),
        }
        pod_guest::cleanup(&st, &p);
    }
    env.say(&format!("RESULT: cold boots: {reached} of {} reached their outcome", runs.len()));
    (reached, runs.len())
}

/// Step 3, the save: the idle guest of the swap into `WORK/warm`. `None`
/// where the emulator cannot save the plan.
fn save(env: &Env, guest: &mut Guest, sw: &Swap) -> Option<Warm> {
    let p = guest.plan;
    let start = Instant::now();
    while !read(sw.current()).is_some_and(|m| warm::idle(&m).is_ok()) {
        assert!(start.elapsed() < COMMAND, "the agent did not go idle on {}", sw.current().display());
        std::thread::sleep(Duration::from_secs(2));
    }
    let fingerprint = warm::fingerprint(p, "sha256:the-disk-of-this-run", agent::AGENT_CMD).expect("the fingerprint");
    let dir = env.work.join("warm");
    env.say(&format!("warm: save of the idle guest ({})", fingerprint.qemu));
    match warm::save(p, &fingerprint, &dir, SAVE) {
        Ok(()) => {
            env.say("warm: the state is saved, and the guest goes on");
            Some(Warm { dir, fingerprint, wait: SAVE })
        }
        Err(e) if e.class == Class::Unavailable => {
            env.say(&format!("RESULT: warm start: not run, the emulator cannot save the plan: {e}"));
            None
        }
        Err(e) => panic!("the save: {e}"),
    }
}

/// Step 3, the restore: a warm start of the saved guest with a fourth
/// command, which must come on a new medium after the load.
fn restore(env: &Env, p: &Plan, disk: &Path, w: &Warm) {
    let st = Settings::new(&env.work);
    let next = request("echo pod-guest-warm& ver", "tok-warm", Mode::KeepAlive);
    env.say("warm: restore with the command tok-warm");
    let started = warm::start(&st, Path::new("qemu-img"), disk, p, agent::AGENT_CMD, &next, w).expect("the start");
    let how = started.how.clone();
    let file = started.swap.current().to_path_buf();
    env.say(&format!("warm: the guest started {how:?}; the command is on {}", file.display()));
    let mut g = Guest { plan: p, child: started.child };
    assert_eq!(how, Start::Warm, "the saved state must restore");
    let o = settled(env, &mut g, &file, "tok-warm", COMMAND).expect("the result of the command after the restore");
    env.say(&format!("warm: code {}, stdout {:?}", o.code, text(&o.stdout)));
    assert_eq!(o.code, 0, "{o:?}");
    assert!(text(&o.stdout).contains("pod-guest-warm"), "{o:?}");
    env.say("RESULT: warm start: the restored agent took the command on a new medium after the load");
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}
