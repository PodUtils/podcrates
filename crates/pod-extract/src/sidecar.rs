//! An extraction with no ownership change, and the record of what the image
//! meant.
//!
//! **The wall.** `chown` and `lchown` to an id that the namespace does not
//! map give **`EINVAL`**, not `EPERM`, and GNU tar, the layer applier of
//! containers/storage (with no `chownOpts` it takes the ids of the tar header
//! and calls `idtools.SafeLchown`), the unpacker of Apptainer, the
//! `DownloadUser` of pacman and the `tar -xpf` of rurima stop there.
//! `/etc/shadow` is usually the first file: it is `root:shadow`, gid 42
//! (`-rw-r----- 0/42 etc/shadow` in alpine).
//!
//! **Do not restore ownership by default.** The model is udocker: no
//! ownership change *by design*, not by a recovery.
//!
//! The sidecar gives three things that `--no-same-owner` alone does not: a
//! true export, a true answer to a workload that asks who owns a file, and a
//! diagnostic that names the gap.
//!
//! **It changes no permission check of the kernel. Do not show it as if it
//! does.** A file that the sidecar records as `gid 42` has the extracting id on
//! disk, and the kernel checks each access with that id.

use std::fmt::Write as _;
use std::io::Write;

/// One line of the sidecar. Written by hand, not with a derive: the shape is
/// fixed, and a field order that changes gives a diff that nobody can read.
#[derive(Debug, Clone)]
pub struct Meta {
    pub path: String,
    pub uid: u64,
    pub gid: u64,
    pub mode: u32,
    pub applied_uid: u64,
    pub applied_gid: u64,
    /// Absent when nothing was dropped: the usual case for a file that the
    /// image gives to the extracting id.
    pub reason: Option<String>,
    /// The image asked for `mode`; this is the mode that stayed on disk when
    /// it is not `mode` (wider with no owner write set back, or with no
    /// `S_ISUID` or `S_ISGID` that the kernel refused), recorded so that an
    /// export can put the mode of the image back.
    pub applied_mode: Option<u32>,
}

impl Meta {
    /// JSON, by hand. Only the path needs escapes: a member name in an image
    /// can have a quote or a backslash.
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        let _ = write!(
            s,
            r#"{{"path":{},"uid":{},"gid":{},"mode":"{:04o}","applied":{{"uid":{},"gid":{}"#,
            json_string(&self.path),
            self.uid,
            self.gid,
            self.mode & 0o7777,
            self.applied_uid,
            self.applied_gid,
        );
        if let Some(m) = self.applied_mode {
            let _ = write!(s, r#","mode":"{:04o}"#, m & 0o7777);
            s.push('"');
        }
        s.push('}');
        if let Some(r) = &self.reason {
            let _ = write!(s, r#","reason":{}"#, json_string(r));
        }
        s.push('}');
        s
    }
}

/// Escapes what JSON needs and nothing more. A path in an image is bytes; a
/// control byte is escaped, not dropped: a record with a byte dropped names a
/// different file.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The first entry that lost an id, with the ids that the kernel applied.
/// The report of the caller names it, so that the diagnostic quotes one
/// measured case, not only a count.
#[derive(Debug, Clone)]
pub struct FirstDropped {
    pub path: String,
    pub uid: u64,
    pub gid: u64,
    pub applied_uid: u64,
    pub applied_gid: u64,
}

/// The writer of the sidecar. One JSON object on each line: a reader does not
/// keep a root file system of 200,000 entries in memory, and an interrupted
/// extraction leaves a file whose complete lines are valid.
pub struct Sidecar {
    out: std::io::BufWriter<std::fs::File>,
    written: u64,
    dropped: u64,
    first_dropped: Option<FirstDropped>,
    /// The last row of each directory, for [`Sidecar::kept_dir`].
    dirs: std::collections::HashMap<String, Meta>,
}

impl Sidecar {
    pub fn create(path: &std::path::Path) -> std::io::Result<Sidecar> {
        Ok(Sidecar {
            out: std::io::BufWriter::new(std::fs::File::create(path)?),
            written: 0,
            dropped: 0,
            first_dropped: None,
            dirs: std::collections::HashMap::new(),
        })
    }

    pub fn push(&mut self, m: &Meta) -> std::io::Result<()> {
        self.written += 1;
        if m.reason.is_some() {
            self.dropped += 1;
            if self.first_dropped.is_none() {
                self.first_dropped = Some(FirstDropped {
                    path: m.path.clone(),
                    uid: m.uid,
                    gid: m.gid,
                    applied_uid: m.applied_uid,
                    applied_gid: m.applied_gid,
                });
            }
        }
        writeln!(self.out, "{}", m.to_json())
    }

    /// [`Sidecar::push`] for a directory: the row is kept for
    /// [`Sidecar::kept_dir`].
    pub fn push_dir(&mut self, m: &Meta) -> std::io::Result<()> {
        self.push(m)?;
        self.dirs.insert(m.path.clone(), m.clone());
        Ok(())
    }

    /// One more row for the directory `path`, with the mode that the kernel
    /// kept after the pass of the last layer. The last row of a path holds,
    /// as for a directory that a later layer names again. False: no
    /// directory row of that path.
    pub fn kept_dir(&mut self, path: &str, applied_mode: u32) -> std::io::Result<bool> {
        let Some(m) = self.dirs.get(path) else {
            return Ok(false);
        };
        let row = Meta { applied_mode: Some(applied_mode), ..m.clone() };
        self.written += 1;
        writeln!(self.out, "{}", row.to_json())?;
        Ok(true)
    }

    pub fn finish(mut self) -> std::io::Result<(u64, u64)> {
        self.out.flush()?;
        Ok((self.written, self.dropped))
    }

    /// How many entries had an id that this machine could not apply. Reported,
    /// not hidden: the number tells how far the ownership of the image and of
    /// the extracted tree differ.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// The first entry that lost an id, if any. The report names it.
    pub fn first_dropped(&self) -> Option<FirstDropped> {
        self.first_dropped.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixed shape, with the measured case of alpine. A consumer check
    /// runs
    /// `jq -e 'select(.path=="etc/shadow") | .uid==0 and .gid==42 and .applied.gid==0'`
    /// over this line: these three fields are what it reads.
    #[test]
    fn the_shadow_line_is_what_the_acceptance_greps_for() {
        let m = Meta {
            path: "etc/shadow".into(),
            uid: 0,
            gid: 42,
            mode: 0o640,
            applied_uid: 0,
            applied_gid: 0,
            reason: Some("gid 42 unmapped".into()),
            applied_mode: None,
        };
        assert_eq!(
            m.to_json(),
            r#"{"path":"etc/shadow","uid":0,"gid":42,"mode":"0640","applied":{"uid":0,"gid":0},"reason":"gid 42 unmapped"}"#
        );
    }

    #[test]
    fn a_widened_mode_is_recorded_beside_the_image_s_own() {
        let m = Meta {
            path: "usr/lib".into(),
            uid: 0,
            gid: 0,
            mode: 0o555,
            applied_uid: 0,
            applied_gid: 0,
            reason: None,
            applied_mode: Some(0o755),
        };
        assert_eq!(
            m.to_json(),
            r#"{"path":"usr/lib","uid":0,"gid":0,"mode":"0555","applied":{"uid":0,"gid":0,"mode":"0755"}}"#
        );
    }

    /// The row of a directory again, with the mode that stayed; a path
    /// with no directory row gets none.
    #[test]
    fn a_kept_directory_mode_is_one_more_row_of_the_directory() {
        let path = std::env::temp_dir().join(format!("pod-extract-keptdir-{}", std::process::id()));
        let mut sc = Sidecar::create(&path).unwrap();
        let m = Meta {
            path: "var/mail".into(),
            uid: 0,
            gid: 8,
            mode: 0o2775,
            applied_uid: 0,
            applied_gid: 0,
            reason: Some("gid 8 unmapped".into()),
            applied_mode: None,
        };
        sc.push_dir(&m).unwrap();
        assert!(sc.kept_dir("var/mail", 0o775).unwrap());
        assert!(!sc.kept_dir("var/spool", 0o755).unwrap());
        assert_eq!(sc.dropped(), 1, "the second row is not a second dropped id");
        assert_eq!(sc.finish().unwrap(), (2, 1));
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let last = text.lines().last().unwrap();
        assert!(last.contains(r#""mode":"2775","applied":{"uid":0,"gid":0,"mode":"0775"}"#), "{last}");
    }

    #[test]
    fn a_path_carrying_a_quote_stays_valid_json() {
        assert_eq!(json_string("a\"b\\c"), r#""a\"b\\c""#);
        assert_eq!(json_string("a\nb"), r#""a\nb""#);
        // Escaped, not dropped: a record of `ab` names a different file from
        // the file of the layer.
        assert_eq!(json_string("a\u{1}b"), r#""a\u0001b""#);
    }
}
