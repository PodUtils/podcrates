//! The fuzz target of the head of an HTTP/1.1 message of `pod-core`.

use pod_core::http1::{Bounds, Framing, find_request, keeps, parse_head, parse_request, request_head};

/// A request head (`pod_core::http1::parse_request`, `find_request`; new
/// in `pod-core`, T-882), as a client sends it to a server of a consumer:
/// a head that is found is within its bounds; a request that is read has a
/// first line with no control character (podrunner's check), a method that
/// is a token, a target of visible ASCII, header names in lower case and
/// values with no control byte of ASCII but a tab, and it reads back the
/// same from the head that `request_head` writes of it. A response head of
/// the same bytes gives a framing with no panic.
pub fn http_request(data: &[u8]) {
    let bounds = Bounds { bytes: usize::from(data.first().copied().unwrap_or(0)) * 64, lines: 8 };
    if let Ok(Some((start, end))) = find_request(data, bounds) {
        assert!(start <= end && end <= data.len(), "{start}..{end} of {}", data.len());
        assert!(end - start <= bounds.bytes, "{} bytes past {}", end - start, bounds.bytes);
    }
    if let Ok(request) = parse_request(data) {
        let first = data.split(|&b| b == b'\n').next().unwrap_or_default();
        let first = String::from_utf8_lossy(first.strip_suffix(b"\r").unwrap_or(first)).into_owned();
        assert!(!first.chars().any(char::is_control), "{first:?}");
        assert!(!request.method.is_empty() && request.target.bytes().all(|b| b.is_ascii_graphic()), "{first:?}");
        for (name, value) in &request.headers {
            assert_eq!(*name, name.to_ascii_lowercase(), "{name:?}");
            assert!(!value.chars().any(|c| (c < ' ' && c != '\t') || c == '\u{7f}'), "{value:?}");
        }
        let fields: Vec<(&str, &str)> = request.headers.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
        let written = request_head(&request.method, &request.target, &fields).expect("a request that was read");
        let again = parse_request(written.as_bytes()).expect("a written head");
        assert_eq!(
            (&again.method, &again.target, &again.headers),
            (&request.method, &request.target, &request.headers)
        );
    }
    if let Ok((status, _, headers)) = parse_head(&String::from_utf8_lossy(data)) {
        let _ = keeps(true, &headers, Framing::of(status, &headers));
    }
}
