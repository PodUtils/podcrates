//! The pairs that the store keeps, by label: their labels, and the pair with
//! the most time left. podnet's node with no pair name takes that one, and
//! at its pair's expiry the next one that the store got since: a caller's
//! `reverse::RepairHook` over [`newest`].

use std::path::PathBuf;

use super::{Pair, file_name, load_from, now_ms};
use crate::cache::{self, Cache};

/// The labels of the pairs in the store, sorted.
pub fn labels(cache: &Cache) -> Vec<String> {
    labels_from(&cache.read_dirs())
}

/// [`labels`] over explicit directories (for tests).
pub fn labels_from(dirs: &[PathBuf]) -> Vec<String> {
    cache::names_from(dirs, "pair-", ".json")
        .into_iter()
        .filter_map(|name| Some(name.strip_prefix("pair-")?.strip_suffix(".json")?.to_string()))
        .filter(|label| file_name(label).is_ok())
        .collect()
}

/// The pair of the store with the most time left, and its label. A pair
/// that cannot be read is passed over. With `after`, only a pair that ends
/// later than `after` and has time left: the next pair of a node whose pair
/// ends.
pub fn newest(cache: &Cache, after: Option<&Pair>) -> Option<(String, Pair)> {
    newest_from(&cache.read_dirs(), after)
}

/// [`newest`] over explicit directories (for tests).
pub fn newest_from(dirs: &[PathBuf], after: Option<&Pair>) -> Option<(String, Pair)> {
    let floor = after.map_or(i64::MIN, |ending| ending.expires_ms.max(now_ms()));
    labels_from(dirs)
        .into_iter()
        .filter_map(|label| {
            let pair = load_from(dirs, &label).ok().flatten()?;
            Some((label, pair))
        })
        .filter(|(_, pair)| pair.expires_ms > floor)
        .max_by_key(|(_, pair)| pair.expires_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pair::store_in_first;
    use crate::relay::Relay;
    use zeroize::Zeroizing;

    fn pair(name: &str, expires_ms: i64) -> Pair {
        let token = || Zeroizing::new(format!("testonly-{name}-token"));
        Pair {
            relay: Relay { host: "relay.example".into(), port: 443 },
            name: name.into(),
            node_token: token(),
            connect_token: token(),
            stop_token: token(),
            expires_ms,
        }
    }

    /// podnet's pair with no name: the one with the most time left; and its
    /// next pair: one that ends later and has time left.
    #[test]
    fn the_newest_pair_and_the_next_one() {
        let dir = std::env::temp_dir().join(format!("pod-relay-newest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        cache::private_dir(&dir).unwrap();
        let dirs = vec![dir.clone()];
        let now = now_ms();
        assert!(newest_from(&dirs, None).is_none(), "an empty store");
        for (label, ends) in [("old", now + 60_000), ("new", now + 7_200_000), ("gone", now - 1)] {
            store_in_first(&dirs, label, &pair(label, ends)).unwrap();
        }
        std::fs::write(dir.join("pair-bad.json"), "not a pair").unwrap();
        let (label, newest) = newest_from(&dirs, None).expect("a pair");
        assert_eq!((label.as_str(), newest.name.as_str()), ("new", "new"));
        let next = newest_from(&dirs, Some(&pair("old", now + 60_000))).expect("the next pair");
        assert_eq!(next.0, "new");
        assert!(newest_from(&dirs, Some(&newest)).is_none(), "none ends later than the newest");
        // A pair that ended is never the next one, also after a pair that ended before it.
        assert!(newest_from(&dirs, Some(&pair("x", now - 10))).is_some_and(|(l, _)| l == "new"));
        std::fs::remove_file(dir.join("pair-new.json")).unwrap();
        assert!(newest_from(&dirs, Some(&pair("x", now - 10))).is_some_and(|(l, _)| l == "old"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
