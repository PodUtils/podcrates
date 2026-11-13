//! The file of the TLS groups that servers took, in the directories of the
//! consumer's cache: a private file, as `pod_core::files` keeps one (mode
//! 0600 in a directory of this user's), and one that others can read is not
//! read. It holds names and group numbers, and no secret.

use std::path::PathBuf;

use super::GroupFile;

/// The file, in the cache.
pub const FILE: &str = "tls-groups.txt";

/// The file in the first of these directories that takes it. A consumer
/// gives its cache's directories (`pod_core::files::candidate_dirs_from`)
/// once, before its first dial: `context.groups().keep_in(Box::new(Dirs(dirs)))`.
pub struct Dirs(pub Vec<PathBuf>);

impl GroupFile for Dirs {
    fn load(&self) -> Option<String> {
        pod_core::files::load_file_from(&self.0, FILE)
    }

    fn store(&self, text: &[u8]) {
        // A file that cannot be kept costs a later run its hint alone.
        let _ = pod_core::files::store_file_in_first(&self.0, FILE, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::groups::HEADER;
    use crate::groups::Kept;

    /// A test's directory of its own, gone when the test ends.
    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("pod-tls-groups-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Scratch(dir)
    }

    #[cfg(unix)]
    fn now_ms() -> i64 {
        let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
    }

    /// A run keeps the group in a private file of the cache, and a later run
    /// reads it there.
    #[test]
    fn the_group_goes_to_a_private_file_and_back() {
        let dir = scratch("round");
        let first = Kept::new();
        first.keep_in(Box::new(Dirs(vec![dir.0.clone()])));
        first.took("relay.example", 23);
        let path = dir.0.join(FILE);
        let text = std::fs::read_to_string(&path).expect("the file");
        assert!(text.contains("relay.example 23 "), "{text}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "the file is the user's alone");
        }
        let later = Kept::new();
        later.keep_in(Box::new(Dirs(vec![dir.0.clone()])));
        assert_eq!(later.group("relay.example"), Some(23));
    }

    /// A file of the cache that does not read gives no group, and a note.
    #[test]
    fn a_file_that_does_not_read_is_set_aside_with_a_note() {
        let dir = scratch("garbage");
        pod_core::files::store_file_in_first(std::slice::from_ref(&dir.0), FILE, b"garbage").expect("a private file");
        let kept = Kept::new();
        kept.keep_in(Box::new(Dirs(vec![dir.0.clone()])));
        assert_eq!(kept.group("relay.example"), None);
        assert!(kept.take_note().is_some_and(|note| note.contains("set aside")));
    }

    /// A file that others can read is not read, as no file of the cache is.
    #[cfg(unix)]
    #[test]
    fn a_file_that_others_can_read_is_not_read() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("open");
        // A group of now, which would be read from a private file.
        let text = format!("{HEADER}\nrelay.example 23 {}\n", now_ms());
        let path = pod_core::files::store_file_in_first(std::slice::from_ref(&dir.0), FILE, text.as_bytes())
            .expect("a private file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let kept = Kept::new();
        kept.keep_in(Box::new(Dirs(vec![dir.0.clone()])));
        assert_eq!(kept.group("relay.example"), None);
        assert_eq!(kept.take_note(), None, "no file read, nothing set aside");
    }
}
