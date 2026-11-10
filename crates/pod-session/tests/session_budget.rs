//! What a far end's sessions hold of its budget (podssh T-586,
//! PodUtils/podssh#112): a session whose target ended holds only the bytes
//! that it still keeps, and one that keeps its id alone holds nothing. The
//! keeper takes both from a handover, so that the test needs no relay.

use std::time::Duration;

use pod_session::handover::{Handed, Kept, LayerState, Moved};
use pod_session::keep::Keeper;
use pod_session::link::Link;
use pod_session::pump::CarryParts;
use pod_session::record::Record;
use pod_session::{Secret, SessionId};
use tokio::io::DuplexStream;

const MIB: usize = 1 << 20;
const EACH: usize = 4 * MIB;

/// A link that sent 300 bytes, of which the peer acknowledged 100: its
/// buffer keeps 200.
fn link() -> Link {
    let mut link = Link::new(0, 0).with_replay(EACH);
    let mut out = Vec::new();
    link.send(&[0x5a; 300], &mut out).unwrap();
    link.on_record(Record::Ack { offset: 100 }).unwrap();
    link
}

/// A connected socket of the loopback: what a handover moves for a target.
fn socket() -> std::net::TcpStream {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap()
}

fn state(id: u8, kept: Kept) -> LayerState {
    LayerState { id: SessionId([id; 16]), secret: Secret::from_bytes([id; 32]), left: Duration::from_secs(60), kept }
}

#[tokio::test]
async fn a_session_whose_target_ended_holds_only_its_unacknowledged_bytes() {
    pod_testkit::need_loopback!();
    let keeper: Keeper<DuplexStream> = Keeper::new(Duration::from_secs(60));
    // A session whose target ended: 200 bytes in its buffer, 20 for the
    // target. A session that keeps its id alone, for a resume that lost its
    // `CLOSE`.
    let carry = CarryParts { undelivered: vec![1; 20], app_ended: true, app_shut: true };
    let closing = state(1, Kept::Carried { link: link().parts(), carry });
    let closed = state(2, Kept::Closed { received: 10, reason: String::new() });
    let handed = vec![
        Handed { state: closing, target: Ok(Moved { socket: socket(), channel: None }) },
        Handed { state: closed, target: Err(String::new()) },
    ];
    let said = keeper.take_over(handed, |_, _| Ok(tokio::io::duplex(64).0));
    assert!(said.is_empty(), "{said:?}");
    assert_eq!((keeper.kept(), keeper.closed()), (1, 1));
    let census = keeper.census(EACH);
    assert_eq!(census.held, 220, "the closing session holds what it keeps; the id alone holds nothing");
    assert_eq!((census.live, census.waiting), (0, 1));
    // A budget of one buffer and a little more has room for one more.
    let admitted = std::sync::atomic::AtomicBool::new(false);
    assert!(keeper.admit(&admitted, EACH, EACH + 220));
    assert!(!keeper.admit(&admitted, EACH, EACH + 219));
}
