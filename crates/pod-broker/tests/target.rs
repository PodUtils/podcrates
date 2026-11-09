//! The strict reading of a target: one host, one port and one path, or 400.

use pod_broker::target::{Host, normal_path, parse_absolute, parse_authority, parse_host};

#[test]
fn an_ip_in_another_spelling_is_refused() {
    for bad in ["2130706433", "0x7f000001", "0177.0.0.1", "0x7f.0.0.1", "127.1", "127.0.1", "010.0.0.1", "1.2.3.4.5"] {
        assert!(parse_host(bad).is_err(), "{bad}");
    }
    assert_eq!(parse_host("169.254.169.254"), Ok(Host::Ip("169.254.169.254".parse().unwrap())));
    assert_eq!(parse_host("a1.0x10.example"), Ok(Host::Name("a1.0x10.example".into())));
}

#[test]
fn an_authority_is_host_and_port_with_v6_in_brackets() {
    let at = parse_authority("[2001:db8::1]:443", None).unwrap();
    assert_eq!((at.host, at.port), (Host::Ip("2001:db8::1".parse().unwrap()), 443));
    let at = parse_authority("Example.COM:8443", None).unwrap();
    assert_eq!(at.to_string(), "example.com:8443");
    assert_eq!(parse_authority("example.com", Some(80)).unwrap().port, 80);
    for bad in [
        "example.com",
        "example.com:",
        "example.com:0",
        "example.com:65536",
        "example.com:99999",
        "example.com:+443",
        "example.com:0443",
        "2001:db8::1:443",
        "2001:db8::1",
        "[2001:db8::1]",
        "[2001:db8::1]x:443",
        "user@example.com:443",
        "name@a.example:443",
        "exa%6dple.com:443",
        "[fe80::1%25eth0]:443",
        ":443",
    ] {
        assert!(parse_authority(bad, None).is_err(), "{bad}");
    }
}

#[test]
fn an_absolute_target_is_http_with_a_normal_path() {
    let target = parse_absolute("http://api.example/v1/repos/x?page=2").unwrap();
    assert_eq!(target.authority.to_string(), "api.example:80");
    assert_eq!((target.path.as_str(), target.query.as_deref()), ("/v1/repos/x", Some("page=2")));
    assert_eq!(parse_absolute("HTTP://a.example:8080").unwrap().path, "/");
    assert_eq!(parse_absolute("http://a.example/v1/re%70os").unwrap().path, "/v1/repos");
    assert_eq!(parse_absolute("http://a.example/a%20b%c3%a9").unwrap().path, "/a%20b%C3%A9");
    for bad in [
        "/v1/repos",
        "https://a.example/",
        "ftp://a.example/",
        "http://user@a.example/",
        "http://name@a.example:80/",
        "http://a.example/v1/%2e%2e/admin",
        "http://a.example/v1/%2E/x",
        "http://a.example/v1%2fx",
        "http://a.example/v1%5cx",
        "http://a.example/v1/../admin",
        "http://a.example/v1/./x",
        "http://a.example//v1",
        "http://a.example/v1//x",
        "http://a.example/v1\\x",
        "http://a.example/v1%",
        "http://a.example/v1%zz",
        "http://a.example/v1%00",
        "http://a.example/#x",
        "http://2130706433/",
        "http://0x7f.1/",
        "http://::1/",
        "http://a.example:0/",
        "http://a.example:70000/",
    ] {
        assert!(parse_absolute(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_normal_path_keeps_each_safe_escape_in_upper_case() {
    assert_eq!(normal_path("/a%2b%7E"), Ok("/a%2B~".into()));
    assert!(normal_path("a").is_err());
}
