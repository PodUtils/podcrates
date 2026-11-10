//! A whiteout is matched on the **basename**, not with a path glob.
//!
//! The first member of each layer of real images is `bin/`, `bin` or `etc/`:
//! no `./` prefix. udocker selects whiteouts with
//! `tar t --wildcards -f LAYER '*/.wh.*'`, and on a crafted archive with one
//! whiteout at the root of the layer and one deeper, **it misses the one at
//! the root**.
//!
//! The defect is the anchor. `*/.wh.*` needs a slash before the marker, and a
//! whiteout at the root of a layer has none, as real OCI layers have no `./`
//! prefix. No glob is correct for both a prefixed and a bare archive, so this
//! is a basename test, not a better glob.

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
    /// `.wh..wh..opq`: remove the **contents** of the directory of this marker,
    /// and keep the directory (udocker lists the directory and removes each
    /// entry).
    Opaque,
}

/// Classify one entry path.
///
/// The basename, and nothing more. `path` can be prefixed, bare, or have a
/// trailing slash; the three give the same answer, which the glob did not.
pub fn classify(path: &str) -> Kind {
    let base = basename(path);
    if base == OPQ {
        return Kind::Opaque;
    }
    match base.strip_prefix(WH) {
        // `.wh.` with nothing after it names no file. It stays an ordinary
        // entry and is not a removal of "", which would remove the directory
        // of the marker.
        Some("") | None => Kind::Normal,
        Some(name) => Kind::Remove(name.to_string()),
    }
}

/// The last non-empty path component. A trailing slash is a directory marker,
/// not a component.
pub fn basename(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path)
}

/// The directory part, as components, with the basename dropped.
pub fn dirname_parts(path: &str) -> Vec<&str> {
    let trimmed = path.trim_end_matches('/');
    let mut parts: Vec<&str> = trimmed.split('/').filter(|c| !c.is_empty() && *c != ".").collect();
    parts.pop();
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The measured case: a whiteout at the root of the layer and one deeper.
    /// udocker's `*/.wh.*` misses the one at the root; both are found here.
    #[test]
    fn a_root_level_whiteout_is_found_and_a_glob_would_miss_it() {
        assert_eq!(classify(".wh.foo"), Kind::Remove("foo".into()));
        assert_eq!(classify("a/.wh.foo"), Kind::Remove("foo".into()));
        // The selector with the defect: it needs a slash before the marker,
        // and the name at the root has none.
        assert!(!".wh.foo".contains("/.wh."));
        assert!("a/.wh.foo".contains("/.wh."));
    }

    /// Real layers have no `./`. A prefixed archive still works: the basename
    /// is the same.
    #[test]
    fn prefixed_and_bare_reach_the_same_answer() {
        assert_eq!(classify("./.wh.foo"), Kind::Remove("foo".into()));
        assert_eq!(classify("./a/.wh.foo"), Kind::Remove("foo".into()));
    }

    #[test]
    fn opaque_is_its_own_kind_and_is_not_a_removal_of_a_file_called_wh_opq() {
        assert_eq!(classify(".wh..wh..opq"), Kind::Opaque);
        assert_eq!(classify("var/cache/.wh..wh..opq"), Kind::Opaque);
        assert_eq!(dirname_parts("var/cache/.wh..wh..opq"), vec!["var", "cache"]);
        assert!(dirname_parts(".wh..wh..opq").is_empty());
    }

    #[test]
    fn an_ordinary_name_that_merely_contains_wh_is_normal() {
        assert_eq!(classify("etc/passwd"), Kind::Normal);
        assert_eq!(classify("a.wh.b"), Kind::Normal);
        assert_eq!(classify("x/y.wh.z"), Kind::Normal);
        // A directory whose name starts with the marker is still a removal;
        // the trailing slash must not change the answer.
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
