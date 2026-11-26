//! The forms of a key in an allowlist and in an argument (PodUtils/podssh#362,
//! ask 1): OpenSSH's public line beside the fingerprint and iroh's forms, the
//! words of a bad line, and the target with no route.

mod cleanup;

use std::path::PathBuf;

use pod_identity::allow::{self, Allowlist};
use pod_identity::{Identity, KEY_FORMS, KeyName, PublicKey};

/// A throwaway Ed25519 key that OpenSSH 10.3p1 made, its `ssh-keygen -lf`,
/// and its public line, which an independent encoder wrote.
const OPENSSH_KEY: &str = "7769b4030e693169deb26e0a5e32a84d5108b6517e0dd05db0d13ca506e854af";
const OPENSSH_FINGERPRINT: &str = "SHA256:4GCq9Leo8YikY6DuOUiMStys+sW6A5GLCsemjyLisjY";
const OPENSSH_LINE: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHdptAMOaTFp3rJuCl4yqE1RCLZRfg3QXbDRPKUG6FSv";
/// The same bytes, with the type `ssh-rsa` in the BASE64.
const RSA_BLOB: &str = "AAAAB3NzaC1yc2EAAAAgd2m0Aw5pMWnesm4KXjKoTVEItlF+DdBdsNE8pQboVK8=";

fn key() -> PublicKey {
    PublicKey::parse(OPENSSH_KEY).unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("podssh-allow-forms-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    pod_core::files::private_dir(&dir).unwrap();
    cleanup::at_test_end(&dir);
    dir
}

/// Write a file that only its owner can change, where modes exist.
fn write_own(path: &std::path::Path, text: &str) {
    std::fs::write(path, text).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
}

/// The public line is OpenSSH's: the same text as an independent encoder, and
/// the fingerprint that `ssh-keygen -lf` gave for it.
#[test]
fn the_public_line_is_openssh_s() {
    assert_eq!(key().openssh(), OPENSSH_LINE);
    assert_eq!(key().fingerprint(), OPENSSH_FINGERPRINT);
    assert_eq!(PublicKey::from_openssh(OPENSSH_LINE), Some(key()));
    assert_eq!(PublicKey::from_openssh(&format!("{OPENSSH_LINE} me@lab two words")), Some(key()));
    let seeded = Identity::from_seed(&[0x35; 32]).public();
    assert_eq!(PublicKey::from_openssh(&seeded.openssh()), Some(seeded));
    let truncated = &OPENSSH_LINE[..OPENSSH_LINE.len() - 4];
    for bad in ["", "ssh-ed25519", truncated, &format!("ssh-rsa {RSA_BLOB}"), &format!("ssh-ed25519 {RSA_BLOB}")] {
        assert_eq!(PublicKey::from_openssh(bad), None, "{bad:?}");
    }
}

/// Each form of a key lets the key in, and the routes after it count; a line
/// that is none is named by its own number. One table, one rule.
#[test]
fn each_form_of_a_key_is_read_by_one_grammar() {
    let hex = key().hex();
    // (the line, whether it is a key, whether it gives the route web)
    let rows: Vec<(String, bool, bool)> = vec![
        (OPENSSH_LINE.into(), true, false),
        (format!("{OPENSSH_LINE} me@lab"), true, false),
        (format!("{OPENSSH_LINE} routes=web me@lab"), true, true),
        (format!("  {OPENSSH_LINE}\t routes=*"), true, true),
        (hex.clone(), true, false),
        (format!("{OPENSSH_FINGERPRINT} routes=web"), true, true),
        (key().display_fingerprint(), true, false),
        ("ssh-ed25519".into(), false, false),
        ("ssh-ed25519 !!!notbase64".into(), false, false),
        (format!("ssh-rsa {RSA_BLOB}"), false, false),
        (format!("ssh-ed25519 {RSA_BLOB}"), false, false),
        ("not a key".into(), false, false),
    ];
    for (line, is_key, web) in rows {
        let list = Allowlist::parse(&format!("# a comment\n{line}\n"));
        assert_eq!(list.admits(&key()), is_key, "{line:?}");
        assert_eq!(list.grants(&key(), "web"), web, "{line:?}");
        let bad: &[usize] = if is_key { &[] } else { &[2] };
        assert_eq!(list.bad_lines(), bad, "{line:?}");
        assert_eq!(list.bad_lines_said().is_some(), !is_key, "{line:?}");
    }
    // The line and the key's hex are one key: their routes add up.
    let both = Allowlist::parse(&format!("{OPENSSH_LINE} routes=web\n{hex} routes=db\n"));
    assert_eq!(both.len(), 1);
    assert!(both.grants(&key(), "web") && both.grants(&key(), "db"));
}

/// The refusal of a bad line names its number and each form of a line.
#[test]
fn a_bad_line_is_refused_with_the_forms_of_a_key() {
    let list = Allowlist::parse(&format!("{OPENSSH_LINE}\nlaptop\nssh-ed25519\n"));
    let said = list.bad_lines_said().unwrap();
    assert!(said.starts_with("lines 2, 3 are not keys: a line holds "), "{said}");
    assert!(said.contains(KEY_FORMS) && said.ends_with("then routes=A,B and a comment, both optional"), "{said}");
    for form in ["ssh-ed25519 BASE64", "64 hex digits", "base32", "SHA256:", "display fingerprint"] {
        assert!(KEY_FORMS.contains(form), "{form}");
    }
    assert!(allow::bad_lines_said(&[4]).starts_with("line 4 is not a key: "));
    // The node's log of a key that is not let in names the bad lines too:
    // one of them can be the line meant for the key.
    let path = scratch("bad").join("allow");
    write_own(&path, "ssh-ed25519 AAAA me@lab\n");
    let why = allow::admits(&path, &key()).unwrap_err();
    assert!(why.contains("is not in") && why.contains("line 1 is not a key: a line holds"), "{why}");
    let why = allow::grants(&path, &key(), "web").unwrap_err();
    assert!(why.contains("line 1 is not a key"), "{why}");
}

/// The target with no route: a line of the key with no routes= word or with
/// routes=*; a key whose lines all say routes=A,B reaches those routes alone.
/// The merge of a key's lines keeps the mark of a line with no routes.
#[test]
fn a_line_of_routes_alone_does_not_reach_the_target_with_no_route() {
    let hex = key().hex();
    let other = Identity::from_seed(&[0x34; 32]).public().hex();
    // (the list, whether the key reaches the target with no route)
    let rows: Vec<(String, bool)> = vec![
        (format!("{hex}\n"), true),
        (format!("{hex} a comment\n"), true),
        (format!("{hex} routes=*\n"), true),
        (format!("{hex} routes=web\n"), false),
        (format!("{hex} routes=web,db\n# {hex}\n"), false),
        (format!("{hex} routes=web\n{hex}\n"), true),
        (format!("{hex}\n{hex} routes=web\n"), true),
        (format!("{OPENSSH_LINE} routes=web\n{hex}\n"), true),
        (format!("{OPENSSH_LINE} routes=web me@lab\n"), false),
        (format!("{OPENSSH_FINGERPRINT} routes=web\n{OPENSSH_LINE} me@lab\n"), true),
        (format!("{other}\n"), false),
    ];
    for (text, reaches) in rows {
        let list = Allowlist::parse(&text);
        assert_eq!(list.reaches_target(&key()), reaches, "{text:?}");
    }
    let path = scratch("target").join("allow");
    write_own(&path, &format!("{hex}\n"));
    assert_eq!(allow::reaches_target(&path, &key()), Ok(()));
    write_own(&path, &format!("{OPENSSH_LINE} routes=web\n"));
    let why = allow::reaches_target(&path, &key()).unwrap_err();
    assert!(why.contains("routes alone, not the target with no route"), "{why}");
    write_own(&path, &format!("{other}\n"));
    let why = allow::reaches_target(&path, &key()).unwrap_err();
    assert!(why.contains("is not in"), "{why}");
}

/// An argument takes OpenSSH's line with a comment, or one word; a word with
/// a stray word after it is none.
#[test]
fn an_argument_is_openssh_s_line_or_one_word() {
    let rows: Vec<(String, bool)> = vec![
        (OPENSSH_LINE.into(), true),
        (format!("{OPENSSH_LINE} me@lab two words"), true),
        (format!("  {OPENSSH_LINE}\n"), true),
        (key().hex(), true),
        (OPENSSH_FINGERPRINT.into(), true),
        (format!("{} me@lab", key().hex()), false),
        (String::new(), false),
        ("ssh-ed25519".into(), false),
        (format!("ssh-rsa {RSA_BLOB}"), false),
    ];
    for (text, is_key) in rows {
        let name = KeyName::parse(&text);
        assert_eq!(name.is_some(), is_key, "{text:?}");
        if let Some(name) = name {
            assert!(name.matches(&key()), "{text:?}");
        }
    }
}
