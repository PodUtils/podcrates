//! `TODO/extract.md` T-0302: ownership-neutral extraction, plus the record of
//! what the image meant.
//!
//! ⭐ **The wall this crate exists to survive.** `chown` and `lchown` to an
//! unmapped id return **`EINVAL`**, not `EPERM`, and that is where GNU tar,
//! containers/storage's layer applier, Apptainer's Go unpacker, pacman's
//! `DownloadUser` and rurima's `tar -xpf` all stop. The wall itself is at
//! `references/containers__storage/tree/pkg/archive/archive.go:798-811`: with
//! `chownOpts` nil it takes the ids straight from the tar header and calls
//! `idtools.SafeLchown`. The switch is at
//! `references/containers__storage/tree/pkg/archive/archive.go:1104`.
//!
//! `/etc/shadow` is the usual first casualty because it is `root:shadow`, gid
//! 42, and that is measured rather than assumed:
//! `experiments/results/whiteout-contract.txt` check B reads
//! `-rw-r----- 0/42 ... etc/shadow` out of a pinned `alpine` image.
//!
//! ⛔ **Never restore ownership by default.** The design to copy is udocker's,
//! which is ownership-neutral *by construction* rather than by recovery:
//! `references/indigo-dc__udocker/tree/udocker/container/structure.py:285-287`.
//!
//! The sidecar buys three things `--no-same-owner` alone does not: faithful
//! re-export, an honest answer to a workload that asks who owns a file, and a
//! diagnostic that names the gap.
//!
//! ⛔ **It changes no kernel permission check and must never be presented as if
//! it does.** A file recorded here as `gid 42` is owned by the extracting id on
//! disk, and every access check the kernel makes uses the latter.

use std::fmt::Write as _;
use std::io::Write;

/// One line of the sidecar. Serialised by hand rather than through a derive,
/// because the shape is fixed by `TODO/extract.md` T-0302 and a field order
/// that drifts is a diff nobody can read.
#[derive(Debug, Clone)]
pub struct Meta {
    pub path: String,
    pub uid: u64,
    pub gid: u64,
    pub mode: u32,
    pub applied_uid: u64,
    pub applied_gid: u64,
    /// Absent when nothing was dropped, which is the ordinary case for a file
    /// the image already owns as the extracting id.
    pub reason: Option<String>,
    /// ⭐ T-0306. The mode the image asked for is `mode`; this is what the
    /// re-permission pass widened it to, and it is recorded so a re-export can
    /// put the image's own mode back.
    pub applied_mode: Option<u32>,
}

impl Meta {
    /// ⚠ JSON, by hand, and the only escaping that matters is the path's: a
    /// member name in an image may carry a quote or a backslash.
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

/// ⛔ Escapes what JSON requires and nothing else. A path in an image is bytes;
/// a control byte is escaped rather than dropped, because dropping one produces
/// a record naming a different file.
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

/// The sidecar writer. One JSON object per line, so a rootfs with 200,000
/// entries does not have to be held in memory to be read back, and so an
/// interrupted extraction leaves a file whose complete lines are still valid.
/// The first entry that lost an id, with what the kernel applied instead.
/// The extraction report names it, so the diagnostic quotes one measured case
/// rather than a count.
#[derive(Debug, Clone)]
pub struct FirstDropped {
    pub path: String,
    pub uid: u64,
    pub gid: u64,
    pub applied_uid: u64,
    pub applied_gid: u64,
}

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

    /// How many entries carried an id this machine could not apply. ⚠ Reported
    /// rather than hidden: it is the number that says how far the image's
    /// ownership and the extracted tree's have diverged.
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

    /// ⭐ THE SHAPE `TODO/extract.md` T-0302 SPECIFIES, and the exact case
    /// `experiments/results/whiteout-contract.txt` check B measured out of
    /// alpine. T-0302's `Prove` runs
    /// `jq -e 'select(.path=="etc/shadow") | .uid==0 and .gid==42 and .applied.gid==0'`
    /// over this line, so these three fields are what the acceptance reads.
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
        // ⛔ Escaped, never dropped: a record naming `ab` describes a
        // different file from the one the layer carried.
        assert_eq!(json_string("a\u{1}b"), r#""a\u0001b""#);
    }
}
