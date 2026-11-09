//! The policy: its JSON and TOML forms, its checks, and the matching of a
//! host on whole labels and of a path on whole segments.

use pod_broker::policy::{PATH_NEEDS_TLS_END, path_holds};
use pod_broker::target::{Authority, parse_authority};
use pod_broker::{Mode, Policy, Verdict};
use pod_core::Class;

fn at(text: &str) -> Authority {
    parse_authority(text, None).unwrap()
}

const ALLOW: &str = r#"{
  "version": 1,
  "mode": "allow",
  "allow": [
    {"host": "api.github.com", "path": "/v1/repos", "ports": [443, 80]},
    {"host": "*.githubusercontent.com"},
    {"host": "203.0.113.9", "ports": [8443]}
  ],
  "deny": ["198.51.100.0/24", "*.ads.example"]
}"#;

#[test]
fn a_policy_reads_from_json_and_from_toml_the_same() {
    let json = Policy::from_json(ALLOW).unwrap();
    let toml = Policy::from_toml(
        r#"
version = 1
mode = "allow"
deny = ["198.51.100.0/24", "*.ads.example"]

[[allow]]
host = "api.github.com"
path = "/v1/repos"
ports = [443, 80]

[[allow]]
host = "*.githubusercontent.com"

[[allow]]
host = "203.0.113.9"
ports = [8443]
"#,
    )
    .unwrap();
    assert_eq!(json, toml);
    let compiled = json.compile().unwrap();
    assert_eq!((compiled.mode(), compiled.rules(), compiled.deny().len()), (Mode::Allow, 3, 2));
    let back = serde_json::to_string(&json).unwrap();
    assert_eq!(Policy::from_json(&back).unwrap(), json);
}

#[test]
fn a_bad_policy_is_a_failure_of_usage() {
    for bad in [
        r#"{"version":2,"mode":"open"}"#,
        r#"{"version":1,"mode":"closed"}"#,
        r#"{"version":1,"mode":"open","allow":[{"host":"a.example"}]}"#,
        r#"{"version":1,"mode":"allow","allow":[{"host":"a.example","extra":1}]}"#,
        r#"{"version":1,"mode":"allow","bindings":[]}"#,
        r#"{"version":1,"mode":"allow","allow":[{"host":"a..example"}]}"#,
        r#"{"version":1,"mode":"allow","allow":[{"host":"a.example","path":"v1"}]}"#,
        r#"{"version":1,"mode":"allow","allow":[{"host":"a.example","path":"/v1/%2e%2e"}]}"#,
        r#"{"version":1,"mode":"allow","allow":[{"host":"a.example","path":"/v1?x"}]}"#,
        r#"{"version":1,"mode":"allow","allow":[{"host":"a.example","ports":[0]}]}"#,
        r#"{"version":1,"mode":"allow","deny":["10.0.0.0/40"]}"#,
        r#"{"mode":"open"}"#,
    ] {
        let failure = Policy::from_json(bad).and_then(|p| p.compile().map(|_| p)).expect_err(bad);
        assert_eq!(failure.class, Class::Usage, "{bad}: {failure}");
        assert_eq!(failure.class.exit_code(), 64);
    }
}

#[test]
fn a_path_prefix_holds_whole_segments_only() {
    assert!(path_holds("/v1/repos", "/v1/repos"));
    assert!(path_holds("/v1/repos", "/v1/repos/"));
    assert!(path_holds("/v1/repos", "/v1/repos/x/y"));
    assert!(!path_holds("/v1/repos", "/v1/reposx"));
    assert!(!path_holds("/v1/repos", "/v1/repo"));
    assert!(!path_holds("/v1/repos", "/v2/repos"));
    assert!(path_holds("/", "/anything"));
}

#[test]
fn the_rules_match_hosts_on_whole_labels_and_their_ports() {
    let compiled = Policy::from_json(ALLOW).unwrap().compile().unwrap();
    let verdict = |target: &str, path: Option<&str>| compiled.verdict(&at(target), path);
    assert_eq!(verdict("api.github.com:443", Some("/v1/repos/x")), Verdict::Allow("allow[0]".into()));
    assert_eq!(verdict("API.GitHub.com.:80", Some("/v1/repos")), Verdict::Allow("allow[0]".into()));
    assert_eq!(verdict("api.github.com:443", Some("/v1/reposx")), Verdict::Deny("policy:no-rule".into()));
    assert_eq!(verdict("api.github.com:8080", Some("/v1/repos")), Verdict::Deny("policy:no-rule".into()));
    assert_eq!(verdict("raw.githubusercontent.com:443", None), Verdict::Allow("allow[1]".into()));
    assert_eq!(verdict("a.b.githubusercontent.com:22", None), Verdict::Allow("allow[1]".into()));
    for denied in ["githubusercontent.com:443", "evilgithubusercontent.com:443", "githubusercontent.com.evil:443"] {
        assert_eq!(verdict(denied, None), Verdict::Deny("policy:no-rule".into()), "{denied}");
    }
    assert_eq!(verdict("203.0.113.9:8443", None), Verdict::Allow("allow[2]".into()));
    assert_eq!(verdict("[::ffff:203.0.113.9]:8443", None), Verdict::Allow("allow[2]".into()));
    assert_eq!(verdict("203.0.113.9:443", None), Verdict::Deny("policy:no-rule".into()));
}

/// A tunnel shows no path: a host whose rules each have a path needs the
/// TLS end of T-100.
#[test]
fn a_tunnel_to_a_host_with_path_rules_only_needs_a_tls_end() {
    let compiled = Policy::from_json(ALLOW).unwrap().compile().unwrap();
    assert_eq!(compiled.verdict(&at("api.github.com:443"), None), Verdict::PathsOnly);
    assert_eq!(PATH_NEEDS_TLS_END, "policy:path-needs-tls-end");
    assert_eq!(compiled.allows_path(&at("api.github.com:443"), "/v1/repos/a"), Some("allow[0]".into()));
    assert_eq!(compiled.allows_path(&at("api.github.com:443"), "/v1/reposx"), None);
}

#[test]
fn an_open_policy_allows_each_target_and_keeps_its_denies() {
    let compiled =
        Policy::from_json(r#"{"version":1,"mode":"open","deny":["*.ads.example"]}"#).unwrap().compile().unwrap();
    assert_eq!(compiled.verdict(&at("example.com:443"), None), Verdict::Allow("open".into()));
    assert_eq!(compiled.deny().check_name("x.ads.example"), Some("policy:deny[0]"));
    assert_eq!(Policy::open().compile().unwrap().mode(), Mode::Open);
}

#[test]
fn a_policy_file_is_toml_by_its_name_or_else_json() {
    let dir = std::env::temp_dir().join(format!("pod-broker-policy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("p.toml"), "version = 1\nmode = \"open\"\n").unwrap();
    std::fs::write(dir.join("p.json"), r#"{"version":1,"mode":"open"}"#).unwrap();
    assert_eq!(Policy::read(&dir.join("p.toml")).unwrap(), Policy::open());
    assert_eq!(Policy::read(&dir.join("p.json")).unwrap(), Policy::open());
    assert!(Policy::read(&dir.join("none.json")).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The JSON Schema of `schemas/` agrees with the reader: each example is a
/// policy, the modes are the same, and each key of a rule is a field.
#[test]
fn the_json_schema_agrees_with_the_policy() {
    let text =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/broker-policy.v1.schema.json"))
            .unwrap();
    let schema: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(schema["properties"]["version"]["const"], pod_broker::policy::VERSION);
    assert_eq!(schema["properties"]["mode"]["enum"], serde_json::json!(["allow", "open"]));
    let rule_keys: Vec<&String> =
        schema["properties"]["allow"]["items"]["properties"].as_object().unwrap().keys().collect();
    assert_eq!(rule_keys, ["host", "path", "ports"]);
    let keys: Vec<&String> = schema["properties"].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["allow", "deny", "mode", "version"], "in the order of a map with sorted keys");
    let examples = schema["examples"].as_array().unwrap();
    assert_eq!(examples.len(), 2);
    for example in examples {
        Policy::from_value(example.clone()).unwrap().compile().unwrap();
    }
}
