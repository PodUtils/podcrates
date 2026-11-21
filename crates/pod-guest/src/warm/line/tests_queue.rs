//! The save and the load against a stand-in of the monitor of QEMU: one
//! client at a time, and a queue of one more connection (`listen` with a
//! backlog of 1). The rows `Late` and `Hangs` are the rows of podvm's
//! stand-in (PodUtils/podvm#633). Here they act at the first
//! `query-migrate` after the `migrate`: [`freeze`] also asks
//! `query-migrate` before the stop.

use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

use super::tests::{Log, names, notes, print, stream_of};
use super::tests_late::LATE;
use super::*;
use crate::qmp::tests::queue::{fill, qemu_listener, within};
use crate::warm::tests::dir;

/// The time limit of a save or a load of a test: much longer than each
/// bound of the call, so that only a wait with no bound reaches it.
const LIMIT: Duration = Duration::from_secs(60);
/// The bound of a save that does not end well, and of a load.
const SHORT: Duration = Duration::from_secs(1);
/// The greeting of the monitor.
const GREETING: &[u8] = b"{\"QMP\": {\"version\": {}}}\n";

/// What a stand-in of the monitor of QEMU does.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Peer {
    /// The first `query-migrate` after the `migrate`, and the first
    /// `query-status`, answer after [`LATE`] on the same connection. The
    /// next client is taken when the one before closed.
    Late,
    /// At the first `query-migrate` after the `migrate`, the emulator
    /// answers nothing, takes no client, and its queue is full.
    Hangs,
    /// The emulator takes no client, and its queue is full from the start.
    Full,
}

/// A stand-in of the monitor, and what it saw.
struct Stand {
    /// The name of each request of each client, in order.
    seen: Log,
    /// The number of clients that it took.
    clients: Arc<AtomicUsize>,
    /// The drop of the stand-in lets a monitor that hangs go.
    _release: mpsc::Sender<()>,
}

impl Stand {
    fn clients(&self) -> usize {
        self.clients.load(Ordering::SeqCst)
    }
}

/// A stand-in of the monitor on `sock` that answers as `peer` says.
fn monitor(sock: &Path, peer: Peer) -> Stand {
    let l = qemu_listener(sock);
    let own = sock.to_path_buf();
    let (seen, clients): (Log, _) = (Log::default(), Arc::new(AtomicUsize::new(0)));
    let (release, held) = mpsc::channel::<()>();
    let (log, count) = (seen.clone(), clients.clone());
    if peer == Peer::Full {
        let full = fill(sock);
        std::thread::spawn(move || {
            let _queue = (l, full);
            let _ = held.recv();
        });
        return Stand { seen, clients, _release: release };
    }
    std::thread::spawn(move || {
        let (mut migrated, mut late_migrate, mut late_status) = (false, peer == Peer::Late, peer == Peer::Late);
        for conn in l.incoming() {
            let Ok(conn) = conn else { return };
            count.fetch_add(1, Ordering::SeqCst);
            let mut w = conn.try_clone().expect("a second handle");
            // A client that closed in the queue is dropped, as QEMU does.
            if w.write_all(GREETING).is_err() {
                continue;
            }
            for line in BufReader::new(conn).lines().map_while(std::result::Result::ok) {
                let verb = qmp::json_member(&line, "execute").unwrap_or_default();
                log.lock().unwrap_or_else(|e| e.into_inner()).push(verb.clone());
                let answer = match verb.as_str() {
                    "migrate" => {
                        let uri = qmp::json_member(&line, "uri").unwrap_or_default();
                        std::fs::write(stream_of(&uri).expect("a URI of a stream"), b"STATE").unwrap();
                        migrated = true;
                        "{\"return\": {}}"
                    }
                    "query-migrate" if migrated && peer == Peer::Hangs => {
                        // Two connections that nobody takes fill the queue.
                        let _full = fill(&own);
                        let _ = held.recv();
                        return;
                    }
                    "query-migrate" if migrated => {
                        if std::mem::take(&mut late_migrate) {
                            std::thread::sleep(LATE);
                        }
                        "{\"return\": {\"status\": \"completed\"}}"
                    }
                    "query-status" => {
                        if std::mem::take(&mut late_status) {
                            std::thread::sleep(LATE);
                        }
                        "{\"return\": {\"status\": \"paused\"}}"
                    }
                    _ => "{\"return\": {}}",
                };
                if w.write_all(format!("{answer}\n").as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    Stand { seen, clients, _release: release }
}

/// A save of the line of `d` through its stand-in, with the bound `wait`,
/// in a thread with the time limit [`LIMIT`].
fn save_in(d: &Path, wait: Duration) -> (SaveEnd, Duration) {
    let d = d.to_path_buf();
    within(LIMIT, move || {
        let (sock, stream, record) = (d.join("qmp"), d.join("state.vm"), d.join("state.rec"));
        let (n, fingerprint) = (notes(), print("k1"));
        let s = Save {
            monitor: &sock,
            stream: &stream,
            record: &record,
            fingerprint: &fingerprint,
            notes: &n,
            transport: Transport::File,
            fallback: false,
            unique: "u1",
            wait,
        };
        save(&s, &mut ())
    })
    .unwrap_or_else(|| panic!("the save did not end in {LIMIT:?}"))
}

/// The wait for the load of the stand-in of `d` with the bound `wait`, in
/// a thread with the time limit [`LIMIT`].
fn running_in(d: &Path, wait: Duration) -> (Result<()>, Duration) {
    let sock = d.join("qmp");
    within(LIMIT, move || running(&sock, None, wait)).unwrap_or_else(|| panic!("the load did not end in {LIMIT:?}"))
}

/// No partial file of a save is in `d`.
fn no_partial(d: &Path) {
    let partial: Vec<_> = std::fs::read_dir(d)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".part-"))
        .collect();
    assert!(partial.is_empty(), "a partial file stays: {partial:?}");
}

/// The late answer of `query-migrate` comes on the connection of the
/// save: the save ends well with one client. The load then gets the next
/// client, and the late answer of `query-status` on its connection.
#[test]
fn a_late_answer_on_a_monitor_of_one_client_keeps_one_connection() {
    pod_testkit::need_loopback!();
    let d = dir("line-queue-late");
    let stand = monitor(&d.join("qmp"), Peer::Late);
    let (end, took) = save_in(&d, 4 * LATE);
    let seen = names(&stand.seen);
    let saved = end.saved.unwrap_or_else(|e| panic!("{e} {seen:?}"));
    assert_eq!((saved.bytes, saved.transport), (5, Transport::File));
    assert!(end.goes_on.is_ok(), "{:?}", end.goes_on);
    assert!(took >= LATE, "the answer was late: {took:?}");
    assert_eq!(stand.clients(), 1, "the save opened a second connection: {seen:?}");
    let want =
        ["qmp_capabilities", "query-migrate", "stop", "migrate-set-parameters", "migrate", "query-migrate", "cont"];
    assert_eq!(seen, want);
    let (ran, _) = running_in(&d, 4 * LATE);
    let seen = names(&stand.seen);
    assert!(ran.is_ok(), "{ran:?} {seen:?}");
    assert_eq!(stand.clients(), 2, "{seen:?}");
    assert_eq!(seen[want.len()..], ["qmp_capabilities", "query-status", "cont"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// A monitor that answers nothing and fills its queue: the save ends
/// within its bound on its one connection and leaves no file, and the
/// load ends within its bound with the line of the full queue.
#[test]
fn a_monitor_that_hangs_with_a_full_queue_ends_the_save_and_the_load_within_their_bounds() {
    pod_testkit::need_loopback!();
    let d = dir("line-queue-hangs");
    let stand = monitor(&d.join("qmp"), Peer::Hangs);
    let (end, took) = save_in(&d, SHORT);
    let seen = names(&stand.seen);
    let e = end.saved.expect_err("no answer gives no state");
    assert_eq!(e.class, Class::Timeout, "{e}");
    assert!(e.to_string().contains("did not end within"), "{e}");
    assert!(end.goes_on.is_err(), "a monitor that answers nothing takes no cont");
    // The poll, `migrate_cancel` and `cont`: each has its bound.
    assert!(took < SHORT + 4 * qmp::ANSWER_WAIT, "{took:?}");
    assert_eq!(stand.clients(), 1, "the save opened a second connection: {seen:?}");
    assert!(seen.iter().position(|v| v == "stop") < seen.iter().position(|v| v == "migrate"), "{seen:?}");
    assert_eq!(seen.last().map(String::as_str), Some("query-migrate"), "{seen:?}");
    assert!(!d.join("state.vm").exists() && !d.join("state.rec").exists());
    no_partial(&d);
    let (ran, took) = running_in(&d, SHORT);
    let e = ran.expect_err("a full queue gives no load");
    assert_eq!(e.class, Class::Timeout, "{e}");
    assert!(e.to_string().contains("is full"), "{e}");
    assert!(took < SHORT + Duration::from_secs(2), "{took:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A queue that is full from the start: the save fails before the stop,
/// so the guest goes on, and the load ends at its bound. Each failure
/// says that the queue is full.
#[test]
fn a_full_queue_fails_the_save_before_the_stop_and_the_load_at_its_bound() {
    pod_testkit::need_loopback!();
    let d = dir("line-queue-full");
    let stand = monitor(&d.join("qmp"), Peer::Full);
    let (end, took) = save_in(&d, Duration::from_secs(30));
    let e = end.saved.expect_err("a full queue gives no state");
    assert_eq!(e.class, Class::Timeout, "{e}");
    assert!(e.to_string().contains("is full"), "{e}");
    assert!(end.goes_on.is_ok(), "nothing stopped the guest: {:?}", end.goes_on);
    assert!(took < MONITOR_WAIT + Duration::from_secs(2), "{took:?}");
    let (ran, took) = running_in(&d, SHORT);
    let e = ran.expect_err("a full queue gives no load");
    assert!(e.to_string().contains("is full"), "{e}");
    assert!(took < SHORT + Duration::from_secs(2), "{took:?}");
    assert_eq!((stand.clients(), names(&stand.seen).len()), (0, 0));
    no_partial(&d);
    let _ = std::fs::remove_dir_all(&d);
}
