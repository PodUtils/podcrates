//! The measured rates, with a margin. `--nocapture` prints each one.

use super::*;

const CONTROLLERS: [CongestionControl; 3] =
    [CongestionControl::None, CongestionControl::Reno, CongestionControl::Cubic];

fn run(case: Case) -> Measure {
    let m = measure(case);
    eprintln!(
        "{:?} link, {:?}, ack delay {:?}, {:?}: {:.2} MB/s, second half {:.2} MB/s (limit {:.2}), {:.2} s; \
         server {} packets, client {}",
        case.link,
        case.controller,
        case.ack_delay.map(|d| d.total_millis()),
        case.ingress,
        m.rate,
        m.half_rate,
        limit(case.link),
        m.seconds,
        m.server_packets,
        m.client_packets
    );
    // The link loses nothing: the server sends each segment one time, with
    // the SYN-ACK and some ACKs.
    assert!(m.server_packets <= data_segments() + 4, "{case:?}: a segment was sent again: {m:?}");
    m
}

/// A burst link: the receiver gives one ACK for each round trip, and
/// smoltcp 0.14 grows the window of Reno and CUBIC by one segment for each
/// ACK. No controller gives more than 80% of the limit.
#[test]
fn each_controller_on_a_burst_link() {
    let [none, reno, cubic] = CONTROLLERS.map(|c| run(Case::new(LinkKind::Burst, c)));
    let top = limit(LinkKind::Burst);
    assert!(none.rate >= 0.8 * top, "None on a burst link: {none:?}");
    for (name, m) in [("Reno", reno), ("CUBIC", cubic)] {
        // About one seventh of no controller; in the second half, less than
        // one third.
        assert!(m.rate <= 0.2 * top, "{name} on a burst link is not slow: {m:?}");
        assert!(m.half_rate <= 0.3 * top, "{name} on a burst link is not slow: {m:?}");
        // One ACK for each round trip.
        let round_trips = m.seconds / ROUND_TRIP_S;
        assert!(
            (m.client_packets as f64) <= 1.5 * round_trips + 10.0,
            "{name}: more than one ACK for each round trip: {m:?}"
        );
    }
    assert!((reno.rate - cubic.rate).abs() <= 0.02 * reno.rate, "the slow start of Reno and CUBIC differs");
}

/// A serial link: the receiver gives one ACK for each two segments, so
/// Reno and CUBIC grow the window by half for each round trip. After the
/// slow start, they move the data at the rate of no controller.
#[test]
fn each_controller_on_a_serial_link() {
    let [none, reno, cubic] = CONTROLLERS.map(|c| run(Case::new(LinkKind::Serial, c)));
    let top = limit(LinkKind::Serial);
    assert!(none.rate >= 0.8 * top, "None on a serial link: {none:?}");
    for (name, m) in [("Reno", reno), ("CUBIC", cubic)] {
        assert!(m.rate >= 0.45 * top, "{name} on a serial link is slow: {m:?}");
        assert!(m.half_rate >= 0.98 * none.half_rate, "{name} after the slow start is slower than None: {m:?}");
        assert!(m.client_packets >= data_segments() / 2, "{name}: less than one ACK for each two segments: {m:?}");
    }
}

/// A receiver with no delayed ACK gives one ACK for each segment on a
/// serial link, and Reno and CUBIC double the window for each round trip.
#[test]
fn a_serial_link_with_no_ack_delay() {
    for controller in [CongestionControl::Reno, CongestionControl::Cubic] {
        let delayed = run(Case::new(LinkKind::Serial, controller));
        let m = run(Case { ack_delay: None, ..Case::new(LinkKind::Serial, controller) });
        assert!(m.rate >= 1.1 * delayed.rate, "{controller:?}: no ACK delay gives no more: {m:?}");
        assert!(m.client_packets >= data_segments(), "{controller:?}: less than one ACK for each segment: {m:?}");
    }
}

/// On a burst link, the fix is in the receiver: a pass of egress after each
/// packet gives one ACK for each two segments (or each segment with no ACK
/// delay). No ACK delay alone changes nothing: one poll still gives one ACK.
#[test]
fn a_burst_link_with_a_receiver_that_takes_each_packet() {
    let base = Case::new(LinkKind::Burst, CongestionControl::Reno);
    let slow = run(base);
    let no_delay = run(Case { ack_delay: None, ..base });
    let each = run(Case { ingress: Ingress::EachPacket, ..base });
    let each_no_delay = run(Case { ingress: Ingress::EachPacket, ack_delay: None, ..base });
    assert!((no_delay.rate - slow.rate).abs() <= 0.05 * slow.rate, "no ACK delay alone changes the rate: {no_delay:?}");
    assert!(each.rate >= 3.0 * slow.rate, "a pass of egress after each packet: {each:?}");
    assert!(
        each_no_delay.rate >= 4.0 * slow.rate,
        "a pass of egress after each packet, no ACK delay: {each_no_delay:?}"
    );
    assert!(each.client_packets >= data_segments() / 2, "less than one ACK for each two segments: {each:?}");
}
