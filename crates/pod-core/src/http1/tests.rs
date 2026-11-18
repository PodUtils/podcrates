use super::*;

/// What [`find_request`] gives.
type Found = Result<Option<(usize, usize)>, RequestError>;

/// A status and its reason, or a refusal.
type Status = Result<(u16, &'static str), ()>;

fn parse(text: &str) -> Result<Request, RequestError> {
    parse_request(text.as_bytes())
}

/// Each head that two readers could read two ways is refused (400), and
/// another version of HTTP gets 505.
#[test]
fn a_head_that_could_be_read_two_ways_is_refused() {
    for bad in [
        "CONNECT a:1 HTTP/1.1\r\nHost : a\r\n\r\n",
        "CONNECT a:1 HTTP/1.1\r\nHost: a\r\n folded\r\n\r\n",
        "CONNECT a:1 HTTP/1.1\r\nX: a\rb\r\n\r\n",
        "CONNECT a:1 HTTP/1.1\r\nX: \x00\r\n\r\n",
        "CONNECT  a:1 HTTP/1.1\r\n\r\n",
        "CONNECT a:1\r\n\r\n",
        "CONNECT a:1 HTTP/1.1 x\r\n\r\n",
        "CONN(ECT a:1 HTTP/1.1\r\n\r\n",
        "CONNECT a:1 HTTP/1.1\r\nno colon\r\n\r\n",
        "CONNECT a:1 SPDY/3\r\n\r\n",
    ] {
        let err = parse(bad).expect_err(bad);
        assert_eq!(err.status(), Some(400), "{bad:?}: {err}");
    }
    assert_eq!(parse("PRI * HTTP/2.0\r\n\r\n").unwrap_err().status(), Some(505));
}

/// podrunner's check (PodUtils/podrunner#289): a request line with a
/// control character is refused, as its path goes into logs and onto a
/// terminal. Each control byte of ASCII, and C1 controls in UTF-8.
#[test]
fn a_request_line_with_a_control_character_is_refused() {
    let mut cases: Vec<String> = (0u8..0x20).chain([0x7f]).map(|b| format!("GET /a{}b HTTP/1.1", b as char)).collect();
    cases.extend(["GET /a\u{85}b HTTP/1.1".to_string(), "GET /\u{1b}[31m HTTP/1.1".to_string()]);
    cases.push("GET\t/ HTTP/1.1".to_string());
    for line in cases {
        let head = format!("{line}\r\nHost: h\r\n\r\n");
        let err = parse(&head).expect_err(&format!("{line:?}"));
        assert_eq!(err.status(), Some(400), "{line:?}: {err}");
    }
    assert!(parse("GET /a-b HTTP/1.1\r\nHost: h\r\n\r\n").is_ok());
}

/// podrunner's check (PodUtils/podrunner#289): `Authorization` is found by
/// its name in any case, and its value keeps its case and its inner spaces;
/// only the spaces and tabs around it go.
#[test]
fn an_authorization_header_keeps_its_value_as_given() {
    let table = [
        ("Authorization: Bearer AbC.dEf-1\r\n", "Bearer AbC.dEf-1"),
        ("AUTHORIZATION:Bearer x\r\n", "Bearer x"),
        ("authorization: \t Basic  dTpw \t\r\n", "Basic  dTpw"),
        ("Authorization:\r\n", ""),
    ];
    for (field, want) in table {
        let request = parse(&format!("GET /v1 HTTP/1.1\r\nHost: h\r\n{field}\r\n")).unwrap();
        for name in ["authorization", "Authorization", "AUTHORIZATION"] {
            assert_eq!(request.header(name), Some(want), "{field:?} by {name}");
        }
    }
    let two = parse("GET / HTTP/1.1\r\nAuthorization: first\r\nAuthorization: second\r\n\r\n").unwrap();
    assert_eq!(two.header("authorization"), Some("first"), "the first value");
}

#[test]
fn a_list_header_holds_its_tokens_in_any_case() {
    let request = parse("GET / HTTP/1.1\r\nConnection: keep-alive, Upgrade\r\nUpgrade: WebSocket\r\n\r\n").unwrap();
    assert!(request.has_token("connection", "upgrade") && request.has_token("Upgrade", "websocket"));
    assert!(!request.has_token("connection", "close"));
}

/// The pod-ws server's end of a head: after its empty line, ended by CRLF
/// or a bare LF.
#[test]
fn the_end_of_a_head_is_after_its_empty_line() {
    assert_eq!(head_bounds(b"GET / HTTP/1.1\r\nHost: h\r\n\r\nrest"), Some((0, 27)));
    assert_eq!(head_bounds(b"GET / HTTP/1.1\nHost: h\n\nrest"), Some((0, 24)));
    assert_eq!(head_bounds(b"GET / HTTP/1.1\r\nHost: h\r\n"), None);
    assert_eq!(crlf_head_end(b"HTTP/1.1 200 OK\r\n\r\nrest"), Some(19));
    assert_eq!(crlf_head_end(b"HTTP/1.1 200 OK\n\n"), None, "a response ends with CRLF CRLF");
}

/// The bounds of a request head: its bytes, and its lines without the
/// empty line; empty lines before it are passed over.
#[test]
fn a_request_head_is_found_within_its_bounds() {
    let bounds = Bounds { bytes: 64, lines: 3 };
    let table: [(&[u8], Found); 7] = [
        (b"GET / HTTP/1.1\r\nA: 1\r\nB: 2\r\n\r\nrest", Ok(Some((0, 30)))),
        (b"\r\n\nGET / HTTP/1.1\r\n\r\n", Ok(Some((3, 21)))),
        (b"GET / HTTP/1.1\r\nA: 1\r\nB: 2\r\n", Ok(None)),
        (b"GET / HTTP/1.1\r\nA: 1\r\nB: 2\r\nC: 3\r\n", Err(RequestError::TooLarge)),
        (b"GET / HTTP/1.1\r\nA: 1\r\nB: 2\r\nC: 3\r\n\r\n", Err(RequestError::TooLarge)),
        (&[b'x'; 65], Err(RequestError::TooLarge)),
        (b"GET / HTTP/1.1\r\nX: 0123456789012345678901234567890123456789012345\r\n\r\n", Err(RequestError::TooLarge)),
    ];
    for (buf, want) in table {
        assert_eq!(find_request(buf, bounds), want, "{:?}", String::from_utf8_lossy(buf));
    }
    let endless = b"\n".repeat(10);
    assert_eq!(find_request(&endless, Bounds::DEFAULT), Ok(None), "empty lines alone are no head");
}

/// A status line, as a proxy or a server sends it: the reason is made safe
/// text on one line; a line that is not HTTP is refused.
#[test]
fn a_status_line_gives_its_status_and_a_safe_reason() {
    let table: [(&str, Status); 6] = [
        ("HTTP/1.1 200 Connection Established\r\n\r\n", Ok((200, "Connection Established"))),
        ("HTTP/1.0 407 Proxy\x1b[31m Auth\r\n\r\n", Ok((407, "Proxy Auth"))),
        ("HTTP/1.1 204\r\n\r\n", Ok((204, ""))),
        ("HTTP/2 101 Switching Protocols", Ok((101, "Switching Protocols"))),
        ("SSH-2.0-x\r\n\r\n", Err(())),
        ("HTTP/1.1 abc OK\r\n\r\n", Err(())),
    ];
    for (head, want) in table {
        let got = parse_status_line(head);
        assert_eq!(got.as_ref().map(|(s, r)| (*s, r.as_str())).map_err(|_| ()), want, "{head:?}");
    }
    let (_, _, headers) = parse_head("HTTP/1.1 200 OK\r\nContent-Length:  3 \r\nno colon\r\nX-A: b\r\n\r\n").unwrap();
    assert_eq!(headers, [("content-length".into(), "3".into()), ("x-a".into(), "b".into())]);
}

/// How an answer's body ends, and whether its connection is kept.
#[test]
fn the_framing_of_an_answer_and_its_keep() {
    let h = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect()
    };
    let table = [
        (200, h(&[("content-length", "5")]), Framing::Length(5)),
        (200, h(&[("transfer-encoding", "Chunked"), ("content-length", "5")]), Framing::Chunked),
        (200, h(&[]), Framing::UntilClose),
        (200, h(&[("content-length", "x")]), Framing::UntilClose),
        (204, h(&[("content-length", "5")]), Framing::Length(0)),
        (304, h(&[]), Framing::Length(0)),
        (100, h(&[]), Framing::Length(0)),
    ];
    for (status, headers, want) in table {
        assert_eq!(Framing::of(status, &headers), want, "{status} {headers:?}");
    }
    assert!(keeps(true, &h(&[("connection", "keep-alive")]), Framing::Length(1)));
    assert!(!keeps(true, &h(&[("connection", "Close")]), Framing::Length(1)));
    assert!(!keeps(false, &h(&[]), Framing::Chunked), "HTTP/1.0");
    assert!(!keeps(true, &h(&[]), Framing::UntilClose));
}

/// A head written by this module reads back the same; a part with CR, LF
/// or NUL is refused.
#[test]
fn a_written_head_reads_back_and_a_line_break_is_refused() {
    let head = request_head("GET", "/v1/jobs?x=1", &[("Host", "h"), ("Authorization", "Bearer AbC")]).unwrap();
    assert_eq!(head, "GET /v1/jobs?x=1 HTTP/1.1\r\nHost: h\r\nAuthorization: Bearer AbC\r\n\r\n");
    let request = parse(&head).unwrap();
    assert_eq!((request.method.as_str(), request.target.as_str()), ("GET", "/v1/jobs?x=1"));
    assert_eq!(request.header("authorization"), Some("Bearer AbC"));
    let answer = response_head(206, reason(206), &[("Content-Length", "4")]).unwrap();
    assert_eq!(answer, "HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\n\r\n");
    assert_eq!(parse_status_line(&answer), Ok((206, "Partial Content".to_string())));
    for bad in ["a\rb", "a\nb", "a\0b"] {
        assert!(request_head(bad, "/", &[]).is_err(), "{bad:?}");
        assert!(request_head("GET", bad, &[]).is_err(), "{bad:?}");
        assert!(request_head("GET", "/", &[("X", bad)]).is_err(), "{bad:?}");
        assert!(request_head("GET", "/", &[(bad, "v")]).is_err(), "{bad:?}");
        assert!(response_head(200, bad, &[]).is_err(), "{bad:?}");
        assert!(response_head(200, "OK", &[("X", bad)]).is_err(), "{bad:?}");
    }
}

#[test]
fn a_refusal_says_its_words_on_one_line_and_closes() {
    let answer = String::from_utf8(refusal(403, "not in the ALLOW list\r\nX: y", &[("A", "b")])).unwrap();
    assert!(answer.starts_with("HTTP/1.1 403 Forbidden: not in the ALLOW list X: y\r\nA: b\r\n"), "{answer}");
    assert!(answer.contains("Connection: close\r\n") && answer.ends_with("\r\n\r\nnot in the ALLOW list X: y\n"));
    let bare = String::from_utf8(refusal(405, "", &[("Allow", "CONNECT")])).unwrap();
    assert_eq!(
        bare,
        "HTTP/1.1 405 Method Not Allowed\r\nAllow: CONNECT\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    assert_eq!(established(), b"HTTP/1.1 200 Connection Established\r\n\r\n");
}

/// Each status that a server of podssh sends has its phrase of RFC 9110,
/// none "Error": the relay role's 404, 409, 413 and 429 among them.
#[test]
fn each_status_that_podssh_sends_has_its_phrase() {
    let named = [
        (404, "Not Found"),
        (409, "Conflict"),
        (410, "Gone"),
        (411, "Length Required"),
        (413, "Content Too Large"),
        (429, "Too Many Requests"),
    ];
    for (status, phrase) in named {
        assert_eq!(reason(status), phrase, "{status}");
    }
    let sent = [101, 200, 400, 403, 404, 405, 407, 408, 409, 410, 411, 413, 426, 429, 431, 502, 503, 504, 505];
    for status in sent {
        assert_ne!(reason(status), "Error", "{status} has no phrase");
    }
    let answer = String::from_utf8(refusal(429, "this relay keeps 64 pairs already", &[])).unwrap();
    assert!(answer.starts_with("HTTP/1.1 429 Too Many Requests: this relay keeps"), "{answer}");
}

/// A page: 200 OK (not a CONNECT's words), its type, its length, and to
/// a HEAD the same head with no body.
#[test]
fn a_page_is_answered_with_its_type_and_length_and_a_head_with_no_body() {
    let page = String::from_utf8(answer(200, "text/html; charset=utf-8", b"<p>hi</p>\n", false)).unwrap();
    assert!(page.starts_with("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n"), "{page}");
    assert!(page.contains("\r\nContent-Length: 10\r\n") && page.contains("\r\nConnection: close\r\n"), "{page}");
    assert!(page.contains("\r\nX-Content-Type-Options: nosniff\r\n") && page.ends_with("\r\n\r\n<p>hi</p>\n"));
    let head = String::from_utf8(answer(404, "text/plain\r\nX: y", b"no such page\n", true)).unwrap();
    assert!(head.starts_with("HTTP/1.1 404 Not Found\r\nContent-Type: text/plain X: y\r\n"), "{head}");
    assert!(head.contains("\r\nContent-Length: 13\r\n") && head.ends_with("\r\n\r\n"), "{head}");
}

/// A head read one byte at a time stops at its empty line; a stream that
/// ends first, or a head past its bound, is an error.
#[test]
fn a_head_is_read_to_its_empty_line_and_no_further() {
    let mut wire: &[u8] = b"HTTP/1.1 200 Connection established\r\n\r\nSSH-2.0-x\r\n";
    let head = read_head_exact(&mut wire, 64).unwrap();
    assert_eq!(head, b"HTTP/1.1 200 Connection established\r\n\r\n");
    assert_eq!(wire, b"SSH-2.0-x\r\n", "the bytes after the head stay");
    let short = read_head_exact(&mut &b"HTTP/1.1 200 OK\r\n"[..], 64).unwrap_err();
    assert_eq!(short.kind(), io::ErrorKind::UnexpectedEof);
    let long = read_head_exact(&mut &[b'x'; 100][..], 64).unwrap_err();
    assert_eq!(
        (long.kind(), long.to_string()),
        (io::ErrorKind::InvalidData, "no end of headers within 64 bytes".into())
    );
}
