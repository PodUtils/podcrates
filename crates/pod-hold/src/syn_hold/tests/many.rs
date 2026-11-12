//! Many connections: a burst to one port, many ports at one time, and the
//! budget of the hold.

use super::*;

/// podnet's test: 48 connects to one port at the same time. Each SYN is
/// held and dialed, the stack adds a listen socket for each, so the burst
/// gets no RST, and each connect gets its greeting.
#[tokio::test]
async fn netns_backend_takes_a_burst_of_connects_to_one_port() {
    const COUNT: usize = 48;
    let payload = payload();
    let mut clients = Vec::new();
    for _ in 0..COUNT {
        clients.push(payload.connect(target(80)).await.unwrap());
    }
    // The SYNs wait on the link until the pump starts: one burst.
    let stand = Stand::new();
    let router = router(payload, Settings::default(), stand.clone(), false);
    for client in &mut clients {
        greeted(client).await;
    }
    assert_eq!((stand.dial_count(), router.link.resets()), (COUNT, 0));
    let hold = router.stop().await;
    assert_eq!(hold.stack().stats().accepted, COUNT as u64);
    assert_eq!((hold.stats().held, hold.stats().flows), (COUNT as u64, COUNT as u64));
}

/// podnet's test: 300 connections to 300 ports at one time, each held open
/// until the payload has them all. The hold has no cap on the ports: it
/// closes the listener of a port when its last flow ends. Then the payload
/// uses six of the ports again.
#[tokio::test]
async fn netns_backend_has_no_cap_on_destination_ports() {
    const COUNT: u16 = 300;
    let stand = Stand::new();
    let mut router = router(payload(), Settings::default(), stand.clone(), false);
    let mut clients = Vec::new();
    for i in 0..COUNT {
        clients.push(router.payload.connect(target(2000 + i)).await.unwrap());
    }
    for client in &mut clients {
        greeted(client).await;
    }
    let seen = router.until("a listener for each port", |seen| seen.flows == usize::from(COUNT)).await;
    assert_eq!(seen.ports, usize::from(COUNT));
    drop(clients);
    router.until("the close of each listener", |seen| seen.ports == 0 && seen.flows == 0).await;
    for i in (0..COUNT).step_by(50) {
        let mut client = router.payload.connect(target(2000 + i)).await.unwrap();
        greeted(&mut client).await;
    }
    assert_eq!((stand.dial_count(), router.link.resets()), (usize::from(COUNT) + 6, 0));
    router.stop().await;
}

/// A new SYN past the budget is dropped with no RST and no dial; the payload
/// sends it again, and it is held when the first flow ended (call C91). Had
/// the stack taken it, it would be accepted with no upstream, and its dial
/// would start after the accept (a late dial).
#[tokio::test]
async fn a_syn_past_the_budget_waits_for_room() {
    let (stand, open) = Stand::gated(80);
    let settings = Settings { max_flows: 1, ..Settings::default() };
    let mut router = router(payload(), settings, stand.clone(), false);
    let mut first = router.payload.connect(target(80)).await.unwrap();
    stand.started(1).await;
    let mut second = router.payload.connect(target(81)).await.unwrap();
    router.until("a SYN past the budget", |seen| seen.stats.over_budget >= 1).await;
    open.send_replace(true);
    greeted(&mut first).await;
    drop(echoes(first, 1024).await);
    // The payload sends the SYN again after its time out (1 s in smoltcp).
    greeted(&mut second).await;
    echoes(second, 1024).await;
    assert_eq!((stand.dial_count(), router.link.resets()), (2, 0));
    let hold = router.stop().await;
    let stats = hold.stats();
    assert_eq!((stats.held, stats.late, stats.flows), (2, 0, 2), "{stats:?}");
}
