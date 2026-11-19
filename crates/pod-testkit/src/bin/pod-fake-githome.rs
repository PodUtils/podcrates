//! pod-fake-githome: the fake of githome's source of jobs of
//! `pod_testkit::githome` as a program for the CI of podrunner. `serve`
//! prints its base URL on one line of stdout, then serves until it is
//! killed; `source` speaks the source wire on stdin and stdout.

use std::io::Write;
use std::net::TcpListener;
use std::process::ExitCode;

use pod_testkit::githome::{FakeGithome, SERVER_VAR, serve_lines};

const USAGE: &str = "\
Usage: pod-fake-githome serve [--listen ADDRESS] [--github URL]
       pod-fake-githome source [--server URL]

Serve a fake of the source of jobs of githome for the tests of podrunner.

Commands:
  serve   Keep the state of the fake on one TCP address: the queued jobs, the
          claims, the credentials, the reports and the refusals, with a
          control API under /_fake/. The first line of stdout is the base URL.
          With --github, each queued job is also queued in that fake GitHub,
          and mint gives the files of a JIT runner there that gets the job.
  source  Answer the source wire of podrunner on stdin and stdout, one request
          line at a time, through the serve process. Use it as the program of
          a [[source]] table of the kind program.

Options:
  --listen ADDRESS  Listen on ADDRESS (default 127.0.0.1:0, a free port).
  --github URL      The base URL of a pod-fake-github (http:// only). Without
                    it, mint gives made-up files that reach no broker.
  --server URL      The base URL of the serve process (default: the value of
                    the environment variable POD_FAKE_GITHOME).
  -h, --help        Show this help.
  -V, --version     Show the version.

Each credential is a made-up value. Exit status: 0 for the help, the version
and the end of stdin, 2 for a wrong command or option, 1 when the address
cannot be used or stdout fails.
";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let mut listen = "127.0.0.1:0".to_string();
    let mut github = None;
    let mut server = std::env::var(SERVER_VAR).ok();
    while let Some(arg) = args.next() {
        let mut value =
            |name: &str| args.next().ok_or_else(|| format!("{name}: the option needs a value; fix: give {name} VALUE"));
        let step = match (command.as_str(), arg.as_str()) {
            ("serve", "--listen") => value("--listen").map(|v| listen = v),
            ("serve", "--github") => value("--github").and_then(|v| {
                if !v.starts_with("http://") {
                    return Err(format!(
                        "--github {v}: the URL is not http://; fix: give the base URL of pod-fake-github"
                    ));
                }
                github = Some(v);
                Ok(())
            }),
            ("source", "--server") => value("--server").map(|v| server = Some(v)),
            (_, other) => Err(format!("{other}: the option is not known; fix: see pod-fake-githome --help")),
        };
        if let Err(e) = step {
            return wrong(&e);
        }
    }
    match command.as_str() {
        "-h" | "--help" => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        "-V" | "--version" => {
            println!("pod-fake-githome {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "serve" => serve(&listen, github.as_deref()),
        "source" => match server {
            Some(base) => source(&base),
            None => wrong(&format!("source: no server; fix: set {SERVER_VAR} or give --server URL")),
        },
        "" => wrong("no command; fix: give serve or source"),
        other => wrong(&format!("{other}: the command is not known; fix: see pod-fake-githome --help")),
    }
}

fn wrong(e: &str) -> ExitCode {
    eprintln!("pod-fake-githome: {e}");
    ExitCode::from(2)
}

fn serve(listen: &str, github: Option<&str>) -> ExitCode {
    let fake = match TcpListener::bind(listen).and_then(FakeGithome::start_on) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("pod-fake-githome: --listen {listen}: {e}; fix: give a free address");
            return ExitCode::FAILURE;
        }
    };
    if let Some(url) = github {
        fake.link_github(url);
    }
    let mut out = std::io::stdout();
    if writeln!(out, "{}", fake.base()).and_then(|()| out.flush()).is_err() {
        return ExitCode::FAILURE;
    }
    loop {
        std::thread::park();
    }
}

fn source(base: &str) -> ExitCode {
    // The base names a directory: each route goes after its slash.
    let base = if base.ends_with('/') { base.to_string() } else { format!("{base}/") };
    match serve_lines(&base, std::io::stdin().lock(), std::io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pod-fake-githome: source: {e}");
            ExitCode::FAILURE
        }
    }
}
