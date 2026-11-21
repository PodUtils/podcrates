//! The POSIX ACLs of an entry in their text form: the PAX records
//! `SCHILY.acl.access` and `SCHILY.acl.default` that GNU tar `--acls`,
//! libarchive (`bsdtar`) and star write.
//!
//! **Why.** GNU tar writes the long text form of `acl_to_text`, one entry on
//! each line; libarchive and star write the entries with commas and the id
//! after the permissions (`user:alice:rw-:1000`). The kernel takes an ACL
//! only in its binary form, the extended attributes `system.posix_acl_access`
//! and `system.posix_acl_default`. [`to_xattr`] converts the text to that
//! form, and [`crate::xattr::read`] sets it as each other attribute: it is
//! set or named, with the same counts.
//!
//! **Ids, not names.** The kernel stores ids. A name comes from the user
//! database of the writer, which the extraction does not have: the
//! database of the image can give the same name a different id. So an
//! entry with a name and no id does not convert, and the ACL is named as
//! not set. GNU tar `--numeric-owner` writes ids; libarchive writes the id
//! after each name.
//!
//! **The binary form** (`include/uapi/linux/posix_acl_xattr.h`, acl(5)): a
//! header of version 2 (`u32`), then each entry as `tag: u16`, `perm: u16`,
//! `id: u32`, little endian, sorted by tag and then by id, as
//! `acl_to_xattr` of libacl gives it. An entry with no qualifier has the id
//! `ACL_UNDEFINED_ID`.

/// The prefix of the PAX key of an ACL in its text form.
pub const PAX_PREFIX: &str = "SCHILY.acl.";
/// The attribute of the access ACL.
pub const ACCESS: &str = "system.posix_acl_access";
/// The attribute of the default ACL of a directory.
pub const DEFAULT: &str = "system.posix_acl_default";
/// The most entries of one ACL: its binary form is inside
/// [`crate::xattr::VALUE_MAX`].
pub const ENTRIES_MAX: usize = (crate::xattr::VALUE_MAX - 4) / 8;

const VERSION: u32 = 2;
const USER_OBJ: u16 = 0x01;
const USER: u16 = 0x02;
const GROUP_OBJ: u16 = 0x04;
const GROUP: u16 = 0x08;
const MASK: u16 = 0x10;
const OTHER: u16 = 0x20;
const UNDEFINED_ID: u32 = u32::MAX;

/// The attribute of the ACL of the key `SCHILY.acl.KIND`: `None` for a kind
/// that is not a POSIX ACL (`ace`, the NFSv4 ACL of star and libarchive).
pub fn attribute(kind: &[u8]) -> Option<&'static str> {
    match kind {
        b"access" => Some(ACCESS),
        b"default" => Some(DEFAULT),
        _ => None,
    }
}

/// One entry: its tag, its permissions and its id.
type Entry = (u16, u16, u32);

/// The binary form of the ACL `text`, or why it does not convert.
///
/// The entries are separated by newlines or commas; a `#` starts a comment
/// to the end of its line (`acl_from_text` of libacl).
pub fn to_xattr(text: &[u8]) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(text).map_err(|_| "the text of the ACL is not UTF-8".to_string())?;
    let mut entries: Vec<Entry> = Vec::new();
    for line in text.split('\n') {
        let line = line.split('#').next().unwrap_or_default();
        for e in line.split(',').map(|e| e.trim_matches([' ', '\t', '\r'])).filter(|e| !e.is_empty()) {
            if entries.len() == ENTRIES_MAX {
                return Err(format!("the ACL has more than {ENTRIES_MAX} entries"));
            }
            entries.push(entry(e)?);
        }
    }
    valid(&entries)?;
    entries.sort_unstable_by_key(|(tag, _, id)| (*tag, *id));
    let mut out = Vec::with_capacity(4 + 8 * entries.len());
    out.extend_from_slice(&VERSION.to_le_bytes());
    for (tag, perm, id) in entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&perm.to_le_bytes());
        out.extend_from_slice(&id.to_le_bytes());
    }
    Ok(out)
}

/// One entry of the text: `TAG:QUALIFIER:PERMS`, with an id after it in the
/// form of libarchive and star. `mask` and `other` can have no qualifier
/// field.
fn entry(e: &str) -> Result<Entry, String> {
    let bad = |why: &str| format!("the ACL entry {:?} does not parse: {why}", shown(e));
    let f: Vec<&str> = e.split(':').collect();
    let named = match f[0] {
        "user" | "u" => Some((USER_OBJ, USER, "user")),
        "group" | "g" => Some((GROUP_OBJ, GROUP, "group")),
        "mask" | "m" | "other" | "o" => None,
        _ => return Err(bad("the tag is not user, group, mask or other")),
    };
    let (qualifier, perms, extra) = match (named, f.len()) {
        (None, 2) => ("", f[1], None),
        (_, 3) => (f[1], f[2], None),
        (_, 4) => (f[1], f[2], Some(f[3])),
        _ => return Err(bad("not three or four fields")),
    };
    let perm = permissions(perms).ok_or_else(|| bad("the permissions are not r, w, x and -"))?;
    let Some((obj, tag, what)) = named else {
        if !qualifier.is_empty() {
            return Err(bad("mask and other take no qualifier"));
        }
        return Ok((if matches!(f[0], "mask" | "m") { MASK } else { OTHER }, perm, UNDEFINED_ID));
    };
    if qualifier.is_empty() {
        return Ok((obj, perm, UNDEFINED_ID));
    }
    // A qualifier of digits is an id; else the id after the permissions.
    let id = number(qualifier).or_else(|| extra.and_then(number)).ok_or_else(|| {
        format!(
            "the ACL names the {what} {:?} with no id; the name comes from the user database of the writer, \
             so the crate does not map it to an id",
            shown(qualifier)
        )
    })?;
    if id == UNDEFINED_ID {
        return Err(bad("the id is ACL_UNDEFINED_ID"));
    }
    Ok((tag, perm, id))
}

fn number(s: &str) -> Option<u32> {
    s.bytes().all(|b| b.is_ascii_digit()).then(|| s.parse().ok()).flatten()
}

fn permissions(s: &str) -> Option<u16> {
    if s.is_empty() || s.len() > 3 {
        return None;
    }
    let mut p = 0;
    for c in s.bytes() {
        let bit = match c {
            b'r' => 4,
            b'w' => 2,
            b'x' => 1,
            b'-' => 0,
            _ => return None,
        };
        if p & bit != 0 {
            return None;
        }
        p |= bit;
    }
    Some(p)
}

/// The rules of a valid ACL (acl(5)): one entry of each of `user::`,
/// `group::` and `other::`; a `mask::` when a named entry is there, and one
/// at most; one entry for each named id.
fn valid(entries: &[Entry]) -> Result<(), String> {
    let count = |t: u16| entries.iter().filter(|e| e.0 == t).count();
    for (tag, name) in [(USER_OBJ, "user::"), (GROUP_OBJ, "group::"), (OTHER, "other::")] {
        let n = count(tag);
        if n != 1 {
            return Err(format!("the ACL has {n} {name} entries, not one"));
        }
    }
    let named = count(USER) + count(GROUP);
    match count(MASK) {
        0 if named > 0 => return Err("the ACL has named entries and no mask:: entry".into()),
        n if n > 1 => return Err(format!("the ACL has {n} mask:: entries")),
        _ => {}
    }
    let mut ids: Vec<(u16, u32)> = entries.iter().filter(|e| e.0 == USER || e.0 == GROUP).map(|e| (e.0, e.2)).collect();
    ids.sort_unstable();
    if let Some(w) = ids.windows(2).find(|w| w[0] == w[1]) {
        let what = if w[0].0 == USER { "user" } else { "group" };
        return Err(format!("the ACL has two entries of the {what} {}", w[0].1));
    }
    Ok(())
}

/// A part of the text for a message: cut at 64 bytes.
fn shown(s: &str) -> String {
    let end = (0..=s.len().min(64)).rev().find(|i| s.is_char_boundary(*i)).unwrap_or(0);
    if end < s.len() { format!("{}...", &s[..end]) } else { s.to_string() }
}

#[cfg(test)]
mod tests;
