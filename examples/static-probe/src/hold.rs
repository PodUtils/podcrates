//! pod-hold is Linux only: on another target the crate is empty.

/// The filter, the budget, the codec of an address and the level; then the
/// hold of a SYN on its stack.
#[cfg(target_os = "linux")]
pub fn hold() -> Option<String> {
    let calls = pod_hold::filter::Calls::sockets();
    let budget = pod_hold::table::derived_ceiling(pod_hold::table::nofile().1);
    let gateway = pod_hold::netns::Tun::default().gateway;
    // AF_INET of Linux.
    let address = pod_hold::sockaddr::encode(2, gateway.into(), 53);
    Some(format!(
        "pod-hold: {} BPF instructions for {} calls, a flow budget of {budget}, the gateway {:?}, level {}; {}",
        calls.program().len(),
        calls.numbers().len(),
        pod_hold::sockaddr::parse(&address).map(|a| a.ip),
        pod_hold::Hold::none("the probe starts no payload").level,
        syn_hold(),
    ))
}

#[cfg(not(target_os = "linux"))]
pub fn hold() -> Option<String> {
    None
}

/// One SYN whose dial fails, through the hold of a SYN in a runtime of one
/// thread: the stack, the dial task and the RST link in.
#[cfg(target_os = "linux")]
fn syn_hold() -> String {
    use pod_hold::syn_hold::{Settings, Syn, SynHold};
    use std::collections::VecDeque;
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => return e.to_string(),
    };
    runtime.block_on(async {
        let config = pod_netstack::StackConfig::default();
        let mut hold: SynHold<(), std::io::Error> = SynHold::new(config, Settings::default());
        let dial = |_: Syn| async { Err::<(), _>(std::io::Error::from(std::io::ErrorKind::ConnectionRefused)) };
        // A SYN from 10.66.0.9:40000 to 192.0.2.1:80.
        let mut syn = vec![0x45, 0, 0, 40, 0, 0, 0x40, 0, 64, 6, 0, 0, 10, 66, 0, 9, 192, 0, 2, 1];
        syn.extend_from_slice(&[0x9c, 0x40, 0, 80, 0, 0, 0, 1, 0, 0, 0, 0, 0x50, 0x02, 0xff, 0xff, 0, 0, 0, 0]);
        let (mut from, mut to) = (VecDeque::from([syn]), VecDeque::new());
        let wait = std::time::Duration::from_secs(2);
        let _ = tokio::time::timeout(wait, async {
            while to.is_empty() {
                hold.pump(&mut from, &mut to, &dial).await;
                if to.is_empty() {
                    hold.activity().await;
                }
            }
        })
        .await;
        hold.shutdown().await;
        format!("the hold of a SYN answered {} packets ({} reset)", to.len(), hold.stats().reset)
    })
}
