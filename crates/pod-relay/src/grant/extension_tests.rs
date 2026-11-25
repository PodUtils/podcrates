use super::Extension;
use crate::grant::{Cert, Mint};
use pod_identity::Identity;

/// The certificate of [`gateway`], as a second writer (Python, with
/// `scripts/wire_ed25519.py`) made it from PROTOCOL.certkeys: the
/// extensions in the order of their names, each with empty data.
const GATEWAY_HEX: &str = concat!(
    "000000207373682d656432353531392d636572742d763031406f70656e7373682e636f6d0000002003030303030303030303030303",
    "0303030303030303030303030303030303030300000020d04ab232742bb4ab3a1368bd4615e4e6d0224ab71a016baf8520a332c977",
    "873700000000000000090000000100000007676174657761790000001000000005616c69636500000003676974000000006955b900",
    "000000006955c710000000240000000e736f757263652d616464726573730000000e0000000a31302e302e302e302f380000009800",
    "00000e6761746577617940706f6473736800000000000000157065726d69742d5831312d666f7277617264696e6700000000000000",
    "177065726d69742d6167656e742d666f7277617264696e6700000000000000167065726d69742d706f72742d666f7277617264696e",
    "67000000000000000a7065726d69742d707479000000000000000e7065726d69742d757365722d7263000000000000000000000033",
    "0000000b7373682d6564323535313900000020a09aa5f47a6759802ff955f8dc2d2a14a5c99d23be97f864127ff9383455a4f00000",
    "00530000000b7373682d65643235353139000000405e732fb98fc32e0b0fe2076d123389c5cb6ecb290a95f62192eae986a134b972",
    "301152932f7d2afef200636b8dfbd86ecaa220ce77d97f6335512b205427530a",
);

/// The critical option `source-address`, whose data is a string of the
/// list, as OpenSSH writes an option with a value.
const SOURCE: &[u8] = b"\x00\x00\x00\x0a10.0.0.0/8";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A certificate for a stock sshd: the extensions given in no order.
fn gateway(extensions: &[Extension]) -> Result<Cert, String> {
    let principals = vec!["alice".to_string(), "git".to_string()];
    Mint {
        nonce: [3; 32],
        key: Identity::from_seed(&[0x11; 32]).public(),
        serial: 9,
        key_id: "gateway",
        principals: &principals,
        valid_after: 1_767_225_600,
        valid_before: 1_767_229_200,
        critical: &[("source-address", SOURCE)],
    }
    .sign_with(&Identity::from_seed(&[0x22; 32]), extensions)
}

fn unordered() -> Vec<Extension<'static>> {
    let mut list: Vec<Extension> = Extension::OPENSSH_DEFAULT.iter().rev().copied().collect();
    list.insert(2, Extension::Other("gateway@podssh", b""));
    list
}

#[test]
fn the_extensions_sign_to_the_bytes_of_a_second_writer() {
    let cert = gateway(&unordered()).unwrap();
    assert_eq!(hex(&cert.blob()), GATEWAY_HEX, "the bytes that the order of the names gives");
    let names: Vec<&str> = cert.extensions.iter().map(|(n, _)| n.as_str()).collect();
    let expected = [
        "gateway@podssh",
        "permit-X11-forwarding",
        "permit-agent-forwarding",
        "permit-port-forwarding",
        "permit-pty",
        "permit-user-rc",
    ];
    assert_eq!(names, expected);
    assert!(cert.extensions.iter().all(|(_, data)| data.is_empty()), "each flag has empty data");
}

#[test]
fn a_certificate_with_extensions_reads_back_whole_and_verifies() {
    let cert = gateway(&unordered()).unwrap();
    let read = Cert::from_text(&cert.to_line("gateway")).unwrap();
    assert_eq!(read, cert);
    assert!(read.signed_by_issuer());
    assert_eq!(read.critical, [("source-address".to_string(), SOURCE.to_vec())]);
}

#[test]
fn no_extension_signs_the_bytes_of_a_grant() {
    let issuer = Identity::from_seed(&[0x22; 32]);
    let principals = vec!["node:x".to_string()];
    let mint = Mint {
        nonce: [4; 32],
        key: Identity::from_seed(&[0x11; 32]).public(),
        serial: 1,
        key_id: "t",
        principals: &principals,
        valid_after: 0,
        valid_before: 10,
        critical: &[(crate::grant::ONCE, b"")],
    };
    let with = mint.sign_with(&issuer, &[]).unwrap();
    assert_eq!(with.blob(), mint.sign(&issuer).blob());
    assert!(with.extensions.is_empty());
}

#[test]
fn a_name_twice_or_a_known_name_as_other_is_refused() {
    let twice = gateway(&[Extension::PermitPty, Extension::PermitPty]).unwrap_err();
    assert!(twice.contains("extension permit-pty twice"), "{twice}");
    let other = gateway(&[Extension::Other("custom@podssh", b""), Extension::Other("custom@podssh", b"x")]);
    assert!(other.unwrap_err().contains("custom@podssh twice"));
    let known = gateway(&[Extension::Other("permit-pty", b"x")]).unwrap_err();
    assert!(known.contains("give its own variant"), "{known}");
    let principals: Vec<String> = Vec::new();
    let critical = Mint {
        nonce: [0; 32],
        key: Identity::from_seed(&[0x11; 32]).public(),
        serial: 0,
        key_id: "",
        principals: &principals,
        valid_after: 0,
        valid_before: 1,
        critical: &[("force-command", b""), ("force-command", b"")],
    }
    .sign_with(&Identity::from_seed(&[0x22; 32]), &[])
    .unwrap_err();
    assert!(critical.contains("critical option force-command twice"), "{critical}");
}

#[test]
fn each_name_of_openssh_reads_to_its_variant() {
    for (variant, name) in Extension::KNOWN {
        assert_eq!(variant.name(), name);
        assert_eq!(Extension::known(name), Some(variant));
        assert!(variant.data().is_empty());
    }
    assert_eq!(Extension::known("permit-PTY"), None, "a name is exact");
    let other = Extension::Other("a@b", b"x");
    assert_eq!((other.name(), other.data()), ("a@b", &b"x"[..]));
}

/// OpenSSH's own reader lists the extensions: `ssh-keygen -L`. The test
/// skips when this host has no `ssh-keygen`.
#[test]
fn ssh_keygen_lists_the_extensions() {
    if std::process::Command::new("ssh-keygen").arg("-?").output().is_err() {
        eprintln!("skipped: no ssh-keygen on this host");
        return;
    }
    let cert = gateway(&Extension::OPENSSH_DEFAULT).unwrap();
    let dir = std::env::temp_dir().join(format!("pod-relay-ext-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("id-cert.pub");
    std::fs::write(&path, cert.to_line("gateway")).unwrap();
    let out = std::process::Command::new("ssh-keygen").arg("-L").arg("-f").arg(&path).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text} {}", String::from_utf8_lossy(&out.stderr));
    let listed = text.split("Extensions:").nth(1).unwrap_or_default();
    let words: Vec<&str> = listed.split_whitespace().collect();
    let names: Vec<&str> = Extension::OPENSSH_DEFAULT.iter().map(Extension::name).collect();
    assert_eq!(words, names, "{text}");
    assert!(text.contains("source-address 10.0.0.0/8"), "{text}");
}
