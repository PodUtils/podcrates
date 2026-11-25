use super::*;

const ID: &str = "made-up-id.access";
const SECRET: &str = "made-up-secret-of-the-test-0123";

fn access() -> Vec<Header> {
    vec![("CF-Access-Client-Id", Zeroizing::new(ID.into())), ("CF-Access-Client-Secret", Zeroizing::new(SECRET.into()))]
}

fn refusal(headers: Vec<Header>) -> String {
    Extra::checked(headers).unwrap_err().to_string()
}

/// A name that is no token, a name that a road writes, a name twice and a
/// control character in a value are refused; the refusal names the
/// header, and never its value.
#[test]
fn a_bad_header_is_refused_by_its_name_alone() {
    for bad in ["\r", "\n", "\0", "\x01", "\t", "\x1b", "\x7f"] {
        let value = format!("{SECRET}{bad}tail");
        let said = refusal(vec![("CF-Access-Client-Secret", Zeroizing::new(value))]);
        assert!(said.contains("CF-Access-Client-Secret"), "{said}");
        assert!(said.contains("control character"), "{said}");
        assert!(!said.contains(SECRET) && !said.contains("tail"), "the value is not said: {said}");
    }
    for name in ["", "CF Access", "CF:Access", "CF-Access\r\nX", "Ä", "(x)"] {
        let said = refusal(vec![(name, Zeroizing::new(SECRET.into()))]);
        assert!(said.contains("is not a name of an HTTP header"), "{name:?}: {said}");
        assert!(!said.contains(SECRET) && !said.contains('\r'), "{said}");
    }
    for name in ["Host", "x-relay-token", "X-PODSSH-PROTO", "Sec-WebSocket-Key", "Upgrade", "Transfer-Encoding"] {
        let said = refusal(vec![(name, Zeroizing::new(SECRET.into()))]);
        assert!(said.contains("writes itself"), "{name}: {said}");
    }
    let twice = refusal(vec![("X-A", Zeroizing::new("1".into())), ("x-a", Zeroizing::new("2".into()))]);
    assert!(twice.contains("twice"), "{twice}");
    let extra = Extra::checked(access()).unwrap();
    assert!(extra.refuse_names(&["X-Session-Key"]).is_ok());
    let own = Extra::checked(vec![("x-session-key", Zeroizing::new(SECRET.into()))]).unwrap();
    assert!(own.refuse_names(&["X-Session-Key"]).unwrap_err().to_string().contains("x-session-key"));
}

/// A value of the caller is never in `Debug`; the names are.
#[test]
fn debug_shows_the_names_and_no_value() {
    let extra = Extra::checked(access()).unwrap();
    let shown = format!("{extra:?} {:?}", Headers::new(|_| access()));
    assert!(shown.contains("CF-Access-Client-Secret"), "{shown}");
    assert!(!shown.contains(SECRET) && !shown.contains(ID), "{shown}");
}

/// The function of the caller decides each host; none and a road in
/// cleartext, unless the caller allows it, give no header.
#[test]
fn the_caller_picks_the_hosts_and_cleartext_is_opt_in() {
    let headers = Headers::new(|host| if host == "relay.example" { access() } else { Vec::new() });
    assert_eq!(headers.for_host("relay.example").unwrap().iter().count(), 2);
    assert!(headers.for_host("other.example").unwrap().is_empty());
    assert!(headers.for_cleartext("relay.example").unwrap().is_empty(), "off by default");
    assert_eq!(headers.clone().in_cleartext(true).for_cleartext("relay.example").unwrap().iter().count(), 2);
    assert!(Headers::default().is_none() && Headers::default().for_host("relay.example").unwrap().is_empty());
    let names: Vec<_> = headers.for_host("relay.example").unwrap().iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["CF-Access-Client-Id", "CF-Access-Client-Secret"], "in the caller's order");
}

/// A refusal that quotes a value back is masked, a cut quote too; the
/// status of a refusal stays, 401 and 403 among them.
#[test]
fn a_value_quoted_back_is_masked_and_the_status_stays() {
    let extra = Extra::checked(access()).unwrap();
    for status in [401, 403, 302] {
        let body = format!("denied {ID} with {SECRET} at the edge");
        let masked = extra.masked(ConnectError::Refused { status, body });
        let ConnectError::Refused { status: kept, body } = &masked else { panic!("{masked:?}") };
        assert_eq!(*kept, status);
        assert_eq!(body, &format!("denied {MASK} with {MASK} at the edge"));
    }
    let cut = format!("denied {}…", &SECRET[..12]);
    assert_eq!(extra.mask(&cut), format!("denied {MASK}…"));
    assert_eq!(extra.mask(&format!("x {}", &SECRET[..20])), format!("x {MASK}"));
    assert_eq!(extra.mask("made-up"), "made-up", "a start shorter than the bound stays");
    let http = extra.masked(ConnectError::Http(format!("HTTP 403 {SECRET}")));
    assert!(!http.to_string().contains(SECRET), "{http}");
    assert!(Extra::default().masked(ConnectError::Http(SECRET.into())).to_string().contains(SECRET), "none given");
    let ended = extra.masked(ConnectError::Ended(format!("the carrier ended at {SECRET}")));
    assert_eq!(ended, ConnectError::Ended(format!("the carrier ended at {MASK}")), "the kind stays");
}

/// The lines of a head of HTTP/1.1, in the caller's order.
#[test]
fn the_lines_are_written_in_order() {
    let mut out = Vec::new();
    Extra::checked(access()).unwrap().write(&mut out);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        format!("CF-Access-Client-Id: {ID}\r\nCF-Access-Client-Secret: {SECRET}\r\n")
    );
}
