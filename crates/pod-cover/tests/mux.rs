//! The multiplexer: the cover page first, then the session, over one budget.

use pod_cover::{Frame, Lane, Mux};

/// Drain the schedule into chunks.
fn drain(mux: &mut Mux, limit: usize) -> Vec<(Lane, Vec<u8>)> {
    let mut out = Vec::new();
    while let Some(chunk) = mux.next(limit) {
        out.push((chunk.lane, chunk.bytes));
    }
    out
}

#[test]
fn the_cover_page_completes_before_the_session_starts() {
    let mut mux = Mux::new(100);
    // The session frame arrives first; the cover still goes first (MUX-2).
    mux.push(Frame::session(vec![2u8; 60]));
    mux.push(Frame::cover(vec![1u8; 40]));
    mux.push(Frame::cover(vec![3u8; 40]));
    let schedule = drain(&mut mux, 64);

    let first_session = schedule.iter().position(|(lane, _)| *lane == Lane::Session).expect("a session chunk");
    assert!(
        schedule[..first_session].iter().all(|(lane, _)| *lane == Lane::Cover),
        "a session chunk came before a cover chunk"
    );
    let cover: Vec<u8> =
        schedule.iter().filter(|(lane, _)| *lane == Lane::Cover).flat_map(|(_, bytes)| bytes.clone()).collect();
    let session: Vec<u8> =
        schedule.iter().filter(|(lane, _)| *lane == Lane::Session).flat_map(|(_, bytes)| bytes.clone()).collect();
    assert_eq!(cover.len(), 80);
    assert_eq!(session.len(), 20, "the session gets the budget that the page left");
    assert_eq!(mux.remaining(), 0);
    assert_eq!(mux.used(), 100);
    assert_eq!(session, vec![2u8; 20]);
}

#[test]
fn a_frame_is_cut_at_a_limit_and_the_rest_waits() {
    let mut mux = Mux::new(10);
    mux.push(Frame::cover(vec![1, 2, 3, 4]));
    mux.push(Frame::cover(vec![5, 6, 7, 8]));
    let schedule = drain(&mut mux, 3);
    assert_eq!(
        schedule,
        vec![
            (Lane::Cover, vec![1, 2, 3]),
            (Lane::Cover, vec![4]),
            (Lane::Cover, vec![5, 6, 7]),
            (Lane::Cover, vec![8]),
        ]
    );
    assert_eq!(mux.remaining(), 2);
}

#[test]
fn a_spent_budget_yields_nothing() {
    let mut mux = Mux::new(0);
    mux.push(Frame::cover(vec![1u8; 10]));
    assert!(mux.next(10).is_none());
    assert_eq!(mux.remaining(), 0);
}

#[test]
fn empty_frames_are_passed_over() {
    let mut mux = Mux::new(5);
    mux.push(Frame::cover(Vec::new()));
    mux.push(Frame::session(Vec::new()));
    mux.push(Frame::session(vec![9u8; 2]));
    let schedule = drain(&mut mux, 5);
    assert_eq!(schedule, vec![(Lane::Session, vec![9, 9])]);
}

#[test]
fn a_session_that_fits_is_not_starved_by_a_page_that_fits() {
    let mut mux = Mux::new(1000);
    mux.push(Frame::cover(vec![1u8; 100]));
    mux.push(Frame::session(vec![2u8; 100]));
    let schedule = drain(&mut mux, 1000);
    assert_eq!(schedule[0].0, Lane::Cover);
    assert_eq!(schedule[1].0, Lane::Session);
    assert_eq!(mux.remaining(), 800);
}
