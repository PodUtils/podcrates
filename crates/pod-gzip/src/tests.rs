//! The pipeline, with a compressor that a test holds: the producer writes
//! while the threads compress, a fast producer waits at the limit, a
//! failure of a thread comes out as an error, and no thread outlives the
//! writer.

use std::io::{self, Read, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use super::{BLOCK, Compress, Members, member};

/// The time limit of an event that must come.
const COMES: Duration = Duration::from_secs(60);
/// The wait that shows that an event does not come while the gate is shut.
const STAYS: Duration = Duration::from_millis(300);

/// A gate that holds each compression until it opens.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    turn: Condvar,
}

impl Gate {
    fn wait(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.turn.wait(open).unwrap();
        }
    }
    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.turn.notify_all();
    }
}

fn held(gate: &Arc<Gate>) -> Compress {
    let gate = Arc::clone(gate);
    Arc::new(move |b: &[u8], level| {
        gate.wait();
        member(b, level)
    })
}

fn gunzip(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(b).read_to_end(&mut out).expect("one gzip file");
    out
}

fn block(i: usize) -> Vec<u8> {
    (0..BLOCK).map(|j| (j / 4099 + i) as u8).collect()
}

/// What the producer thread reports after each block: the count of blocks
/// written, and the blocks in flight with the limit.
type Step = (usize, usize, usize);

/// A producer thread that writes `blocks` full blocks through a writer
/// with `threads` threads and `compress`, and reports each step.
fn producer(threads: usize, blocks: usize, compress: Compress) -> (mpsc::Receiver<Step>, mpsc::Receiver<Vec<u8>>) {
    let (steps, step_rx) = mpsc::channel();
    let (done, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut w = Members::with_compress(Vec::new(), threads, 1, compress);
        for i in 0..blocks {
            w.write_all(&block(i)).unwrap();
            let _ = steps.send((i + 1, w.in_flight(), w.limit()));
        }
        let _ = done.send(w.finish().unwrap());
    });
    (step_rx, done_rx)
}

fn input(blocks: usize) -> Vec<u8> {
    (0..blocks).flat_map(block).collect()
}

/// The producer does not wait for a batch: while each thread holds its
/// block, the producer gives the next blocks.
#[test]
fn the_producer_writes_while_the_threads_compress() {
    let gate = Arc::new(Gate::default());
    let (steps, done) = producer(2, 2, held(&gate));
    for want in 1..=2 {
        let (n, _, _) = steps.recv_timeout(COMES).unwrap_or_else(|e| panic!("block {want} waits on the threads: {e}"));
        assert_eq!(n, want);
    }
    gate.open();
    assert!(gunzip(&done.recv_timeout(COMES).unwrap()) == input(2));
}

/// A fast producer waits: the blocks in flight stay at the limit
/// (`threads` + 1), so a slow pool bounds the memory.
#[test]
fn a_fast_producer_waits_at_the_limit() {
    let gate = Arc::new(Gate::default());
    let threads = 2;
    let (steps, done) = producer(threads, 8, held(&gate));
    let mut limit = 0;
    for want in 1..=threads + 1 {
        let (n, flight, l) = steps.recv_timeout(COMES).unwrap_or_else(|e| panic!("block {want}: {e}"));
        assert_eq!(n, want);
        assert!(flight <= l, "{flight} blocks in flight, limit {l}");
        limit = l;
    }
    assert_eq!(limit, threads + 1);
    match steps.recv_timeout(STAYS) {
        Err(RecvTimeoutError::Timeout) => {}
        other => panic!("the producer did not wait at the limit: {other:?}"),
    }
    gate.open();
    for (n, flight, l) in steps.iter() {
        assert!(flight <= l, "block {n}: {flight} blocks in flight, limit {l}");
    }
    assert!(gunzip(&done.recv_timeout(COMES).unwrap()) == input(8));
}

/// The result of `job` within the time limit: a hang fails the test.
fn bounded<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(job());
    });
    rx.recv_timeout(COMES).expect("the writer hangs")
}

/// The first error of the stream: from a write, else from `finish`.
fn first_error(threads: usize, blocks: usize, compress: Compress) -> io::Error {
    let mut w = Members::with_compress(Vec::new(), threads, 1, compress);
    for i in 0..blocks {
        if let Err(e) = w.write_all(&block(i)) {
            return e;
        }
    }
    w.finish().expect_err("an error")
}

#[test]
fn a_panic_of_a_thread_is_an_error_not_a_hang() {
    let calls = Arc::new(Mutex::new(0));
    let c = Arc::clone(&calls);
    let compress: Compress = Arc::new(move |b: &[u8], level| {
        let n = {
            let mut n = c.lock().unwrap();
            *n += 1;
            *n
        };
        assert!(n != 2, "a planted panic of the second block");
        member(b, level)
    });
    let e = bounded(move || first_error(4, 6, compress));
    assert!(e.to_string().contains("panic"), "{e}");
}

#[test]
fn an_error_of_the_compressor_comes_out() {
    let compress: Compress = Arc::new(|_: &[u8], _| Err(io::Error::new(io::ErrorKind::InvalidData, "planted")));
    let e = bounded(move || first_error(3, 6, compress));
    assert_eq!(e.kind(), io::ErrorKind::InvalidData);
}

/// Drop joins the threads: no thread holds the compressor after the
/// writer, also after an error.
#[test]
fn no_thread_outlives_the_writer() {
    let token = Arc::new(());
    for fail in [false, true] {
        let t = Arc::clone(&token);
        let compress: Compress = Arc::new(move |b: &[u8], level| {
            let _hold = &t;
            if fail { Err(io::Error::other("planted")) } else { member(b, level) }
        });
        let mut w = Members::with_compress(Vec::new(), 4, 1, compress);
        let _ = w.write_all(&input(3));
        drop(w);
        assert_eq!(Arc::strong_count(&token), 1, "a thread holds the compressor (fail {fail})");
    }
}

#[test]
fn a_write_after_an_error_gives_the_error_again() {
    let compress: Compress = Arc::new(|_: &[u8], _| Err(io::Error::new(io::ErrorKind::BrokenPipe, "planted")));
    let mut w = Members::with_compress(Vec::new(), 2, 1, compress);
    let mut errors = 0;
    for i in 0..6 {
        if let Err(e) = w.write_all(&block(i)) {
            assert_eq!(e.kind(), io::ErrorKind::BrokenPipe, "block {i}");
            errors += 1;
        }
    }
    assert!(errors >= 2, "{errors} errors: a write after the first error did not fail");
    assert_eq!(w.flush().unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(w.finish().unwrap_err().kind(), io::ErrorKind::BrokenPipe);
}
