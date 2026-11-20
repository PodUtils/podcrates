//! The control: `load`, `drop` and `list` as rows of the catalogue of
//! `pod-control`, on the Unix socket of mode 0600 and as single calls; the
//! tiers of the rows, the conformance of the server, `drain` and `stop`; the
//! end of a job at `drop`, at a new `load` and at its TTL, with each of its
//! open connections.

mod common;

use std::time::Duration;

use common::*;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;

fn load_params(job: &str, token: &str, ttl: u64) -> Value {
    json!({"job": job, "token": token, "ttl_s": ttl, "policy": {"schema": 1, "mode": "open"}})
}

#[cfg(unix)]
mod socket {
    use std::path::PathBuf;
    use std::sync::Arc;

    use pod_broker::Broker;
    use pod_broker::control::{ControlServer, Ending, MAX_LINE};
    use pod_control::{Client, ClientSettings, Code, Error, Tier, conformance};
    use tokio::runtime::Runtime;

    use super::*;

    /// A made-up master token of the control.
    const MASTER: &str = "made-up-master-token-of-the-control";

    struct Served {
        rt: Runtime,
        server: ControlServer,
        path: PathBuf,
        dir: PathBuf,
        broker: Arc<Broker>,
        audit: Arc<Seen>,
    }

    impl Served {
        fn client(&self) -> Client {
            Client::connect_unix(&self.path, ClientSettings::default()).unwrap()
        }

        fn status(&self, request: &str) -> u16 {
            self.rt.block_on(status(&self.broker, request))
        }
    }

    impl Drop for Served {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A broker and its control at `DIR/control.sock`: a peer of the user of
    /// the broker has `tier`, and `auth` with [`MASTER`] gives the master tier.
    fn serve(name: &str, tier: Tier) -> Served {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let dir = pod_testkit::host::socket_dir(&format!("pod-broker-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control.sock");
        let (broker, audit) = broker(Far::new(Kind::Echo), Answers::public());
        let mut settings = pod_broker::control::settings("pod-broker-test", "1.0.0");
        settings.same_user = tier;
        settings.master = Some(pod_control::Token::new(MASTER));
        let server = {
            let _entered = rt.enter();
            pod_broker::control::serve_control(broker.clone(), &path, settings).unwrap()
        };
        Served { rt, server, path, dir, broker, audit }
    }

    fn remote(e: Error) -> pod_control::Fault {
        match e {
            Error::Remote(fault) => fault,
            other => panic!("not an answer of the server: {other}"),
        }
    }

    #[test]
    fn the_control_socket_loads_lists_and_drops_jobs() {
        use std::os::unix::fs::PermissionsExt;
        let s = serve("control", Tier::Operate);
        assert_eq!(std::fs::metadata(&s.path).unwrap().permissions().mode() & 0o777, 0o600);
        let mut c = s.client();
        assert_eq!(c.auth(&pod_control::Token::new(MASTER)).unwrap(), Tier::Master);
        let loaded = c.call("load", load_params("job1", TOKEN, 600)).unwrap();
        assert_eq!(loaded, json!({"job":"job1","mode":"open","rules":0,"expires_in_s":600}));
        assert!(!loaded.to_string().contains(TOKEN));
        assert_eq!(s.status(&connect("a.example:443")), 200);
        let listed = c.call("list", json!({})).unwrap();
        assert_eq!(
            (listed["jobs"][0]["job"].as_str(), listed["jobs"][0]["mode"].as_str()),
            (Some("job1"), Some("open"))
        );
        assert_eq!(c.call("drop", json!({"job": "job1"})).unwrap(), json!({"job": "job1", "dropped": true}));
        assert_eq!(c.call("drop", json!({"job": "job1"})).unwrap()["dropped"], false);
        assert_eq!(c.call("list", json!({})).unwrap(), json!({"jobs": []}));
        assert_eq!(s.status(&connect("a.example:443")), 407);
        // A bad request gets bad-params (the class usage), with no byte of the
        // token, and the connection goes on.
        for bad in [
            json!({"job": "job1"}),
            json!({"job": "job1", "token": TOKEN, "ttl_s": 600, "policy": {"schema": 1, "mode": "open"}, "x": 1}),
            load_params("job 1", TOKEN, 600),
            load_params("job1", "short", 600),
            load_params("job1", TOKEN, 0),
            load_params("job1", TOKEN, 99_999_999),
            json!({"job":"j","token":TOKEN,"ttl_s":"600","policy":{"schema":1,"mode":"open"}}),
            json!({"job":"j","token":TOKEN,"ttl_s":60,"policy":{"schema":9,"mode":"open"}}),
        ] {
            let fault = remote(c.call("load", bad.clone()).unwrap_err());
            assert_eq!((fault.code, fault.code.class()), (Code::BadParams, pod_core::Class::Usage), "{bad}");
            assert!(!fault.detail.contains(TOKEN), "{bad}: {fault}");
        }
        assert_eq!(remote(c.call("drop", json!({"jobs": "job1"})).unwrap_err()).code, Code::BadParams);
        assert_eq!(remote(c.call("list", json!({"all": true})).unwrap_err()).code, Code::BadParams);
        assert_eq!(remote(c.call("halt", json!({})).unwrap_err()).code, Code::UnknownMethod);
        let line = c.raw(br#"{"op":"list"}"#).unwrap().unwrap();
        assert_eq!(line["error"]["code"], "bad-request");
        assert!(c.version().is_ok(), "the connection goes on");
        assert!(!s.audit.lines().iter().any(|l| l.to_json().contains(TOKEN)));
    }

    /// `load` has the master tier and `drop` the operate tier: above the tier
    /// of the caller each answers as a name that is not there, word for word,
    /// before its params are read; `methods` does not list it.
    #[test]
    fn load_takes_the_master_tier_and_drop_the_operate_tier() {
        let unknown = |c: &mut Client, method: &str| {
            let line = json!({"schema": 1, "id": 9, "method": method, "params": {"bogus": 1}}).to_string();
            c.raw(line.as_bytes()).unwrap().unwrap()["error"].clone()
        };
        let listed = |c: &mut Client| -> Vec<(String, String)> {
            let rows = c.methods().unwrap().into_iter().filter(|m| ["load", "drop", "list"].contains(&m.name.as_str()));
            rows.map(|m| (m.name, m.tier)).collect()
        };
        let row = |n: &str, t: &str| (n.to_string(), t.to_string());
        let s = serve("tier-read", Tier::Read);
        let mut c = s.client();
        let none = unknown(&mut c, "no-such-method");
        assert_eq!(none, json!({"code": "unknown-method", "detail": "no such method"}));
        assert_eq!((unknown(&mut c, "load"), unknown(&mut c, "drop")), (none.clone(), none.clone()));
        assert_eq!(listed(&mut c), [row("list", "read")]);
        assert_eq!(c.call("list", json!({})).unwrap(), json!({"jobs": []}));
        drop(s);

        let s = serve("tier-operate", Tier::Operate);
        let mut c = s.client();
        assert_eq!(unknown(&mut c, "load"), none);
        assert_eq!(listed(&mut c), [row("drop", "operate"), row("list", "read")]);
        assert_eq!(c.call("drop", json!({"job": "job1"})).unwrap()["dropped"], false);
        assert_eq!(remote(c.call("load", load_params("job1", TOKEN, 60)).unwrap_err()).code, Code::UnknownMethod);
        assert!(s.broker.list().is_empty());
        c.auth(&pod_control::Token::new(MASTER)).unwrap();
        assert_eq!(listed(&mut c), [row("load", "master"), row("drop", "operate"), row("list", "read")]);
        assert_eq!(c.call("load", load_params("job1", TOKEN, 60)).unwrap()["job"], "job1");

        let rows = pod_broker::control::extras();
        let load = rows.iter().find(|e| e.name == "load").unwrap();
        assert_eq!((load.tier, load.outlives), (Tier::Master, true));
    }

    /// `pod_control::conformance::check` finds no fault in the control of the
    /// broker at each tier, and changes no job.
    #[test]
    fn the_control_of_the_broker_conforms_at_each_tier() {
        let extras: Vec<String> = pod_broker::control::extras().into_iter().map(|e| e.name).collect();
        for tier in Tier::ALL {
            let s = serve(&format!("conform-{}", tier.name()), tier);
            let mut connect = || Client::connect_unix(&s.path, ClientSettings::default());
            let options = conformance::Options { line_max: MAX_LINE, extras: extras.clone() };
            let found = conformance::check(&mut connect, &options);
            assert!(found.is_empty(), "{tier}: {found:#?}");
            assert!(s.broker.list().is_empty() && s.audit.lines().is_empty(), "{tier}");
        }
    }

    /// `drain` takes no new `load` and keeps each job; `stop` ends each job.
    /// The host reads each in `ending`; `status` and `metrics` show the jobs
    /// and the counts of the audit lines.
    #[test]
    fn drain_takes_no_new_load_and_stop_drops_each_job() {
        let s = serve("ending", Tier::Master);
        let mut c = s.client();
        let ending = s.server.ending();
        c.call("load", load_params("job1", TOKEN, 600)).unwrap();
        let status = c.status().unwrap();
        assert_eq!((status["jobs"][0]["job"].as_str(), &status["ending"]), (Some("job1"), &Value::Null));
        assert_eq!(c.drain(pod_control::Drain { exit: false }).unwrap()["jobs"], 1);
        assert_eq!(*ending.borrow(), Some(Ending::Drain { exit: false }));
        assert_eq!(c.status().unwrap()["ending"], "drain");
        assert_eq!(remote(c.call("load", load_params("job2", TOKEN_TWO, 600)).unwrap_err()).code, Code::Busy);
        assert_eq!(s.status(&connect("a.example:443")), 200);
        assert_eq!(c.stop().unwrap(), json!({"stopping": true, "dropped": 1}));
        assert_eq!(*ending.borrow(), Some(Ending::Stop));
        assert_eq!(s.status(&connect("a.example:443")), 407);
        s.rt.block_on(s.audit.connection("allow"));
        let metrics = c.metrics().unwrap();
        assert_eq!((metrics.counters.get("job_load"), metrics.counters.get("job_drop")), (Some(&1), Some(&1)));
        assert_eq!(metrics.counters.get("connection_allow"), Some(&1));
        assert_eq!(metrics.gauges.get("jobs"), Some(&0));
        assert_eq!(remote(c.reload().unwrap_err()).code, Code::Unsupported);
    }

    #[test]
    fn a_socket_path_that_is_another_file_is_refused() {
        // A socket path holds at most 107 bytes: a long temporary directory
        // gives way to /tmp.
        let dir = pod_testkit::host::socket_dir("pod-broker-bind");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file");
        std::fs::write(&path, "x").unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let _guard = runtime.enter();
        assert!(pod_broker::bind_unix(&path, 0o600).is_err());
        let (broker, _) = broker(Far::new(Kind::Echo), Answers::public());
        let settings = pod_broker::control::settings("pod-broker-test", "1.0.0");
        let refused = pod_broker::control::serve_control(broker.clone(), &path, settings.clone());
        assert_eq!(refused.err().map(|e| e.kind()), Some(std::io::ErrorKind::AlreadyExists));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "x");
        // A socket that is there is taken away and bound again.
        let socket = dir.join("s.sock");
        drop(pod_broker::bind_unix(&socket, 0o600).unwrap());
        assert!(pod_broker::bind_unix(&socket, 0o600).is_ok());
        // The control keeps a live socket: a second control is AddrInUse.
        let control = dir.join("c.sock");
        let _live = pod_broker::control::serve_control(broker.clone(), &control, settings.clone()).unwrap();
        let second = pod_broker::control::serve_control(broker, &control, settings);
        assert_eq!(second.err().map(|e| e.kind()), Some(std::io::ErrorKind::AddrInUse));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// A token is the token of one job.
#[tokio::test]
async fn a_token_of_another_job_is_refused_and_a_reload_replaces_the_job() {
    let (broker, _) = broker(Far::new(Kind::Echo), Answers::public());
    assert!(broker.control("load", &load_params("job1", TOKEN, 600)).is_ok());
    let taken = broker.control("load", &load_params("job2", TOKEN, 600)).unwrap_err();
    assert_eq!(taken.code, pod_control::Code::BadParams);
    // A new load of job1 with a new token: the old token is gone.
    assert!(broker.control("load", &load_params("job1", TOKEN_TWO, 600)).is_ok());
    assert_eq!(status(&broker, &connect("a.example:443")).await, 407);
    let bearer = format!("CONNECT a.example:443 HTTP/1.1\r\nProxy-Authorization: Bearer {TOKEN_TWO}\r\n\r\n");
    assert_eq!(status(&broker, &bearer).await, 200);
}

/// `drop` ends each open connection of the job, and its state is gone.
#[tokio::test]
async fn a_dropped_job_ends_its_open_connection() {
    let (broker, audit) = broker(Far::new(Kind::Echo), Answers::public());
    load(&broker, "job1", TOKEN, OPEN);
    let (head, mut tunnel) = ask(&broker, &connect("a.example:443")).await;
    assert_eq!(status_of(&head), 200);
    assert_eq!(round_trip(&mut tunnel, b"before").await, b"before");
    assert_eq!(broker.list()[0].open, 1);
    assert_eq!(broker.control("drop", &json!({"job": "job1"})).unwrap()["dropped"], true);
    let mut rest = Vec::new();
    let read =
        tokio::time::timeout(pod_testkit::host::scaled(Duration::from_secs(10)), tunnel.read_to_end(&mut rest)).await;
    assert!(matches!(read, Ok(Ok(0))), "the tunnel ends: {read:?}");
    let line = audit.connection("allow").await;
    assert_eq!((line.fields["end"].as_str(), line.class), (Some("job-ended"), Some("revoked")));
    assert_eq!(line.fields["up"], 6);
    assert!(broker.list().is_empty());
    assert_eq!(status(&broker, &connect("a.example:443")).await, 407);
    let drop = audit.wait(|l| l.event == "broker.job.drop").await;
    assert_eq!((drop.fields["job"].as_str(), drop.fields["dropped"].as_bool()), (Some("job1"), Some(true)));
}

/// At its TTL a job ends with each open connection, and its token is gone.
#[tokio::test(start_paused = true)]
async fn a_job_ends_at_its_ttl_with_its_open_connection() {
    let (broker, audit) = broker(Far::new(Kind::Echo), Answers::public());
    assert_eq!(broker.control("load", &load_params("job1", TOKEN, 5)).unwrap()["expires_in_s"], 5);
    let (head, mut tunnel) = ask(&broker, &connect("a.example:443")).await;
    assert_eq!(status_of(&head), 200);
    let started = tokio::time::Instant::now();
    let mut rest = Vec::new();
    assert_eq!(tunnel.read_to_end(&mut rest).await.unwrap(), 0);
    assert!(started.elapsed() >= Duration::from_secs(4), "{:?}", started.elapsed());
    let line = audit.connection("allow").await;
    assert_eq!(line.fields["end"], "job-ended");
    assert!(broker.list().is_empty());
    assert_eq!(status(&broker, &connect("a.example:443")).await, 407);
}

/// A job that expired is gone also with no timer: a lookup drops it.
#[test]
fn a_job_expires_at_a_lookup_with_no_runtime_timer() {
    let (broker, _) = broker(Far::new(Kind::Echo), Answers::public());
    // No runtime yet: the load starts no timer.
    assert!(broker.control("load", &load_params("job1", TOKEN, 2)).is_ok());
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().start_paused(true).build().unwrap();
    runtime.block_on(async {
        assert_eq!(broker.list().len(), 1);
        tokio::time::advance(Duration::from_secs(3)).await;
        assert!(broker.list().is_empty());
        assert_eq!(status(&broker, &connect("a.example:443")).await, 407);
    });
}

/// The worker of [`a_socket_has_its_mode_under_each_umask`]: it runs only
/// in the process that test starts (the variable names the umask). A
/// thread reads the mode of the path while the worker binds it again and
/// again: a bind at the path shows the mode of the umask until its chmod.
/// The worker prints each other mode that the thread saw, the mode of the
/// last bind and the names that the directory holds.
#[cfg(unix)]
#[test]
fn bind_worker_under_a_umask() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    let Some(mask) = std::env::var_os("POD_BROKER_TEST_UMASK") else { return };
    let dir = std::path::PathBuf::from(std::env::var_os("POD_BROKER_TEST_DIR").unwrap());
    let mask = u32::from_str_radix(&mask.to_string_lossy(), 8).unwrap();
    // SAFETY: umask takes a mode and touches no memory.
    unsafe { libc::umask(mask as libc::mode_t) };
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let _guard = runtime.enter();
    let path = dir.join("b.sock");
    let done = AtomicBool::new(false);
    let odd = std::thread::scope(|s| {
        let watcher = s.spawn(|| {
            let mut odd = std::collections::BTreeSet::new();
            while !done.load(Ordering::Relaxed) {
                if let Ok(meta) = std::fs::symlink_metadata(&path) {
                    let mode = meta.permissions().mode() & 0o777;
                    if mode != 0o600 {
                        odd.insert(format!("{mode:o}"));
                    }
                }
            }
            odd
        });
        for _ in 0..200 {
            drop(pod_broker::bind_unix(&path, 0o600).unwrap());
        }
        done.store(true, Ordering::Relaxed);
        watcher.join().unwrap()
    });
    let _listener = pod_broker::bind_unix(&path, 0o600).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    let mut names: Vec<String> =
        std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    let odd: Vec<String> = odd.into_iter().collect();
    println!("odd=[{}] mode={mode:o} names={}", odd.join(","), names.join(","));
}

/// Under the umasks 000, 002 and 022 the socket has the mode of the bind
/// from the first instant that the path holds it, and no directory of the
/// stage stays (T-196, PodUtils/podnet#444). Each umask runs in a new
/// process: the umask is of the process.
#[cfg(unix)]
#[test]
fn a_socket_has_its_mode_under_each_umask() {
    let me = std::env::current_exe().unwrap();
    for mask in ["000", "002", "022"] {
        let dir = pod_testkit::host::socket_dir(&format!("pod-broker-umask-{mask}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = std::process::Command::new(&me)
            .args(["--exact", "bind_worker_under_a_umask", "--nocapture", "--test-threads", "1"])
            .env("POD_BROKER_TEST_UMASK", mask)
            .env("POD_BROKER_TEST_DIR", &dir)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "umask {mask}: {text}\n{}", String::from_utf8_lossy(&out.stderr));
        assert!(text.contains("odd=[] mode=600 names=b.sock\n"), "umask {mask}: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
