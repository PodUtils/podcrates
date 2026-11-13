//! The live round trip of each file host of `pod-services` (T-528): 32 KiB
//! of random bytes up by the catalog's form, down by its form, compared,
//! and deleted where the host deletes. The stand-ins of
//! `tests/watches/file_host.rs` prove the code on each push; this run
//! proves the facts against the hosts. `live.yml` runs it on Thursdays and
//! by hand:
//!
//!     cargo test -p pod-services-watch --test file_host_live -- --ignored --nocapture
//!
//! `POD_FILE_HOSTS` names the hosts (ids, by commas); with none, each host
//! with `on-by-default` true. A host that is not on by default (0x0.st:
//! its terms refuse CI uploads) runs only when the variable names it. The
//! proxy comes from the environment (`HTTPS_PROXY`). No token is printed.

use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pod_proxy::resolve::System;
use pod_proxy::{Dialer, Environment, ProxyChoice, Settings};
use pod_services::Date;
use pod_services_watch::Net;
use pod_services_watch::file_host::trip;
use pod_tls::{Context, Trust, Words};

/// The bytes of each upload.
const PIECE: usize = 32 * 1024;
/// The bound of one host's trip.
const HOST_LIMIT: Duration = Duration::from_secs(180);

fn random(count: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; count];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).expect("random bytes");
    bytes
}

fn today() -> Date {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, m, d) = pod_core::clock::civil_from_days((secs / 86_400) as i64);
    Date::new(y as u16, m as u8, d as u8).expect("a day")
}

fn net() -> Net {
    let var = |name: &str| std::env::var_os(name);
    let settings = Settings::new("pod-services-watch", ProxyChoice::FromEnvironment(Environment::read(&var)));
    let provider = Arc::new(pod_tls::crypto::provider());
    let context = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("pod-services-watch"))));
    let dialer = Dialer::new(settings, Arc::new(System::new()));
    Net::new(context, Arc::new(dialer), Trust::Default, today())
}

#[tokio::test]
#[ignore = "live: uploads to the public file hosts; live.yml runs it"]
async fn a_file_goes_up_and_down_through_each_file_host() {
    let named: Vec<String> = std::env::var("POD_FILE_HOSTS")
        .unwrap_or_default()
        .split(',')
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    for id in &named {
        let service = pod_services::get(id).unwrap_or_else(|| panic!("no service has the id {id:?}"));
        assert_eq!(service.kind(), "file-host", "{id} is no file host");
    }
    let net = net();
    let (mut ran, mut failed) = (0, Vec::new());
    for service in pod_services::of_kind("file-host") {
        let id = service.id();
        let on = service.bool("on-by-default") == Some(true);
        if (named.is_empty() && !on) || (!named.is_empty() && !named.iter().any(|n| n == id)) {
            println!("{id}: skipped: not on by default, and POD_FILE_HOSTS does not name it");
            continue;
        }
        ran += 1;
        let bytes = random(PIECE);
        let boundary = format!("podcrates{}", random(12).iter().map(|b| format!("{b:02x}")).collect::<String>());
        let start = Instant::now();
        match tokio::time::timeout(HOST_LIMIT, trip(service, &net, &bytes, &boundary)).await {
            Ok(Ok(trip)) => {
                for line in &trip.lines {
                    println!("{id}: {line}");
                }
                println!("{id}: holds in {} ms", start.elapsed().as_millis());
            }
            Ok(Err(finding)) => {
                println!("{id}: FAILED: {finding:?}");
                failed.push(id);
            }
            Err(_) => {
                println!("{id}: FAILED: no end within {} s", HOST_LIMIT.as_secs());
                failed.push(id);
            }
        }
    }
    println!("file hosts: {ran} ran, {} failed", failed.len());
    assert!(ran > 0, "no file host ran");
    assert!(failed.is_empty(), "the file hosts that failed: {failed:?}");
}
