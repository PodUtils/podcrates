//! `SHA256SUMS`: the lines of `sha256sum`, `HEX  NAME` (text mode) or
//! `HEX *NAME` (binary mode). A name has its SHA-256 only when exactly one
//! line names it and that line has 64 hex digits: two lines of one name
//! (the same hash or another) are a fault, as a file with two answers is
//! no answer.

use crate::{hex, sha256};

/// The lines of a `SHA256SUMS` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sums {
    /// Each line that has a name: the name, and its hash when the line has
    /// 64 hex digits.
    lines: Vec<(String, Option<[u8; 32]>)>,
}

impl Sums {
    /// Read `bytes`. A file that is not UTF-8 is refused; a line with no
    /// name (no `"  "` and no `" *"`) is skipped, as `sha256sum -c` warns
    /// and goes on.
    pub fn parse(bytes: &[u8]) -> Result<Sums, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "SHA256SUMS is not text".to_string())?;
        let mut lines = Vec::new();
        for line in text.lines() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let Some((digest, name)) = line.split_once("  ").or_else(|| line.split_once(" *")) else {
                continue;
            };
            lines.push((name.to_string(), digest_of(digest)));
        }
        Ok(Sums { lines })
    }

    /// The SHA-256 of `name`: exactly one line names it, with 64 hex digits.
    pub fn get(&self, name: &str) -> Result<[u8; 32], String> {
        let mut named = self.lines.iter().filter(|(n, _)| n == name);
        match (named.next(), named.next()) {
            (Some((_, Some(sum))), None) => Ok(*sum),
            (None, _) => Err(format!("SHA256SUMS names no {name}")),
            (Some(_), None) => Err(format!("the line of {name} in SHA256SUMS has no SHA-256 of 64 hex digits")),
            (Some(_), Some(_)) => Err(format!("SHA256SUMS names {name} more than once")),
        }
    }

    /// Each name, in the order of the file, once.
    pub fn names(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for (name, _) in &self.lines {
            if !out.contains(&name.as_str()) {
                out.push(name);
            }
        }
        out
    }

    /// `bytes` are the file `name` of this list.
    pub fn check(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let want = self.get(name)?;
        let got = sha256(bytes);
        if got != want {
            return Err(format!("the SHA-256 of {name} is {}, and SHA256SUMS gives {}", hex(&got), hex(&want)));
        }
        Ok(())
    }
}

/// 64 hex digits as 32 bytes, in either case.
pub fn digest_of(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// From podnet's `sha256sums_must_have_exactly_one_valid_line`.
    #[test]
    fn sha256sums_must_have_exactly_one_valid_line() {
        let good = "a".repeat(64);
        let bad = "b".repeat(64);
        let name = "podnet-x";
        let cases: Vec<(String, bool)> = vec![
            (String::new(), false),
            (format!("{good}  other\n"), false),
            (format!("{}  {name}\n", "a".repeat(63)), false),
            (format!("{}  {name}\n", "g".repeat(64)), false),
            (format!("{good}  {name} extra\n"), false),
            (format!("{good}  {name}\n{bad}  {name}\n"), false),
            (format!("{bad}  {name}\n{good}  {name}\n"), false),
            (format!("{good}  {name}\n{good}  {name}\n"), false),
            (format!("{good}  {name}\n"), true),
            (format!("{good} *{name}\n"), true),
            (format!("{}  {name}\r\n", "A".repeat(64)), true),
        ];
        for (sums, ok) in cases {
            let r = Sums::parse(sums.as_bytes()).unwrap().get(name);
            assert_eq!(r.is_ok(), ok, "{sums:?}: {r:?}");
        }
        assert!(Sums::parse(b"\xff\xfe").is_err());
    }

    /// From podssh's `a_line_of_sha256sum_gives_its_file_s_digest`.
    #[test]
    fn a_line_of_sha256sum_gives_its_file_s_digest() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let text =
            format!("{a}  podssh-x86_64-unknown-linux-musl\n{b} *podssh-x86_64-pc-windows-msvc.exe\nshort  podssh-x\n");
        let sums = Sums::parse(text.as_bytes()).unwrap();
        assert_eq!(sums.get("podssh-x86_64-unknown-linux-musl").map(|s| hex(&s)), Ok(a));
        assert_eq!(sums.get("podssh-x86_64-pc-windows-msvc.exe").map(|s| hex(&s)), Ok("b".repeat(64)));
        assert!(sums.get("podssh-x").is_err());
        assert!(sums.get("podssh-aarch64-apple-darwin").is_err());
        assert_eq!(sums.names().len(), 3);
    }

    #[test]
    fn a_file_is_checked_by_its_line() {
        let body = b"podlab";
        let text = format!("{}  podlab\n", hex(&sha256(body)));
        let sums = Sums::parse(text.as_bytes()).unwrap();
        assert_eq!(sums.check("podlab", body), Ok(()));
        let why = sums.check("podlab", b"podlaB").unwrap_err();
        assert!(why.contains("the SHA-256 of podlab is"), "{why}");
    }
}
