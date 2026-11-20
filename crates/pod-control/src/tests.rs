//! The catalogue is the dispatch, in both directions; the tiers; the lines
//! of the wire against the schema file.
//!
//! After a change of the catalogue or the codes, write the schema file
//! again: `POD_CONTROL_WRITE_SCHEMAS=1 cargo test -p pod-control schema`.

use std::path::Path;

use serde_json::{Value, json};

use crate::catalogue::{Catalogue, CatalogueFault, Extra, Method, Route, Tier, is_method_name};
use crate::dispatch::{self, DELEGATED, LOCAL};
use crate::host::Peer;
use crate::schema::{SCHEMA_ID, schema};
use crate::server::{Settings, admit};
use crate::wire::{self, Code, Fault, Inbound, PROTOCOL};

#[test]
fn each_method_of_the_catalogue_is_served() {
    for m in Method::ALL {
        let s = m.spec();
        let table: Vec<&str> = match s.route {
            Route::Local => LOCAL.iter().map(|(n, _)| *n).collect(),
            Route::Delegated => DELEGATED.iter().map(|(n, _)| *n).collect(),
        };
        assert!(table.contains(&s.name), "{} is not served by its route", s.name);
        assert!(dispatch::find(&Catalogue::core(), s.name, Tier::Master).is_ok(), "{}", s.name);
    }
}

#[test]
fn each_served_method_is_in_the_catalogue() {
    for (name, route) in
        LOCAL.iter().map(|(n, _)| (*n, Route::Local)).chain(DELEGATED.iter().map(|(n, _)| (*n, Route::Delegated)))
    {
        let spec = Method::from_name(name).map(Method::spec);
        assert!(spec.is_some(), "{name} is served and not in the catalogue");
        assert_eq!(spec.map(|s| s.route), Some(route), "{name} is served by another route");
    }
    assert_eq!(LOCAL.len() + DELEGATED.len(), Method::ALL.len());
}

#[test]
fn the_local_and_the_delegated_methods_do_not_overlap() {
    for (name, _) in LOCAL {
        assert!(!DELEGATED.iter().any(|(n, _)| n == name), "{name} is local and delegated");
    }
}

#[test]
fn the_names_of_the_catalogue_are_unique_and_of_the_wire_form() {
    for (i, m) in Method::ALL.iter().enumerate() {
        assert!(is_method_name(m.name()), "{}", m.name());
        assert!(!Method::ALL[..i].iter().any(|o| o.name() == m.name()), "{} twice", m.name());
        assert_eq!(Method::from_name(m.name()), Some(*m));
    }
    for bad in ["", "Status", "1st", "a_b", "a b", &"a".repeat(64)] {
        assert!(!is_method_name(bad), "{bad:?}");
    }
}

#[test]
fn a_method_that_outlives_its_token_takes_the_master_tier() {
    for s in Method::ALL.map(Method::spec) {
        if s.outlives {
            assert_eq!(s.tier, Tier::Master, "{} outlives its token", s.name);
        }
    }
    assert!(Method::Reload.spec().outlives, "a reload can bring a new principal");
    let mut e = Extra::new("grant", Tier::Operate, "Add a principal.");
    e.outlives = true;
    assert_eq!(Catalogue::new(vec![e.clone()]), Err(CatalogueFault::Outlives("grant".into())));
    e.tier = Tier::Master;
    assert!(Catalogue::new(vec![e]).is_ok());
}

#[test]
fn an_extra_takes_no_name_of_the_catalogue_and_no_name_twice() {
    let e = |n: &str| Extra::new(n, Tier::Read, "x");
    assert_eq!(Catalogue::new(vec![e("status")]), Err(CatalogueFault::Twice("status".into())));
    assert_eq!(Catalogue::new(vec![e("a"), e("a")]), Err(CatalogueFault::Twice("a".into())));
    assert_eq!(Catalogue::new(vec![e("A")]), Err(CatalogueFault::Name("A".into())));
}

#[test]
fn a_method_above_the_tier_is_found_as_a_name_that_is_not_there() {
    let c = Catalogue::new(vec![Extra::new("rotate", Tier::Master, "x")]).unwrap();
    let unknown = dispatch::find(&c, "no-such-method", Tier::Master).err();
    assert_eq!(unknown, Some(Fault::unknown_method()));
    for (name, tier) in
        [("drain", Tier::Read), ("stop", Tier::Read), ("reload", Tier::Operate), ("rotate", Tier::Operate)]
    {
        assert_eq!(dispatch::find(&c, name, tier).err(), unknown, "{name} at {tier}");
        assert!(dispatch::find(&c, name, Tier::Master).is_ok(), "{name}");
    }
}

#[test]
fn a_peer_of_another_user_is_refused_unless_granted() {
    let mut s = Settings::new("p", "1");
    let peer = |uid| Peer { uid: Some(uid), ..Peer::default() };
    assert_eq!(admit(&s, &peer(1000), Some(1000), None), Some(Tier::Operate));
    assert_eq!(admit(&s, &peer(1001), Some(1000), None), None);
    s.uids.push((1001, Tier::Read));
    assert_eq!(admit(&s, &peer(1001), Some(1000), None), Some(Tier::Read));
    let sid = |s: &str| Peer { sid: Some(s.into()), ..Peer::default() };
    assert_eq!(admit(&s, &sid("S-1-5-21-1"), None, Some("S-1-5-21-1")), Some(Tier::Operate));
    assert_eq!(admit(&s, &sid("S-1-5-21-2"), None, Some("S-1-5-21-1")), None);
    assert_eq!(admit(&s, &Peer::default(), Some(1000), Some("S-1-5-21-1")), None);
}

#[test]
fn bad_lines_are_bad_requests_with_no_byte_of_the_line() {
    for (line, id) in [
        ("x", None),
        ("[1]", None),
        (r#"{"id":1,"method":"status"}"#, Some(1)),
        (r#"{"v":2,"id":1,"method":"status"}"#, Some(1)),
        (r#"{"v":1,"method":"status"}"#, None),
        (r#"{"v":1,"id":-1,"method":"status"}"#, None),
        (r#"{"v":1,"id":3,"method":7}"#, Some(3)),
        (r#"{"v":1,"id":3,"method":"auth","params":"secret-x"}"#, Some(3)),
        (r#"{"v":1,"id":3,"method":"auth","token":"secret-x"}"#, Some(3)),
        (r#"{"v":1,"id":3,"ok":true}"#, Some(3)),
    ] {
        let (got, fault) = wire::parse(line).expect_err(line);
        assert_eq!((got, fault.code), (id, Code::BadRequest), "{line}");
        assert!(!fault.detail.contains("secret"), "{line}: {fault}");
    }
    let parsed = wire::parse(r#"{"v":1,"id":3,"method":"status"}"#).unwrap();
    assert!(matches!(parsed, Inbound::Request(r) if r.params == json!({}) && r.for_id.is_none()));
}

#[test]
fn a_line_of_each_kind_reads_back() {
    let req = wire::request_line(7, "drain", &json!({ "exit": true }), Some(2));
    assert_eq!(req, "{\"v\":1,\"id\":7,\"for\":2,\"method\":\"drain\",\"params\":{\"exit\":true}}\n");
    let Inbound::Request(r) = wire::parse(req.trim_end()).unwrap() else { panic!() };
    assert_eq!((r.id, r.method.as_str(), r.for_id), (7, "drain", Some(2)));
    let err = wire::answer_line(None, &Err(Fault::new(Code::Busy, "later")));
    assert_eq!(
        wire::parse(err.trim_end()).unwrap(),
        Inbound::Answer { id: None, outcome: Err(Fault::new(Code::Busy, "later")) }
    );
    for c in Code::ALL {
        assert_eq!(Code::from_name(c.name()), Some(c));
    }
}

#[test]
fn the_schema_file_is_generated_from_the_catalogue() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/control.v1.schema.json");
    pod_testkit::schema::holds(&file, &schema(), "POD_CONTROL_WRITE_SCHEMAS");
    assert!(SCHEMA_ID.ends_with(&format!("/control.v{PROTOCOL}.schema.json")), "{SCHEMA_ID}");
}

#[test]
fn each_line_of_the_wire_is_valid_for_the_schema_and_a_broken_one_is_not() {
    let (schemas, at) = pod_testkit::schema::compiled(SCHEMA_ID, schema());
    let valid = [
        wire::request_line(1, "status", &json!({}), None),
        wire::request_line(2, "drain", &json!({ "exit": false }), None),
        wire::request_line(3, "auth", &json!({ "token": "made-up" }), None),
        wire::request_line(4, "ask", &json!({ "n": 2 }), None),
        wire::request_line(1, "confirm", &json!({}), Some(4)),
        wire::answer_line(Some(1), &Ok(json!({ "schema": 1 }))),
        wire::answer_line(None, &Err(Fault::new(Code::TooLong, "a line is 64 bytes at most"))),
    ];
    for line in valid {
        let v: Value = serde_json::from_str(&line).unwrap();
        assert!(schemas.validate(&v, at).is_ok(), "the schema refuses {line}");
    }
    for line in [
        r#"{"v":2,"id":1,"method":"status"}"#,
        r#"{"v":1,"id":1,"method":"drain","params":{"exit":"yes"}}"#,
        r#"{"v":1,"id":1,"method":"auth","params":{}}"#,
        r#"{"v":1,"id":1,"method":"status","params":{"x":1}}"#,
        r#"{"v":1,"id":1,"ok":true}"#,
        r#"{"v":1,"id":1,"ok":false,"error":{"code":"no-such-code","detail":""}}"#,
        r#"{"v":1,"id":1,"ok":true,"result":1,"error":{"code":"busy","detail":""}}"#,
    ] {
        let v: Value = serde_json::from_str(line).unwrap();
        assert!(schemas.validate(&v, at).is_err(), "the schema takes {line}");
    }
    let rows = schema()["$defs"]["catalogue"]["const"].as_array().unwrap().len();
    assert_eq!(rows, Method::ALL.len());
}

#[test]
fn metrics_in_the_text_of_prometheus() {
    let mut m = crate::host::Metrics::default();
    m.counters.insert("jobs-done".into(), 3);
    m.gauges.insert("slots".into(), -1);
    assert_eq!(
        m.prometheus("podrunner"),
        "# TYPE podrunner_jobs_done counter\npodrunner_jobs_done 3\n# TYPE podrunner_slots gauge\npodrunner_slots -1\n"
    );
}
