//! The lab of the clients of a job (T-973): a broker with the TLS end on a
//! TCP port of 127.0.0.1, the far end of `tls_common` with an answer that
//! names the `Authorization` it got, the bundle of the job in a directory
//! of the test, and a run of one client with a clean environment.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::TOKEN;
use crate::tls_common::{Answer, PH, TlsFar, load_secret, ok, policy, tls_broker};

pub mod far;
pub mod git;
pub mod registry;
pub mod sources;

/// The variable that makes a skip a failure.
const REQUIRED: &str = "POD_BROKER_TEST_CLIENTS";

/// The longest run of one client.
const RUN_MAX: Duration = Duration::from_secs(60);

/// Write the skip line, or fail when the run requires each client.
pub fn skip(why: &str) {
    if std::env::var(REQUIRED).as_deref() == Ok("required") {
        panic!("{REQUIRED}=required, and {why}");
    }
    pod_testkit::host::skip_line(why);
}

/// The program of a client: the value of `var` when it is set, else
/// `tool` when this host has it; `None` (after the skip line) when not.
pub fn need(var: &str, tool: &str) -> Option<String> {
    let program = std::env::var(var).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| tool.to_string());
    match pod_testkit::host::tool(&program) {
        Ok(()) => Some(program),
        Err(why) => {
            skip(&why);
            None
        }
    }
}

/// `Err(reason)` when `python` cannot import `module`.
pub fn python_imports(python: &str, module: &str) -> Result<(), String> {
    let status = Command::new(python)
        .args(["-I", "-c", &format!("import {module}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => Err(format!("{python} cannot import {module}")),
    }
}

/// `Err(reason)` when `node` has no `--use-env-proxy` (Node.js before
/// 22.21 and 24.5).
pub fn node_has_env_proxy(node: &str) -> Result<(), String> {
    let help = Command::new(node).arg("--help").stderr(Stdio::null()).output();
    match help {
        Ok(out) if String::from_utf8_lossy(&out.stdout).contains("--use-env-proxy") => Ok(()),
        _ => Err(format!("{node} has no --use-env-proxy")),
    }
}

/// The `MIX_HOME` that holds the archive of Hex: the value of
/// `POD_BROKER_TEST_MIX_HOME`, else of `MIX_HOME`, else `~/.mix`; a run of
/// a client has a `HOME` of its own.
pub fn mix_home_with_hex() -> Result<String, String> {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let home = var("POD_BROKER_TEST_MIX_HOME")
        .or_else(|| var("MIX_HOME"))
        .or_else(|| var("HOME").map(|home| format!("{home}/.mix")))
        .ok_or("no MIX_HOME and no HOME")?;
    let archives = std::fs::read_dir(Path::new(&home).join("archives")).map_err(|e| format!("{home}/archives: {e}"))?;
    let hex = archives.filter_map(Result::ok).any(|e| e.file_name().to_string_lossy().starts_with("hex-"));
    if hex { Ok(home) } else { Err(format!("{home}/archives holds no archive of Hex")) }
}

/// The far end of each client but git: 401 with a Basic challenge to a
/// request with no `Authorization`; else the value in the header `X-Seen`
/// and in the body (`echo=VALUE`).
pub fn echo_auth(_dir: &Path) -> Answer {
    Arc::new(|head: &str, _body: &[u8]| {
        let Some(auth) = far::auth_of(head) else { return far::unauthorized() };
        ok(&format!("X-Seen: {auth}\r\n"), format!("echo={auth}\n").as_bytes())
    })
}

/// The roots of a CA file of the system, as a host gives them to the
/// bundle; empty on a host with no such file.
fn system_roots() -> Vec<u8> {
    pod_tls::SYSTEM_BUNDLES
        .iter()
        .filter_map(|path| std::fs::read(path).ok())
        .find(|bytes| pod_tls::bundle::pem_certificates(bytes).is_ok_and(|c| !c.is_empty()))
        .unwrap_or_default()
}

/// What a run of a client trusts.
#[derive(Clone, Copy)]
pub enum Trust {
    /// No variable of the bundle.
    None,
    /// Each variable of `ca::env`, and `JAVA_TOOL_OPTIONS` of
    /// `ca::java_options`.
    All,
    /// Each of [`Trust::All`] but the variables named.
    Without(&'static [&'static str]),
}

/// One broker, its far end and the bundle of the job, for one client.
pub struct Lab {
    pub far: Arc<TlsFar>,
    pub dir: PathBuf,
    bundle: PathBuf,
    store: PathBuf,
    /// The URL of the broker as a proxy, with the job and its token.
    pub proxy: String,
}

impl Lab {
    /// Load `job1` with the planted secret, write its bundle, and serve
    /// the broker on a port of 127.0.0.1; the far end answers by `answer`
    /// of the directory of the lab.
    pub async fn start(name: &str, answer: impl FnOnce(&Path) -> Answer) -> Lab {
        let dir = std::env::temp_dir().join(format!("pod-broker-clients-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        let far = TlsFar::new(answer(&dir));
        let (broker, _audit) = tls_broker(far.clone());
        let ca = load_secret(&broker, &policy("", ""));
        // The roots of the host hold the CA of the far end: a host that no
        // credential names gets its tunnel as it is, and the job checks the
        // leaf of the far end with the roots of the bundle.
        let mut roots = system_roots();
        if !roots.is_empty() && !roots.ends_with(b"\n") {
            roots.push(b'\n');
        }
        roots.extend_from_slice(far.ca.pem().as_bytes());
        let bundle = dir.join("ca.pem");
        pod_broker::ca::write_bundle(&bundle, &ca, &roots).unwrap();
        let store = dir.join("ca.p12");
        pod_broker::ca::write_trust_store(&store, &ca, &roots).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(broker.serve_tcp(listener));
        Lab { far, dir, bundle, store, proxy: format!("http://job1:{TOKEN}@127.0.0.1:{port}") }
    }

    /// Run `program` with `args` and a clean environment: the proxy, the
    /// placeholder in `JOB_TOKEN`, `env`, and with `bundle` each variable
    /// of `ca::env` and `JAVA_TOOL_OPTIONS`.
    pub async fn run(&self, program: &str, args: &[&str], env: &[(&str, &str)], bundle: bool) -> Run {
        self.run_trusting(program, args, env, if bundle { Trust::All } else { Trust::None }).await
    }

    /// [`Lab::run`] with the trust `trust`.
    pub async fn run_trusting(&self, program: &str, args: &[&str], env: &[(&str, &str)], trust: Trust) -> Run {
        let mut command = Command::new(program);
        command.args(args).env_clear().current_dir(&self.dir).envs(env.iter().copied());
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        command
            .env("HOME", self.dir.join("home"))
            .env("HTTPS_PROXY", &self.proxy)
            .env("https_proxy", &self.proxy)
            .env("JOB_TOKEN", PH)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_TRACE_CURL", "1")
            .env("GIT_TRACE_CURL_NO_DATA", "1")
            .env("GIT_TRACE_REDACT", "0");
        let mut vars: Vec<(&str, std::ffi::OsString)> = Vec::new();
        if !matches!(trust, Trust::None) {
            vars.extend(pod_broker::ca::env(&self.bundle).into_iter().map(|(var, path)| (var, path.into())));
            let java = pod_broker::ca::java_options(&self.store, None).unwrap();
            vars.push(("JAVA_TOOL_OPTIONS", java.into()));
        }
        if let Trust::Without(left_out) = trust {
            vars.retain(|(var, _)| !left_out.contains(var));
        }
        command.envs(vars);
        let (out, err) = (self.dir.join("stdout"), self.dir.join("stderr"));
        command.stdin(Stdio::null());
        command.stdout(std::fs::File::create(&out).unwrap());
        command.stderr(std::fs::File::create(&err).unwrap());
        let child = command.spawn().unwrap_or_else(|e| panic!("{program}: {e}"));
        let code = tokio::task::spawn_blocking(move || wait(child)).await.unwrap();
        Run { code, stdout: read(&out), stderr: read(&err) }
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The exit code of `child`, or `None` after a kill at [`RUN_MAX`] or on a
/// signal.
fn wait(mut child: std::process::Child) -> Option<i32> {
    let end = Instant::now() + pod_testkit::host::scaled(RUN_MAX);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code();
        }
        if Instant::now() > end {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn read(path: &Path) -> String {
    String::from_utf8_lossy(&std::fs::read(path).unwrap_or_default()).into_owned()
}

/// The end of a client: its exit code and its output.
pub struct Run {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    pub fn all(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

impl fmt::Display for Run {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "exit {:?}\n--- stdout\n{}\n--- stderr\n{}", self.code, self.stdout, self.stderr)
    }
}
