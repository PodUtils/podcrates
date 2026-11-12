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

/// Each watch of each built-in service has the arguments of its kind, and
/// each name in them is an endpoint, a role, a fact or a service of the
/// data (T-324).
#[test]
fn each_watch_of_the_data_has_the_arguments_of_its_kind() {
    let mut faults = Vec::new();
    for service in pod_services::all() {
        for watch in service.watches() {
            for fault in pod_services_watch::schema::faults(service, watch, |id| pod_services::get(id).is_some()) {
                faults.push(format!("{} {}: {fault}", service.id(), watch.kind()));
            }
        }
    }
    assert!(faults.is_empty(), "{faults:#?}");
}

/// Each kind has its table of arguments.
#[test]
fn each_kind_has_a_table_of_arguments() {
    for kind in pod_services_watch::kinds() {
        assert!(pod_services_watch::schema::arguments(kind.kind()).is_some(), "{}", kind.kind());
    }
}
