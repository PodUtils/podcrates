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

/// Each built-in service of a kind with a table of facts has each fact and
/// each endpoint of its kind, of its type (T-416).
#[test]
fn each_service_has_the_facts_of_its_kind() {
    let mut faults = Vec::new();
    for service in pod_services::all() {
        for fault in pod_services_watch::schema::service_faults(service) {
            faults.push(format!("{}: {fault}", service.id()));
        }
    }
    assert!(faults.is_empty(), "{faults:#?}");
    assert!(pod_services::of_kind("file-host").next().is_some(), "the data has a file host");
}

/// A file host of a consumer's own file with a fact missing, a fact of
/// another type, a word that the kind does not name, a fact that the kind
/// does not name, and a word that needs a fact: each one is a fault.
#[test]
fn a_file_host_with_faults_is_found_before_the_network() {
    let text = r#"
id = "own-host"
kind = "file-host"
party = "the test"
about = "a file host of the test"

[[endpoint]]
name = "upload"
scheme = "https"
host = "files.example"
port = 443
path = "/upload"
source = { url = "https://files.example/", read = 2026-11-13 }

[[fact]]
name = "file-field"
value = 7
source = { url = "https://files.example/", read = 2026-11-13 }

[[fact]]
name = "answer"
value = "json"
source = { url = "https://files.example/", read = 2026-11-13 }

[[fact]]
name = "delete"
value = "never"
source = { url = "https://files.example/", read = 2026-11-13 }

[[fact]]
name = "colour"
value = "blue"
source = { url = "https://files.example/", read = 2026-11-13 }

[[watch]]
kind = "tls-name"
endpoint = "upload"
"#;
    let mut catalog = pod_services::Catalog::new();
    let service = catalog.lay("own-host.toml", text).expect("a file of the schema");
    let faults = pod_services_watch::schema::service_faults(service).join("\n");
    for want in [
        "no fact \"fields\" (texts)",
        "no fact \"one-person\" (bool)",
        "the fact \"file-field\" is of the type int, not text",
        "the fact \"delete\" is \"never\", not one of none, token-form",
        "the fact \"colour\" is not one of the kind \"file-host\"",
        "the fact \"answer\" \"json\" needs the fact \"answer-pointer\"",
    ] {
        assert!(faults.contains(want), "{want:?} is not in:\n{faults}");
    }
    assert!(!faults.contains("endpoint"), "the endpoint upload is there:\n{faults}");
}
