//! The usage of a cgroup at the end of a run: the CPU time, the peak of
//! memory and the count of OOM kills.
//!
//! A caller opens the files of the cgroup before the run starts
//! ([`Meter::open`]) and reads them at the end ([`Meter::read`]). The
//! files stay open, so a read after the cgroup went, or after the run
//! changed its mounts, still gets the values of the kernel. Each read
//! starts at offset 0: the kernel makes the text again at each such read.
//!
//! The selection of the hierarchy: cgroup2 serves only where its memory
//! controller is there (the file `memory.events`), else cgroup v1 (the
//! hierarchies `memory` and `cpuacct`). On a host with both versions, a
//! cgroup2 with no controllers gives the `cpu.stat` of the host, not the
//! time of the run. `memory.events` and not `memory.peak` is the sign of
//! the controller: Linux 5.8 to 5.18 have the controller and no
//! `memory.peak`.
//!
//! Each value is an `Option`; [`Usage::why`] gives the reason of each
//! `None`.

use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use pod_core::{Class, Failure, Stage};

/// The CPU time of cgroup2 (line `usage_usec N`, microseconds).
pub const CPU_STAT: &str = "cpu.stat";
/// The peak of memory of cgroup2, in bytes (Linux 5.19 and later).
pub const MEMORY_PEAK: &str = "memory.peak";
/// The events of the memory controller of cgroup2 (line `oom_kill N`).
pub const MEMORY_EVENTS: &str = "memory.events";
/// The CPU time of cgroup v1, in nanoseconds.
pub const CPUACCT_USAGE: &str = "cpuacct.usage";
/// The peak of memory of cgroup v1, in bytes.
pub const MEMORY_MAX_USAGE: &str = "memory.max_usage_in_bytes";
/// The OOM state of cgroup v1 (line `oom_kill N`, Linux 4.13 and later).
pub const MEMORY_OOM_CONTROL: &str = "memory.oom_control";
/// The most bytes of a file of a cgroup that a read takes.
pub const MAX_FILE: u64 = 64 * 1024;

/// The CPU time in microseconds of the text of `cpu.stat` (cgroup2).
pub fn cpu_usec_v2(text: &str) -> Option<u64> {
    count_of(text, "usage_usec")
}

/// The CPU time in microseconds of the text of `cpuacct.usage` (cgroup
/// v1, nanoseconds). The value rounds down to the full microsecond.
pub fn cpu_usec_v1(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok().map(|ns| ns / 1000)
}

/// The bytes of the text of `memory.peak` (cgroup2) or
/// `memory.max_usage_in_bytes` (cgroup v1).
pub fn bytes(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

/// The count of the line `oom_kill N` of the text of `memory.events`
/// (cgroup2) or `memory.oom_control` (cgroup v1).
pub fn oom_kills_in(text: &str) -> Option<u64> {
    count_of(text, "oom_kill")
}

/// The count `oom_kill` of `memory.events` of the cgroup2 directory `dir`,
/// or `None` where the file or the line is not there.
pub fn oom_kills_of(dir: &Path) -> Option<u64> {
    oom_kills_in(&std::fs::read_to_string(dir.join(MEMORY_EVENTS)).ok()?)
}

/// The count of the line `KEY N`. `KEY` must be the whole first word:
/// `oom_kill` does not take the line `oom_kill_disable 0`.
fn count_of(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(' ')).and_then(|n| n.trim().parse().ok())
}

/// The usage of a cgroup. A value is `None` where it could not be read;
/// [`Usage::why`] gives the reason of each `None`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub cpu_usec: Option<u64>,
    pub peak_bytes: Option<u64>,
    pub oom_kills: Option<u64>,
    /// One line for each `None`, and one for a cgroup2 that the meter did
    /// not take. Each line starts with the name of its value or with the
    /// version of the hierarchy.
    pub why: Vec<String>,
}

/// The hierarchy of the files of a [`Meter`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    /// cgroup2 with its memory controller.
    V2,
    /// cgroup v1: the hierarchies `memory` and `cpuacct`.
    V1,
}

/// An open file of a cgroup, or the reason that there is none.
type Slot = Result<(PathBuf, File), String>;

fn open(dir: &Path, name: &str) -> Slot {
    let path = dir.join(name);
    match File::open(&path) {
        Ok(f) => Ok((path, f)),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// The open files of one cgroup. Make it with [`Meter::open`], or with
/// [`Meter::v2`], [`Meter::v1_memory`] and [`Meter::v1_cpu`] where the
/// caller mounts each hierarchy in turn.
#[derive(Debug)]
pub struct Meter {
    version: Option<Version>,
    cpu: Slot,
    peak: Slot,
    oom: Slot,
    notes: Vec<String>,
}

impl Default for Meter {
    /// A meter with no file: each value reads as `None`.
    fn default() -> Meter {
        let none = || Err("no cgroup file is open".to_string());
        Meter { version: None, cpu: none(), peak: none(), oom: none(), notes: Vec::new() }
    }
}

impl Meter {
    /// Open the files of the cgroup2 directory `dir`. A failure where
    /// `memory.events` does not open: the memory controller is not there,
    /// and the caller goes to cgroup v1.
    pub fn v2(dir: &Path) -> Result<Meter, Failure> {
        let events = dir.join(MEMORY_EVENTS);
        let oom = File::open(&events).map_err(|e| {
            Failure::caused(
                Stage::Host,
                Class::Unsupported,
                format!("cgroup2 {}: no memory controller ({})", dir.display(), events.display()),
                e,
            )
        })?;
        Ok(Meter {
            version: Some(Version::V2),
            cpu: open(dir, CPU_STAT),
            peak: open(dir, MEMORY_PEAK),
            oom: Ok((events, oom)),
            notes: Vec::new(),
        })
    }

    /// Open the files of the cgroup v1 directory `dir` of the hierarchy
    /// `memory`: the peak and the OOM kills.
    pub fn v1_memory(&mut self, dir: &Path) {
        self.version = Some(Version::V1);
        self.peak = open(dir, MEMORY_MAX_USAGE);
        self.oom = open(dir, MEMORY_OOM_CONTROL);
    }

    /// Open the file of the cgroup v1 directory `dir` of the hierarchy
    /// `cpuacct` (or `cpu,cpuacct`): the CPU time.
    pub fn v1_cpu(&mut self, dir: &Path) {
        self.version = Some(Version::V1);
        self.cpu = open(dir, CPUACCT_USAGE);
    }

    /// Add a line to [`Usage::why`] of each read: for example why a
    /// hierarchy could not be mounted.
    pub fn note(&mut self, why: impl Into<String>) {
        self.notes.push(why.into());
    }

    /// Open the files of one cgroup by the selection rule of this module:
    /// the cgroup2 directory `v2` where its memory controller is there,
    /// else the cgroup v1 directories `v1_memory` and `v1_cpuacct`. A
    /// directory that is `None` is not on this host.
    pub fn open(v2: Option<&Path>, v1_memory: Option<&Path>, v1_cpuacct: Option<&Path>) -> Meter {
        let mut m = Meter::default();
        if let Some(dir) = v2 {
            match Meter::v2(dir) {
                Ok(m) => return m,
                Err(f) => m.note(f.chain()),
            }
        }
        match v1_memory {
            Some(dir) => m.v1_memory(dir),
            None => {
                let why = || Err("no cgroup v1 hierarchy memory".to_string());
                (m.peak, m.oom) = (why(), why());
            }
        }
        match v1_cpuacct {
            Some(dir) => m.v1_cpu(dir),
            None => m.cpu = Err("no cgroup v1 hierarchy cpuacct".to_string()),
        }
        m
    }

    /// The hierarchy of the open files, or `None` where none is open.
    pub fn version(&self) -> Option<Version> {
        self.version
    }

    /// The values of the files now. A meter can read more than one time.
    pub fn read(&mut self) -> Usage {
        let mut why = self.notes.clone();
        let v2 = self.version == Some(Version::V2);
        let cpu_parse = if v2 { cpu_usec_v2 } else { cpu_usec_v1 };
        let cpu_usec = value(&mut self.cpu, "cpu_usec", cpu_parse, &mut why);
        let peak_bytes = value(&mut self.peak, "peak_bytes", bytes, &mut why);
        let oom_kills = value(&mut self.oom, "oom_kills", oom_kills_in, &mut why);
        Usage { cpu_usec, peak_bytes, oom_kills, why }
    }
}

/// The value of one slot, or `None` with its reason in `why`.
fn value(slot: &mut Slot, name: &str, parse: fn(&str) -> Option<u64>, why: &mut Vec<String>) -> Option<u64> {
    let read = match slot {
        Err(e) => Err(e.clone()),
        Ok((path, f)) => match reread(f) {
            Err(e) => Err(format!("{}: {e}", path.display())),
            Ok(text) => parse(&text).ok_or_else(|| format!("{}: no count", path.display())),
        },
    };
    read.map_err(|e| why.push(format!("{name}: {e}"))).ok()
}

/// The text of an open file of a cgroup from its start.
fn reread(f: &mut File) -> std::io::Result<String> {
    f.rewind()?;
    let mut s = String::new();
    f.take(MAX_FILE).read_to_string(&mut s)?;
    Ok(s)
}

/// The usage of one cgroup, read one time ([`Meter::open`], then
/// [`Meter::read`]).
pub fn read(v2: Option<&Path>, v1_memory: Option<&Path>, v1_cpuacct: Option<&Path>) -> Usage {
    Meter::open(v2, v1_memory, v1_cpuacct).read()
}

#[cfg(test)]
mod tests;
