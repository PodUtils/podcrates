//! The keys of the iroh road (PodUtils/podssh@T-163) are the identities of
//! PodUtils/podssh@T-087: iroh's secret key is the seed of the key file, which
//! iroh itself parses; a key is shown by its fingerprint, which the allowlist
//! takes; and a key file of PodUtils/podssh@T-163 stays the key. The files' own
//! rules are tested in `pod-identity` (`tests/identity_file.rs`).

mod cleanup;

use std::path::PathBuf;
use std::str::FromStr;

use iroh::SecretKey;
use pod_iroh::Allowlist;
use pod_iroh::keys::{self, Place};
use pod_session::{Entropy, NoRandom, OsEntropy};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pod-iroh-keys-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    cleanup::at_test_end(&dir);
    dir
}

/// Entropy that gives one byte again and again: a known seed.
struct Fixed(u8);

impl Entropy for Fixed {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), NoRandom> {
        buf.fill(self.0);
        Ok(())
    }
}

#[test]
fn irohs_key_is_the_seed_of_the_key_file() {
    let path = scratch("seed").join("node.key");
    let key = keys::load_in(&[], &Place::File(path.clone()), &mut Fixed(0x2a)).unwrap();
    assert_eq!(key.secret.to_bytes(), [0x2a; 32]);
    assert_eq!(key.public().as_bytes(), &key.identity.public().0, "one key, as iroh and as the channel see it");
    // iroh parses the file as its secret key: the same key.
    let text = std::fs::read_to_string(&path).unwrap();
    let parsed = SecretKey::from_str(text.trim()).unwrap();
    assert_eq!(parsed.public(), key.public());
}

#[test]
fn a_key_is_shown_by_its_fingerprint_which_the_allowlist_takes() {
    let path = scratch("shown").join("client.key");
    let key = keys::load_in(&[], &Place::File(path), &mut Fixed(0x5c)).unwrap();
    let shown = keys::fingerprint(&key.public());
    assert_eq!(shown, key.identity.public().fingerprint());
    assert!(shown.starts_with("SHA256:"), "{shown}");
    assert!(!shown.contains(&"5c".repeat(32)) && !format!("{key:?}").contains(&"5c".repeat(32)));
    let list = Allowlist::parse(&format!("{shown} the client\n"));
    assert!(list.admits(&key.public()), "the fingerprint that a node shows is the line to add");
    assert!(Allowlist::parse(&format!("{}\n", key.public())).admits(&key.public()), "iroh's hex too");
}

/// The iroh road reads the allowlist with pod-identity's one parser
/// (PodUtils/podssh#362, ask 1): OpenSSH's public line lets the key in, its
/// routes count, and a key whose lines all name routes does not reach the
/// target with no route.
#[test]
fn the_line_of_keygen_y_lets_the_iroh_key_in() {
    let path = scratch("openssh").join("client.key");
    let key = keys::load_in(&[], &Place::File(path), &mut Fixed(0x5d)).unwrap();
    let line = key.identity.public().openssh();
    assert!(line.starts_with("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5"), "{line}");
    let list = Allowlist::parse(&format!("{line} routes=web me@lab\n"));
    assert!(list.admits(&key.public()) && list.grants(&key.public(), "web") && !list.grants(&key.public(), "db"));
    assert!(!list.reaches_target(&key.public()), "routes alone");
    let bare = Allowlist::parse(&format!("{line} routes=web\n{line} me@lab\n"));
    assert!(bare.reaches_target(&key.public()), "a line with no routes= word");
    let bad = Allowlist::parse("ssh-ed25519\n");
    assert!(bad.bad_lines_said().is_some_and(|said| said.contains("ssh-ed25519 BASE64")));
}

#[test]
fn a_key_file_of_t163_stays_the_iroh_key() {
    let dir = scratch("old");
    let old = dir.join(keys::old_node_file("lab"));
    let before = keys::load_in(&[], &Place::File(old), &mut OsEntropy).unwrap();
    let found = keys::load_in(std::slice::from_ref(&dir), &keys::node_place("lab"), &mut OsEntropy).unwrap();
    assert!(!found.made);
    assert_eq!(found.public(), before.public(), "the node's ticket stays");
}
