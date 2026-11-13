//! The round trip of a file host (`pod_services_watch::file_host`, T-528)
//! against HTTPS stand-ins of the three forms of the catalog: a text
//! answer with a delete by token (0x0.st), a JSON answer whose page links
//! the signed download (tmpfiles.org), and a refusal with its text
//! (litterbox's 412).

use std::sync::{Arc, Mutex};

use pod_services::Catalog;
use pod_services_watch::Finding;
use pod_services_watch::file_host::{Trip, trip};
use pod_testkit::stand::{Answer, Request, Stand};

use crate::common::{NAME, SOURCE, https, net};

const BYTES: &[u8] = b"\x00\x01random bytes of the test\xff\r\n--not a boundary";
const BOUNDARY: &str = "podcratesB0";

/// A file host of the test at the stand-in's `port`: its upload path and
/// its facts (TOML lines).
fn host_file(port: u16, path: &str, facts: &[(&str, &str)]) -> String {
    let mut text = format!(
        "id = \"stand-host\"\nkind = \"file-host\"\nparty = \"a test\"\nabout = \"a stand-in\"\n\n\
         [[endpoint]]\nname = \"upload\"\nscheme = \"https\"\nhost = \"{NAME}\"\nport = {port}\npath = \"{path}\"\n{SOURCE}\n\n"
    );
    for (name, value) in [("serves", &*format!("[\"{NAME}\"]")), ("file-field", "\"file\"")].iter().chain(facts) {
        text.push_str(&format!("[[fact]]\nname = \"{name}\"\nvalue = {value}\n{SOURCE}\n\n"));
    }
    text + "[[watch]]\nkind = \"tls-name\"\nendpoint = \"upload\"\n"
}

/// The trip of the host of `text`.
async fn run(text: &str) -> Result<Trip, Finding> {
    let mut catalog = Catalog::new();
    let service = catalog.lay("stand-host.toml", text).expect("a file of the schema").clone();
    trip(&service, &net(), BYTES, BOUNDARY).await
}

/// The bytes of the file in a form that the stand-in read.
fn file_of(body: &[u8]) -> Vec<u8> {
    let text = body;
    let start = find(text, b"filename=").and_then(|at| find(&text[at..], b"\r\n\r\n").map(|e| at + e + 4));
    let start = start.expect("a file in the form");
    let end = find(&text[start..], format!("\r\n--{BOUNDARY}--").as_bytes()).expect("the end of the form");
    text[start..start + end].to_vec()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The origin of the stand-in, as its requests name it.
fn origin(request: &Request) -> String {
    format!("https://{}", request.header("host").unwrap_or(NAME))
}

/// A host of the form of 0x0.st: the URL as text, the token in a header,
/// the delete by a POST of the token to the URL.
fn token_host(stored: Arc<Mutex<Vec<u8>>>, deleted: Arc<Mutex<bool>>) -> Stand {
    https(move |request| match (request.method.as_str(), request.path()) {
        ("POST", "/") => {
            let body = String::from_utf8_lossy(&request.body).into_owned();
            if !body.contains("name=\"expires\"\r\n\r\n1\r\n") {
                return Answer::new(400, "no expires");
            }
            *stored.lock().unwrap() = file_of(&request.body);
            Answer::new(200, format!("{}/abc.bin\n", origin(request))).header("X-Token", "made-up-token")
        }
        ("GET", "/abc.bin") => Answer::new(200, stored.lock().unwrap().clone()),
        ("POST", "/abc.bin") => {
            let body = String::from_utf8_lossy(&request.body).into_owned();
            let ok = body.contains("name=\"token\"\r\n\r\nmade-up-token\r\n") && body.contains("name=\"delete\"");
            *deleted.lock().unwrap() = ok;
            Answer::new(if ok { 200 } else { 401 }, "")
        }
        _ => Answer::new(404, "no"),
    })
}

#[tokio::test]
async fn a_text_answer_a_download_and_a_delete_by_token() {
    pod_testkit::need_loopback!();
    let (stored, deleted) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(false)));
    let stand = token_host(stored.clone(), deleted.clone());
    let text = host_file(
        stand.port(),
        "/",
        &[
            ("fields", "[\"secret=\", \"expires=1\"]"),
            ("answer", "\"text\""),
            ("fetch-segment", "\"\""),
            ("fetch-method", "\"GET\""),
            ("delete", "\"token-form\""),
            ("delete-token-header", "\"X-Token\""),
            ("delete-token-field", "\"token\""),
            ("delete-fields", "[\"delete=\"]"),
        ],
    );
    let trip = run(&text).await.expect("a trip");
    assert_eq!(*stored.lock().unwrap(), BYTES, "the file of the form");
    assert!(trip.deleted && *deleted.lock().unwrap(), "{trip:?}");
    assert!(trip.lines.iter().all(|l| !l.contains("made-up-token")), "no token in a line: {trip:?}");
}

/// A host of the form of tmpfiles.org: JSON, a page at the URL, the file
/// at the signed link of the page; `/dl/ID/NAME` sends to the page.
fn page_host(stored: Arc<Mutex<Vec<u8>>>) -> Stand {
    https(move |request| {
        let at = origin(request);
        match (request.method.as_str(), request.path()) {
            ("POST", "/api/v1/upload") => {
                *stored.lock().unwrap() = file_of(&request.body);
                Answer::new(200, format!(r#"{{"status":"success","data":{{"url":"{at}/ID12/piece.bin"}}}}"#))
            }
            ("GET", "/ID12/piece.bin") => Answer::new(
                200,
                format!(
                    "<html><a href=\"{at}/dl/1.sig/OTHER/piece.bin\">x</a>\
                     <a href=\"{at}/dl/1794413600.b4bf/ID12/piece.bin\">Download</a></html>"
                ),
            ),
            ("GET", "/dl/1794413600.b4bf/ID12/piece.bin") => Answer::new(200, stored.lock().unwrap().clone()),
            ("GET", "/dl/ID12/piece.bin") => Answer::new(302, "").header("Location", "/ID12/piece.bin"),
            _ => Answer::new(404, "no"),
        }
    })
}

fn page_facts(prefix: &str) -> Vec<(&'static str, String)> {
    vec![
        ("fields", "[\"expire=60\"]".into()),
        ("answer", "\"json\"".into()),
        ("answer-pointer", "\"/data/url\"".into()),
        ("fetch-segment", "\"\"".into()),
        ("fetch-link-prefix", format!("\"{prefix}\"")),
        ("fetch-method", "\"GET\"".into()),
        ("delete", "\"none\"".into()),
    ]
}

#[tokio::test]
async fn a_json_answer_and_the_signed_link_of_its_page() {
    pod_testkit::need_loopback!();
    let stand = page_host(Arc::new(Mutex::new(Vec::new())));
    let prefix = format!("https://{NAME}:{}/dl/", stand.port());
    let facts = page_facts(&prefix);
    let facts: Vec<(&str, &str)> = facts.iter().map(|(n, v)| (*n, v.as_str())).collect();
    let trip = run(&host_file(stand.port(), "/api/v1/upload", &facts)).await.expect("a trip");
    assert!(trip.fetched.ends_with("/dl/1794413600.b4bf/ID12/piece.bin"), "{trip:?}");
    assert!(!trip.deleted);
}

/// The form of before T-528 (`fetch-segment` `dl`, no link prefix) gets
/// the page, and the trip says so.
#[tokio::test]
async fn the_page_in_place_of_the_file_is_a_finding() {
    pod_testkit::need_loopback!();
    let stand = page_host(Arc::new(Mutex::new(Vec::new())));
    let mut facts = page_facts("");
    facts.retain(|(n, _)| *n != "fetch-link-prefix" && *n != "fetch-segment");
    facts.push(("fetch-segment", "\"dl\"".into()));
    let facts: Vec<(&str, &str)> = facts.iter().map(|(n, v)| (*n, v.as_str())).collect();
    let why = run(&host_file(stand.port(), "/api/v1/upload", &facts)).await.expect_err("the page is no file");
    assert!(format!("{why:?}").contains("they begin: <html>"), "{why:?}");
}

#[tokio::test]
async fn a_page_with_no_link_of_this_file_is_a_finding() {
    pod_testkit::need_loopback!();
    let stand = page_host(Arc::new(Mutex::new(Vec::new())));
    let prefix = format!("https://{NAME}:{}/elsewhere/", stand.port());
    let facts = page_facts(&prefix);
    let facts: Vec<(&str, &str)> = facts.iter().map(|(n, v)| (*n, v.as_str())).collect();
    let why = run(&host_file(stand.port(), "/api/v1/upload", &facts)).await.expect_err("no link");
    assert!(format!("{why:?}").contains("the page holds no link"), "{why:?}");
}

/// litterbox answers 412 with a sentence that names the fault: the
/// finding keeps the status and the sentence.
#[tokio::test]
async fn a_refusal_names_its_status_and_its_text() {
    pod_testkit::need_loopback!();
    let stand = https(|_| Answer::new(412, "No request type given."));
    let text = host_file(
        stand.port(),
        "/resources/internals/api.php",
        &[
            ("fields", "[\"time=1h\", \"fileNameLength=16\", \"reqtype=fileupload\"]"),
            ("answer", "\"text\""),
            ("fetch-segment", "\"\""),
            ("fetch-method", "\"GET\""),
            ("delete", "\"none\""),
        ],
    );
    let why = run(&text).await.expect_err("a refusal");
    let why = format!("{why:?}");
    assert!(why.contains("the upload answered HTTP 412: No request type given."), "{why:?}");
}
