//! The guards return from a test on a host that lacks what it needs, and let
//! it run on a host that has it.

#[test]
fn need_loopback_lets_a_test_bind_on_a_host_with_loopback() {
    pod_testkit::need_loopback!();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("the probe passed, so a bind works");
    assert!(listener.local_addr().unwrap().port() > 0);
}

#[test]
fn need_loopback_gives_a_value_to_a_test_that_returns_one() -> Result<(), String> {
    pod_testkit::need_loopback!(Ok(()));
    std::net::UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    Ok(())
}

#[test]
fn the_probes_agree_with_the_host() {
    let bind = std::net::TcpListener::bind("127.0.0.1:0").is_ok();
    assert_eq!(pod_testkit::host::loopback().is_ok(), bind || pod_testkit::host::loopback().is_ok());
    if pod_testkit::host::localhost().is_ok() {
        use std::net::ToSocketAddrs;
        assert!(("localhost", 0).to_socket_addrs().unwrap().next().is_some());
    }
}
