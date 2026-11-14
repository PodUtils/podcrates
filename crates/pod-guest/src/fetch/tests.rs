use super::*;
use pod_core::Class;
use std::io::Cursor;

mod stand;

/// The sha256 of the empty stream: the vector that matches.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
/// A sha256 that matches nothing: the vector of a mismatch.
const WRONG_SHA256: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn ceiling(max: u64, fsize: u64) -> Ceiling {
    Ceiling { max_bytes: max, fsize }
}

/// A temporary directory of one test.
fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pod-guest-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// One stream through [`Stream`], with one try.
fn store_stream(body: Vec<u8>, declared: Option<u64>, dest: &Path, want: Option<&str>, c: Ceiling) -> Result<String> {
    store(&mut Stream::new(Cursor::new(body), declared), dest, want, c, 1)
}

#[test]
fn the_effective_ceiling_is_the_smaller_of_the_two() {
    assert_eq!(ceiling(10, 4).effective(), 4);
    assert_eq!(ceiling(4, 10).effective(), 4);
    assert_eq!(ceiling(u64::MAX, u64::MAX).effective(), u64::MAX);
}

#[test]
fn a_declared_length_over_the_ceiling_refuses_before_any_byte_is_written() {
    let e = check_declared(Some(1 << 30), ceiling(1 << 20, u64::MAX)).unwrap_err();
    assert!(matches!(refusal(&e), Some(Refusal::DeclaredTooLarge { .. })), "{e}");
    assert_eq!(e.class, Class::Policy);
    assert!(format!("{e}").contains("nothing was downloaded"), "{e}");
    // At the ceiling is allowed; over it is not.
    assert!(check_declared(Some(1024), ceiling(1024, u64::MAX)).is_ok());
    // The check during the transfer bounds a length that was not declared.
    assert!(check_declared(None, ceiling(1, 1)).is_ok());
    // The store refuses it with no byte on the disk.
    let d = dir("declared");
    let dest = d.join("big.img");
    let e = store_stream(vec![1u8; 64], Some(1 << 30), &dest, None, ceiling(1 << 20, u64::MAX)).unwrap_err();
    assert!(matches!(refusal(&e), Some(Refusal::DeclaredTooLarge { .. })), "{e}");
    assert!(!dest.exists() && !partial(&dest).exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_undeclared_transfer_is_stopped_at_the_ceiling_and_the_partial_removed() {
    let d = dir("fetch");
    let dest = d.join("big.img");
    let e = store_stream(vec![7u8; 4096], None, &dest, None, ceiling(1000, u64::MAX)).unwrap_err();
    assert_eq!(refusal(&e), Some(&Refusal::OverCeiling { ceiling: 1000 }), "{e}");
    assert!(!dest.exists(), "nothing is left at the destination");
    assert!(!partial(&dest).exists(), "and no partial either");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_file_size_bound_binds_even_when_the_caller_asked_for_more() {
    let d = dir("fsize");
    let dest = d.join("fsize.img");
    let e = store_stream(vec![0u8; 2048], None, &dest, None, ceiling(u64::MAX, 1024)).unwrap_err();
    assert_eq!(refusal(&e), Some(&Refusal::OverCeiling { ceiling: 1024 }), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_good_download_lands_and_reports_its_digest() {
    let d = dir("ok");
    let dest = d.join("ok.img");
    // The sha256 of an empty stream.
    let got = store_stream(Vec::new(), Some(0), &dest, Some(EMPTY_SHA256), ceiling(1024, 1024)).unwrap();
    assert!(got.starts_with("e3b0c442"), "{got}");
    assert_eq!(std::fs::read(&dest).unwrap().len(), 0);
    assert!(!partial(&dest).exists(), "the partial file has the name of the destination now");
    // The digest ignores the case: a pin can have either case.
    let d2 = d.join("ok2.img");
    assert!(
        store_stream(
            b"guest".to_vec(),
            None,
            &d2,
            Some("2D7C5F0E1B1E4B1E9C0F1A2B3C4D5E6F708192A3B4C5D6E7F8091A2B3C4D5E6F"),
            ceiling(1024, 1024),
        )
        .is_err()
    );
    assert!(!d2.exists(), "a mismatched download leaves nothing behind");
    let d3 = d.join("ok3.img");
    let upper = EMPTY_SHA256.to_ascii_uppercase();
    assert!(store_stream(Vec::new(), Some(0), &d3, Some(&upper), ceiling(1024, 1024)).is_ok());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_digest_mismatch_names_both_digests_and_removes_the_file() {
    let d = dir("bad");
    let dest = d.join("bad.img");
    let e = store_stream(b"payload".to_vec(), None, &dest, Some(WRONG_SHA256), ceiling(1 << 20, u64::MAX)).unwrap_err();
    assert!(matches!(refusal(&e), Some(Refusal::Digest { .. })), "{e}");
    assert_eq!(e.class, Class::Protocol);
    let msg = format!("{e}");
    assert!(msg.contains("0000") && msg.contains("sha256"), "{msg}");
    assert!(!dest.exists());
    assert!(!partial(&dest).exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// The partial file has a fixed name, so that a later call resumes it. Its
/// lock keeps a second store out: two writers in one file give a digest
/// that matches nothing.
#[test]
fn two_fetches_of_one_destination_do_not_share_a_partial_file() {
    let d = dir("busy");
    let dest = d.join("base.vhdx");
    assert_eq!(partial(&dest), d.join("base.vhdx.part"));
    std::fs::write(partial(&dest), b"first").unwrap();
    let held = File::open(partial(&dest)).unwrap();
    held.lock().unwrap();
    let e = store_stream(b"second".to_vec(), None, &dest, None, ceiling(1024, 1024)).unwrap_err();
    assert!(matches!(refusal(&e), Some(Refusal::Busy { .. })), "{e}");
    assert_eq!(e.class, Class::TempFail);
    assert_eq!(std::fs::read(partial(&dest)).unwrap(), b"first", "the file of the first store stays as it was");
    assert!(!dest.exists());
    drop(held);
    store_stream(b"second".to_vec(), None, &dest, None, ceiling(1024, 1024)).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"second");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_content_range_parses_by_its_grammar_alone() {
    let span = |start, end, total| Some(Span { start, end, total });
    assert_eq!(parse_content_range("bytes 400-999/1000"), span(400, Some(999), Some(1000)));
    assert_eq!(parse_content_range("Bytes 0-0/*"), span(0, Some(0), None));
    assert_eq!(parse_content_range("bytes */1000"), span(0, None, Some(1000)));
    for bad in ["bytes 9-1/10", "items 0-1/2", "bytes -1-2/3", "bytes 1-2", "bytes */*", "bytes +1-2/3", ""] {
        assert_eq!(parse_content_range(bad), None, "{bad:?}");
    }
}

/// A stream that ends in a break: [`Stream`] cannot resume, and the bytes
/// so far stay for a later call.
#[test]
fn a_stream_that_breaks_keeps_its_bytes_for_a_later_call() {
    struct Broken(usize);
    impl Read for Broken {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.0 == 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset"));
            }
            let n = self.0.min(buf.len());
            buf[..n].fill(9);
            self.0 -= n;
            Ok(n)
        }
    }
    let d = dir("broken");
    let dest = d.join("img");
    let e = store(&mut Stream::new(Broken(100), Some(300)), &dest, None, ceiling(1024, 1024), 3).unwrap_err();
    let Some(Refusal::Broken { have: 100, total: Some(300), tries: 3, last }) = refusal(&e) else { panic!("{e}") };
    assert!(last.contains("does not resume"), "{last}");
    assert_eq!(e.class, Class::TempFail);
    assert_eq!(std::fs::read(partial(&dest)).unwrap().len(), 100);
    let _ = std::fs::remove_dir_all(&d);
}
