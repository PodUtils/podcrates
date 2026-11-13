use super::*;

#[test]
fn the_form_holds_each_field_then_the_file() {
    let body = multipart("B0", &[("time", "1h"), ("reqtype", "fileupload")], Some(("fileToUpload", b"\x00\xff")));
    let want = "--B0\r\nContent-Disposition: form-data; name=\"time\"\r\n\r\n1h\r\n\
                --B0\r\nContent-Disposition: form-data; name=\"reqtype\"\r\n\r\nfileupload\r\n\
                --B0\r\nContent-Disposition: form-data; name=\"fileToUpload\"; filename=\"piece.bin\"\r\n\
                Content-Type: application/octet-stream\r\n\r\n";
    assert!(body.starts_with(want.as_bytes()), "{}", String::from_utf8_lossy(&body));
    assert!(body.ends_with(b"\x00\xff\r\n--B0--\r\n"));
    let empty = multipart("B0", &[("delete", "")], None);
    assert_eq!(empty, b"--B0\r\nContent-Disposition: form-data; name=\"delete\"\r\n\r\n\r\n--B0--\r\n");
}

#[test]
fn the_segment_goes_after_the_host() {
    assert_eq!(fetch_url("https://h.test/ID/a.bin", "dl"), "https://h.test/dl/ID/a.bin");
    assert_eq!(fetch_url("https://h.test:8443/ID/a.bin", ""), "https://h.test:8443/ID/a.bin");
    assert_eq!(fetch_url("https://h.test", "dl"), "https://h.test/dl");
}

#[test]
fn the_host_has_no_port_and_no_user() {
    assert_eq!(host_of("https://U@Tmpfiles.org:443/x").as_deref(), Some("tmpfiles.org"));
    assert_eq!(host_of("http://tmpfiles.org/x"), None);
    assert_eq!(path_of("https://h.test/ID/a.bin"), "/ID/a.bin");
    assert_eq!(path_of("https://h.test"), "/");
}

/// The page of a file of tmpfiles.org links the signed download of this
/// file, and can link others.
#[test]
fn the_link_of_the_page_is_the_one_of_this_file() {
    let page = r#"<a href="https://tmpfiles.org/dl/1794413600.b4bf/OTHER/piece.bin">other</a>
        <a href="https://tmpfiles.org/api">API</a>
        <a class="download" href="https://tmpfiles.org/dl/1794413600.b4bfd9d193d255dd/wCAu0tnyvx7c/piece.bin">Download</a>"#;
    let answer = "https://tmpfiles.org/wCAu0tnyvx7c/piece.bin";
    assert_eq!(
        page_link(page, "https://tmpfiles.org/dl/", answer).as_deref(),
        Some("https://tmpfiles.org/dl/1794413600.b4bfd9d193d255dd/wCAu0tnyvx7c/piece.bin")
    );
    assert_eq!(page_link(page, "https://tmpfiles.org/dl/", "https://tmpfiles.org/NONE/piece.bin"), None);
    assert_eq!(page_link(page, "https://other.test/dl/", answer), None);
    // A link at the end of the text, with no quote after it.
    let bare = "see https://h.test/dl/9.s/ID/a.bin";
    assert_eq!(
        page_link(bare, "https://h.test/dl/", "https://h.test/ID/a.bin").as_deref(),
        Some("https://h.test/dl/9.s/ID/a.bin")
    );
    // The root is no path of a file.
    assert_eq!(page_link(bare, "https://h.test/dl/", "https://h.test"), None);
}
