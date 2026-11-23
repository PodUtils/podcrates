//! The rules of a secret in a unit. Each made-up token is built from parts
//! at run time, so that no scanner of secrets reads one in this file.

use std::path::Path;

use super::*;
use crate::Env;

/// A made-up token: `prefix` and `n` characters of a random spread.
fn token(prefix: &str, n: usize) -> String {
    let alphabet = b"q7Zk2LmP9xR4tVw8Yb3NcH6jD5sFgA1e";
    let tail: String = (0..n).map(|i| alphabet[(i * 7 + 3) % alphabet.len()] as char).collect();
    format!("{prefix}{tail}")
}

fn unit() -> Unit {
    Unit::new("podrunner", Path::new("/usr/local/bin/podrunner"), "podrunner print-unit").args(["serve"])
}

#[test]
fn each_known_token_is_refused_by_its_rule() {
    let cases = [
        (token(&["gh", "p_"].concat(), 36), "a GitHub token"),
        (token(&["github", "_pat_"].concat(), 60), "a GitHub token"),
        (token(&["gl", "pat-"].concat(), 20), "a GitLab token"),
        (token(&["AK", "IA"].concat(), 16).to_ascii_uppercase(), "an AWS access key"),
        (token(&["ey", "J"].concat(), 120), "a token of base64 JSON (a JWT, a tunnel token)"),
        (format!("-----BEGIN OPENSSH {}-----", ["PRIVATE", " KEY"].concat()), "a private key"),
        (format!("https://user:{}@proxy.example:8080", "pw"), "a password in a URL"),
        (token("", 40), "a long random word"),
    ];
    for (word, rule) in cases {
        assert_eq!(looks_secret(&word), Some(rule), "{rule}");
        // In a flag's value, and after a key.
        assert_eq!(looks_secret(&format!("--x={word}")), Some(rule), "{rule} after =");
    }
}

#[test]
fn a_word_of_a_unit_that_is_no_secret_passes() {
    for word in [
        "/usr/local/bin/podrunner",
        "--config=/etc/podrunner/podrunner.toml",
        "https://user@relay.example/path",
        "https://relay.example/a:b",
        "runner@sha256:0f343b0931126a20f133d67c2b018a3b0f343b0931126a20f133d67c2b018a3b",
        "PODCRED_GITHUB_TOKEN_0001",
        "550e8400-e29b-41d4-a716-446655440000",
        "a-very-long-name-of-a-service-with-many-words-2026",
        "task-of-the-skeleton-crew",
        "C:\\Program Files\\PodUtils\\podrunner.exe",
    ] {
        assert_eq!(looks_secret(word), None, "{word}");
    }
}

#[test]
fn a_key_names_a_secret_but_not_its_place() {
    for key in ["GITHUB_TOKEN", "TUNNEL_TOKEN", "api-key", "DB_PASSWORD", "CLIENT_SECRET", "GH_PAT", "AWS_ACCESS_KEY"] {
        assert!(names_secret(key), "{key}");
    }
    for key in ["GITHUB_TOKEN_FILE", "token-file", "CREDENTIALS_DIRECTORY", "HOME", "TOKEN_URL", "PATH", "AUTHOR"] {
        assert!(!names_secret(key), "{key}");
    }
}

/// The plant `unit-secret-argument-not-scanned` makes the scan skip the
/// arguments; this test then renders a unit with a token in it.
#[test]
fn a_token_in_an_argument_is_refused_and_not_named() {
    let secret = token(&["gh", "p_"].concat(), 36);
    let unit = unit().arg("--relay").arg(&secret);
    let fault = unit.check().expect_err("a token in an argument");
    assert_eq!(fault.class, pod_core::Class::Usage);
    assert!(fault.text().contains("argument 3 looks like a secret (a GitHub token)"), "{}", fault.text());
    assert!(!fault.text().contains(&secret), "the refusal names the token");
    assert!(crate::systemd::render(&unit).is_err());
    assert!(crate::launchd::render(&unit).is_err());
    assert!(crate::windows::task_xml(&unit, "HOST\\me").is_err());
}

#[test]
fn a_secret_in_a_variable_a_flag_or_a_path_is_refused() {
    let secret = token(&["gh", "p_"].concat(), 36);
    let refused = [
        unit().env("TUNNEL_TOKEN", "short"),
        unit().env("RELAY", &secret),
        unit().args(["--token", "abc"]),
        unit().arg("--password=abc"),
        unit().arg(&format!("--proxy=http://u:{}@proxy.example", "pw")),
        unit().secret_file("gh", &Path::new("/etc/creds").join(&secret), "GH_FILE"),
    ];
    for unit in refused {
        assert!(unit.check().is_err(), "{unit:?}");
    }
}

#[test]
fn a_reference_a_placeholder_and_a_flag_with_no_value_pass() {
    let unit = unit()
        .env("GITHUB_TOKEN_FILE", "/run/secrets/github")
        .env("TUNNEL_TOKEN", "")
        .args(["--token-file", "/etc/podrunner/token", "--password", "--next"])
        .secret_file("github", Path::new("/etc/podrunner/github.token"), "GITHUB_TOKEN_FILE")
        .placeholder("GITHUB_TOKEN", "PODCRED_GITHUB_TOKEN_0001")
        .expect("a placeholder");
    unit.check().expect("no secret");
}

#[test]
fn a_placeholder_has_the_form_of_the_broker_and_no_form_of_a_token() {
    assert!(Env::placeholder("GITHUB_TOKEN", "short").is_err());
    assert!(Env::placeholder("GITHUB_TOKEN", "PODCRED GITHUB 0001").is_err());
    // A real token given as a placeholder is still a token.
    let real = unit().placeholder("GITHUB_TOKEN", &token(&["gh", "p_"].concat(), 36)).expect("its form");
    assert!(real.check().is_err());
}

/// A path splits at each `/` first, also after a `.`: no run crosses a
/// component. A directory of `mktemp -d` (`/tmp/tmp.XXXXXXXXXX`) has a random
/// part of 10 characters, far below the length of a random word.
#[test]
fn a_path_under_a_directory_of_mktemp_is_no_secret() {
    for word in [
        "/tmp/tmp.Ab3dEf9kQ2/serve-config/podrunner.toml",
        "--config=/tmp/tmp.Ab3dEf9kQ2/serve-config/podrunner.toml",
        "/tmp/tmp.Ab3dEf9kQ2",
        "tmp.Ab3dEf9kQ2/serve-config/podrunner.toml",
        "/home/u/.cache/tmp.Xy7Qm2Lp9Z/Ab3dEf9kQ2-serve/state.d/podrunner",
    ] {
        assert_eq!(looks_secret(word), None, "{word}");
    }
    let unit = unit().args(["--config", "/tmp/tmp.Ab3dEf9kQ2/serve-config/podrunner.toml"]);
    unit.check().expect("a path under a directory of mktemp");
}

/// A token of base64 holds `/` and no `.`: it stays one run. A component of
/// a path, or a value after `=`, of the length of a secret is still refused.
#[test]
fn a_secret_in_a_component_or_a_token_with_a_slash_is_still_refused() {
    let random = token("", 40);
    let base64 = format!("{}/{}+{}", &random[..12], &random[12..26], &random[26..]);
    for word in [
        base64.clone(),
        format!("--x={base64}"),
        format!("/etc/creds/{random}"),
        format!("/tmp/tmp.Ab3dEf9kQ2/{random}.toml"),
        format!("/tmp/tmp.Ab3dEf9kQ2/x={random}"),
    ] {
        assert_eq!(looks_secret(&word), Some("a long random word"), "{word}");
    }
    let github = token(&["gh", "p_"].concat(), 36);
    assert_eq!(looks_secret(&format!("/run/tmp.Ab3dEf9kQ2/{github}")), Some("a GitHub token"));
}

/// The bytes of a made-up key: a fixed spread, not a real key.
fn spread(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (x >> 16) as u8
        })
        .collect()
}

/// The blob of an OpenSSH public key: each part with its length (RFC 4253).
fn blob(parts: &[&[u8]]) -> String {
    use base64::Engine as _;
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u32).to_be_bytes());
        bytes.extend_from_slice(part);
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn public_keys(seed: u32) -> Vec<String> {
    let mut rsa = spread(257, seed);
    rsa[0] = 0;
    let mut point = spread(65, seed + 1);
    point[0] = 4;
    let ed = spread(32, seed + 2);
    vec![
        format!("ssh-ed25519 {} user@host.example", blob(&[b"ssh-ed25519", &ed])),
        format!("ssh-rsa {}", blob(&[b"ssh-rsa", &[1, 0, 1], &rsa])),
        format!("ecdsa-sha2-nistp256 {} ci", blob(&[b"ecdsa-sha2-nistp256", b"nistp256", &point])),
        format!("sk-ssh-ed25519@openssh.com {}", blob(&[b"sk-ssh-ed25519@openssh.com", &ed, b"ssh:"])),
    ]
}

/// A public key of OpenSSH and a fingerprint are no secret (PodUtils/podssh#342).
#[test]
fn a_public_key_and_a_fingerprint_of_openssh_pass() {
    use base64::Engine as _;
    for seed in 0..50 {
        let digest = base64::engine::general_purpose::STANDARD_NO_PAD.encode(spread(32, seed + 9));
        let md5: Vec<String> = spread(16, seed + 7).iter().map(|b| format!("{b:02x}")).collect();
        let mut words = public_keys(seed);
        words.push(format!("SHA256:{digest}"));
        words.push(format!("--owner=SHA256:{digest}"));
        words.push(format!("MD5:{}", md5.join(":")));
        for word in words {
            assert_eq!(looks_secret(&word), None, "{word}");
        }
    }
    let key = public_keys(3).remove(0);
    unit().args(["--authorized-key", &key]).check().expect("a public key");
}

/// A blob with bytes after its parts, a fingerprint of the wrong length and
/// a token are still refused.
#[test]
fn a_form_near_a_public_key_is_still_refused() {
    let ed = spread(32, 5);
    let long = format!("SHA256:{}", token("", 60));
    for word in [
        format!("ssh-ed25519 {}", blob(&[b"ssh-ed25519", &ed, &spread(40, 6)])),
        format!("ssh-ed25519 {}{}", &blob(&[b"ssh-ed25519", &ed])[..60], token("", 40)),
        format!("ssh-ed25519 {}", blob(&[b"openssh-key-v1", &spread(64, 8)])),
        long,
    ] {
        assert!(looks_secret(&word).is_some(), "{word}");
    }
}

/// A caller names the words that are public; the scan skips them, and a
/// secret beside them is still refused.
#[test]
fn a_word_that_the_caller_names_public_passes() {
    let public = token("", 40);
    unit().arg(&format!("--peer={public}")).public(&public).check().expect("a public word");
    assert!(unit().arg(&format!("--peer={public}")).check().is_err());
    assert!(unit().arg(&format!("--peer={public}")).public("another").check().is_err());
    let secret = token(&["gh", "p_"].concat(), 36);
    assert!(unit().args([&public, &secret]).public(&public).check().is_err());
}
