use std::os::fd::AsRawFd;

use super::*;

// Each class has its own counter, and the summary names each.
#[test]
fn refusals_are_counted_by_class() {
    let refusals = Refusals::default();
    assert!(refusals.summary(480).is_none());
    assert_eq!(refusals.count(Refusal::TableFull), 1);
    assert_eq!(refusals.count(Refusal::TableFull), 2);
    assert_eq!(refusals.count(Refusal::SocketPair), 1);
    let counts = refusals.counts();
    assert_eq!(counts[0], (Refusal::TableFull, 2));
    assert_eq!(counts[1], (Refusal::SocketPair, 1));
    assert_eq!(counts[2], (Refusal::Other, 0));
    let line = refusals.summary(480).unwrap();
    assert!(line.starts_with("3 socket calls refused (table full 2, socketpair failed 1, other 0)"), "{line}");
    assert!(line.ends_with("the flow budget is 480"), "{line}");
}

// The ceiling follows the descriptor limit, within its bounds.
#[test]
fn the_ceiling_follows_rlimit_nofile() {
    assert_eq!(derived_ceiling(1024), 480);
    assert_eq!(derived_ceiling(20_000), 9968);
    assert_eq!(derived_ceiling(100), MIN_CEILING);
    assert_eq!(derived_ceiling(u64::MAX), 65_536);
}

#[test]
fn a_socket_link_gives_its_inode() {
    assert_eq!(parse_socket_link(b"socket:[123456]"), 123456);
    assert_eq!(parse_socket_link(b"pipe:[77]"), 0);
    assert_eq!(parse_socket_link(b"/dev/null"), 0);
    assert_eq!(parse_socket_link(b"socket:[x]"), 0);
}

#[test]
fn a_copy_of_a_socket_has_the_inode_of_the_socket() {
    let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
    let copy = a.try_clone().unwrap();
    let me = std::process::id();
    let key = Table::<()>::new(me as i32, 64).key_of(me, a.as_raw_fd());
    assert_ne!(key, 0);
    assert_eq!(key, inode_of(a.as_raw_fd()).unwrap());
    assert_eq!(socket_inode(me as i32, copy.as_raw_fd()), key);
}

// The table refuses an entry at its ceiling (the program then gets
// EMFILE), and the ceiling is clamped to its bounds.
#[test]
fn the_table_has_room_up_to_its_ceiling() {
    let me = std::process::id() as i32;
    assert_eq!(Table::<()>::new(me, 1).ceiling(), MIN_CEILING);
    assert_eq!(Table::<()>::new(me, usize::MAX).ceiling(), MAX_CEILING);
    // Keys of sockets that this process holds, so that a sweep keeps them.
    let pairs: Vec<_> = (0..MIN_CEILING).map(|_| std::os::unix::net::UnixStream::pair().unwrap()).collect();
    let mut table = Table::new(me, MIN_CEILING);
    for (a, _) in &pairs {
        assert!(table.has_room());
        table.insert(inode_of(a.as_raw_fd()).unwrap(), ());
    }
    assert!(!table.has_room(), "the table is full at its ceiling");
    // A socket that closed leaves the table at the sweep.
    let gone = inode_of(pairs[0].0.as_raw_fd()).unwrap();
    drop(pairs);
    assert!(table.has_room(), "the sweep removes the closed sockets");
    assert!(!table.contains_key(&gone));
}

// A close of the last descriptor drops the entry; a close of one copy
// keeps it. The kernel runs each close.
#[test]
fn a_close_drops_the_entry_of_the_last_descriptor() {
    let me = std::process::id();
    let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
    let copy = a.try_clone().unwrap();
    let mut table = Table::new(me as i32, 64);
    let key = table.key_of(me, a.as_raw_fd());
    table.insert(key, "entry");
    let answers = crate::reply::Recorded::default();
    handle_close(&answers, 1, a.as_raw_fd(), key, me, &mut table).unwrap();
    assert!(table.contains_key(&key), "a copy holds the socket");
    drop(copy);
    handle_close(&answers, 2, a.as_raw_fd(), key, me, &mut table).unwrap();
    assert!(!table.contains_key(&key));
    assert_eq!(answers.replies(), vec![(1, None), (2, None)]);
}

#[test]
fn the_descriptor_limit_has_a_soft_and_a_hard_value() {
    let (soft, hard) = nofile();
    assert!(soft <= hard);
    assert!(soft > 0);
}
