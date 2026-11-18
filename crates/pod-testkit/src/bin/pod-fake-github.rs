//! pod-fake-github: the fake GitHub of `pod_testkit::github` as a program
//! for the CI of a consumer. It prints its base URL on one line of stdout,
//! then serves until it is killed.

use std::io::Write;
use std::net::TcpListener;
use std::process::ExitCode;

use pod_testkit::github::FakeGithub;

const USAGE: &str = "\
Usage: pod-fake-github [--listen ADDRESS] [--hook URL]

Serve a fake GitHub for tests on one TCP address: an App, the JIT runners of
each repository, their runner broker, the workflow_job webhooks and a control
API under /_fake/. The first line of stdout is the base URL. Each credential
is a made-up value.

Options:
  --listen ADDRESS  Listen on ADDRESS (default 127.0.0.1:0, a free port).
  --hook URL        Send each webhook to URL (http:// only).
  -h, --help        Show this help.
  -V, --version     Show the version.

Exit status: 0 for the help and the version, 2 for a wrong option, 1 when the
address cannot be used.
";

fn main() -> ExitCode {
    let mut listen = "127.0.0.1:0".to_string();
    let mut hook = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value =
            |name: &str| args.next().ok_or_else(|| format!("{name}: the option needs a value; fix: give {name} VALUE"));
        let step = match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("pod-fake-github {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--listen" => value("--listen").map(|v| listen = v),
            "--hook" => value("--hook").map(|v| hook = Some(v)),
            other => Err(format!("{other}: the option is not known; fix: see pod-fake-github --help")),
        };
        if let Err(e) = step {
            eprintln!("pod-fake-github: {e}");
            return ExitCode::from(2);
        }
    }
    let fake = match TcpListener::bind(&listen).and_then(FakeGithub::start_on) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("pod-fake-github: --listen {listen}: {e}; fix: give a free address");
            return ExitCode::FAILURE;
        }
    };
    if let Some(url) = hook {
        fake.set_hook(&url);
    }
    let mut out = std::io::stdout();
    if writeln!(out, "{}", fake.base()).and_then(|()| out.flush()).is_err() {
        return ExitCode::FAILURE;
    }
    loop {
        std::thread::park();
    }
}
