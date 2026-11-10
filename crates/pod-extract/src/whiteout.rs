//! `TODO/extract.md` T-0303: a whiteout is matched on the **basename**, never
//! with a path glob.
//!
//! ⭐ **Measured here, and the measurement contradicted the first reading of
//! the code.** `experiments/70-whiteout-contract.sh` check A reads the first
//! member of every layer of two pinned images and gets `bin/`, `bin` and
//! `etc/`: no `./` prefix anywhere. Check D then runs udocker's own selector,
//! `tar t --wildcards -f LAYER '*/.wh.*'`
//! (`references/indigo-dc__udocker/tree/udocker/container/structure.py:242`),
//! against a crafted archive with one whiteout at the layer root and one
//! nested, and **the root one is missed**. The output is in
//! `experiments/results/whiteout-contract.txt`.
//!
//! ⛔ The defect is the anchor. `*/.wh.*` needs a slash before the marker, and a
//! whiteout at the root of a layer has none, because real OCI layers do not
//! write a `./` prefix. There is no glob that is both correct for a prefixed
//! archive and a bare one, which is why this is a basename test and not a
//! better glob.

/// The prefix, and the opaque marker, exactly as the OCI layer specification
/// writes them.
pub const WH: &str = ".wh.";
pub const OPQ: &str = ".wh..wh..opq";

/// What an entry's name says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// An ordinary entry, extracted as itself.
    Normal,
    /// `.wh.<name>`: remove `<name>` from the accumulated tree, in the
    /// directory this marker sits in. The payload carried is the name to
    /// remove, not the marker.
    Remove(String),
    /// `.wh..wh..opq`: remove the **contents** of the directory this marker
    /// sits in, keeping the directory itself. What
    /// `references/indigo-dc__udocker/tree/udocker/container/structure.py:249-256`
    /// does by listing that directory and removing each entry.
    Opaque,
}

/// Classify one entry path.
///
/// ⛔ The basename, and nothing else. `path` may be prefixed, bare, or carry a
/// trailing slash; all three reach the same answer, which is the property the
/// glob did not have.
pub fn classify(path: &str) -> Kind {
    let base = basename(path);
    if base == OPQ {
        return Kind::Opaque;
    }
    match base.strip_prefix(WH) {
        // ⚠ `.wh.` with nothing after it names no file. It is left as an
        // ordinary entry rather than turned into a removal of "", which would
        // be a removal of the directory the marker sits in.
        Some("") | None => Kind::Normal,
        Some(name) => Kind::Remove(name.to_string()),
    }
}

/// The last non-empty path component. A trailing slash is a directory marker,
/// not a component.
pub fn basename(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}

/// The directory part, as components, with the basename dropped.
pub fn dirname_parts(path: &str) -> Vec<&str> {
    let trimmed = path.trim_end_matches('/');
    let mut parts: Vec<&str> = trimmed
        .split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .collect();
    parts.pop();
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ THE MEASURED CASE. `experiments/70-whiteout-contract.sh` check D
    /// crafts an archive with a whiteout at the layer root and one nested, runs
    /// udocker's `*/.wh.*` selector over it, and the root one is missed. Both
    /// are found here, and that is the entire point of T-0303.
    #[test]
    fn a_root_level_whiteout_is_found_and_a_glob_would_miss_it() {
        assert_eq!(classify(".wh.foo"), Kind::Remove("foo".into()));
        assert_eq!(classify("a/.wh.foo"), Kind::Remove("foo".into()));
        // The selector that produced the defect, for the record: it needs a
        // slash before the marker, and the root-level name has none.
        assert!(!".wh.foo".contains("/.wh."));
        assert!("a/.wh.foo".contains("/.wh."));
    }

    /// ⚠ Real layers carry no `./`, measured in check A. A prefixed archive is
    /// still handled, because the basename is the same either way.
    #[test]
    fn prefixed_and_bare_reach_the_same_answer() {
        assert_eq!(classify("./.wh.foo"), Kind::Remove("foo".into()));
        assert_eq!(classify("./a/.wh.foo"), Kind::Remove("foo".into()));
    }

    #[test]
    fn opaque_is_its_own_kind_and_is_not_a_removal_of_a_file_called_wh_opq() {
        assert_eq!(classify(".wh..wh..opq"), Kind::Opaque);
        assert_eq!(classify("var/cache/.wh..wh..opq"), Kind::Opaque);
        assert_eq!(
            dirname_parts("var/cache/.wh..wh..opq"),
            vec!["var", "cache"]
        );
        assert!(dirname_parts(".wh..wh..opq").is_empty());
    }

    #[test]
    fn an_ordinary_name_that_merely_contains_wh_is_normal() {
        assert_eq!(classify("etc/passwd"), Kind::Normal);
        assert_eq!(classify("a.wh.b"), Kind::Normal);
        assert_eq!(classify("x/y.wh.z"), Kind::Normal);
        // ⚠ A directory whose own name starts with the marker is still a
        // removal; the trailing slash must not change the answer.
        assert_eq!(classify("a/.wh.d/"), Kind::Remove("d".into()));
    }

    /// `.wh.` alone names nothing, and turning it into `Remove("")` would
    /// delete the directory the marker sits in.
    #[test]
    fn a_bare_marker_removes_nothing() {
        assert_eq!(classify(".wh."), Kind::Normal);
        assert_eq!(classify("a/.wh."), Kind::Normal);
    }
}
