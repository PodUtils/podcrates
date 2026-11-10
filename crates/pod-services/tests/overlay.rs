//! The overlay: a consumer's own files of the same schema over the
//! built-in set (feature `overlay`).
#![cfg(feature = "overlay")]

use pod_services::{Catalog, Origin};

const MY_RELAY: &str = r#"
id = "my-relay"
kind = "relay"
party = "The user"
about = "A relay of the user's own"

[rank]
value = 0
source = { ruling = "the user's setting", read = 2026-11-06 }

[[endpoint]]
name = "default"
scheme = "https"
host = "relay.example"
port = 8443
source = { ruling = "the user's setting", read = 2026-11-06 }

[[watch]]
kind = "tls-name"
endpoint = "default"
"#;

#[test]
fn a_file_of_the_overlay_adds_a_service() {
    let mut catalog = Catalog::new();
    let laid = catalog.lay("my-relay.toml", MY_RELAY).unwrap();
    assert_eq!(laid.endpoint("default").unwrap().url(), "https://relay.example:8443");
    let relays: Vec<&str> = catalog.of_kind("relay").map(|s| s.id()).collect();
    // A rank of 0 each, then by id; the built-in relays with no rank (croc)
    // come after.
    assert_eq!(relays[..2], ["ajam-relay", "my-relay"], "a rank of 0 each; then by id");
    assert_eq!(relays.len(), pod_services::of_kind("relay").count() + 1);
    assert_eq!(catalog.iter().count(), pod_services::all().len() + 1);
    assert!(pod_services::get("my-relay").is_none(), "the built-in set does not change");
}

#[test]
fn a_file_with_the_id_of_a_built_in_service_replaces_it() {
    let text = MY_RELAY.replace("id = \"my-relay\"", "id = \"ajam-relay\"");
    let catalog = Catalog::with_overlay([("ajam-relay.toml", text.as_str())]).unwrap();
    let relay = catalog.get("ajam-relay").unwrap();
    assert_eq!(relay.endpoint("default").unwrap().host(), "relay.example");
    assert!(relay.endpoint("seed-us-east-1").is_none(), "the whole service is replaced");
    assert!(
        matches!(relay.endpoint("default").unwrap().source().origin(), Origin::Ruling(r) if r == "the user's setting")
    );
    assert_eq!(catalog.of_kind("relay").count(), pod_services::of_kind("relay").count());
    assert_eq!(catalog.iter().count(), pod_services::all().len());
}

#[test]
fn a_file_that_breaks_the_schema_is_an_error_of_the_settings() {
    let text =
        MY_RELAY.replace("source = { ruling = \"the user's setting\", read = 2026-11-06 }\n\n[[watch]]", "\n[[watch]]");
    let mut catalog = Catalog::new();
    let err = catalog.lay("/home/user/relay.toml", &text).unwrap_err();
    assert_eq!(err.label, "/home/user/relay.toml");
    assert_eq!(err.field, "endpoint \"default\"");
    assert!(err.why.starts_with("no source"), "{}", err.why);
    assert_eq!(err.class(), pod_core::Class::Config);
    assert!(err.to_string().starts_with("/home/user/relay.toml: endpoint \"default\": no source"));
    assert!(catalog.overlay().is_empty(), "nothing is laid");
    // A label with a line break is shown on one line.
    let err = catalog.lay("a\nb.toml", "id = ").unwrap_err();
    assert!(!err.to_string().contains('\n'), "{err}");
}

#[test]
fn one_id_is_laid_once() {
    let mut catalog = Catalog::new();
    catalog.lay("one.toml", MY_RELAY).unwrap();
    let err = catalog.lay("two.toml", MY_RELAY).unwrap_err();
    assert_eq!(err.to_string(), "two.toml: id: the id \"my-relay\" is laid already");
    assert_eq!(catalog.overlay().len(), 1);
}

#[test]
fn an_overlay_gives_the_text_of_a_fact_in_its_value() {
    let pem = MY_RELAY.replace(
        "[[watch]]",
        "[[fact]]\nname = \"roots\"\nvalue = \"\"\"\n-----BEGIN CERTIFICATE-----\nAA==\n-----END CERTIFICATE-----\n\"\"\"\nsource = { ruling = \"the user's setting\", read = 2026-11-06 }\n\n[[watch]]",
    );
    let mut catalog = Catalog::new();
    let laid = catalog.lay("my-relay.toml", &pem).unwrap();
    assert!(laid.text("roots").unwrap().starts_with("-----BEGIN CERTIFICATE-----\n"));
    let file = MY_RELAY.replace(
        "[[watch]]",
        "[[fact]]\nname = \"roots\"\nfile = \"roots.pem\"\nsource = { ruling = \"the user's setting\", read = 2026-11-06 }\n\n[[watch]]",
    )
    .replace("my-relay", "other-relay");
    let err = catalog.lay("other.toml", &file).unwrap_err();
    assert_eq!(err.field, "fact \"roots\".file");
    assert_eq!(err.why, "an overlay gives the text in value, not a file");
}
