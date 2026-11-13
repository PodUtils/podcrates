use super::*;

#[test]
fn the_connect_request() {
    let h = connect_headers_form("example.com:443", Form::Plain);
    assert_eq!(&h[..5], &[0, 0, 0xcf, 0x50, 15]);
    assert_eq!(&h[5..], b"example.com:443");
    let long = "a".repeat(200) + ":1";
    let h = connect_headers_form(&long, Form::Plain);
    let e = connect_headers_form("a:1", Form::Extended);
    assert_eq!(&e[..5], &[0, 0, 0xcf, 0xd7, 0xc1]);
    assert!(e.windows(14).any(|w| w == b"cf-connect-tcp"));
    assert!(e.windows(9).any(|w| w == b":protocol"));
    let p = connect_headers_form("a:1", Form::Header);
    assert!(p.windows(16).any(|w| w == b"cf-connect-proto"));
    assert_eq!(h[4], 0x7f);
    assert_eq!(authority("2606:4700::1111", 443), "[2606:4700::1111]:443");
    assert_eq!(authority("1.1.1.1", 80), "1.1.1.1:80");
}

/// podnet's `PODNET_WARP_L4_FORM`: an empty value is the plain form, and a
/// value that is not a form is refused (podnet took it as plain).
#[test]
fn the_form_of_the_connect_is_checked() {
    assert_eq!(Form::parse("").unwrap(), Form::Plain);
    assert_eq!(Form::parse("plain").unwrap(), Form::Plain);
    assert_eq!(Form::parse(" Extended ").unwrap(), Form::Extended);
    assert_eq!(Form::parse("header").unwrap(), Form::Header);
    let err = Form::parse("rfc9220").unwrap_err();
    assert_eq!(err.class, Class::Config);
    assert!(err.chain().contains("plain, extended, header"), "{}", err.chain());
}

/// The DATA frames of a CONNECT stream reach the pipe in order, in chunks
/// that cut each header and each payload, with no limit on the size of a
/// frame; a frame of another type is skipped.
#[test]
fn the_data_of_a_stream_crosses_each_cut() {
    let large = vec![7u8; crate::h3::wire::MAX_FRAME + 10];
    let mut stream = frame(FRAME_DATA, b"one");
    stream.extend_from_slice(&frame(0x21, b"a reserved frame"));
    stream.extend_from_slice(&frame(FRAME_DATA, &large));
    stream.extend_from_slice(&frame(crate::h3::FRAME_HEADERS, b"trailers"));
    stream.extend_from_slice(&frame(FRAME_DATA, b"two"));
    let mut want = b"one".to_vec();
    want.extend_from_slice(&large);
    want.extend_from_slice(b"two");
    for cut in [1, 2, 3, 7, 1000, stream.len()] {
        let mut data = DataFrames::default();
        let mut out = Vec::new();
        for chunk in stream.chunks(cut) {
            data.feed(chunk, &mut out);
        }
        assert!(out == want, "a cut of {cut} bytes gives {} bytes", out.len());
    }
}
