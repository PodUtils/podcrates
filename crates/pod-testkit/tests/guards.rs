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

fn reached_after_need_tool(tool: &str, reached: &std::cell::Cell<bool>) {
    pod_testkit::need_tool!(tool);
    reached.set(true);
}

#[test]
fn need_tool_returns_for_a_tool_that_the_host_lacks_and_runs_for_one_it_has() {
    let reached = std::cell::Cell::new(false);
    reached_after_need_tool("/no/such/dir/no-such-tool", &reached);
    assert!(!reached.get(), "the guard let a test run with no tool");
    reached_after_need_tool("no-such-tool-of-pod-testkit", &reached);
    assert!(!reached.get(), "the guard found a name that is in no directory of PATH");
    let me = std::env::current_exe().expect("the path of this test binary");
    reached_after_need_tool(me.to_str().expect("a UTF-8 path"), &reached);
    assert!(reached.get(), "the guard refused this test binary, which executes");
}

fn reached_after_need_userns(reached: &std::cell::Cell<bool>) {
    pod_testkit::need_userns!();
    reached.set(true);
}

#[test]
fn need_userns_agrees_with_its_probe() {
    let reached = std::cell::Cell::new(false);
    reached_after_need_userns(&reached);
    match pod_testkit::host::userns() {
        Ok(()) => assert!(reached.get(), "the probe passed and the guard returned"),
        Err(why) => {
            assert!(!reached.get(), "the probe failed and the guard let the test run");
            assert!(!why.is_empty());
        }
    }
    assert_eq!(pod_testkit::host::userns(), pod_testkit::host::userns(), "the probe runs one time");
}

/// A variable of qemu-user in the environment means an emulator: `scaled`
/// gives five times the bound there and the bound itself elsewhere.
#[test]
fn the_bound_scales_under_an_emulator() {
    use std::time::Duration;
    let qemu = std::env::var_os("QEMU_LD_PREFIX").is_some();
    if qemu {
        assert!(pod_testkit::host::emulated(), "QEMU_LD_PREFIX is set");
    }
    let one = Duration::from_secs(1);
    let want = if pod_testkit::host::emulated() { one * 5 } else { one };
    assert_eq!(pod_testkit::host::scaled(one), want);
}
