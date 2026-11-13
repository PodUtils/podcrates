//! The public file hosts (PodUtils/podssh#220, for PodUtils/podssh@T-738):
//! each service of the kind `file-host` has each fact of its kind, of its
//! type and form, an endpoint `upload` (https, 443, a path), a source with a
//! date for each value, and a watch. podssh's order is the order of the
//! ranks; a host whose terms refuse automated uploads has no rank and a
//! note that says why.

use pod_services::{Origin, Service};

fn hosts() -> Vec<&'static Service> {
    pod_services::of_kind("file-host").collect()
}

fn is_host(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '.'])
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
}

#[test]
fn the_file_hosts_are_in_podssh_s_order() {
    let ids: Vec<&str> = hosts().iter().map(|s| s.id()).collect();
    assert_eq!(ids, ["tmpfiles-org", "litterbox", "temp-sh", "null-pointer-0x0"]);
}

#[test]
fn each_file_host_has_each_fact_of_its_kind() {
    for s in hosts() {
        let id = s.id();
        let upload = s.endpoint("upload").unwrap_or_else(|| panic!("{id}: no endpoint upload"));
        assert_eq!((upload.scheme(), upload.port()), ("https", 443), "{id}");
        assert!(upload.path().is_some_and(|p| p.starts_with('/')), "{id}: a path");
        for other in s.endpoints().iter().filter(|e| e.name() != "upload") {
            assert_eq!(other.role(), Some("document"), "{id}: {} is a document", other.name());
        }
        let field = s.text("file-field").unwrap_or_else(|| panic!("{id}: file-field"));
        assert!(!field.is_empty() && field.chars().all(|c| c.is_ascii_alphanumeric()), "{id}: {field}");
        for pair in s.texts("fields").unwrap_or_else(|| panic!("{id}: fields")) {
            let (name, _) = pair.split_once('=').unwrap_or_else(|| panic!("{id}: {pair} is no name=value"));
            assert!(!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric()), "{id}: {pair}");
        }
        assert!(s.int("life-seconds").is_some_and(|l| l > 0), "{id}: life-seconds");
        assert_eq!(s.fact("life-seconds").and_then(|f| f.unit()), Some("s"), "{id}");
        match s.text("answer") {
            Some("json") => assert!(s.text("answer-pointer").is_some_and(|p| p.starts_with('/')), "{id}: pointer"),
            Some("text") => assert!(s.fact("answer-pointer").is_none(), "{id}: a pointer of a text answer"),
            other => panic!("{id}: answer {other:?}"),
        }
        let serves = s.texts("serves").unwrap_or_else(|| panic!("{id}: serves"));
        assert!(!serves.is_empty() && serves.iter().all(|h| is_host(h)), "{id}: {serves:?}");
        let segment = s.text("fetch-segment").unwrap_or_else(|| panic!("{id}: fetch-segment"));
        assert!(segment.chars().all(|c| c.is_ascii_alphanumeric()), "{id}: the segment {segment:?}");
        assert!(matches!(s.text("fetch-method"), Some("GET" | "POST")), "{id}: fetch-method");
        match s.text("delete") {
            Some("none") => assert!(s.fact("delete-token-header").is_none(), "{id}"),
            Some("token-form") => {
                assert!(s.text("delete-token-header").is_some_and(|h| !h.is_empty()), "{id}");
                assert!(s.text("delete-token-field").is_some_and(|f| !f.is_empty()), "{id}");
                assert!(s.texts("delete-fields").is_some(), "{id}");
            }
            other => panic!("{id}: delete {other:?}"),
        }
        assert!(s.int("upload-bytes-most").is_some_and(|b| b > 0), "{id}: upload-bytes-most");
        assert_eq!(s.fact("upload-bytes-most").and_then(|f| f.unit()), Some("bytes"), "{id}");
        assert!(s.fact("uploads-per-hour").is_none_or(|f| f.value().as_int().is_some_and(|n| n > 0)), "{id}");
        assert!(s.bool("one-person").is_some(), "{id}: one-person");
        let refused = s.bool("automated-uploads-refused").unwrap_or_else(|| panic!("{id}: automated-uploads-refused"));
        if refused {
            assert!(s.rank().is_none(), "{id}: a host that refuses automated uploads has no rank");
            assert!(s.text("note").is_some_and(|n| n.contains("automated")), "{id}: a note says why");
        } else {
            assert!(s.rank().is_some(), "{id}: no rank");
        }
        assert!(!s.watches().is_empty(), "{id}: no watch");
    }
}

#[test]
fn each_value_of_a_file_host_has_a_source_and_a_date() {
    for s in hosts() {
        let sources = s
            .endpoints()
            .iter()
            .map(|e| e.source())
            .chain(s.facts().iter().map(|f| f.source()))
            .chain(s.rank().map(|r| r.source()));
        for source in sources {
            assert!(source.read().year() >= 2026, "{}", s.id());
            match source.origin() {
                Origin::Url(u) => assert!(u.starts_with("https://"), "{}: {u}", s.id()),
                Origin::Ruling(r) => assert!(r.contains("PodUtils/podssh#220"), "{}: {r}", s.id()),
                Origin::Measured(m) => panic!("{}: a measurement {m}: no upload is made", s.id()),
            }
        }
    }
}

#[test]
fn the_values_of_the_file_hosts() {
    let tmp = pod_services::get("tmpfiles-org").expect("tmpfiles-org");
    assert_eq!(tmp.endpoint("upload").unwrap().url(), "https://tmpfiles.org/api/v1/upload");
    assert_eq!(tmp.texts("fields").unwrap(), ["expire=60"]);
    assert_eq!((tmp.int("life-seconds"), tmp.text("answer-pointer")), (Some(60), Some("/data/url")));
    assert_eq!((tmp.text("fetch-segment"), tmp.int("uploads-per-hour")), (Some("dl"), Some(200)));
    let litter = pod_services::get("litterbox").expect("litterbox");
    assert_eq!(litter.texts("fields").unwrap(), ["reqtype=fileupload", "time=1h"]);
    assert_eq!(litter.texts("serves").unwrap(), ["litter.catbox.moe"]);
    let temp = pod_services::get("temp-sh").expect("temp-sh");
    assert!(temp.texts("fields").unwrap().is_empty(), "temp.sh takes no field of life");
    assert_eq!((temp.text("fetch-method"), temp.bool("one-person")), (Some("POST"), Some(true)));
    let null = pod_services::get("null-pointer-0x0").expect("null-pointer-0x0");
    assert_eq!(null.endpoint("upload").unwrap().url(), "https://0x0.st/");
    assert_eq!(null.texts("fields").unwrap(), ["secret=", "expires=1"]);
    assert_eq!((null.text("delete-token-header"), null.text("delete-token-field")), (Some("X-Token"), Some("token")));
    assert_eq!(null.texts("delete-fields").unwrap(), ["delete="]);
    assert_eq!(null.bool("user-agent-names-program"), Some(true));
}
