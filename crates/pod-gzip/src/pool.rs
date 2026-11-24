//! The pool of a writer: a fixed count of threads takes the blocks from a
//! queue, and gives back each member with the number of its block. The
//! writer bounds the blocks in flight; the pool has no bound of its own.

use std::collections::BTreeMap;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::Compress;

/// A block with its number.
type Job = (u64, Vec<u8>);
/// A member with the number of its block.
type Done = (u64, io::Result<Vec<u8>>);

pub(crate) struct Pool {
    jobs: Option<SyncSender<Job>>,
    done: Receiver<Done>,
    threads: Vec<JoinHandle<()>>,
    /// The members that came before the members of lower numbers.
    early: BTreeMap<u64, io::Result<Vec<u8>>>,
}

impl Pool {
    /// A pool of `threads` threads. The queue holds `queue` blocks: the
    /// writer gives no more than that in flight, so a send never waits.
    pub(crate) fn new(threads: usize, queue: usize, level: u32, compress: Compress) -> Pool {
        let (jobs, rx) = mpsc::sync_channel::<Job>(queue);
        let (tx, done) = mpsc::channel::<Done>();
        let rx = Arc::new(Mutex::new(rx));
        let threads = (0..threads)
            .map(|_| {
                let (rx, tx, compress) = (Arc::clone(&rx), tx.clone(), Arc::clone(&compress));
                std::thread::spawn(move || work(&rx, &tx, level, &compress))
            })
            .collect();
        Pool { jobs: Some(jobs), done, threads, early: BTreeMap::new() }
    }

    /// Give block `n` to the threads.
    pub(crate) fn send(&self, n: u64, block: Vec<u8>) -> io::Result<()> {
        let jobs = self.jobs.as_ref().ok_or_else(stopped)?;
        jobs.send((n, block)).map_err(|_| stopped())
    }

    /// The member of block `n`: with `wait`, it waits for it; else `None`
    /// where it is not there.
    pub(crate) fn take(&mut self, n: u64, wait: bool) -> io::Result<Option<Vec<u8>>> {
        loop {
            if let Some(m) = self.early.remove(&n) {
                return m.map(Some);
            }
            let (k, m) = if wait {
                self.done.recv().map_err(|_| stopped())?
            } else {
                match self.done.try_recv() {
                    Ok(d) => d,
                    Err(TryRecvError::Empty) => return Ok(None),
                    Err(TryRecvError::Disconnected) => return Err(stopped()),
                }
            };
            self.early.insert(k, m);
        }
    }

    /// Stop the threads: close the queue and join each one. A thread ends
    /// after its block, so the join waits for one block at most.
    pub(crate) fn stop(&mut self) {
        self.jobs = None;
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
        self.early.clear();
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The loop of one thread: a block in, its member out, until the queue
/// closes or the writer is gone. A panic of the compressor is an error of
/// its block, so that the writer does not wait for a member that does not
/// come.
fn work(rx: &Mutex<Receiver<Job>>, tx: &mpsc::Sender<Done>, level: u32, compress: &Compress) {
    loop {
        let job = match rx.lock() {
            Ok(rx) => rx.recv(),
            Err(_) => return,
        };
        let Ok((n, block)) = job else { return };
        let m = catch_unwind(AssertUnwindSafe(|| compress(&block, level)))
            .unwrap_or_else(|_| Err(io::Error::other("a thread of the gzip writer ended with a panic")));
        if tx.send((n, m)).is_err() {
            return;
        }
    }
}

fn stopped() -> io::Error {
    io::Error::other("the threads of the gzip writer stopped")
}
