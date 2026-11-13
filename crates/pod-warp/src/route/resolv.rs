//! The resolver file of a TUN that takes the host.
//!
//! Before the first change, [`set_file`] writes a record of the original
//! file to the caller's directory (mode 0600, synced). A restart of the
//! caller restores the original when the process was killed
//! ([`reconcile`]). The record is applied only when the file still starts
//! with the marker line of [`Paths::marker`].

use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use pod_core::{Class, Failure, Stage};
use serde::{Deserialize, Serialize};

/// The first line of a file that the crate writes, by default.
pub const MARKER: &str = "# a TUN of pod-warp wrote this file: DNS through WARP";
/// The resolver file of the host.
pub const RESOLV: &str = "/etc/resolv.conf";
const VERSION: u32 = 1;
/// The mode of a new resolver file when the original has no mode.
const MODE: u32 = 0o644;

/// What a TUN changed in the resolver.
#[derive(Debug)]
pub enum Dns {
    Resolved,
    File {
        paths: Paths,
        /// The lock of the record; held until the value drops.
        _lock: crate::files::Held,
    },
}

/// The files of the change of the resolver.
#[derive(Debug, Clone)]
pub struct Paths {
    /// The resolver file ([`RESOLV`]).
    pub resolv: PathBuf,
    /// The record of the original.
    pub record: PathBuf,
    /// The lock of the record: one TUN at a time.
    pub lock: PathBuf,
    /// The first line of the file that the crate writes ([`MARKER`]; podnet
    /// gives its own).
    pub marker: String,
}

impl Paths {
    /// `resolv`, and the record and the lock in the caller's directory
    /// `dir` (podnet: its state or run directory).
    pub fn in_dir(resolv: PathBuf, dir: &Path) -> Paths {
        Paths {
            resolv,
            record: dir.join("resolv.record.json"),
            lock: dir.join("resolv.lock"),
            marker: MARKER.to_string(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    path: PathBuf,
    /// `file`, `symlink` or `absent`.
    kind: String,
    text: Option<String>,
    link_target: Option<PathBuf>,
    mode: Option<u32>,
}

fn host_io(text: String, e: std::io::Error) -> Failure {
    Failure::caused(Stage::Host, Class::of_io(e.kind()), text, e)
}

/// Change the resolver file to `text`, after a record of the original.
/// Another TUN that holds the lock is `TempFail`.
pub fn set_file(paths: &Paths, text: &str) -> Result<Dns, Failure> {
    let Some(lock) = crate::files::try_lock(&paths.lock)? else {
        let text = format!("another TUN holds the resolver file (the lock {})", paths.lock.display());
        return Err(Failure::new(Stage::Host, Class::TempFail, text));
    };
    // The lock is ours: a record on disk is from a process that ended.
    reconcile_held(paths)?;
    let record = read_original(&paths.resolv)?;
    write_record(&paths.record, &record)?;
    replace_file(&paths.resolv, text, record.mode.unwrap_or(MODE))?;
    Ok(Dns::File { paths: paths.clone(), _lock: lock })
}

fn read_original(path: &Path) -> Result<Record, Failure> {
    let mut record = Record {
        version: VERSION,
        path: path.to_path_buf(),
        kind: "absent".into(),
        text: None,
        link_target: None,
        mode: None,
    };
    let reading = |e| host_io(format!("reading {}", path.display()), e);
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            record.kind = "symlink".into();
            record.link_target = Some(std::fs::read_link(path).map_err(reading)?);
        }
        Ok(meta) => {
            record.kind = "file".into();
            record.text = Some(std::fs::read_to_string(path).unwrap_or_default());
            record.mode = Some(meta.permissions().mode() & 0o7777);
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(reading(err)),
    }
    Ok(record)
}

fn write_record(path: &Path, record: &Record) -> Result<(), Failure> {
    use std::io::Write as _;
    let temp = path.with_extension("tmp");
    let writing = |e| host_io(format!("writing {}", path.display()), e);
    let mut file =
        std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&temp).map_err(writing)?;
    let json = serde_json::to_string(record).expect("a record is JSON");
    file.write_all(json.as_bytes()).map_err(writing)?;
    file.sync_all().map_err(writing)?;
    std::fs::rename(&temp, path).map_err(writing)?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::File::open(dir).and_then(|d| d.sync_all());
    }
    Ok(())
}

/// Restore the record of a process that ended, when no live TUN holds the
/// lock.
pub fn reconcile(paths: &Paths) -> Result<(), Failure> {
    match crate::files::try_lock(&paths.lock)? {
        Some(_held) => reconcile_held(paths),
        None => Ok(()),
    }
}

/// [`reconcile`] for the caller that holds the lock.
fn reconcile_held(paths: &Paths) -> Result<(), Failure> {
    let Ok(text) = std::fs::read_to_string(&paths.record) else {
        return Ok(());
    };
    let record: Record = serde_json::from_str(&text)
        .map_err(|e| Failure::caused(Stage::Host, Class::Config, format!("reading {}", paths.record.display()), e))?;
    if record.version != VERSION {
        let text = format!(
            "{} has the version {}; this crate knows the version {VERSION}. The record stays",
            paths.record.display(),
            record.version
        );
        return Err(Failure::new(Stage::Host, Class::Config, text));
    }
    let ours = std::fs::read_to_string(&paths.resolv).is_ok_and(|t| t.starts_with(&paths.marker));
    if ours {
        restore(&record)?;
        tracing::info!(path = %record.path.display(), "the resolver file is restored");
    } else {
        tracing::warn!(path = %record.path.display(), "another program changed the resolver file; it stays");
    }
    let _ = std::fs::remove_file(&paths.record);
    Ok(())
}

fn restore(record: &Record) -> Result<(), Failure> {
    let path = &record.path;
    match record.kind.as_str() {
        "symlink" => {
            let target = record.link_target.clone().unwrap_or_default();
            let _ = std::fs::remove_file(path);
            std::os::unix::fs::symlink(&target, path).map_err(|e| host_io(format!("restoring {}", path.display()), e))
        }
        "file" => replace_file(path, record.text.as_deref().unwrap_or_default(), record.mode.unwrap_or(MODE)),
        _ => {
            let _ = std::fs::remove_file(path);
            Ok(())
        }
    }
}

/// Put the resolver back (the drop of `Routes`).
pub fn undo(paths: &Paths) {
    if let Err(err) = reconcile_held(paths) {
        tracing::warn!(error = %err.chain(), "the resolver file is not restored");
    }
}

/// Write `text` as the file `path` with the mode `mode` (PodUtils/podnet
/// issue 278). A new file in the same directory is renamed over the old
/// one: a failure keeps the old file, and a symbolic link is replaced, not
/// written through. Where a rename over the file is not possible (a bind
/// mount: EBUSY, EXDEV), the file is written in place.
pub fn replace_file(path: &Path, text: &str, mode: u32) -> Result<(), Failure> {
    let temp = path.with_extension(format!("pod-warp-{}", std::process::id()));
    std::fs::write(&temp, text).map_err(|e| host_io(format!("writing {}", temp.display()), e))?;
    // The umask does not change the mode.
    std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(mode))
        .map_err(|e| host_io(format!("setting the mode of {}", temp.display()), e))?;
    match std::fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = std::fs::remove_file(&temp);
            tracing::debug!(error = %err, path = %path.display(), "no rename over the file; it is written in place");
            std::fs::write(path, text).map_err(|e| host_io(format!("writing {}", path.display()), e))
        }
    }
}

#[cfg(test)]
mod tests;
