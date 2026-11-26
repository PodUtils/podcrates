//! The bounds and the grammar of the parts of a reference, as docker's
//! `distribution/reference` has them. The checks are here and not in
//! `Reference::parse`, so each one has a name and a test.

use crate::error::{Result, usage};

/// The most characters of a name: the domain and the path, before the tag
/// (docker's `NameTotalLengthMax`). A registry answers a longer name with
/// an error that does not say why.
pub const NAME_MAX: usize = 255;
/// The most characters of a tag (docker's `[\w][\w.-]{0,127}`).
pub const TAG_MAX: usize = 128;

/// The text in quotes, cut to 64 characters: an error must not repeat an
/// input of 5000 characters.
fn cut(text: &str) -> String {
    match text.char_indices().nth(64) {
        Some((end, _)) => format!("\"{}...\"", text[..end].escape_debug()),
        None => format!("{text:?}"),
    }
}

/// Refuse a name of more than [`NAME_MAX`] characters. `len` is the length of
/// the name as written, with no `library/` that the parse adds.
pub(crate) fn check_name_length(input: &str, len: usize) -> Result<()> {
    if len > NAME_MAX {
        return Err(usage(format!(
            "invalid reference: {}: the name has {len} characters, and the most is {NAME_MAX}",
            cut(input)
        )));
    }
    Ok(())
}

/// Refuse a tag that is empty, is longer than [`TAG_MAX`], starts with `.` or
/// `-`, or has a character outside `[A-Za-z0-9_.-]`.
pub(crate) fn check_tag(input: &str, tag: &str) -> Result<()> {
    if tag.len() > TAG_MAX {
        return Err(usage(format!(
            "invalid reference: {}: the tag has {} characters, and the most is {TAG_MAX}",
            cut(input),
            tag.len()
        )));
    }
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bad_start = !tag.bytes().next().is_some_and(word);
    if bad_start || !tag.bytes().all(|b| word(b) || b == b'.' || b == b'-') {
        return Err(usage(format!(
            "invalid reference: {}: tag {} is not valid: it has 1 to {TAG_MAX} characters of \
             letters, digits, \"_\", \".\" and \"-\", and does not start with \".\" or \"-\"",
            cut(input),
            cut(tag)
        )));
    }
    Ok(())
}

/// Whether a path component has docker's grammar
/// `[a-z0-9]+(?:(?:[._]|__|[-]+)[a-z0-9]+)*`.
pub(crate) fn component_ok(c: &str) -> bool {
    let word = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let mut rest = c.as_bytes();
    loop {
        let n = rest.iter().take_while(|b| word(b)).count();
        if n == 0 {
            return false;
        }
        rest = &rest[n..];
        if rest.is_empty() {
            return true;
        }
        // One separator, then a word again: `.`, `_`, `__` or a run of `-`.
        let sep = match rest[0] {
            b'.' => 1,
            b'_' if rest.get(1) == Some(&b'_') => 2,
            b'_' => 1,
            b'-' => rest.iter().take_while(|b| **b == b'-').count(),
            _ => return false,
        };
        rest = &rest[sep..];
    }
}

/// Refuse a path component outside docker's grammar.
pub(crate) fn check_component(input: &str, component: &str) -> Result<()> {
    if component_ok(component) {
        return Ok(());
    }
    Err(usage(format!(
        "invalid reference: {}: repository component {} is not lowercase letters and \
         digits with one separator (\".\", \"_\", \"__\" or \"-\") between words",
        cut(input),
        cut(component)
    )))
}

#[cfg(test)]
mod tests;
