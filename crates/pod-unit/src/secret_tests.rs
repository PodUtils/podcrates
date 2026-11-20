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
