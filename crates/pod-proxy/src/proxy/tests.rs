//! The proxy URLs of podnet: the carriers, the schemes, the ports, the
//! credentials and the refusals.

use super::*;

// A proxy on a Unix socket, or at a command.
#[test]
fn the_carriers_parse() {
    for spec in ["socks5h+unix:/run/w.sock", "socks5h://unix:/run/w.sock"] {
        let p = Proxy::parse(spec).unwrap();
        assert_eq!(p.carrier(), &Carrier::Unix("/run/w.sock".into()), "{spec}");
        assert_eq!(p.kind(), ProxyKind::Socks5 { remote_dns: true });
        assert_eq!(p.label(), "socks5h+unix:/run/w.sock");
    }
    assert!(Proxy::parse("http+unix:relative.sock").is_err());
    let spec = "http+exec:ssh -i /secret/key gw nc 127.0.0.1 3128";
    assert!(Proxy::parse(spec).is_err(), "a variable does not start a command");
    let p = Proxy::parse_trusted(spec).unwrap();
    assert_eq!(p.kind(), ProxyKind::Http);
    assert!(matches!(p.carrier(), Carrier::Exec(c) if c.starts_with("ssh -i")));
    assert_eq!(p.label(), "http+exec:ssh ...");
    assert!(!format!("{p:?}").contains("secret"));
    assert!(Proxy::parse_trusted("http+exec:  ").is_err());
}

#[test]
fn parses_a_full_url() {
    let p = Proxy::parse("http://user:p%40ss@10.0.0.1:3128").unwrap();
    assert_eq!(p.address(), ("10.0.0.1", 3128));
    assert_eq!(p.credentials(), Some(("user", "p@ss")));
    assert_eq!(p.label(), "http://10.0.0.1:3128");
}

#[test]
fn debug_and_display_hold_no_password() {
    let p = Proxy::parse("http://u:secret@h:3128").unwrap();
    let debug = format!("{p:?}");
    assert!(!debug.contains("secret") && debug.contains("<hidden>"), "{debug}");
    assert!(!p.to_string().contains("secret"));
    let s = Proxy::parse("socks5h://u:secret@h").unwrap();
    assert!(!format!("{s:?}").contains("secret"));
    assert_eq!(s.label(), "socks5h://h:1080");
}

#[test]
fn the_default_ports() {
    // podnet's port of an http:// URL with no port, as it gives it.
    let podnet = |spec: &str| Proxy::parse_with(spec, false, 8080).unwrap();
    assert_eq!(podnet("http://proxy.example").address(), ("proxy.example", 8080));
    assert_eq!(podnet("proxy.example:3128").address(), ("proxy.example", 3128));
    assert_eq!(podnet("socks5://127.0.0.1").address(), ("127.0.0.1", 1080));
}

#[test]
fn parses_a_bracketed_ipv6_authority() {
    let p = Proxy::parse("http://[::1]:9000").unwrap();
    assert_eq!(p.address(), ("::1", 9000));
    assert_eq!(p.label(), "http://[::1]:9000");
}

// Only an IPv6 address has a colon in its host. A name with a colon took
// the default port, and its URL did not parse back (the fuzz target
// `proxy_url`, T-017).
#[test]
fn a_host_with_a_colon_is_an_ipv6_address() {
    for spec in ["http://pr:xy:8080", "socks5://a:b:c", "h:x:1"] {
        assert!(Proxy::parse(spec).is_err(), "{spec} was accepted");
    }
    for spec in ["::1", "http://2001:db8::1", "socks5h://fe80::1%25eth0"] {
        let p = Proxy::parse(spec).unwrap_or_else(|e| panic!("{spec}: {e}"));
        assert_eq!(Proxy::parse(&p.url()), Ok(p.clone()), "{spec}");
    }
}

#[test]
fn the_schemes() {
    assert_eq!(Proxy::parse("socks5://127.0.0.1:1080").unwrap().kind(), ProxyKind::Socks5 { remote_dns: false });
    assert_eq!(Proxy::parse("SOCKS5H://127.0.0.1:1080").unwrap().kind(), ProxyKind::Socks5 { remote_dns: true });
    for (spec, want) in [
        ("socks4://h:1080", "SOCKS4 is not supported"),
        ("https://h:443", "https:// proxies"),
        ("ftp://h:21", "ftp:// is not supported"),
    ] {
        let err = Proxy::parse(spec).unwrap_err().to_string();
        assert!(err.contains(want), "{spec}: {err}");
    }
}

#[test]
fn bad_urls_are_refused() {
    for spec in [
        "http://u:p%4@h:1",
        "http://u:p%zz@h:1",
        "http://u:%ff%fe@h:1",
        "http://h:1/path",
        "http://h:notaport",
        "http://h:0",
        "http://[::1",
        "http://h st:1",
        "socks5://:p@h:1",
    ] {
        assert!(Proxy::parse(spec).is_err(), "{spec} was accepted");
    }
    // An escaped colon stays in the user.
    let p = Proxy::parse("http://a%3Ab:c@h:1").unwrap();
    assert_eq!(p.credentials(), Some(("a:b", "c")));
}

// An unencoded / in the password names the fix.
#[test]
fn a_slash_in_the_password_names_the_encoding() {
    let err = format!("{:#}", Proxy::parse("http://user:pa/ss@proxy:3128").unwrap_err());
    assert!(err.contains("percent-encode"), "{err}");
    assert!(Proxy::parse("http://user:pa%2Fss@proxy:3128").is_ok());
}

/// The secret parts of a URL: none with no user part, the user and the
/// password as written and decoded, and a command whole.
#[test]
fn the_secret_parts_of_a_url() {
    assert!(Proxy::secret_parts("http://proxy.example:3128").is_empty());
    assert_eq!(Proxy::secret_parts("socks5h://alice:s%40cret@h"), ["alice:s%40cret", "s%40cret", "s@cret"]);
    assert_eq!(Proxy::secret_parts(" http+exec:ssh -i k gw "), ["http+exec:ssh -i k gw", "ssh -i k gw"]);
}

/// A `+exec:` or a `+unix:` inside the user or the password is a part of
/// it, and `://unix:` with a port is a host named `unix`.
#[test]
fn a_carrier_word_after_the_scheme_is_no_carrier() {
    let p = Proxy::parse("http://u:p+exec:x@proxy.example:3128").unwrap();
    assert_eq!((p.carrier(), p.credentials()), (&Carrier::Tcp, Some(("u", "p+exec:x"))));
    let p = Proxy::parse("http://unix:3128").unwrap();
    assert_eq!((p.address(), p.carrier()), (("unix", 3128), &Carrier::Tcp));
    let p = Proxy::parse_trusted("socks5+exec:nc gw 1080").unwrap();
    assert_eq!((p.kind_text(), p.to_string(), p.outcome()), ("socks5+exec".into(), "exec:nc ...".into(), "exec"));
    assert_eq!(p.url(), "socks5+exec:nc gw 1080");
}

// The URL form of a Unix socket has no host and no user: the label shows the
// path whole (PodUtils/podnet#555; the crash input of podnet's `proxy_url`).
#[test]
fn a_unix_proxy_has_no_host_and_no_user() {
    for spec in [
        "socks5+unix://ss@proxy:2]:/",
        "socks5h+unix://u:pw@x",
        "socks5h+unix://x/run/w.sock",
        "socks5h+unix://",
        "socks5h://unix://u:pw@x/run/w.sock",
        "socks5h+unix:/run/alice@x.sock",
        "socks5h+unix:/run/w\n.sock",
    ] {
        assert!(Proxy::parse(spec).is_err(), "{spec:?} was accepted");
    }
    for spec in ["socks5h+unix:///run/w.sock", "socks5h://unix:///run/w.sock"] {
        let p = Proxy::parse(spec).unwrap_or_else(|e| panic!("{spec}: {e}"));
        assert_eq!(p.carrier(), &Carrier::Unix("///run/w.sock".into()));
        assert_eq!(Proxy::parse(&p.url()), Ok(p.clone()));
    }
}

/// Each form of a spec with a user and a password: no text of a proxy that
/// parses, and no error of one that does not, holds the user or the password.
#[test]
fn the_label_and_the_errors_of_a_spec_hold_no_secret() {
    // `{U}` stands for the user and the password, and `{P}` for the
    // password, so that a scanner of secrets finds no URL with a password in
    // the source.
    const PASSWORD: &str = "hunter2";
    let forms = [
        "socks5+unix://ss@proxy:2]:/",
        "socks5h+unix://{U}@x",
        "socks5h+unix://{U}@x/run/w.sock",
        "socks5h+unix:/run/{U}@x",
        "socks5h+unix:{U}@/run/w.sock",
        "socks5h://unix://{U}@x",
        "socks5h://unix:/{U}@x",
        "ftp+unix://{U}@x",
        "{U}@proxy+unix:/run/w.sock",
        "http://{U}@proxy.example:3128",
        "http://{U}]@proxy.example:3128",
        "http://{U}%40@proxy.example:3128",
        "http://{U}@@proxy.example:3128",
        "http://{U}@proxy.example:3128]",
        "http://{U}@[::1",
        "http://{U}@[::1]x",
        "http://{U}@pr:xy:1",
        "http://{U}@proxy.example:99999",
        "http://{U}@proxy.example/p",
        "http://{U}%zz@proxy.example",
        "http://{U}@",
        "socks5://:{P}@proxy.example",
        "socks5h://{U}@[::1]:1080",
        "socks4://{U}@proxy.example",
        "socks5h+exec:TOKEN={P} nc gw 1080",
        "http+exec:{U}@gw nc 127.0.0.1 3128",
    ];
    for form in forms {
        let spec = form.replace("{U}", &format!("alice:{PASSWORD}")).replace("{P}", PASSWORD);
        for (trusted, parsed) in [(false, Proxy::parse(&spec)), (true, Proxy::parse_trusted(&spec))] {
            let texts = match &parsed {
                Ok(p) => vec![p.label(), p.to_string(), format!("{p:?}"), p.kind_text()],
                Err(e) => vec![e.clone()],
            };
            for text in texts {
                let held = text.contains(PASSWORD) || text.contains("alice") || text.contains("ss@");
                assert!(!held, "{form:?} (trusted: {trusted}): {text:?}");
            }
            if let Ok(p) = &parsed {
                assert!(!p.label().contains('@'), "{form:?}: {}", p.label());
            }
        }
    }
}
