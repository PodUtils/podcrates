//! The runner of the watches: each `[[watch]]` of each service of
//! `pod-services` (or of the services that `--service` names), one line
//! for each, then a count. The workflow `services.yml` of podcrates runs it
//! each day.
//!
//!   pod-services-watch [--service ID]... [--kind KIND]... [--skip-kind KIND]...
//!                      [--file PATH]...
//!
//! `--skip-kind` leaves out the watches of a kind: a run by hand of each
//! watch opens no second session with a relay on one day (`forward-url`).
//! `--file` lays a service file of the same schema over the built-in set
//! (a test of a new file before its commit). The proxy comes from the
//! variables of the environment (`HTTPS_PROXY`, `NO_PROXY`, ...), and
//! `GITHUB_TOKEN`, when set, goes to GitHub's API alone.
//!
//! Exit 0: each watch holds or is skipped. Exit 1: a watch found a fact
//! that moved or failed. Exit 2: a fault of the command line or of a file.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pod_proxy::resolve::System;
use pod_proxy::{Dialer, Environment, ProxyChoice, Settings};
use pod_services::{Catalog, Date};
use pod_services_watch::Net;
use pod_tls::{Context, Trust, Words};

/// The bound of one watch, the largest download in it too.
const WATCH_LIMIT: Duration = Duration::from_secs(900);

struct Options {
    services: Vec<String>,
    kinds: Vec<String>,
    skip_kinds: Vec<String>,
    files: Vec<String>,
}

fn options() -> Result<Options, String> {
    let mut out = Options { services: Vec::new(), kinds: Vec::new(), skip_kinds: Vec::new(), files: Vec::new() };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let list = match arg.as_str() {
            "--service" => &mut out.services,
            "--kind" => &mut out.kinds,
            "--skip-kind" => &mut out.skip_kinds,
            "--file" => &mut out.files,
            "-h" | "--help" => {
                return Err("usage: pod-services-watch [--service ID]... [--kind KIND]... [--skip-kind KIND]... \
                            [--file PATH]..."
                    .into());
            }
            other => return Err(format!("pod-services-watch: unknown argument {other:?}")),
        };
        list.push(args.next().ok_or_else(|| format!("pod-services-watch: {arg} takes a value"))?);
    }
    Ok(out)
}

/// The day of the run, in UTC.
fn today() -> Date {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, m, d) = pod_core::clock::civil_from_days((secs / 86_400) as i64);
    Date::new(y as u16, m as u8, d as u8).unwrap_or_else(|| Date::new(1970, 1, 1).expect("a day"))
}

/// True when the host has a route for IPv6: the connect of a UDP socket
/// sends nothing, and fails with no route.
fn ipv6_route() -> bool {
    let probe = std::net::UdpSocket::bind("[::]:0");
    probe.and_then(|s| s.connect("[2001:4860:4860::8888]:53")).is_ok()
}

fn main() -> ExitCode {
    let options = match options() {
        Ok(options) => options,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::from(2);
        }
    };
    let mut services = Catalog::new();
    for path in &options.files {
        let laid = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| services.lay(path, &t).map_err(|e| e.to_string()));
        if let Err(why) = laid {
            eprintln!("pod-services-watch: {path}: {why}");
            return ExitCode::from(2);
        }
    }
    for id in &options.services {
        if services.get(id).is_none() {
            eprintln!("pod-services-watch: no service has the id {id:?}");
            return ExitCode::from(2);
        }
    }
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("pod-services-watch: the runtime: {e}");
            return ExitCode::from(2);
        }
    };
    let var = |name: &str| std::env::var_os(name);
    let settings = Settings::new("pod-services-watch", ProxyChoice::FromEnvironment(Environment::read(&var)));
    let provider = Arc::new(pod_tls::crypto::provider());
    let context = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("pod-services-watch"))));
    let (mut holds, mut skipped, mut found) = (0, 0, 0);
    runtime.block_on(async {
        let dialer = Dialer::new(settings, Arc::new(System::new()));
        let mut net = Net::new(context, Arc::new(dialer), Trust::Default, today());
        net.github_token = std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.trim().is_empty());
        net.ipv6 = ipv6_route();
        net.services = services.clone();
        for service in services.iter() {
            if !options.services.is_empty() && !options.services.iter().any(|id| id == service.id()) {
                continue;
            }
            for watch in service.watches() {
                let named = options.kinds.is_empty() || options.kinds.iter().any(|k| k == watch.kind());
                if !named || options.skip_kinds.iter().any(|k| k == watch.kind()) {
                    continue;
                }
                let report =
                    match tokio::time::timeout(WATCH_LIMIT, pod_services_watch::run(service, watch, &net)).await {
                        Ok(report) => report,
                        Err(_) => {
                            let outcome = pod_services_watch::Finding::failed(format!("no end within {WATCH_LIMIT:?}"));
                            pod_services_watch::Report::new(service, watch, outcome.into())
                        }
                    };
                for line in report.lines() {
                    println!("{line}");
                }
                match (report.holds(), report.outcome.skipped.is_some()) {
                    (true, true) => skipped += 1,
                    (true, false) => holds += 1,
                    (false, _) => found += 1,
                }
            }
        }
    });
    println!("pod-services-watch: {holds} hold, {skipped} skipped, {found} moved or failed");
    if found > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS }
}
