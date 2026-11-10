//! The data and the kinds: each watch of each built-in service has its
//! kind here, and each kind here is used by a service.

use std::collections::BTreeSet;

#[test]
fn each_watch_of_the_data_has_its_kind() {
    let words: Vec<&str> = pod_services_watch::kinds().iter().map(|k| k.kind()).collect();
    let unique: BTreeSet<&str> = words.iter().copied().collect();
    assert_eq!(unique.len(), words.len(), "a kind twice: {words:?}");
    let used: BTreeSet<&str> = pod_services::all().iter().flat_map(|s| s.watches()).map(|w| w.kind()).collect();
    let missing: Vec<&&str> = used.difference(&unique).collect();
    assert!(missing.is_empty(), "the data names watch kinds with no implementation: {missing:?}");
    let unused: Vec<&&str> = unique.difference(&used).collect();
    assert!(unused.is_empty(), "kinds that no service uses: {unused:?}");
}
