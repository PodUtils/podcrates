//! The tests of the schema check: each rule of the README's schema refuses
//! its fault with the file and the field, and the built-in files pass.

use crate::schema::{self, OriginSpec, Spec, ValueSpec};

/// A service that holds to the schema; each test changes one part of it.
const GOOD: &str = r#"
id = "example-relay"
kind = "relay"
party = "Example"
about = "A relay of a test"

[[endpoint]]
name = "default"
scheme = "https"
host = "relay.example"
port = 443
source = { url = "https://relay.example/relays.json", read = 2026-11-06 }

[[fact]]
name = "idle-timeout-ms"
value = 180000
unit = "ms"
source = { measured = "a session with no traffic closed after 180 s", read = 2026-11-06 }

[[watch]]
kind = "http-json"
url = "https://relay.example/relays.json"
pointer = "/limits"
"#;

fn parse(text: &str) -> Result<Spec, String> {
    schema::parse("services/example-relay.toml", text, true).map_err(|f| f.to_string())
}

fn fault(text: &str) -> String {
    parse(text).expect_err("the file must fail")
}

#[test]
fn a_file_that_holds_to_the_schema_is_read() {
    let spec = parse(GOOD).unwrap();
    assert_eq!((spec.id.as_str(), spec.kind.as_str()), ("example-relay", "relay"));
    assert_eq!(spec.endpoints[0].port, 443);
    assert_eq!(spec.facts[0].value, ValueSpec::Int(180000));
    assert_eq!(
        spec.facts[0].source.origin,
        OriginSpec::Measured("a session with no traffic closed after 180 s".into())
    );
    assert_eq!(spec.watches[0].kind, "http-json");
    assert_eq!(spec.watches[0].args.len(), 2);
}

#[test]
fn a_value_with_no_source_fails_with_the_file_and_the_field() {
    let text = GOOD
        .replace("source = { measured = \"a session with no traffic closed after 180 s\", read = 2026-11-06 }\n", "");
    assert_eq!(
        fault(&text),
        "services/example-relay.toml: fact \"idle-timeout-ms\": no source (a table with url, measured or ruling, and read)"
    );
    let text = GOOD.replace("source = { url = \"https://relay.example/relays.json\", read = 2026-11-06 }\n", "");
    assert!(
        fault(&text).starts_with("services/example-relay.toml: endpoint \"default\": no source"),
        "{}",
        fault(&text)
    );
}

#[test]
fn a_source_names_one_origin_and_the_date_it_was_read() {
    let no_date = GOOD.replace(", read = 2026-11-06 }\n\n[[fact]]", " }\n\n[[fact]]");
    assert!(fault(&no_date).ends_with("endpoint \"default\".source.read: missing: the date when the value was read"));
    let two = GOOD.replace(
        "{ url = \"https://relay.example/relays.json\",",
        "{ url = \"https://relay.example/\", ruling = \"Q1\",",
    );
    assert!(fault(&two).ends_with("more than one of url, measured and ruling"), "{}", fault(&two));
    let none = GOOD.replace("{ url = \"https://relay.example/relays.json\",", "{");
    assert!(fault(&none).ends_with("endpoint \"default\".source: no url, measured or ruling"));
    let text_date = GOOD.replace("read = 2026-11-06 }\n\n[[fact]]", "read = \"2026-11-06\" }\n\n[[fact]]");
    assert!(fault(&text_date).contains("not a date"), "{}", fault(&text_date));
    let with_time = GOOD.replace("read = 2026-11-06 }\n\n[[fact]]", "read = 2026-11-06T10:00:00Z }\n\n[[fact]]");
    assert!(fault(&with_time).contains("with no time"), "{}", fault(&with_time));
    let not_a_url = GOOD.replace("url = \"https://relay.example/relays.json\"", "url = \"relay.example\"");
    assert!(fault(&not_a_url).ends_with("not an https:// or http:// URL"));
}

#[test]
fn each_service_names_a_watch() {
    let cut = GOOD.find("[[watch]]").unwrap();
    assert_eq!(
        fault(&GOOD[..cut]),
        "services/example-relay.toml: watch: no watch: each service names at least one [[watch]]"
    );
    let no_kind = GOOD.replace("kind = \"http-json\"", "");
    assert!(fault(&no_kind).ends_with("watch 0.kind: missing"));
}

#[test]
fn a_kind_is_a_word_and_a_new_kind_needs_no_change_of_the_schema() {
    let spaced = GOOD.replace("kind = \"relay\"", "kind = \"Relay Server\"");
    assert!(fault(&spaced).ends_with("top.kind: \"Relay Server\" is not a word (a-z, 0-9, -)"));
    let new = GOOD.replace("kind = \"relay\"", "kind = \"carrier-pigeon\"");
    assert_eq!(parse(&new).unwrap().kind, "carrier-pigeon");
    for bad in ["", "-relay", "relay-", "re--lay", "1relay", "relay_server"] {
        assert!(!schema::is_word(bad), "{bad:?}");
    }
}

#[test]
fn a_key_that_the_schema_does_not_name_fails() {
    let typo = GOOD.replace("unit = \"ms\"", "unit = \"ms\"\nsorce = \"x\"");
    assert!(fault(&typo).contains("fact 0: the key \"sorce\" is not one of"), "{}", fault(&typo));
    let top = format!("colour = \"red\"\n{GOOD}");
    assert!(fault(&top).contains("top: the key \"colour\""));
}

#[test]
fn the_ids_are_unique_and_each_file_is_named_for_its_id() {
    let spec = parse(GOOD).unwrap();
    let ok = [("services/example-relay.toml", &spec)];
    assert_eq!(schema::check_set(ok.into_iter(), true), Ok(()));
    let misnamed = [("services/relay.toml", &spec)];
    let why = schema::check_set(misnamed.into_iter(), true).unwrap_err().to_string();
    assert_eq!(why, "services/relay.toml: id: the id \"example-relay\" is not the name of its file (relay.toml)");
    let twice = [("a/example-relay.toml", &spec), ("b/example-relay.toml", &spec)];
    let why = schema::check_set(twice.into_iter(), true).unwrap_err().to_string();
    assert!(why.ends_with("the id \"example-relay\" is the id of a/example-relay.toml too"), "{why}");
}

#[test]
fn the_values_are_typed() {
    let fact = |value: &str, kind: &str| {
        let kind = if kind.is_empty() { String::new() } else { format!("type = \"{kind}\"\n") };
        GOOD.replace("value = 180000\n", &format!("value = {value}\n{kind}"))
    };
    assert_eq!(
        parse(&fact("[\"a\", \"b\"]", "")).unwrap().facts[0].value,
        ValueSpec::Texts(vec!["a".into(), "b".into()])
    );
    assert_eq!(parse(&fact("[1, 2]", "ints")).unwrap().facts[0].value, ValueSpec::Ints(vec![1, 2]));
    assert_eq!(parse(&fact("[]", "texts")).unwrap().facts[0].value, ValueSpec::Texts(Vec::new()));
    assert!(fault(&fact("[]", "")).ends_with("an empty list needs type = \"texts\" or \"ints\""));
    assert!(fault(&fact("[1, \"a\"]", "")).ends_with("a list holds texts alone or integers alone"));
    assert!(fault(&fact("1.5", "")).ends_with("a value is a text, an integer, a bool, a date or a list"));
    assert!(fault(&fact("\"x\"", "int")).ends_with("the type is int, and the value is of type text"));
    assert!(fault(&fact("1", "number")).contains("\"number\" is not one of text, int, bool, date, texts, ints"));
}

#[test]
fn an_endpoint_is_checked_part_by_part() {
    let swap = |from: &str, to: &str| fault(&GOOD.replace(from, to));
    assert!(swap("port = 443", "port = 0").ends_with("endpoint \"default\".port: not a port (1 to 65535)"));
    assert!(swap("host = \"relay.example\"", "host = \"relay example\"").contains(".host: \"relay example\" is not"));
    assert!(swap("scheme = \"https\"", "scheme = \"HTTPS\"").contains(".scheme: \"HTTPS\" is not a scheme"));
    let address = "host = \"relay.example\"\naddresses = [\"192.0.2.300\"]";
    assert!(swap("host = \"relay.example\"", address).ends_with("\"192.0.2.300\" is not an IP address"));
    assert!(swap("port = 443", "port = 443\npath = \"relays.json\"").contains("a path starts with /"));
    let named = "name = \"default\"\nname2 = 1";
    assert!(swap("name = \"default\"", named).contains("the key \"name2\""));
    let second = GOOD.replace("[[fact]]", "[[endpoint]]\nname = \"default\"\nscheme = \"https\"\nhost = \"b.example\"\nport = 443\nsource = { ruling = \"Q1\", read = 2026-11-06 }\n\n[[fact]]");
    assert_eq!(fault(&second), "services/example-relay.toml: endpoint \"default\": a second endpoint of this name");
}

#[test]
fn an_overlay_gives_each_text_in_its_value() {
    let file = GOOD.replace("value = 180000\nunit = \"ms\"", "file = \"files/limits.txt\"");
    assert_eq!(parse(&file).unwrap().facts[0].value, ValueSpec::File("files/limits.txt".into()));
    let why = schema::parse("my-relay.toml", &file, false).unwrap_err().to_string();
    assert_eq!(why, "my-relay.toml: fact \"idle-timeout-ms\".file: an overlay gives the text in value, not a file");
    let up = GOOD.replace("value = 180000\nunit = \"ms\"", "file = \"../Cargo.toml\"");
    assert!(fault(&up).ends_with("not a path of a text file"));
}

#[test]
fn text_that_is_not_toml_names_its_file() {
    assert!(fault("id = ").starts_with("services/example-relay.toml: TOML: "), "{}", fault("id = "));
}

/// The built-in files hold to the schema at test time too, through the
/// same reader as the build's.
#[test]
fn each_built_in_file_holds_to_the_schema() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("services");
    let mut specs = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "toml") {
            let file = format!("services/{}", path.file_name().unwrap().to_string_lossy());
            let spec = schema::parse(&file, &std::fs::read_to_string(&path).unwrap(), true).unwrap();
            specs.push((file, spec));
        }
    }
    assert_eq!(specs.len(), crate::all().len());
    assert_eq!(schema::check_set(specs.iter().map(|(f, s)| (f.as_str(), s)), true), Ok(()));
}
