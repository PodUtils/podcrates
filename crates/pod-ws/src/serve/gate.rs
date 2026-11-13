//! The book of the connections before `ready`: the slot of each, its
//! stage, its prefix and its bytes. An accept takes a slot here, or evicts
//! the connection of the earliest stage, the oldest in it, or is refused.

use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;

use super::Caps;

/// The stage of a connection before `ready`, in the order of eviction:
/// the earliest stage goes first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Stage {
    /// The TLS handshake.
    Tls,
    /// The head of the upgrade.
    Head,
    /// After the 101, until the first frame.
    First,
    /// In the hands of the caller, until [`PreAuth::ready`].
    Ready,
}

/// The source of a connection, for the cap of each source: the /32 of an
/// IPv4 address, the /64 of an IPv6 address. An IPv4 address mapped into
/// IPv6 is the IPv4 address. The loopback gets no exemption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Prefix {
    V4([u8; 4]),
    V6([u8; 8]),
}

impl Prefix {
    pub(super) fn of(addr: &SocketAddr) -> Prefix {
        match addr.ip().to_canonical() {
            IpAddr::V4(v4) => Prefix::V4(v4.octets()),
            IpAddr::V6(v6) => {
                let mut head = [0; 8];
                head.copy_from_slice(&v6.octets()[..8]);
                Prefix::V6(head)
            }
        }
    }
}

/// Why an accept gets no slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    /// Its prefix holds `per_prefix` connections before `ready`.
    Prefix,
    /// The caps are full and no connection can go.
    Full,
}

struct Entry {
    stage: Stage,
    prefix: Prefix,
    /// The eviction of the connection: its task cuts it.
    evict: oneshot::Sender<()>,
}

#[derive(Default)]
struct Book {
    /// The next id: the ids go up with the accepts, so the lowest id of a
    /// stage is the oldest connection in it.
    next: u64,
    entries: BTreeMap<u64, Entry>,
    prefixes: HashMap<Prefix, usize>,
    bytes: usize,
}

/// The shared book of one loop.
pub(super) struct Gate {
    book: Mutex<Book>,
    caps: Caps,
    /// The bytes that one connection holds against `preauth_bytes`.
    reserve: usize,
}

/// A slot of the book: its id, and the signal of its eviction.
pub(super) struct Slot {
    pub(super) id: u64,
    pub(super) evicted: oneshot::Receiver<()>,
}

impl Gate {
    pub(super) fn new(caps: Caps, reserve: usize) -> Gate {
        Gate { book: Mutex::new(Book::default()), caps, reserve }
    }

    fn book(&self) -> std::sync::MutexGuard<'_, Book> {
        self.book.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A slot for a connection of `prefix` in the stage TLS. A full prefix
    /// refuses it. Full caps evict the connection of the earliest stage, the
    /// oldest in it, until the new one fits.
    pub(super) fn admit(&self, prefix: Prefix) -> Result<Slot, Refusal> {
        let mut book = self.book();
        if book.prefixes.get(&prefix).copied().unwrap_or(0) >= self.caps.per_prefix {
            return Err(Refusal::Prefix);
        }
        while book.entries.len() >= self.caps.max_unauth || book.bytes + self.reserve > self.caps.preauth_bytes {
            let victim = book.entries.iter().min_by_key(|(id, entry)| (entry.stage, **id)).map(|(id, _)| *id);
            let Some(victim) = victim else { return Err(Refusal::Full) };
            if let Some(entry) = Self::remove(&mut book, victim, self.reserve) {
                let _ = entry.evict.send(());
            }
        }
        let id = book.next;
        book.next += 1;
        let (evict, evicted) = oneshot::channel();
        book.entries.insert(id, Entry { stage: Stage::Tls, prefix, evict });
        *book.prefixes.entry(prefix).or_insert(0) += 1;
        book.bytes += self.reserve;
        Ok(Slot { id, evicted })
    }

    /// Move the connection `id` to `stage`: false when it is gone (evicted).
    pub(super) fn advance(&self, id: u64, stage: Stage) -> bool {
        match self.book().entries.get_mut(&id) {
            Some(entry) => {
                entry.stage = stage;
                true
            }
            None => false,
        }
    }

    /// Free the slot of `id`: true when it still held one, so that of a
    /// ready and a deadline at the same time only one acts.
    pub(super) fn release(&self, id: u64) -> bool {
        let mut book = self.book();
        Self::remove(&mut book, id, self.reserve).is_some()
    }

    fn remove(book: &mut Book, id: u64, reserve: usize) -> Option<Entry> {
        let entry = book.entries.remove(&id)?;
        if let Some(count) = book.prefixes.get_mut(&entry.prefix) {
            *count -= 1;
            if *count == 0 {
                book.prefixes.remove(&entry.prefix);
            }
        }
        book.bytes -= reserve;
        Some(entry)
    }

    /// The connections before `ready`, for the tests of the module.
    #[cfg(test)]
    fn held(&self) -> usize {
        self.book().entries.len()
    }
}

/// The gate of one session before the caller admits its peer. The session
/// holds a slot of the caps until [`PreAuth::ready`]; a deadline that
/// passes, an eviction, or a drop with no `ready` cuts its connection.
pub struct PreAuth {
    gate: Arc<Gate>,
    id: u64,
    ready: Option<oneshot::Sender<()>>,
}

impl std::fmt::Debug for PreAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreAuth").field("id", &self.id).finish_non_exhaustive()
    }
}

impl PreAuth {
    pub(super) fn new(gate: Arc<Gate>, id: u64, ready: oneshot::Sender<()>) -> PreAuth {
        PreAuth { gate, id, ready: Some(ready) }
    }

    /// The caller admitted the peer: its slot is free, and its deadlines
    /// stop. False when an eviction or the deadline of `ready` came first:
    /// the session is cut, and stays cut.
    pub fn ready(mut self) -> bool {
        let held = self.gate.release(self.id);
        if let Some(ready) = self.ready.take() {
            let _ = ready.send(());
        }
        held
    }
}

impl Drop for PreAuth {
    /// No `ready`: the slot is free, and the task of the connection cuts it
    /// when it sees the gate gone.
    fn drop(&mut self) {
        if self.ready.is_some() {
            self.gate.release(self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(max_unauth: usize, per_prefix: usize, preauth_bytes: usize) -> Caps {
        Caps { max_unauth, per_prefix, preauth_bytes, ..Caps::default() }
    }

    fn at(text: &str) -> Prefix {
        Prefix::of(&text.parse().unwrap())
    }

    #[test]
    fn a_prefix_is_the_slash_32_or_the_slash_64_and_a_mapped_address_is_ipv4() {
        assert_eq!(at("127.0.0.1:1"), at("127.0.0.1:2"));
        assert_ne!(at("127.0.0.1:1"), at("127.0.0.2:1"));
        assert_eq!(at("[2001:db8:1:2::1]:1"), at("[2001:db8:1:2:ffff::9]:1"));
        assert_ne!(at("[2001:db8:1:2::1]:1"), at("[2001:db8:1:3::1]:1"));
        assert_eq!(at("[::ffff:192.0.2.7]:1"), at("192.0.2.7:9"));
    }

    #[test]
    fn a_full_prefix_refuses_and_a_full_book_evicts_the_earliest_stage_then_the_oldest() {
        let gate = Gate::new(caps(3, 2, usize::MAX), 1);
        let a = gate.admit(at("10.0.0.1:1")).unwrap();
        let mut b = gate.admit(at("10.0.0.1:2")).unwrap();
        assert_eq!(gate.admit(at("10.0.0.1:3")).err(), Some(Refusal::Prefix));
        let mut c = gate.admit(at("10.0.0.2:1")).unwrap();
        assert!(gate.advance(a.id, Stage::Head));
        // b and c are in the stage TLS; b is the older.
        let d = gate.admit(at("10.0.0.3:1")).unwrap();
        assert!(b.evicted.try_recv().is_ok(), "the oldest of the earliest stage goes");
        assert!(c.evicted.try_recv().is_err());
        assert!(!gate.advance(b.id, Stage::Head), "an evicted connection does not advance");
        assert_eq!(gate.held(), 3);
        assert!(gate.release(c.id));
        assert!(gate.release(d.id));
        assert!(!gate.release(d.id), "a release is idempotent");
        assert_eq!(gate.held(), 1);
        // The prefix of b is free again.
        gate.admit(at("10.0.0.1:4")).unwrap();
    }

    #[test]
    fn the_bytes_evict_as_the_count_does_and_nothing_to_evict_refuses() {
        let gate = Gate::new(caps(10, 10, 5), 2);
        let mut a = gate.admit(at("10.0.0.1:1")).unwrap();
        let _b = gate.admit(at("10.0.0.2:1")).unwrap();
        let _c = gate.admit(at("10.0.0.3:1")).unwrap();
        assert!(a.evicted.try_recv().is_ok(), "two of 2 bytes in 5: the third evicts the oldest");
        assert_eq!(gate.held(), 2);
        let none = Gate::new(caps(10, 10, 1), 2);
        assert_eq!(none.admit(at("10.0.0.1:1")).err(), Some(Refusal::Full));
    }

    #[test]
    fn ready_frees_the_slot_and_a_drop_with_no_ready_frees_it_too() {
        let gate = Arc::new(Gate::new(caps(10, 10, usize::MAX), 1));
        let slot = gate.admit(at("10.0.0.1:1")).unwrap();
        let (tx, mut rx) = oneshot::channel();
        assert!(PreAuth::new(gate.clone(), slot.id, tx).ready(), "the slot was held");
        assert_eq!(gate.held(), 0);
        assert!(rx.try_recv().is_ok(), "the task hears the ready");
        let slot = gate.admit(at("10.0.0.1:1")).unwrap();
        let (tx, mut rx) = oneshot::channel();
        drop(PreAuth::new(gate.clone(), slot.id, tx));
        assert_eq!(gate.held(), 0);
        assert!(matches!(rx.try_recv(), Err(oneshot::error::TryRecvError::Closed)), "a drop is no ready");
        let slot = gate.admit(at("10.0.0.1:1")).unwrap();
        let (tx, _rx) = oneshot::channel();
        let late = PreAuth::new(gate.clone(), slot.id, tx);
        assert!(gate.release(slot.id), "the deadline takes the slot");
        assert!(!late.ready(), "a ready after the deadline finds no slot");
    }
}
