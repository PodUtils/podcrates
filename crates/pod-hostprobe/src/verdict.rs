//! The verdict type, and the line a probe child speaks to its parent.
//!
//! A verdict is the operation's, and "could not run" never reads as
//! "denied". Three variants, not a boolean and a comment, because a boolean
//! cannot carry "not measured".

use crate::sys::Errno;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// The operation ran and succeeded.
    Ok,
    /// The operation ran and the kernel refused it. Always carries an errno.
    Denied,
    /// The operation did not run. Never rendered as a denial.
    Skip,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Ok => "ok",
            Verdict::Denied => "denied",
            Verdict::Skip => "skip",
        }
    }

    /// The exit status of a probe child: 0 ran and matched, 1 ran and something failed, 2 could not
    /// run. The parent cross-checks this against the spoken verdict, so a
    /// child that disagrees with itself is caught rather than believed.
    pub fn exit_code(self) -> i32 {
        match self {
            Verdict::Ok => 0,
            Verdict::Denied => 1,
            Verdict::Skip => 2,
        }
    }

    pub fn from_exit_code(code: i32) -> Option<Verdict> {
        match code {
            0 => Some(Verdict::Ok),
            1 => Some(Verdict::Denied),
            2 => Some(Verdict::Skip),
            _ => None,
        }
    }
}

/// One probe's answer.
///
/// `errno` is the record, and the verdict is derived from it. A `Denied`
/// without an errno cannot be constructed through [`Outcome::denied`].
#[derive(Clone, Debug)]
pub struct Outcome {
    pub verdict: Verdict,
    pub errno: Option<Errno>,
    /// Why, for a `Skip`, and any extra fact worth carrying for the others.
    /// Empty when there is nothing to add.
    pub reason: String,
}

impl Outcome {
    pub fn ok() -> Outcome {
        Outcome { verdict: Verdict::Ok, errno: None, reason: String::new() }
    }

    pub fn ok_with(reason: impl Into<String>) -> Outcome {
        Outcome { verdict: Verdict::Ok, errno: None, reason: reason.into() }
    }

    pub fn denied(errno: Errno) -> Outcome {
        Outcome { verdict: Verdict::Denied, errno: Some(errno), reason: String::new() }
    }

    /// A precondition was missing. The errno that established it travels with
    /// the skip: no verdict other than `ok` is given without the errno of
    /// what happened, where there is one.
    pub fn skip(errno: Option<Errno>, reason: impl Into<String>) -> Outcome {
        Outcome { verdict: Verdict::Skip, errno, reason: reason.into() }
    }

    /// The shape every probe body ends in: the kernel's answer, unmodified.
    pub fn from(res: Result<i64, Errno>) -> Outcome {
        match res {
            Ok(_) => Outcome::ok(),
            Err(e) => Outcome::denied(e),
        }
    }

    pub fn errno_name(&self) -> Option<String> {
        self.errno.map(|e| e.name())
    }

    /// The row format of the Go reference instrument (`verification/probe`
    /// of container-research), so that a run of this crate and a run of the
    /// instrument diff against each other line by line.
    pub fn row(&self, name: &str) -> String {
        match self.verdict {
            Verdict::Ok => format!("{name:<34} OK"),
            Verdict::Denied => {
                let e = self.errno.expect("Denied always carries an errno");
                format!("{name:<34} FAIL errno={} {}", e.0, e.name())
            }
            Verdict::Skip => format!("{name:<34} SKIP {}", self.reason),
        }
    }
}

// ------------------------------------------------------------------ the wire
//
// Self-describing and versioned. A positional record mis-reads silently the day a field is inserted; these are selected by
// name and carry the version that names their shape.

pub const WIRE_MAGIC: &str = "pod-hostprobe/1";

/// One line, no trailing newline. Tabs and newlines are folded to spaces so
/// the record stays one line whatever a reason string contains.
pub fn encode(out: &Outcome) -> String {
    let reason: String = out.reason.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let errno = match out.errno {
        Some(e) => e.0.to_string(),
        None => "-".to_string(),
    };
    format!("{WIRE_MAGIC} verdict={} errno={errno} reason={reason}", out.verdict.word())
}

/// The parse. Anything it cannot read becomes a `Skip` naming the shortfall,
/// never a `Denied`: a parent that cannot read its child has not measured a
/// denial.
pub fn decode(line: &str) -> Result<Outcome, String> {
    let rest = line
        .strip_prefix(WIRE_MAGIC)
        .ok_or_else(|| format!("child spoke {:?}, not {WIRE_MAGIC}", first_token(line)))?;
    let mut verdict = None;
    let mut errno: Option<Option<Errno>> = None;
    let mut reason = String::new();
    let mut cursor = rest.trim_start();
    while !cursor.is_empty() {
        let (tok, tail) = match cursor.find(' ') {
            Some(i) => (&cursor[..i], cursor[i + 1..].trim_start()),
            None => (cursor, ""),
        };
        let (key, value) = tok.split_once('=').ok_or_else(|| format!("field {tok:?} is not key=value"))?;
        match key {
            "verdict" => {
                verdict = Some(match value {
                    "ok" => Verdict::Ok,
                    "denied" => Verdict::Denied,
                    "skip" => Verdict::Skip,
                    other => return Err(format!("unknown verdict {other:?}")),
                })
            }
            "errno" => {
                errno = Some(if value == "-" {
                    None
                } else {
                    Some(Errno(value.parse::<i32>().map_err(|_| format!("errno {value:?} is not a number"))?))
                })
            }
            // `reason` is last and takes the remainder, so it may hold spaces.
            "reason" => {
                let start = cursor.find('=').map(|i| i + 1).unwrap_or(cursor.len());
                reason = cursor[start..].to_string();
                cursor = "";
                continue;
            }
            other => return Err(format!("unknown field {other:?}")),
        }
        cursor = tail;
    }
    let verdict = verdict.ok_or_else(|| "no verdict= field".to_string())?;
    let errno = errno.ok_or_else(|| "no errno= field".to_string())?;
    if verdict == Verdict::Denied && errno.is_none() {
        return Err("a denial with no errno is not a measurement".to_string());
    }
    Ok(Outcome { verdict, errno, reason })
}

fn first_token(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_denial_round_trips_with_its_errno() {
        let got = decode(&encode(&Outcome::denied(crate::sys::EPERM))).unwrap();
        assert_eq!(got.verdict, Verdict::Denied);
        assert_eq!(got.errno, Some(crate::sys::EPERM));
    }

    #[test]
    fn a_skip_keeps_its_reason_and_its_spaces() {
        let out = Outcome::skip(Some(crate::sys::ENOENT), "fixture /tmp/x absent, create it");
        let got = decode(&encode(&out)).unwrap();
        assert_eq!(got.verdict, Verdict::Skip);
        assert_eq!(got.reason, "fixture /tmp/x absent, create it");
        assert_eq!(got.errno, Some(crate::sys::ENOENT));
    }

    #[test]
    fn a_reason_carrying_a_newline_stays_one_line() {
        let line = encode(&Outcome::skip(None, "two\nlines\tand a tab"));
        assert!(!line.contains('\n') && !line.contains('\t'));
        assert_eq!(decode(&line).unwrap().reason, "two lines and a tab");
    }

    #[test]
    fn a_denial_without_an_errno_is_refused_rather_than_believed() {
        let line = format!("{WIRE_MAGIC} verdict=denied errno=- reason=");
        assert!(decode(&line).is_err());
    }

    #[test]
    fn an_unknown_protocol_version_is_refused() {
        assert!(decode("pod-hostprobe/2 verdict=ok errno=- reason=").is_err());
    }

    #[test]
    fn the_row_format_matches_the_reference_harness() {
        assert_eq!(
            Outcome::denied(crate::sys::ENOENT).row("mount(2) bogus target"),
            "mount(2) bogus target              FAIL errno=2 ENOENT"
        );
        assert_eq!(Outcome::ok().row("openat(detached tmpfs, O_DIRECTORY)"), "openat(detached tmpfs, O_DIRECTORY) OK");
    }
}
