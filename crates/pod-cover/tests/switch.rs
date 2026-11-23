//! The switch: a valid proof opens a session, a replay and a wrong proof fall
//! through to the site, and the exporter binds a proof to its handshake.

use std::fs;
use std::path::{Path, PathBuf};

use pod_cover::{Outcome, Site, Switch};

const KEY: [u8; 32] = [7u8; 32];
const NONCE: [u8; 16] = [9u8; 16];
const NOW: i64 = 1_780_000_000;

static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A temporary site tree with one page.
struct Temp {
    path: PathBuf,
}

impl Temp {
    fn new() -> Temp {
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("pod-cover-switch-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a temporary directory");
        fs::write(path.join("index.html"), "<!doctype html><title>The hill</title>\nThe hill.\n").expect("the index");
        Temp { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn a_valid_proof_opens_a_session_one_time() {
    let mut server = Switch::new(KEY);
    let proof = Switch::new(KEY).prove(NONCE, NOW);
    assert_eq!(server.authenticate(&proof, NOW), Outcome::Session);
    // SWITCH-3: the same proof in the window is a repeat.
    assert_eq!(server.authenticate(&proof, NOW + 1), Outcome::Cover);
    // A different nonce still opens a session.
    let other = Switch::new(KEY).prove([3u8; 16], NOW + 1);
    assert_eq!(server.authenticate(&other, NOW + 1), Outcome::Session);
}

#[test]
fn a_replayed_proof_outside_the_window_is_cover() {
    let now = NOW;
    let proof = Switch::new(KEY).prove(NONCE, now);
    let mut server = Switch::new(KEY);
    assert_eq!(server.authenticate(&proof, now + pod_cover::switch::WINDOW_SECS + 1), Outcome::Cover);
    // A timestamp too far in the future is refused too.
    let future = Switch::new(KEY).prove([4u8; 16], now + pod_cover::switch::SKEW_SECS + 1);
    assert_eq!(server.authenticate(&future, now), Outcome::Cover);
}

#[test]
fn a_wrong_proof_falls_through_and_the_site_serves_the_same_bytes() {
    let temp = Temp::new();
    let site = Site::load(temp.path()).expect("a site");
    let mut server = Switch::new(KEY);
    let request = b"GET / HTTP/1.1\r\nHost: cover.example\r\n\r\n".to_vec();
    assert_eq!(server.authenticate(&request, NOW), Outcome::Cover);
    let response = site.respond(&request);
    assert_eq!(response.status, 200);
    assert!(String::from_utf8_lossy(&response.body).contains("The hill"));
    // A proof with one changed byte is a wrong proof, not a close.
    let mut proof = Switch::new(KEY).prove(NONCE, NOW);
    let last = proof.len() - 1;
    proof[last] ^= 0x01;
    assert_eq!(server.authenticate(&proof, NOW), Outcome::Cover);
    // So is a proof of another key.
    let foreign = Switch::new([8u8; 32]).prove(NONCE, NOW);
    assert_eq!(server.authenticate(&foreign, NOW), Outcome::Cover);
    // A short frame is the cover site's request.
    assert_eq!(server.authenticate(b"GET", NOW), Outcome::Cover);
}

#[test]
fn the_exporter_binds_a_proof_to_its_handshake() {
    let exporter = b"the exporter of this handshake";
    let mut server = Switch::new(KEY).bind(exporter);
    let proof = Switch::new(KEY).bind(exporter).prove(NONCE, NOW);
    assert_eq!(server.authenticate(&proof, NOW), Outcome::Session);
    // The same proof on another connection, with another exporter, is cover.
    let mut other = Switch::new(KEY).bind(b"another handshake");
    assert_eq!(other.authenticate(&proof, NOW), Outcome::Cover);
    // A server with no binding also refuses it.
    assert_eq!(Switch::new(KEY).authenticate(&proof, NOW), Outcome::Cover);
}

#[test]
fn the_proof_has_the_documented_length() {
    let proof = Switch::new(KEY).prove(NONCE, NOW);
    assert_eq!(proof.len(), pod_cover::switch::PROOF_LEN);
    assert_eq!(&proof[..8], &NOW.to_be_bytes());
    assert_eq!(&proof[8..24], &NONCE);
    // More bytes after the proof are a session too, and they are not read.
    let mut more = proof.clone();
    more.extend_from_slice(b"tunnel bytes");
    assert_eq!(Switch::new(KEY).authenticate(&more, NOW), Outcome::Session);
}
