//! The jobs of a broker: for each job id, its compiled policy, the SHA-256
//! of its token, its end and the signal that ends its connections. A job
//! ends at `drop`, at its TTL, or when a `load` of the same id replaces it;
//! each of its connections ends with it.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use serde::{Deserialize, Deserializer};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tokio::time::Instant;
use zeroize::Zeroizing;

use crate::credentials::Secrets;
use crate::policy::{Compiled, Mode, Policy};
use crate::vault::Vault;

/// The longest TTL of a job.
pub const MAX_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

/// The shortest token: 16 characters.
pub const MIN_TOKEN: usize = 16;

/// The token of a job: printable ASCII with no space. Its text is wiped when
/// it is dropped, and no format shows it.
#[derive(Clone)]
pub struct Token(Zeroizing<String>);

impl Token {
    pub fn new(text: impl Into<String>) -> Token {
        Token(Zeroizing::new(text.into()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    fn check(&self) -> Result<(), String> {
        if self.0.len() < MIN_TOKEN || self.0.len() > 4096 {
            return Err(format!("a job token has {MIN_TOKEN} to 4096 characters"));
        }
        if !self.0.bytes().all(|b| b.is_ascii_graphic()) {
            return Err("a job token is printable ASCII with no space".into());
        }
        Ok(())
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(..)")
    }
}

impl<'de> Deserialize<'de> for Token {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Token, D::Error> {
        String::deserialize(d).map(Token::new)
    }
}

fn hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// A job to load: its id, its token, its TTL, its policy, and the values
/// of the secrets that the credentials of the policy name.
#[derive(Debug, Clone)]
pub struct Load {
    pub job: String,
    pub token: Token,
    pub ttl: Duration,
    pub policy: Policy,
    pub secrets: Secrets,
}

impl Load {
    /// A job with no secret.
    pub fn new(job: impl Into<String>, token: Token, ttl: Duration, policy: Policy) -> Load {
        Load { job: job.into(), token, ttl, policy, secrets: Secrets::new() }
    }
}

/// A job as `list` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobInfo {
    pub job: String,
    pub mode: Mode,
    pub rules: usize,
    /// The allow rules under a suffix where anyone can register a name
    /// ([`crate::names::OPEN_SUFFIXES`]): each reaches other parties.
    pub flagged: Vec<String>,
    pub expires_in: Duration,
    /// The connections of the job that are open.
    pub open: usize,
    /// The certificate of the CA of the job as PEM, when its tunnels have a
    /// TLS end: the job trusts it ([`crate::ca::bundle`]).
    pub ca_pem: Option<String>,
}

/// A job id: 1 to 128 characters of letters, digits, `.`, `_`, `:` and `-`.
pub fn check_job_id(job: &str) -> Result<(), String> {
    let fine = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-');
    if job.is_empty() || job.len() > 128 || !job.chars().all(fine) {
        return Err(format!("{:?} is not a job id (1 to 128 of A-Z a-z 0-9 . _ : -)", pod_core::text::one_line(job)));
    }
    Ok(())
}

struct Entry {
    policy: Arc<Compiled>,
    vault: Option<Arc<Vault>>,
    hash: [u8; 32],
    // Dropped with the entry: each receiver then sees the end of the job.
    stop: watch::Sender<bool>,
    generation: u64,
    expires: Instant,
    open: Arc<AtomicUsize>,
}

#[derive(Default)]
struct Table {
    by_id: HashMap<String, Entry>,
    by_token: HashMap<[u8; 32], String>,
    generation: u64,
}

impl Table {
    fn remove(&mut self, job: &str) -> bool {
        match self.by_id.remove(job) {
            Some(entry) => {
                self.by_token.remove(&entry.hash);
                let _ = entry.stop.send(true);
                true
            }
            None => false,
        }
    }

    fn purge(&mut self, now: Instant) {
        let gone: Vec<String> = self.by_id.iter().filter(|(_, e)| e.expires <= now).map(|(id, _)| id.clone()).collect();
        for job in gone {
            self.remove(&job);
        }
    }
}

/// A connection's hold on its job: the policy, the end of the job and its
/// TTL. The count of open connections falls when the lease drops.
pub(crate) struct Lease {
    pub job: String,
    pub policy: Arc<Compiled>,
    pub vault: Option<Arc<Vault>>,
    pub stop: watch::Receiver<bool>,
    pub expires: Instant,
    open: Arc<AtomicUsize>,
}

impl Lease {
    /// Wait for the end of the job: a drop, a replace or the TTL.
    pub async fn ended(&self) {
        let mut stop = self.stop.clone();
        tokio::select! {
            _ = stop.wait_for(|s| *s) => {}
            () = tokio::time::sleep_until(self.expires) => {}
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.open.fetch_sub(1, Ordering::Relaxed);
    }
}

/// A lease of the entry of `job` in `table`: the open count rises by one.
fn lease(table: &Table, job: String) -> Option<Lease> {
    let entry = table.by_id.get(&job)?;
    entry.open.fetch_add(1, Ordering::Relaxed);
    Some(Lease {
        policy: entry.policy.clone(),
        vault: entry.vault.clone(),
        stop: entry.stop.subscribe(),
        expires: entry.expires,
        open: entry.open.clone(),
        job,
    })
}

pub(crate) struct Jobs {
    table: Mutex<Table>,
    max_ttl: Duration,
}

impl Jobs {
    pub fn new(max_ttl: Duration) -> Arc<Jobs> {
        Arc::new(Jobs { table: Mutex::new(Table::default()), max_ttl })
    }

    fn table(&self) -> std::sync::MutexGuard<'_, Table> {
        self.table.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Load `load`, in place of a job of the same id. A timer drops the job
    /// at its TTL when a tokio runtime runs; each lookup drops an expired
    /// job too. With `tls_end`, a job whose policy needs a TLS end gets its
    /// vault: its CA and its secrets.
    pub fn load(self: &Arc<Self>, load: Load, tls_end: bool) -> Result<(Compiled, Option<Arc<Vault>>), Failure> {
        let usage = |words: String| Failure::new(Stage::Proxy, Class::Usage, words);
        check_job_id(&load.job).map_err(usage)?;
        load.token.check().map_err(usage)?;
        if load.ttl.is_zero() || load.ttl > self.max_ttl {
            return Err(usage(format!("a TTL is from 1 s to {} s", self.max_ttl.as_secs())));
        }
        let compiled = load.policy.compile()?;
        let vault = if tls_end && compiled.needs_tls_end() {
            Some(Arc::new(Vault::new(&load.job, load.ttl, &compiled, &load.secrets)?))
        } else if !compiled.credentials().is_empty() {
            return Err(usage("a credential needs the TLS end of the broker (Broker::with_tunnel_end)".into()));
        } else if !load.secrets.is_empty() {
            return Err(usage("the load gives a secret, and no credential of the policy names one".into()));
        } else {
            None
        };
        let hash = hash(load.token.expose());
        let now = Instant::now();
        let expires = now + load.ttl;
        let generation = {
            let mut table = self.table();
            table.purge(now);
            if table.by_token.get(&hash).is_some_and(|owner| *owner != load.job) {
                return Err(usage("the token is the token of another job".into()));
            }
            table.remove(&load.job);
            table.generation += 1;
            let generation = table.generation;
            let (stop, _) = watch::channel(false);
            let entry = Entry {
                policy: Arc::new(compiled.clone()),
                vault: vault.clone(),
                hash,
                stop,
                generation,
                expires,
                open: Arc::new(AtomicUsize::new(0)),
            };
            table.by_token.insert(hash, load.job.clone());
            table.by_id.insert(load.job.clone(), entry);
            generation
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let jobs: Weak<Jobs> = Arc::downgrade(self);
            let job = load.job.clone();
            runtime.spawn(async move {
                tokio::time::sleep_until(expires).await;
                if let Some(jobs) = jobs.upgrade() {
                    jobs.expire(&job, generation);
                }
            });
        }
        Ok((compiled, vault))
    }

    fn expire(&self, job: &str, generation: u64) {
        let mut table = self.table();
        if table.by_id.get(job).is_some_and(|e| e.generation == generation) {
            table.remove(job);
            tracing::debug!(job, "a job ended at its TTL");
        }
    }

    /// End the job `job` and each of its connections: whether it was there.
    pub fn drop_job(&self, job: &str) -> bool {
        let mut table = self.table();
        table.purge(Instant::now());
        table.remove(job)
    }

    /// The job of `token`, when one holds it and has not expired.
    pub fn find(&self, token: &str) -> Option<Lease> {
        let hash = hash(token);
        let mut table = self.table();
        table.purge(Instant::now());
        let job = table.by_token.get(&hash)?.clone();
        lease(&table, job)
    }

    /// The job `job` by its id, for a listener bound to the job: no token.
    pub fn lease_of(&self, job: &str) -> Option<Lease> {
        let mut table = self.table();
        table.purge(Instant::now());
        lease(&table, job.to_string())
    }

    /// The end signal and the expiry of the job `job` as it is now: `None`
    /// when it is gone.
    pub fn watch(&self, job: &str) -> Option<(watch::Receiver<bool>, Instant)> {
        let mut table = self.table();
        table.purge(Instant::now());
        table.by_id.get(job).map(|e| (e.stop.subscribe(), e.expires))
    }

    /// Each job, by id.
    pub fn list(&self) -> Vec<JobInfo> {
        let now = Instant::now();
        let mut table = self.table();
        table.purge(now);
        let mut jobs: Vec<JobInfo> = table
            .by_id
            .iter()
            .map(|(job, e)| JobInfo {
                job: job.clone(),
                mode: e.policy.mode(),
                rules: e.policy.rules(),
                flagged: e.policy.flagged().to_vec(),
                expires_in: e.expires.saturating_duration_since(now),
                open: e.open.load(Ordering::Relaxed),
                ca_pem: e.vault.as_ref().map(|v| v.ca().pem().to_string()),
            })
            .collect();
        jobs.sort_by(|a, b| a.job.cmp(&b.job));
        jobs
    }
}
