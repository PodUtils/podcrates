use std::io::{Error, ErrorKind};

use pod_tls::Words;
use rustls::CertificateError;

use super::*;
use crate::lookup::fold;
use crate::order::{Known, LEAST, LINE, most, resolve_in};

const V6: [&str; 4] = ["2606:4700:4700::1111", "2606:4700:4700::1001", "2001:4860:4860::8888", "2001:4860:4860::8844"];
const V4: [&str; 4] = ["1.1.1.1", "1.0.0.1", "8.8.8.8", "8.8.4.4"];

/// The failure of each resolver of `ips`, as `over` keeps it.
fn each(ips: &[&str], why: impl Fn(&str) -> String) -> Vec<(String, String)> {
    ips.iter().map(|ip| (ip.to_string(), why(ip))).collect()
}

fn no_family(ip: &str) -> String {
    format!("could not connect to [{ip}]:443: Address family not supported by protocol (os error 97)")
}

fn unreachable(ip: &str) -> String {
    format!("could not connect to {ip}:443: Network is unreachable (os error 101)")
}

/// The words of a middlebox that re-signs TLS with its own CA, as a
/// resolver's failure gives them.
fn unknown_issuer(ip: &str) -> String {
    let refused =
        Error::new(ErrorKind::InvalidData, rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer));
    format!("{ip}: {}", pod_proxy::Missed::tls(&refused, &Words::new("podnet")).why)
}

fn other_name(ip: &str) -> String {
    format!(
        "{ip}: TLS: invalid peer certificate: certificate not valid for name \"{ip}\"; certificate is only valid \
         for default.domain"
    )
}

/// The line of the dialer for `host`, with the failure of the system
/// resolver and of each resolver of DNS over HTTPS.
async fn line(host: &str, system: &str, resolvers: &[(String, String)]) -> String {
    let known = Known { asks: true, ..Default::default() };
    let system = async { Err(system.to_string()) };
    let doh = async { Err(fold(resolvers)) };
    let detail = resolve_in(&known, host, 80, None, system, doh).await.unwrap_err();
    assert!(detail.len() <= most(host), "{} bytes: {detail}", detail.len());
    pod_proxy::DialError::Resolve { host: host.into(), detail }.to_string()
}

/// The error line of podnet with the line of the dialer: podnet's words
/// before it (`connecting to HOST:PORT`, `failed` in podnet's
/// `src/egress.rs`), with a port of 5 digits.
fn podnet(host: &str, line: &str) -> String {
    format!("podnet: connecting to {host}:65535: {line}")
}

/// podnet's bound of each error line (podnet's `tests/error_words.rs`): a
/// line has fewer bytes.
const PODNET: usize = 300;

/// The two lines of PodUtils/podnet#753, a proxy that re-signs TLS for
/// another name, and the first of them for a name of 63 bytes: the bound
/// holds, also in the line of podnet; no cut falls inside a cause; each
/// group of resolvers has its cause. A cause that does not fit with each
/// of its words keeps its class (`TLS`, `could not connect`).
#[tokio::test]
async fn each_group_of_resolvers_keeps_its_cause_whole_in_the_bound() {
    let mut middlebox = each(&V4, unknown_issuer);
    middlebox.extend(each(&V6, no_family));
    let mut lab = each(&V6, no_family);
    lab.extend(each(&V4, unreachable));
    let mut renamed = each(&V4, other_name);
    renamed.extend(each(&V6, no_family));
    let long = format!("{}.example.com", "x".repeat(51));
    let no_name = "failed to lookup address information: Name does not resolve";
    let rows = [
        (
            "nothing.invalid".to_string(),
            no_name,
            middlebox.clone(),
            "could not resolve nothing.invalid: system resolver: Name does not resolve; DNS over HTTPS: 8 resolvers \
             failed: 4 with a certificate that no trusted CA signed; 4 with Address family not supported by protocol"
                .to_string(),
        ),
        (
            "example.com".to_string(),
            "failed to lookup address information: Try again",
            lab,
            "could not resolve example.com: system resolver: Try again; DNS over HTTPS: 8 resolvers failed: 4 with \
             Address family not supported by protocol; 4 with could not connect: Network is unreachable"
                .to_string(),
        ),
        (
            "nothing.invalid".to_string(),
            no_name,
            renamed,
            "could not resolve nothing.invalid: system resolver: Name does not resolve; DNS over HTTPS: 8 resolvers \
             failed: 4 with TLS; 4 with Address family not supported by protocol"
                .to_string(),
        ),
        (
            long.clone(),
            no_name,
            middlebox,
            format!(
                "could not resolve {long}: system resolver: Name does not resolve; DNS over HTTPS: 8 resolvers failed: \
                 4 with TLS; 4 with could not connect"
            ),
        ),
    ];
    assert_eq!(long.len(), 63);
    for (host, system, resolvers, want) in rows {
        let line = line(&host, system, &resolvers).await;
        assert_eq!(line, want);
        assert!(!line.contains("..."), "a cut inside a cause: {line}");
        assert!(line.len() <= LINE, "{} bytes: {line}", line.len());
        let podnet = podnet(&host, &line);
        assert!(podnet.len() < PODNET, "{} bytes: {podnet}", podnet.len());
        let groups = line.split_once("8 resolvers failed: ").map(|(_, g)| g).unwrap_or_default();
        let causes: Vec<&str> = groups.split("4 with ").filter(|g| !g.is_empty()).collect();
        assert_eq!(causes.len(), 2, "each group has its cause: {line}");
        assert!(causes.iter().all(|c| c.trim_end_matches("; ").len() >= 3), "{line}");
    }
}

/// The failures of a longer name get fewer bytes, and a name too long for
/// the bound keeps the leads and the classes ([`LEAST`]).
#[test]
fn a_longer_name_leaves_fewer_bytes_to_the_failures() {
    assert_eq!(most("nothing.invalid"), LINE - "could not resolve nothing.invalid: ".len());
    assert!(most("example.com") > most("nothing.invalid"));
    assert_eq!(most(&"x".repeat(253)), LEAST);
}

/// The forms of a brief cause, each at a part boundary.
#[test]
fn a_cause_becomes_brief_at_a_part_boundary() {
    let tls = "TLS: invalid peer certificate: UnknownIssuer; no root signs it: a proxy needs its CA";
    let forms: Vec<Option<String>> = (0..STEPS).map(|step| form(tls, step)).collect();
    let want = ["TLS: invalid peer certificate: UnknownIssuer", "TLS: UnknownIssuer", "UnknownIssuer", "TLS"];
    assert_eq!(forms, want.map(|w| Some(w.to_string())));
    let connect = "could not connect: Network is unreachable";
    assert_eq!(form(connect, 1), None, "two parts have no middle");
    assert_eq!(form(connect, 2).as_deref(), Some("Network is unreachable"));
    assert_eq!(form(connect, 3).as_deref(), Some("could not connect"));
    assert_eq!((form("out of time", 2), form("out of time", 3)), (None, None));
    assert_eq!(form("x: ", 2), None, "an empty part is no cause");
}

/// A line that fits is whole; the leads stay; a cause with no part
/// boundary that alone does not fit is the only cut.
#[test]
fn the_leads_stay_and_only_a_cause_with_no_boundary_is_cut() {
    let short = vec![Phrase::of("a: b").led("system resolver: "), Phrase::of("c")];
    assert_eq!(fit(short, 160), "system resolver: a: b; c");
    let causes = vec![("2 with ".to_string(), "x: y: z".to_string()), (String::new(), "out of time".to_string())];
    let line = fit(vec![Phrase::new("3 resolvers failed: ", causes).led("DNS over HTTPS: ")], 60);
    assert_eq!(line, "DNS over HTTPS: 3 resolvers failed: 2 with x: z; out of time");
    let line = fit(vec![Phrase::of("w".repeat(300)).led("system resolver: "), Phrase::of("no").led("DNS: ")], 100);
    assert!(line.len() <= 100 && line.starts_with("system resolver: www") && line.ends_with("...; DNS: no"), "{line}");
}
