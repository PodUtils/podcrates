//! The end that a row of a failure wants: `podlab run --expect-exit CODE
//! --expect-stderr TEXT`. A lane row of an error path (a dead relay, a
//! refused proxy, no DNS) must end with its code and its line, not only
//! end: a row that wants a failure and gets a success, or gets another
//! failure, is a defect of the command.

use std::io::{Read, Write};

/// The most bytes of the error output that a check keeps: the last ones.
/// The tail holds the line that names the failure.
pub const KEEP: usize = 4 << 20;

/// What the end of a command must show. A field that is `None` is not
/// checked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wanted {
    /// The exit code of the command, as [`crate::exit_code`] gives it (a
    /// signal N is 128 + N).
    pub exit: Option<i32>,
    /// A text that the error output of the command must hold.
    pub stderr: Option<String>,
}

impl Wanted {
    /// Whether a check is set: with none, the run gives the code of the
    /// command.
    pub fn is_set(&self) -> bool {
        self.exit.is_some() || self.stderr.is_some()
    }

    /// Each difference between the end of the command and the wanted end,
    /// one line each. Empty: the end is the wanted one.
    pub fn misses(&self, code: i32, stderr: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(want) = self.exit.filter(|w| *w != code) {
            out.push(format!("the command exited {code}; --expect-exit wants {want}"));
        }
        if let Some(text) = self.stderr.as_deref().filter(|t| !holds(stderr, t.as_bytes())) {
            out.push(format!("the error output of the command does not hold {text:?} (--expect-stderr)"));
        }
        out
    }
}

/// Whether `hay` holds `needle`. An empty needle is in each text.
fn holds(hay: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || hay.windows(needle.len()).any(|w| w == needle)
}

/// Copy `from` to `to` until its end, and give the last `keep` bytes. A
/// write that fails stops the copy to `to` but not the read: the command
/// must not stop on a full or closed error output of the caller.
pub fn tee(mut from: impl Read, mut to: impl Write, keep: usize) -> std::io::Result<Vec<u8>> {
    let mut kept: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    let mut copy = true;
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if copy && to.write_all(&buf[..n]).and_then(|()| to.flush()).is_err() {
            copy = false;
        }
        kept.extend_from_slice(&buf[..n]);
        if kept.len() > keep.saturating_mul(2) {
            kept.drain(..kept.len() - keep);
        }
    }
    if kept.len() > keep {
        kept.drain(..kept.len() - keep);
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted(exit: Option<i32>, stderr: Option<&str>) -> Wanted {
        Wanted { exit, stderr: stderr.map(str::to_string) }
    }

    #[test]
    fn a_wanted_end_names_each_difference() {
        let w = wanted(Some(69), Some("could not reach"));
        assert!(w.misses(69, b"podssh: the relay could not reach host\n").is_empty());
        let both = w.misses(0, b"connected\n");
        assert_eq!(both.len(), 2, "{both:?}");
        assert!(both[0].contains("exited 0; --expect-exit wants 69"), "{both:?}");
        assert!(both[1].contains("\"could not reach\""), "{both:?}");
        assert_eq!(w.misses(1, b"could not reach").len(), 1, "another failure is a miss");
        assert!(wanted(None, Some("x")).misses(3, b"x").is_empty(), "no code wanted: any code");
        assert!(wanted(Some(0), None).misses(0, b"").is_empty());
        assert!(!Wanted::default().is_set());
        assert!(wanted(None, Some("")).is_set());
    }

    #[test]
    fn the_tee_copies_each_byte_and_keeps_the_tail() {
        let text: Vec<u8> = (0..50_000u32).flat_map(|i| format!("{i}\n").into_bytes()).collect();
        let mut out = Vec::new();
        let kept = tee(&text[..], &mut out, 1000).unwrap();
        assert_eq!(out, text);
        assert_eq!(kept, text[text.len() - 1000..]);
        let kept = tee(&b"short"[..], std::io::sink(), 1000).unwrap();
        assert_eq!(kept, b"short");
    }

    /// A caller whose error output is closed still gets the whole read.
    #[test]
    fn a_failed_write_does_not_stop_the_read() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let kept = tee(&b"the end line"[..], Closed, 100).unwrap();
        assert_eq!(kept, b"the end line");
    }
}
