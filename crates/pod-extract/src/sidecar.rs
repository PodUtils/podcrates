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
}

impl Sidecar {
    pub fn create(path: &std::path::Path) -> std::io::Result<Sidecar> {
        Ok(Sidecar {
            out: std::io::BufWriter::new(std::fs::File::create(path)?),
            written: 0,
            dropped: 0,
            first_dropped: None,
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

    #[test]
    fn a_path_carrying_a_quote_stays_valid_json() {
        assert_eq!(json_string("a\"b\\c"), r#""a\"b\\c""#);
        assert_eq!(json_string("a\nb"), r#""a\nb""#);
        // Escaped, not dropped: a record of `ab` names a different file from
        // the file of the layer.
        assert_eq!(json_string("a\u{1}b"), r#""a\u0001b""#);
    }
}
