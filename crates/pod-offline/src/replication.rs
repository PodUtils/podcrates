//! Replication of the state of a session and of the registry of peers
//! across a partition: an append-only log, a vector clock for each record,
//! and last-writer-wins for each key when two writers are concurrent.
//!
//! The log is append-only: a merge copies each record that this replica
//! does not have, and it never removes or rewrites one. The state of a key
//! is the record that no other record of that key dominates. Two records
//! are concurrent when neither clock covers the other; then the record with
//! the greater own counter wins, and the greater hex of the actor's key
//! breaks an equal counter. The loser is written to [`Replica::conflicts`],
//! and the rule gives the same winner on each replica.
//!
//! The unit of replication is the state of the session and the registry of
//! peers: [`Replica::set_session`] and [`Replica::set_peer`] name the two
//! namespaces. The README states the model.

use std::collections::BTreeMap;
use std::path::Path;

use pod_identity::{Identity, PublicKey};

use crate::Error;
use crate::hex;

/// The namespace of the state of a session.
pub const SESSION: &str = "session:";
/// The namespace of the registry of peers.
pub const PEER: &str = "peer:";

/// The counts of one key, by the key of each actor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Clock(BTreeMap<String, u64>);

impl Clock {
    /// The count of `actor`.
    pub fn get(&self, actor: &PublicKey) -> u64 {
        self.0.get(&actor.hex()).copied().unwrap_or(0)
    }

    /// Whether this clock knows each count of `other`, and more.
    pub fn dominates(&self, other: &Clock) -> bool {
        self.covers(other) && !other.covers(self)
    }

    /// Whether two clocks disagree: neither covers the other.
    pub fn concurrent(&self, other: &Clock) -> bool {
        !self.covers(other) && !other.covers(self)
    }

    /// The text of the clock: `hex:count` for each actor, with commas.
    pub fn encode(&self) -> String {
        self.0.iter().map(|(actor, count)| format!("{actor}:{count}")).collect::<Vec<_>>().join(",")
    }

    /// Read the text of a clock, and `None` for a bad one.
    pub fn decode(text: &str) -> Option<Clock> {
        let mut clock = Clock::default();
        if text.is_empty() {
            return Some(clock);
        }
        for part in text.split(',') {
            let (actor, count) = part.split_once(':')?;
            let count = count.parse::<u64>().ok()?;
            if actor.len() != 64 || hex::decode(actor)?.len() != 32 {
                return None;
            }
            clock.0.insert(actor.to_string(), count);
        }
        Some(clock)
    }

    fn covers(&self, other: &Clock) -> bool {
        other.0.iter().all(|(actor, count)| self.0.get(actor).copied().unwrap_or(0) >= *count)
    }

    fn tick(&mut self, actor: &PublicKey) {
        let count = self.0.entry(actor.hex()).or_insert(0);
        *count += 1;
    }

    fn join(&mut self, other: &Clock) {
        for (actor, count) in &other.0 {
            let mine = self.0.entry(actor.clone()).or_insert(0);
            *mine = (*mine).max(*count);
        }
    }
}

/// One record of the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub key: String,
    pub value: Vec<u8>,
    pub clock: Clock,
    pub actor: PublicKey,
    pub stamp: u64,
}

impl Record {
    /// The line of a log file: actor, stamp, key, clock and value, in hex
    /// where a space could appear.
    pub fn encode(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.actor.hex(),
            self.stamp,
            hex::encode(self.key.as_bytes()),
            self.clock.encode(),
            hex::encode(&self.value)
        )
    }

    /// Read one line of a log file.
    pub fn decode(line: &str) -> Result<Record, Error> {
        let mut parts = line.splitn(5, ' ');
        let actor = parts.next().and_then(PublicKey::parse).ok_or_else(|| Error::Log { why: "a bad actor".into() })?;
        let stamp = parts
            .next()
            .and_then(|text| text.parse::<u64>().ok())
            .ok_or_else(|| Error::Log { why: "a bad stamp".into() })?;
        let key = parts
            .next()
            .and_then(hex::decode)
            .and_then(|raw| String::from_utf8(raw).ok())
            .ok_or_else(|| Error::Log { why: "a bad key".into() })?;
        let clock = parts.next().and_then(Clock::decode).ok_or_else(|| Error::Log { why: "a bad clock".into() })?;
        let value =
            parts.next().and_then(hex::decode).ok_or_else(|| Error::Log { why: "the value is not hex".into() })?;
        Ok(Record { key, value, clock, actor, stamp })
    }
}

/// Two records of one key that were written at the same time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub key: String,
    pub winner: PublicKey,
    pub losers: Vec<PublicKey>,
    pub winner_hash: String,
    pub loser_hashes: Vec<String>,
}

/// What a merge did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Merge {
    pub appended: usize,
    pub conflicts: usize,
}

/// One replica: its actor, its clock and its log.
#[derive(Debug, Clone)]
pub struct Replica {
    actor: PublicKey,
    clock: Clock,
    log: Vec<Record>,
    conflicts: Vec<Conflict>,
}

impl Replica {
    pub fn new(identity: &Identity) -> Replica {
        Replica { actor: identity.public(), clock: Clock::default(), log: Vec::new(), conflicts: Vec::new() }
    }

    pub fn actor(&self) -> PublicKey {
        self.actor
    }

    pub fn log(&self) -> &[Record] {
        &self.log
    }

    /// The conflicts that the rule resolved.
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// Write one value of one key, and give the record.
    pub fn set(&mut self, key: &str, value: &[u8]) -> Record {
        self.clock.tick(&self.actor);
        let record = Record {
            key: key.to_string(),
            value: value.to_vec(),
            clock: self.clock.clone(),
            actor: self.actor,
            stamp: self.log.len() as u64,
        };
        self.log.push(record.clone());
        self.recompute();
        record
    }

    /// Write one value of the state of the session.
    pub fn set_session(&mut self, key: &str, value: &[u8]) -> Record {
        self.set(&format!("{SESSION}{key}"), value)
    }

    /// Write one value of the registry of peers.
    pub fn set_peer(&mut self, key: &str, value: &[u8]) -> Record {
        self.set(&format!("{PEER}{key}"), value)
    }

    /// The value of each key under the rule.
    pub fn state(&self) -> BTreeMap<String, Vec<u8>> {
        let mut by_key: BTreeMap<&str, Vec<&Record>> = BTreeMap::new();
        for record in &self.log {
            by_key.entry(record.key.as_str()).or_default().push(record);
        }
        by_key.into_iter().map(|(key, records)| (key.to_string(), pick(&records).value.clone())).collect()
    }

    /// The value of one key.
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let records: Vec<&Record> = self.log.iter().filter(|record| record.key == key).collect();
        if records.is_empty() { None } else { Some(pick(&records).value.clone()) }
    }

    /// Copy each record of `records` that this replica does not have. The
    /// clock of this replica then knows each count that came.
    pub fn merge(&mut self, records: &[Record]) -> Merge {
        let mut appended = 0;
        for record in records {
            self.clock.join(&record.clock);
            if !self.log.iter().any(|own| same(own, record)) {
                self.log.push(record.clone());
                appended += 1;
            }
        }
        self.recompute();
        Merge { appended, conflicts: self.conflicts.len() }
    }

    /// Write the log, one line for each record.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = path.as_ref();
        let mut text = String::new();
        for record in &self.log {
            text.push_str(&record.encode());
            text.push('\n');
        }
        std::fs::write(path, text).map_err(|e| Error::file(path, e))
    }

    /// Read a log into a replica of `identity`.
    pub fn load(path: impl AsRef<Path>, identity: &Identity) -> Result<Replica, Error> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| Error::file(path, e))?;
        let mut replica = Replica::new(identity);
        let records: Vec<Record> =
            text.lines().filter(|line| !line.is_empty()).map(Record::decode).collect::<Result<_, _>>()?;
        replica.merge(&records);
        Ok(replica)
    }

    fn recompute(&mut self) {
        let mut by_key: BTreeMap<&str, Vec<&Record>> = BTreeMap::new();
        for record in &self.log {
            by_key.entry(record.key.as_str()).or_default().push(record);
        }
        let mut conflicts = Vec::new();
        for (key, records) in by_key {
            let top = top(&records);
            if top.len() > 1 && top.iter().any(|record| record.value != top[0].value) {
                let winner = pick(&top);
                let losers: Vec<&Record> =
                    top.iter().copied().filter(|record| !std::ptr::eq(*record, winner)).collect();
                conflicts.push(Conflict {
                    key: key.to_string(),
                    winner: winner.actor,
                    losers: losers.iter().map(|record| record.actor).collect(),
                    winner_hash: hex::encode(&hex::sha256(&winner.value)),
                    loser_hashes: losers.iter().map(|record| hex::encode(&hex::sha256(&record.value))).collect(),
                });
            }
        }
        self.conflicts = conflicts;
    }
}

fn same(one: &Record, other: &Record) -> bool {
    one.key == other.key && one.actor == other.actor && one.clock == other.clock && one.value == other.value
}

/// The records of a key that no other record of the key supersedes.
fn top<'a>(records: &[&'a Record]) -> Vec<&'a Record> {
    records
        .iter()
        .copied()
        .filter(|record| {
            !records.iter().any(|other| !std::ptr::eq(*record, *other) && other.clock.dominates(&record.clock))
        })
        .collect()
}

/// The winner of the top records: the greatest own counter, then the
/// greatest hex of the actor's key.
fn pick<'a>(records: &[&'a Record]) -> &'a Record {
    let top = top(records);
    top.into_iter().max_by(|a, b| rank(a).cmp(&rank(b))).expect("a key of the log has one record at least")
}

fn rank(record: &Record) -> (u64, String) {
    (record.clock.get(&record.actor), record.actor.hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(n: u8) -> Identity {
        Identity::from_seed(&[n; 32])
    }

    #[test]
    fn a_record_survives_its_line() {
        let mut replica = Replica::new(&seed(1));
        let record = replica.set("session:x", b"value");
        assert_eq!(Record::decode(&record.encode()).unwrap(), record);
        assert_eq!(Clock::decode(&record.clock.encode()).unwrap(), record.clock);
        assert_eq!(Record::decode("bad").unwrap_err().name(), "log");
    }

    #[test]
    fn a_causal_write_wins_without_a_conflict() {
        let mut a = Replica::new(&seed(1));
        let mut b = Replica::new(&seed(2));
        a.set_session("x", b"one");
        b.merge(a.log());
        b.set_session("x", b"two");
        a.merge(b.log());
        assert_eq!(a.get("session:x").unwrap(), b"two");
        assert!(a.conflicts().is_empty());
    }

    #[test]
    fn a_concurrent_write_is_one_conflict_and_one_winner() {
        let mut a = Replica::new(&seed(1));
        let mut b = Replica::new(&seed(2));
        a.set_session("x", b"one");
        b.set_session("x", b"two");
        let merged = a.merge(b.log());
        b.merge(a.log());
        assert_eq!(merged.conflicts, 1);
        assert_eq!(a.get("session:x"), b.get("session:x"));
        assert_eq!(a.conflicts(), b.conflicts());
        assert_eq!(a.conflicts()[0].key, "session:x");
    }

    #[test]
    fn the_conflict_log_names_the_winner_and_the_losers() {
        let mut a = Replica::new(&seed(1));
        let mut b = Replica::new(&seed(2));
        a.set_peer("p", b"one");
        b.set_peer("p", b"two");
        a.merge(b.log());
        let conflict = &a.conflicts()[0];
        assert_eq!(conflict.losers.len(), 1);
        assert_eq!(conflict.winner_hash.len(), 64);
        assert_eq!(conflict.loser_hashes.len(), 1);
    }
}
