//! The harness of the two TPMs in software of other parties (call C151):
//! swtpm (libtpms) on a Unix socket, and the simulator of the reference
//! implementation (`ms-tpm-20-ref`) on 127.0.0.1. Each test starts its own,
//! with a state of its own in a directory of its own, so a test that
//! extends a PCR changes no other test. They are C programs of the tests:
//! no crate links them (call C927).
//!
//! swtpm is the program `POD_TPM_TEST_SWTPM_BIN`, else `swtpm` on the
//! path; the simulator is `POD_TPM_TEST_MSSIM_BIN`, else `tpm2-simulator`
//! on the path. A host with none skips each test with the reason, unless
//! `POD_TPM_TEST_SWTPM=required` or `POD_TPM_TEST_MSSIM=required`, which
//! `scripts/ci/tpm-tests.sh` sets with the pinned programs.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use pod_tpm::Transport;
use pod_tpm::transport::{Framing, Limits, Socket};

/// The bound of the start of a simulator.
const START: Duration = Duration::from_secs(20);

/// `TPM2_Startup(TPM_SU_CLEAR)`: not a command of the crate; the harness
/// sends it to the simulator of the reference after its power on.
const STARTUP_CLEAR: [u8; 12] = [0x80, 0x01, 0, 0, 0, 0x0C, 0, 0, 0x01, 0x44, 0, 0];

/// The variable that makes a skip a failure, and that names the program.
fn required(var: &str) -> bool {
    std::env::var(var).is_ok_and(|v| v == "required")
}

/// The program of `var`_BIN, else `name` on the path.
fn program(var: &str, name: &str) -> Option<PathBuf> {
    if let Some(named) = std::env::var_os(format!("{var}_BIN")).filter(|n| !n.is_empty()) {
        return Some(PathBuf::from(named)).filter(|p| p.is_file());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(name)).find(|p| p.is_file())
}

/// The program, or `None` with the line of the skip; a skip under
/// `var=required` is a failure.
fn found(var: &str, name: &str) -> Option<PathBuf> {
    let found = program(var, name);
    if found.is_none() {
        let why = format!("this host has no {name} (set {var}_BIN, or put {name} on the path)");
        assert!(!required(var), "{var}=required: {why}");
        pod_testkit::host::skip_line(&why);
    }
    found
}

/// A directory of this test alone, short enough for a socket path.
fn scratch(tag: &str) -> PathBuf {
    let dir = pod_testkit::host::socket_dir(&format!("pod-tpm-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("state")).expect("a directory");
    dir
}

/// A simulator that runs: killed, and its directory removed, on drop.
pub struct Simulator {
    child: Child,
    dir: PathBuf,
    /// The program and its version, for the lines of a test.
    pub name: String,
}

impl Drop for Simulator {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Simulator {
    /// The last lines of the log of the simulator, for a failure.
    pub fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("log")).unwrap_or_default()
    }
}

/// swtpm with a new state, started (`startup-clear`), and a transport to
/// its command channel (`Framing::Raw`). `None` when this host has no
/// swtpm.
pub fn swtpm(tag: &str) -> Option<(Simulator, Socket)> {
    let program = found("POD_TPM_TEST_SWTPM", "swtpm")?;
    let dir = scratch(tag);
    let socket = dir.join("tpm.sock");
    let child = Command::new(&program)
        .args(["socket", "--tpm2", "--flags", "not-need-init,startup-clear"])
        .arg("--server")
        .arg(format!("type=unixio,path={}", socket.display()))
        .arg("--tpmstate")
        .arg(format!("dir={}", dir.join("state").display()))
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(dir.join("log")).unwrap())
        .stderr(Stdio::from(std::fs::File::options().append(true).open(dir.join("log")).unwrap()))
        .spawn()
        .expect("swtpm starts");
    let mut sim = Simulator { child, dir, name: version(&program) };
    let transport = wait_for(&mut sim, || Socket::connect_unix(&socket, Limits::default()).ok());
    Some((sim, transport))
}

/// The first line of `program --version`.
fn version(program: &Path) -> String {
    let out = Command::new(program).arg("--version").output().map(|o| o.stdout).unwrap_or_default();
    String::from_utf8_lossy(&out).lines().next().unwrap_or("swtpm").trim().to_string()
}

/// Try `connect` until it gives a transport, while the simulator runs, for
/// [`START`] at most.
fn wait_for<T>(sim: &mut Simulator, mut connect: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + pod_testkit::host::scaled(START);
    loop {
        if let Some(found) = connect() {
            return found;
        }
        if let Ok(Some(status)) = sim.child.try_wait() {
            panic!("the simulator ended ({status}): {}", sim.log());
        }
        assert!(Instant::now() < deadline, "the simulator did not listen in time: {}", sim.log());
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The simulator of the reference, manufactured new on a free port of
/// 127.0.0.1 (the command port, and the platform port after it), powered
/// on and started, with a transport to its command channel
/// (`Framing::Mssim`). `None` when this host has no simulator.
pub fn mssim(tag: &str) -> Option<(Simulator, Socket)> {
    let program = found("POD_TPM_TEST_MSSIM", "tpm2-simulator")?;
    let dir = scratch(tag);
    let (child, port) = pod_testkit::listener::start(pod_testkit::host::scaled(START), |port| {
        Command::new(&program)
            .arg("-m")
            .arg(port.to_string())
            .current_dir(dir.join("state"))
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(dir.join("log")).unwrap())
            .stderr(Stdio::null())
            .spawn()
            .expect("the simulator starts")
    })
    .expect("the simulator listens on 127.0.0.1");
    let mut sim = Simulator { child, dir, name: "ms-tpm-20-ref".into() };
    let platform = SocketAddr::from((Ipv4Addr::LOCALHOST, port + 1));
    let mut signals = wait_for(&mut sim, || TcpStream::connect(platform).ok());
    // TPM_SIGNAL_POWER_ON (1), then TPM_SIGNAL_NV_ON (11): each is
    // acknowledged with a u32 of 0.
    for signal in [1u32, 11] {
        signals.write_all(&signal.to_be_bytes()).unwrap();
        let mut ack = [0u8; 4];
        signals.read_exact(&mut ack).unwrap();
        assert_eq!(ack, [0; 4], "the simulator refused the signal {signal}");
    }
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut transport = Socket::connect_tcp(addr, Limits::default()).unwrap().with_framing(Framing::Mssim);
    let answer = transport.submit(&STARTUP_CLEAR).unwrap();
    assert_eq!(answer[6..10], [0; 4], "TPM2_Startup: {answer:02x?}");
    Some((sim, transport))
}
