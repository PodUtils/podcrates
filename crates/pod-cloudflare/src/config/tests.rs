use super::*;

#[test]
fn the_next_step_follows_the_flags_of_the_edge() {
    use crate::cloudflare::rpc::Refused;
    let policy = ReconnectPolicy::default();
    let refused = |should_retry, secs| {
        crate::fail::wrap(
            Failure::from(Refused {
                client: "podnet".into(),
                cause: "Unauthorized: tunnel not found".into(),
                should_retry,
                retry_after: Duration::from_secs(secs),
            }),
            "registering",
        )
    };
    let text = format!("{:#}", refused(false, 0));
    assert!(text.contains("podnet speaks cloudflared 2026.10.7"), "{text}");
    match policy.next(1, &refused(false, 0)) {
        Next::Stop(reason) => assert!(reason.contains("tunnel not found"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(policy.next(1, &refused(true, 5)), Next::Retry(Duration::from_secs(5)));
    assert_eq!(policy.next(1, &refused(true, 9999)), Next::Retry(MAX_RETRY_AFTER));
    // A network error is retried with no limit, with 0-25 % jitter.
    let net = crate::fail::new(pod_core::Stage::Connect, pod_core::Class::Unavailable, "connection refused");
    for attempt in [1, 5, 1000, u32::MAX - 1] {
        let Next::Retry(d) = policy.next(attempt, &net) else {
            panic!("attempt {attempt} stopped");
        };
        let base = policy.backoff(attempt.saturating_add(1));
        assert!(d >= base && d <= base + base / 4, "{d:?} for {base:?}");
    }
    assert_eq!(policy.backoff(1000), Duration::from_secs(30));
    let capped = ReconnectPolicy { max_attempts: 3, ..policy };
    assert!(matches!(capped.next(3, &net), Next::Stop(_)));
}

/// The cloudflared forms of tunnel: and credentials-file:, a typo in a
/// key, and a last rule that is not a catch-all.
#[test]
fn load_file_follows_cloudflared() {
    let dir = tempfile::tempdir().unwrap();
    let creds = dir.path().join("creds.json");
    std::fs::write(
        &creds,
        r#"{"AccountTag":"0123456789abcdef","TunnelID":"ffd86bd5-1bff-456b-8c2c-75a014cdc4c8","TunnelSecret":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="}"#,
    )
    .unwrap();
    let path = dir.path().join("config.yml");
    let load = |text: String| {
        std::fs::write(&path, text).unwrap();
        load_file(&path).map_err(|e| format!("{e:#}"))
    };
    let c = creds.display();
    let catch_all = "ingress:\n  - service: http://127.0.0.1:1\n";

    // A UUID, or a name, with credentials-file:.
    for tunnel in ["ffd86bd5-1bff-456b-8c2c-75a014cdc4c8", "my-tunnel"] {
        let config = load(format!("tunnel: {tunnel}\ncredentials-file: {c}\n{catch_all}")).unwrap();
        assert!(matches!(config.credentials, Credentials::Explicit { .. }));
    }
    let err =
        load(format!("tunnel: 00000000-1bff-456b-8c2c-75a014cdc4c8\ncredentials-file: {c}\n{catch_all}")).unwrap_err();
    assert!(err.contains("is not the TunnelID"), "{err}");

    let err = load(format!("tunnel: my-tunnel\n{catch_all}")).unwrap_err();
    assert!(err.contains("give its credentials-file:"), "{err}");

    let err = load(format!("credentails-file: {c}\n{catch_all}")).unwrap_err();
    assert!(
        err.contains("\"credentails-file\" is not a key") && err.contains("did you mean credentials-file?"),
        "{err}"
    );

    let err = load(format!(
        "credentials-file: {c}\ningress:\n  - hostname: a.example.com\n    service: http://127.0.0.1:1\n"
    ))
    .unwrap_err();
    assert!(err.contains("the last ingress rule must match each request"), "{err}");

    // Keys of cloudflared that the client does not use are accepted.
    load(format!("credentials-file: {c}\nmetrics: localhost:2000\nno-autoupdate: true\nwarp-routing:\n  enabled: true\n{catch_all}")).unwrap();
}

#[test]
fn load_file_reads_a_cloudflared_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yml");
    std::fs::write(
        &path,
        "protocol: http2\nha-connections: 3\nloglevel: info # not read\ningress:\n  - hostname: a.example.com\n    service: http://127.0.0.1:8000\n    originRequest:\n      noTLSVerify: true\n  - service: http_status:404\n",
    )
    .unwrap();
    let config = load_file(&path).unwrap();
    assert_eq!(config.ha_connections, 3);
    assert!(matches!(config.protocol, Protocol::Http2));
    assert_eq!(config.ingress.rules.len(), 2);
    assert_eq!(config.ingress.rules[0].hostname.as_deref(), Some("a.example.com"));
    assert!(config.ingress.rules[0].origin.no_tls_verify);

    std::fs::write(&path, "ingress:\n  - service: a\n    service: b\n").unwrap();
    let err = format!("{:#}", load_file(&path).unwrap_err());
    assert!(err.contains("line 3: the key \"service\" is given two times"), "{err}");
}

// The key token: of a file gives the named tunnel, not a quick tunnel.
#[test]
fn a_token_key_makes_a_named_tunnel() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yml");
    let load = |text: String| {
        std::fs::write(&path, text).unwrap();
        load_file(&path).map_err(|e| format!("{e:#}"))
    };
    let id = "ffd86bd5-1bff-456b-8c2c-75a014cdc4c8";
    let token = crate::cloudflare::http2::base64_encode(
        format!(r#"{{"a":"0123456789abcdef","t":"{id}","s":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="}}"#)
            .as_bytes(),
    );
    let catch_all = "ingress:\n  - service: http://127.0.0.1:1\n";
    for head in [format!("token: {token}\n"), format!("tunnel: {id}\ntoken: {token}\n")] {
        let config = load(format!("{head}{catch_all}")).unwrap();
        match config.credentials {
            Credentials::Explicit { tunnel_id, .. } => assert_eq!(tunnel_id.to_string(), id),
            other => panic!("{other:?}"),
        }
    }
    let err = load(format!("token: not-a-token\n{catch_all}")).unwrap_err();
    assert!(err.contains("token: is not a tunnel token"), "{err}");
    let err = load(format!("tunnel: 00000000-1bff-456b-8c2c-75a014cdc4c8\ntoken: {token}\n{catch_all}")).unwrap_err();
    assert!(err.contains("is not the tunnel id of token:"), "{err}");
    let err = load(format!("token: {token}\ncredentials-file: /nonexistent.json\n{catch_all}")).unwrap_err();
    assert!(err.contains("give one"), "{err}");
    let err = load(format!("token: 5\n{catch_all}")).unwrap_err();
    assert!(err.contains("token: give a string"), "{err}");
}

// The keys url: and hello-world: give a catch-all rule; with ingress:
// they are an error.
#[test]
fn url_and_hello_world_make_a_catch_all_rule() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yml");
    let load = |text: &str| {
        std::fs::write(&path, text).unwrap();
        load_file(&path).map_err(|e| format!("{e:#}"))
    };
    let config = load("url: http://127.0.0.1:8080\n").unwrap();
    assert_eq!(config.ingress.rules.len(), 1);
    assert!(matches!(config.ingress.rules[0].service, crate::ingress::Service::Http(_)));
    let config = load("hello-world: true\n").unwrap();
    assert!(matches!(config.ingress.rules[0].service, crate::ingress::Service::HelloWorld));
    let config = load("hello-world: false\nloglevel: info\n").unwrap();
    assert!(config.ingress.is_empty());
    let err = load("url: http://127.0.0.1:1\ningress:\n  - service: http_status:404\n").unwrap_err();
    assert!(err.contains("url: and ingress: both give the origin"), "{err}");
    let err = load("hello-world: true\ningress:\n  - service: http_status:404\n").unwrap_err();
    assert!(err.contains("hello-world: and ingress:"), "{err}");
    let err = load("url: http://127.0.0.1:1\nhello-world: true\n").unwrap_err();
    assert!(err.contains("give one"), "{err}");
    let err = load("hello-world: yes please\n").unwrap_err();
    assert!(err.contains("hello-world: give true or false"), "{err}");
}

// The key path: is a regular expression, as in
// cloudflared. A path that does not compile is an error that names the
// file and the rule.
#[test]
fn a_path_is_a_regular_expression() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yml");
    let load = |rule_path: &str| {
        let text = format!(
            "ingress:\n  - service: http_status:404\n    path: {rule_path}\n  - path: '{rule_path}'\n    service: http://127.0.0.1:1\n  - service: http_status:503\n"
        );
        std::fs::write(&path, text).unwrap();
        load_file(&path).map_err(|e| format!("{e:#}"))
    };
    let config = load("^/api/.*$").unwrap();
    let ingress = &config.ingress;
    assert_eq!(ingress.resolve_index("h", "/api/x"), Some(0));
    assert_eq!(ingress.resolve_index("h", "/api/x?q=1"), Some(0));
    assert_eq!(ingress.resolve_index("h", "/x/api/y"), Some(2));
    assert_eq!(ingress.resolve_index("h", "/apix"), Some(2));
    assert_eq!(ingress.rules[0].path.as_ref().unwrap().as_str(), "^/api/.*$");
    // Unanchored, as in cloudflared: /api also matches /x/api.
    let config = load("/api").unwrap();
    assert_eq!(config.ingress.resolve_index("h", "/x/api"), Some(0));
    assert_eq!(config.ingress.resolve_index("h", "/ap"), Some(2));
    // An unclosed group is an error with the file and the rule.
    let err = load("^/api/(").unwrap_err();
    assert!(err.contains(&path.display().to_string()), "{err}");
    assert!(err.contains("ingress rule 1 (line 2): path:"), "{err}");
    assert!(err.contains("not a valid regular expression"), "{err}");
    assert!(err.contains("unclosed group"), "{err}");
    assert!(!err.contains('\n'), "{err}");
}

/// The field names that serde reads for `T`. The deserializer stops at
/// `deserialize_struct` and keeps its list of fields.
fn serde_fields<'de, T: serde::Deserialize<'de>>() -> &'static [&'static str] {
    struct Fields(Option<&'static [&'static str]>);
    #[derive(Debug)]
    struct Stop;
    impl std::fmt::Display for Stop {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("stop")
        }
    }
    impl std::error::Error for Stop {}
    impl serde::de::Error for Stop {
        fn custom<M: std::fmt::Display>(_: M) -> Self {
            Stop
        }
    }
    impl<'de> serde::Deserializer<'de> for &mut Fields {
        type Error = Stop;
        fn deserialize_any<V: serde::de::Visitor<'de>>(self, _: V) -> Result<V::Value, Stop> {
            Err(Stop)
        }
        fn deserialize_struct<V: serde::de::Visitor<'de>>(
            self,
            _: &'static str,
            fields: &'static [&'static str],
            _: V,
        ) -> Result<V::Value, Stop> {
            self.0 = Some(fields);
            Err(Stop)
        }
        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map enum identifier ignored_any
        }
    }
    let mut fields = Fields(None);
    let _ = T::deserialize(&mut fields);
    fields.0.expect("a struct")
}

// Each key that a raw struct reads is in its key list. A field
// that is not in the list is refused by check_keys and never read.
#[test]
fn each_field_of_a_raw_struct_is_in_its_key_list() {
    let lists: [(&[&str], &[&str]); 3] = [
        (serde_fields::<RawConfig>(), CLOUDFLARED_KEYS),
        (serde_fields::<crate::ingress::RawIngress>(), RULE_KEYS),
        (serde_fields::<crate::ingress::RawOriginOptions>(), ORIGIN_REQUEST_KEYS),
    ];
    for (fields, keys) in lists {
        assert!(!fields.is_empty());
        for field in fields {
            assert!(keys.contains(field), "{field} is not in {keys:?}");
        }
    }
    // The keys of a rule are the fields of RawIngress.
    let mut rule: Vec<&str> = serde_fields::<crate::ingress::RawIngress>().to_vec();
    let mut keys = RULE_KEYS.to_vec();
    rule.sort();
    keys.sort();
    assert_eq!(rule, keys);
    let unknown: Result<crate::ingress::RawIngress, _> =
        serde_json::from_value(serde_json::json!({"service": "x", "hostnam": "a"}));
    assert!(unknown.is_err());
}

// A misspelt key of a rule or of originRequest is an error.
#[test]
fn a_misspelt_rule_key_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yml");
    let load = |text: &str| {
        std::fs::write(&path, text).unwrap();
        load_file(&path).map_err(|e| format!("{e:#}"))
    };
    let err =
        load("ingress:\n  - hostnam: a.example.com\n    service: http://127.0.0.1:1\n  - service: http_status:404\n")
            .unwrap_err();
    assert!(err.contains("ingress rule 1"), "{err}");
    assert!(err.contains("did you mean hostname?"), "{err}");

    let err =
        load("ingress:\n  - service: http://127.0.0.1:1\n    originRequest:\n      noTLSVerfy: true\n").unwrap_err();
    assert!(err.contains("rule 1 (line 2), originRequest"), "{err}");
    assert!(err.contains("did you mean noTLSVerify?"), "{err}");

    let err = load("originRequest:\n  bogusKey: 1\ningress:\n  - service: http_status:404\n").unwrap_err();
    assert!(err.contains("not a key of originRequest"), "{err}");

    // Keys of cloudflared that the client does not read still load.
    load("ingress:\n  - service: http://127.0.0.1:1\n    originRequest:\n      tcpKeepAlive: 30s\n      caPool: /x.pem\n").unwrap();
}

// The top-level originRequest is the default of each rule.
#[test]
fn a_top_level_origin_request_is_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yml");
    std::fs::write(
        &path,
        "originRequest:\n  noTLSVerify: true\n  httpHostHeader: top\ningress:\n  - hostname: a.example.com\n    service: https://127.0.0.1:1\n    originRequest:\n      httpHostHeader: rule\n  - service: https://127.0.0.1:2\n",
    )
    .unwrap();
    let config = load_file(&path).unwrap();
    let rules = &config.ingress.rules;
    assert!(rules[0].origin.no_tls_verify);
    assert!(rules[1].origin.no_tls_verify);
    assert_eq!(rules[0].origin.http_host_header.as_deref(), Some("rule"));
    assert_eq!(rules[1].origin.http_host_header.as_deref(), Some("top"));
}

// Clear errors for a list, a BOM, an empty file, bad types,
// a short key and a rule with no service.
#[test]
fn config_errors_are_clear() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yml");
    let load = |text: &str| {
        std::fs::write(&path, text).unwrap();
        load_file(&path).map(|_| ()).map_err(|e| format!("{e:#}"))
    };
    let err = load("- a\n- b\n").unwrap_err();
    assert!(err.contains("must be a mapping"), "{err}");
    load("\u{feff}ingress:\n  - service: http_status:404\n").unwrap();
    let err = load("# nothing\n").unwrap_err();
    assert!(err.contains("has no keys"), "{err}");
    let err = load("ingress: 5\n").unwrap_err();
    assert!(err.contains("ingress: give a list"), "{err}");
    let err =
        load("ingress:\n  - service: http_status:404\n    originRequest:\n      connectTimeout: 5\n").unwrap_err();
    assert!(err.contains("connectTimeout: 5 needs a unit"), "{err}");
    let err = load("ingress:\n  - hostname: a.example.com\n  - service: http_status:404\n").unwrap_err();
    assert!(err.contains("(line 2): service: is missing"), "{err}");
    let err = load("a: 1\n").unwrap_err();
    assert!(!err.contains("did you mean"), "{err}");
    let err = load("tunel: x\n").unwrap_err();
    assert!(err.contains("did you mean tunnel?"), "{err}");
    let err = load("credentails-file: x\n").unwrap_err();
    assert!(err.contains("did you mean credentials-file?"), "{err}");
}

// A credentials file with no TunnelID names the field.
#[test]
fn a_credentials_file_error_names_the_field() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.json");
    std::fs::write(&path, r#"{"AccountTag":"a","TunnelSecret":"AAAA"}"#).unwrap();
    let err = format!("{:#}", parse_credentials_file(&path).unwrap_err());
    assert!(err.contains("TunnelID"), "{err}");
    assert!(!err.contains("not JSON"), "{err}");
}

// A file value out of range is an error, not a silent clamp.
#[test]
fn ha_connections_of_a_file_is_checked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yml");
    std::fs::write(&path, "ha-connections: 9\n").unwrap();
    let err = format!("{:#}", load_file(&path).unwrap_err());
    assert!(err.contains("not from 1 to 8"), "{err}");
}

#[test]
fn transport_names_round_trip() {
    for (text, want) in [
        ("auto", Transport::Auto),
        ("direct", Transport::Direct),
        ("warp", Transport::Warp),
        ("masque", Transport::Warp),
        ("WARP+MASQUE", Transport::Warp),
        ("  Direct ", Transport::Direct),
        ("relay", Transport::Relay),
    ] {
        assert_eq!(Transport::parse(text.trim()).unwrap(), want, "{text:?}");
    }
}

#[test]
fn an_unknown_transport_names_the_alternatives() {
    let err = Transport::parse("carrier-pigeon").unwrap_err().to_string();
    assert!(err.contains("auto"), "{err}");
    assert!(err.contains("direct"), "{err}");
    assert!(err.contains("warp"), "{err}");
}

#[test]
fn the_default_transport_is_auto() {
    // With this default, a quick tunnel works on a host whose egress permits
    // only 443, and the caller gives no flag.
    assert_eq!(Transport::default(), Transport::Auto);
    assert_eq!(Transport::default().to_string(), "auto");
}

/// A file with no credentials starts a quick tunnel, and the user gets an
/// event that says so.
#[test]
fn a_file_with_no_credentials_gives_an_event() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yml");
    std::fs::write(&path, "url: http://127.0.0.1:8080\n").unwrap();
    let (report, seen) = crate::testing::recorded();
    let config = load_file_with(&path, &report).unwrap();
    assert!(matches!(config.credentials, Credentials::Quick));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].kind, "config.quick");
    assert!(seen[0].text.contains("a quick tunnel starts"), "{}", seen[0].text);
}

/// `--protocol` takes the spellings of cloudflared, and gives its name back.
#[test]
fn protocols_parse_the_way_cloudflared_spells_them() {
    for (text, want) in
        [("auto", Protocol::Auto), ("QUIC", Protocol::Quic), ("http2", Protocol::Http2), ("h2", Protocol::Http2)]
    {
        assert_eq!(Protocol::parse(text).unwrap(), want, "{text}");
    }
    assert!(Protocol::parse("carrier-pigeon").is_err());
    for p in [Protocol::Auto, Protocol::Quic, Protocol::Http2] {
        assert_eq!(Protocol::parse(p.as_str()).unwrap(), p);
    }
}
