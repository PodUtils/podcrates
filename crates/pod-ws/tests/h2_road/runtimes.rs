//! A shared connection and the runtime that drives it
//! (PodUtils/podssh#310): the task of a connection of the pool runs on the
//! runtime that opened it, and ends with that runtime.

use super::*;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
}

/// A session of runtime B does not take the connection that runtime A
/// drives: B opens its own, and B's session carries bytes after A is
/// dropped. A session of B after that takes B's connection, not A's.
#[test]
fn a_session_takes_only_a_connection_of_its_own_runtime() {
    pod_testkit::need_loopback!();
    let server = runtime();
    let root = root_file("runtimes");
    let (port, seen) = server.block_on(stand_in(Offer::Extended));
    let (a, b) = (runtime(), runtime());
    let config = config(port, &root);
    let (first, road) = a.block_on(connect(&config, TOKEN)).expect("a session of runtime A");
    assert_eq!(road, Road::Stream { shared: false });
    let (second, road) = b.block_on(connect(&config, TOKEN)).expect("a session of runtime B");
    assert_eq!(road, Road::Stream { shared: false }, "B takes no connection of A");
    assert_eq!(seen.accepted.load(Ordering::SeqCst), 2, "a TLS connection for each runtime");
    drop(first);
    drop(a);
    b.block_on(each_way(second, 64 * 1024, 1));
    let (third, road) = b.block_on(connect(&config, TOKEN)).expect("another session of runtime B");
    assert_eq!(road, Road::Stream { shared: true }, "B shares its own connection");
    b.block_on(each_way(third, 64 * 1024, 2));
    assert_eq!(seen.accepted.load(Ordering::SeqCst), 2, "no connection more");
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}
