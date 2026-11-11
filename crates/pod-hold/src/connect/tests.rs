use super::*;

use crate::mem::memfd;
use crate::reply::Recorded;

const KEY: u64 = 7;

fn call(id: u64, len: usize) -> ErrorCall {
    ErrorCall { id, pid: 1, fd: 3, optval: 8, optlen: 16, len }
}

/// A memory of 32 bytes for the `SO_ERROR` calls: the option at 8, its
/// length at 16.
fn memory() -> Memory {
    Memory::of_fd(memfd(&[0u8; 32]).expect("a memfd"), true)
}

fn option_of(memory: &Memory) -> (i32, u32) {
    let value = memory.read(8, 4).unwrap();
    let len = memory.read(16, 4).unwrap();
    (i32::from_ne_bytes(value[..4].try_into().unwrap()), u32::from_ne_bytes(len[..4].try_into().unwrap()))
}

#[test]
fn a_second_blocking_connect_waits_for_the_same_dial() {
    let mut dials = Dials::new();
    assert_eq!(dials.join(KEY, 1, false), None, "no dial yet");
    assert_eq!(dials.start(KEY, 1, false), Started::Waits);
    assert!(dials.in_progress(KEY) && !dials.nonblocking(KEY));
    assert_eq!(dials.join(KEY, 2, false), Some(Joined::Waits));
    let done = dials.finish(KEY, |_| true).expect("two calls wait");
    assert_eq!(done.connects, vec![1, 2]);
    assert!(!dials.in_progress(KEY));
    let answers = Recorded::default();
    assert!(done.answer(&answers, 0, |_, _| unreachable!("no SO_ERROR waits")));
    assert_eq!(answers.replies(), vec![(1, Some((0, 0))), (2, Some((0, 0)))]);
}

#[test]
fn a_nonblocking_connect_gets_einprogress_then_ealready() {
    let mut dials = Dials::new();
    assert_eq!(dials.start(KEY, 1, true), Started::InProgress);
    assert!(dials.nonblocking(KEY));
    assert_eq!(dials.join(KEY, 2, true), Some(Joined::Already));
    // Another socket has no dial.
    assert_eq!(dials.join(KEY + 1, 3, true), None);
}

#[test]
fn a_blocking_dial_with_no_waiting_call_is_dropped() {
    let mut dials = Dials::new();
    dials.start(KEY, 1, false);
    // The caller left: the kernel no longer knows its notification.
    assert_eq!(dials.finish(KEY, |_| false), None);
    assert!(!dials.in_progress(KEY));
}

#[test]
fn a_nonblocking_dial_stays_with_no_waiting_call() {
    let mut dials = Dials::new();
    dials.start(KEY, 1, true);
    let done = dials.finish(KEY, |_| true).expect("a nonblocking dial stays");
    assert!(done.connects.is_empty() && done.errors.is_empty() && done.nonblocking);
    let answers = Recorded::default();
    // No call takes the error: the consumer keeps it on the socket.
    assert!(!done.answer(&answers, libc::ECONNREFUSED, |_, _| true));
    assert!(answers.replies().is_empty());
}

#[test]
fn so_error_waits_and_the_first_takes_the_error() {
    let mut dials = Dials::new();
    assert!(!dials.wait_error(KEY, call(5, 4)), "no dial: the consumer answers from the socket");
    dials.start(KEY, 1, true);
    assert!(dials.wait_error(KEY, call(5, 4)));
    assert!(dials.wait_error(KEY, call(6, 4)));
    let done = dials.finish(KEY, |_| true).unwrap();
    let first = memory();
    let second = memory();
    let answers = Recorded::default();
    let took =
        done.answer(&answers, libc::ECONNREFUSED, |c, v| write_error(if c.id == 5 { &first } else { &second }, c, v));
    assert!(took);
    assert_eq!(option_of(&first), (libc::ECONNREFUSED, 4));
    assert_eq!(option_of(&second), (0, 4), "the error goes to one call only");
    assert_eq!(answers.replies(), vec![(5, Some((0, 0))), (6, Some((0, 0)))]);
}

#[test]
fn a_connect_takes_the_error_before_so_error() {
    let mut dials = Dials::new();
    dials.start(KEY, 1, true);
    assert_eq!(dials.join(KEY, 2, false), Some(Joined::Waits), "a blocking connect joins a nonblocking dial");
    dials.wait_error(KEY, call(5, 4));
    let done = dials.finish(KEY, |_| true).unwrap();
    let memory = memory();
    let answers = Recorded::default();
    assert!(done.answer(&answers, libc::ECONNREFUSED, |c, v| write_error(&memory, c, v)));
    assert_eq!(answers.replies(), vec![(2, Some((-1, libc::ECONNREFUSED))), (5, Some((0, 0)))]);
    assert_eq!(option_of(&memory), (0, 4));
}

#[test]
fn a_call_that_left_gets_no_answer_and_a_failed_write_gets_efault() {
    let mut dials = Dials::new();
    dials.start(KEY, 1, false);
    dials.join(KEY, 2, false);
    dials.wait_error(KEY, call(5, 4));
    let done = dials.finish(KEY, |id| id != 1).unwrap();
    assert_eq!(done.connects, vec![2]);
    let answers = Recorded::default();
    assert!(done.answer(&answers, 0, |_, _| false));
    assert_eq!(answers.replies(), vec![(2, Some((0, 0))), (5, Some((-1, libc::EFAULT)))]);
}

#[test]
fn the_taker_is_the_blocking_connect_or_the_one_so_error() {
    let first = Caller { id: 1, pid: 10, fd: 3 };
    let blocking = Finished { connects: vec![1, 2], errors: Vec::new(), nonblocking: false };
    assert_eq!(blocking.taker(first), (first, vec![1, 2]));
    let one_error = Finished { connects: Vec::new(), errors: vec![call(5, 4)], nonblocking: true };
    assert_eq!(one_error.taker(first), (Caller { id: 5, pid: 1, fd: 3 }, vec![5]));
    let two_errors = Finished { connects: Vec::new(), errors: vec![call(5, 4), call(6, 4)], nonblocking: true };
    assert_eq!(two_errors.taker(first), (first, Vec::new()));
}

#[test]
fn a_short_option_length_writes_that_many_bytes() {
    let memory = memory();
    assert!(write_error(&memory, &call(5, 2), libc::ECONNREFUSED));
    let value = memory.read(8, 4).unwrap();
    let mut want = [0u8; 4];
    want[..2].copy_from_slice(&libc::ECONNREFUSED.to_ne_bytes()[..2]);
    assert_eq!(value[..4], want);
    assert_eq!(memory.read(16, 4).unwrap()[..4], 2u32.to_ne_bytes());
    // An option past the memory fails: the call then gets EFAULT.
    let far = ErrorCall { optval: 64, ..call(5, 4) };
    assert!(!write_error(&memory, &far, 0));
}
