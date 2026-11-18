use super::*;
use serde_json::json;

/// `levels` nested mappings, one key on each line.
fn nested(levels: usize) -> String {
    (0..levels).map(|i| format!("{}a:\n", " ".repeat(i))).collect()
}

// A deep document is an error with a line number, not a
// stack overflow.
#[test]
fn a_deep_document_is_an_error() {
    assert!(parse(&nested(MAX_DEPTH)).is_ok());
    let err = format!("{:#}", parse(&nested(200)).unwrap_err());
    assert!(err.contains("nests deeper than 64 levels"), "{err}");
    assert!(err.contains("line 66"), "{err}");
    let items: String = (0..200).map(|i| format!("{}- a:\n", "  ".repeat(i))).collect();
    let err = format!("{:#}", parse(&items).unwrap_err());
    assert!(err.contains("nests deeper"), "{err}");
    // A small stack for 3000 levels: the cap stops the recursion before an
    // overflow. A debug build of Windows needs more than 256 KiB for the 64
    // levels of the cap.
    let text = nested(3000);
    let result = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || parse(&text).map(|_| ()).map_err(|e| e.to_string()))
        .unwrap()
        .join()
        .unwrap();
    assert!(result.unwrap_err().contains("nests deeper"));
}

#[test]
fn a_cloudflared_config() {
    let text = r#"
# A named tunnel.
tunnel: 6ff42ae2-765d-4adf-8112-31c55c1551ef
credentials-file: /root/.cloudflared/6ff42ae2.json   # the credentials
protocol: http2
ha-connections: 4
warp-routing:
  enabled: true
ingress:
  - hostname: app.example.com
    service: http://localhost:8000
    originRequest:
      noTLSVerify: true
      connectTimeout: 30s
  - hostname: "*.example.com"
    path: ^/api/.*$
    service: 'http://localhost:9000'
  - service: http_status:404
"#;
    let value = parse(text).unwrap();
    assert_eq!(
        value,
        json!({
            "tunnel": "6ff42ae2-765d-4adf-8112-31c55c1551ef",
            "credentials-file": "/root/.cloudflared/6ff42ae2.json",
            "protocol": "http2",
            "ha-connections": 4,
            "warp-routing": {"enabled": true},
            "ingress": [
                {
                    "hostname": "app.example.com",
                    "service": "http://localhost:8000",
                    "originRequest": {"noTLSVerify": true, "connectTimeout": "30s"}
                },
                {"hostname": "*.example.com", "path": "^/api/.*$", "service": "http://localhost:9000"},
                {"service": "http_status:404"}
            ]
        })
    );
}

#[test]
fn a_sequence_at_the_indentation_of_its_key() {
    let value = parse("ingress:\n- service: a\n- service: b\nprotocol: quic\n").unwrap();
    assert_eq!(value, json!({"ingress": [{"service": "a"}, {"service": "b"}], "protocol": "quic"}));
}

#[test]
fn scalars() {
    let value = parse("a: ~\nb:\nc: -3\nd: 1.5\ne: 'it''s'\nf: \"x\\ty \\u00e9\"\ng: a#b\nh: []\ni: {}\nj: 10.0.0.1\n")
        .unwrap();
    assert_eq!(
        value,
        json!({"a": null, "b": null, "c": -3, "d": 1.5, "e": "it's", "f": "x\ty \u{e9}",
               "g": "a#b", "h": [], "i": {}, "j": "10.0.0.1"})
    );
    assert_eq!(parse("- a\n- 2\n-\n  - b\n").unwrap(), json!(["a", 2, ["b"]]));
    assert_eq!(parse("# only a comment\n").unwrap(), Value::Null);
    assert_eq!(parse("---\nk: v\n").unwrap(), json!({"k": "v"}));
}

#[test]
fn the_unsupported_parts_are_refused_with_the_line() {
    for (text, want) in [
        ("a: 1\na: 2\n", "line 2: the key \"a\" is given two times"),
        ("a:\n\tb: 1\n", "line 2: a tab"),
        ("a: &x 1\n", "line 1: anchors"),
        ("a: *x\n", "line 1: anchors"),
        ("a: !!str 1\n", "line 1: tags"),
        ("a: |\n  text\n", "line 1: block scalars"),
        ("a: [1, 2]\n", "line 1: flow collections"),
        ("a: \"open\n", "line 1: a quoted scalar has no end"),
        ("a: 1\n  b: 2\n", "line 2: a scalar has no children"),
        ("a:\n  b: 1\n c: 2\n", "line 3: unexpected indentation"),
        ("a: 1\njust text\n", "line 2: expected `key: value`"),
    ] {
        let err = parse(text).unwrap_err().to_string();
        assert!(err.starts_with(want), "{text:?}: {err}");
    }
}

#[test]
fn the_line_of_each_ingress_rule() {
    let text = "tunnel: x\n# a comment\ningress:\n  - hostname: a\n    service: http://a\n\n  - service: http_status:404\nwarp-routing: {}\n";
    assert_eq!(item_lines(text, "ingress"), [4, 7]);
    let flat = "ingress:\n- service: a\n- service: b\n";
    assert_eq!(item_lines(flat, "ingress"), [2, 3]);
    assert!(item_lines(text, "none").is_empty());
}
