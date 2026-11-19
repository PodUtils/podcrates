//! The credentials of a policy and the load of their secrets (T-100): the
//! checks of a placeholder, of a secret and of its value; a credential
//! needs the TLS end; the CA of a job, its bundle and its variables.

mod common;
mod tls_common;

use std::time::Duration;

use common::*;
use pod_broker::{Load, Policy, Secrets, Token};
use pod_core::Class;
use tls_common::*;

fn try_load(
    broker: &pod_broker::Broker,
    policy: &str,
    secrets: Secrets,
) -> Result<pod_broker::JobInfo, pod_core::Failure> {
    let policy = Policy::from_json(policy)?;
    let mut load = Load::new("job1", Token::new(TOKEN), Duration::from_secs(600), policy);
    load.secrets = secrets;
    broker.load(load)
}

fn credential(placeholder: &str, secret: &str, hosts: &str) -> String {
    format!(
        r#"{{"schema":1,"mode":"open","credentials":[{{"placeholder":"{placeholder}","secret":"{secret}","hosts":{hosts}}}]}}"#
    )
}

#[test]
fn a_credential_is_checked_at_the_load() {
    let (broker, _) = tls_broker(TlsFar::new(echo()));
    let secret = || Secrets::new().with("github", SECRET);
    for (policy, secrets, words) in [
        (credential("short", "github", r#"["a.example"]"#), secret(), "placeholder"),
        (credential("PODCRED_WITH SPACE_0001", "github", r#"["a.example"]"#), secret(), "placeholder"),
        (credential(PH, "bad name", r#"["a.example"]"#), secret(), "secret name"),
        (credential(PH, "github", "[]"), secret(), "one host"),
        (credential(PH, "github", r#"["a..example"]"#), secret(), "credential 0"),
        (credential(PH, "github", r#"["a.example"]"#), Secrets::new(), "does not give"),
        (credential(PH, "github", r#"["a.example"]"#), secret().with("other", SECRET), "no credential names"),
        (credential(PH, "github", r#"["a.example"]"#), Secrets::new().with("github", "short"), "8 to 4096"),
        (credential(PH, "github", r#"["a.example"]"#), Secrets::new().with("github", "has a space"), "8 to 4096"),
        (credential(PH, "github", r#"["a.example"]"#), Secrets::new().with("github", &PH[2..]), "hold each other"),
        (OPEN.to_string(), secret(), "no credential of the policy"),
    ] {
        let failed = try_load(&broker, &policy, secrets).expect_err(&policy);
        assert_eq!(failed.class, Class::Usage, "{policy}");
        assert!(failed.text().contains(words), "{policy}: {}", failed.text());
        assert!(!failed.text().contains(SECRET), "{}", failed.text());
    }
    let two = format!(
        r#"{{"schema":1,"mode":"open","credentials":[{{"placeholder":"{PH}","secret":"a","hosts":["x.example"]}},{{"placeholder":"{PH}X","secret":"b","hosts":["x.example"]}}]}}"#
    );
    assert!(try_load(&broker, &two, Secrets::new()).unwrap_err().text().contains("hold each other"));
    let unknown = r#"{"schema":1,"mode":"open","credentials":[{"placeholder":"PODCRED_GITHUB_TOKEN_0001","secret":"a","hosts":["x"],"in":["cookie"]}]}"#;
    assert!(Policy::from_json(unknown).is_err());
}

#[test]
fn a_credential_needs_the_tls_end_and_a_policy_with_none_needs_no_ca() {
    let (plain, _) = broker(Far::new(Kind::Echo), Answers::public());
    let failed = try_load(&plain, &policy("", ""), Secrets::new().with("github", SECRET)).unwrap_err();
    assert!(failed.text().contains("TLS end"), "{}", failed.text());
    let (broker, audit) = tls_broker(TlsFar::new(echo()));
    assert_eq!(try_load(&broker, OPEN, Secrets::new()).unwrap().ca_pem, None);
    let info = try_load(&broker, &policy("", ""), Secrets::new().with("github", SECRET)).unwrap();
    let pem = info.ca_pem.expect("a CA");
    assert!(pem.starts_with("-----BEGIN CERTIFICATE-----\n"), "{pem}");
    assert_eq!(broker.ca_pem("job1").as_deref(), Some(pem.as_str()));
    let line = audit.lines().into_iter().rfind(|l| l.event == "broker.job.load").unwrap();
    assert_eq!((line.fields["credentials"].as_str(), line.fields["ca"].as_bool()), (Some("github"), Some(true)));
    // A rule with a path needs the TLS end too: the job gets a CA.
    let paths = r#"{"schema":1,"mode":"allow","allow":[{"host":"a.example","path":"/v1"}]}"#;
    assert!(try_load(&broker, paths, Secrets::new()).unwrap().ca_pem.is_some());
}

#[test]
fn the_control_takes_the_secrets_and_answers_the_ca_with_no_value() {
    let (broker, _) = tls_broker(TlsFar::new(echo()));
    let line = format!(
        r#"{{"op":"load","job":"job1","token":"{TOKEN}","ttl_s":60,"policy":{},"secrets":{{"github":"{SECRET}"}}}}"#,
        policy("", "")
    );
    let answer = broker.control_line(&line);
    assert_eq!(answer["ok"], true, "{answer}");
    assert!(answer["ca_pem"].as_str().is_some_and(|p| p.contains("BEGIN CERTIFICATE")), "{answer}");
    let list = broker.control_line(r#"{"op":"list"}"#).to_string();
    for text in [answer.to_string(), list] {
        assert!(!text.contains(SECRET) && !text.contains("github\":\""), "{text}");
    }
}

#[test]
fn the_bundle_holds_the_ca_of_the_job_and_the_roots_and_each_variable_names_it() {
    let ca = pod_broker::ca::JobCa::new("job1", Duration::from_secs(60)).unwrap();
    let roots = pod_broker::ca::JobCa::new("roots", Duration::from_secs(60)).unwrap();
    let bundle = pod_broker::ca::bundle(ca.pem(), roots.pem().as_bytes()).unwrap();
    assert_eq!(bundle, format!("{}{}", ca.pem(), roots.pem()));
    assert_eq!(pod_tls::bundle::pem_certificates(bundle.as_bytes()).unwrap().len(), 2);
    assert!(pod_broker::ca::bundle(ca.pem(), b"no PEM here").is_err());
    let dir = std::env::temp_dir().join(format!("pod-broker-bundle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ca.pem");
    pod_broker::ca::write_bundle(&path, ca.pem(), roots.pem().as_bytes()).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), bundle);
    let vars: Vec<&str> = pod_broker::ca::env(&path)
        .iter()
        .map(|(v, p)| {
            assert_eq!(p, &path);
            *v
        })
        .collect();
    for var in ["SSL_CERT_FILE", "GIT_SSL_CAINFO", "NODE_EXTRA_CA_CERTS", "REQUESTS_CA_BUNDLE"] {
        assert!(vars.contains(&var), "{vars:?}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The leaf of a host verifies with OpenSSL in its strict mode (the mode of
/// Python 3.13) against the CA of the job alone, for a TLS server: the trust
/// of curl, git and Python's requests. A host with no `openssl` skips it.
#[test]
fn a_leaf_verifies_with_openssl_in_its_strict_mode() {
    if std::process::Command::new("openssl").arg("version").output().is_err() {
        eprintln!("no openssl here: the check of the leaf is skipped");
        return;
    }
    let ca = pod_broker::ca::JobCa::new("job1", Duration::from_secs(600)).unwrap();
    let leaf = ca.leaf(&pod_broker::target::Host::Name("api.example.com".into())).unwrap();
    let dir = std::env::temp_dir().join(format!("pod-broker-leaf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ca.pem"), ca.pem()).unwrap();
    std::fs::write(dir.join("leaf.pem"), pod_acme::certs::pem("CERTIFICATE", &leaf.cert[0])).unwrap();
    let verify = |extra: &[&str]| {
        let mut command = std::process::Command::new("openssl");
        command.args(["verify", "-x509_strict", "-purpose", "sslserver", "-CAfile"]).arg(dir.join("ca.pem"));
        command.args(extra).arg(dir.join("leaf.pem"));
        command.output().unwrap()
    };
    let out = verify(&["-verify_hostname", "api.example.com"]);
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(!verify(&["-verify_hostname", "other.example.com"]).status.success());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_debug_forms_show_no_value() {
    let secrets = Secrets::new().with("github", SECRET);
    assert_eq!(format!("{secrets:?}"), r#"["github"]"#);
    let ca = pod_broker::ca::JobCa::new("job1", Duration::from_secs(60)).unwrap();
    assert!(!format!("{ca:?}").contains("PRIVATE"));
}
