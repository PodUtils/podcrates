use super::*;

/// The cause of I/O comes back from a chain, and "not found" has its
/// own class.
#[test]
fn the_io_cause_comes_back_and_not_found_has_its_class() {
    let missing = io::Error::new(io::ErrorKind::NotFound, "/store: no such directory");
    assert_eq!(Class::of_io(missing.kind()), Class::NotFound);
    let f = Failure::caused(Stage::Host, Class::of_io(missing.kind()), "the store", missing);
    assert_eq!(f.io_cause().map(|e| e.kind()), Some(io::ErrorKind::NotFound));
    assert_eq!(f.class.name(), "notfound");
    assert!(Failure::new(Stage::Host, Class::Unavailable, "no space").io_cause().is_none());
}

#[test]
fn an_io_error_keeps_its_kind_as_a_class() {
    let f = Failure::io(Stage::Connect, "the connection to h:1", io::Error::new(io::ErrorKind::TimedOut, "no answer"));
    assert_eq!((f.stage, f.class), (Stage::Connect, Class::Timeout));
    assert_eq!(f.to_string(), "the connection to h:1");
    assert_eq!(f.chain(), "the connection to h:1: no answer");
    let refused = Failure::io(Stage::Connect, "a step", io::Error::from(io::ErrorKind::ConnectionRefused));
    assert_eq!(refused.class, Class::Unavailable);
}

#[test]
fn the_chain_names_each_cause_once() {
    let cause = io::Error::new(io::ErrorKind::ConnectionReset, "reset by peer");
    let f = Failure::caused(Stage::Tls, Class::Unavailable, "the handshake ended", cause);
    assert_eq!(f.chain(), "the handshake ended: reset by peer");
    assert!(f.source().is_some());
    let same = Failure::caused(Stage::Dns, Class::Unavailable, "no answer", io::Error::other("no answer"));
    assert_eq!(same.chain(), "no answer");
    let quoted = io::Error::other("invalid peer certificate: UnknownIssuer");
    let hinted = "invalid peer certificate: UnknownIssuer; give the CA of the proxy";
    let f = Failure::caused(Stage::Tls, Class::Protocol, hinted, quoted);
    assert_eq!(f.chain(), hinted);
}

/// Each class has its own name, and the codes are the table of the
/// README: the classes that a supervisor must not restart end with 78.
#[test]
fn each_class_has_a_name_and_an_exit_code() {
    let names: std::collections::HashSet<_> = Class::ALL.iter().map(|c| c.name()).collect();
    assert_eq!(names.len(), Class::ALL.len());
    assert_eq!(Class::Revoked.exit_code(), Class::Config.exit_code());
    assert_eq!(Class::Usage.exit_code(), 64);
    assert_eq!(Class::Timeout.exit_code(), Class::TempFail.exit_code());
    let own: std::collections::HashSet<_> =
        [Class::Refused, Class::Revoked, Class::Policy, Class::Unsupported].iter().map(|c| c.exit_code()).collect();
    assert_eq!(own.len(), 4, "refused, revoked, policy and unsupported each have a code of their own");
    // podssh#352: 73, 70 and 65 of sysexits(3), each in one row only.
    let rows = [(Class::CantCreate, 73, "cantcreate"), (Class::Internal, 70, "internal"), (Class::Data, 65, "data")];
    for (class, code, name) in rows {
        assert_eq!((class.exit_code(), class.name()), (code, name), "{class:?}");
        let rows = Class::ALL.iter().filter(|c| c.exit_code() == code).count();
        assert_eq!(rows, 1, "the code {code} is in {rows} rows");
    }
}

/// A lead that the text names already is not said again; a text that
/// starts with a longer word gets the lead (PodUtils/podnet#658).
#[test]
fn a_lead_is_said_once() {
    let request = "POST https://api.example/tunnel";
    let text = "POST https://api.example/tunnel: could not reach the proxy 127.0.0.1:1";
    assert_eq!(lead(request, text), text);
    assert_eq!(lead(request, request), request);
    let longer = "POST https://api.example/tunnels: refused";
    assert_eq!(lead(request, longer), format!("{request}: {longer}"));
    assert_eq!(lead("could not connect to 127.0.0.1:1", "refused"), "could not connect to 127.0.0.1:1: refused");
    assert_eq!(strip_lead("connecting to h:1: refused", "connecting to h:1"), "refused");
    assert_eq!(strip_lead("connecting to h:12: refused", "connecting to h:1"), "connecting to h:12: refused");
    let cause = io::Error::other("reset");
    let f = Failure::caused(Stage::Tls, Class::Unavailable, "TLS: reset", cause);
    assert_eq!(chain_of(&f), f.chain());
    assert_eq!(f.into_source().map(|e| e.to_string()).as_deref(), Some("reset"));
}

/// The six codes of the verification harness (podnet#501) are in this
/// table, and each code is one value: 64, 66, 69, 71, 77 and 78.
#[test]
fn the_codes_of_the_harness_are_in_the_table_and_distinct() {
    let table = [
        (Class::Usage, 64_u8),
        (Class::NotFound, 66),
        (Class::Unavailable, 69),
        (Class::Permission, 71),
        (Class::Refused, 77),
        (Class::Suite, 78),
    ];
    let codes: std::collections::HashSet<u8> = table.iter().map(|(_, code)| *code).collect();
    assert_eq!(codes.len(), 6, "each code is one value");
    for (class, code) in table {
        assert_eq!(class.exit_code(), code, "{class:?}");
        assert!(Class::ALL.contains(&class), "{class:?}");
    }
    assert_eq!(Class::Permission.name(), "permission");
    assert_eq!(Class::Suite.name(), "suite");
}

#[test]
fn a_layer_can_move_the_stage() {
    let f = Failure::new(Stage::Connect, Class::Timeout, "slow").at(Stage::Proxy);
    assert_eq!(f.stage.name(), "proxy");
    assert_eq!(f.class.to_string(), "timeout");
}

/// An error of I/O is the cause of its step: a reader that joins the chain
/// says its words once.
#[test]
fn an_io_failure_says_its_cause_once_on_a_join() {
    let e = io::Error::new(io::ErrorKind::NotFound, "No such file or directory (os error 2)");
    let f = Failure::io(Stage::Host, "reading /etc/pod.toml", e);
    let line = joined(&f);
    assert_eq!(line.matches("No such file").count(), 1, "{line}");
    assert_eq!(line, f.chain());
    assert_eq!(line, "reading /etc/pod.toml: No such file or directory (os error 2)");
    assert_eq!(f.io_cause().map(|e| e.kind()), Some(io::ErrorKind::NotFound));
}

/// The root of a chain is its first cause that is not a `Failure`; the text
/// keeps each step above it once.
#[test]
fn the_root_of_a_chain_comes_out_with_the_steps_as_text() {
    let reset = io::Error::new(io::ErrorKind::ConnectionReset, "reset by peer");
    let step = Failure::caused(Stage::Tls, Class::Unavailable, "the handshake", reset);
    let outer = Failure::caused(Stage::Tls, Class::Unavailable, "the edge at h:1", step);
    let (text, root) = outer.into_root();
    assert_eq!(text, "the edge at h:1: the handshake");
    let root = root.expect("a root");
    assert_eq!(root.downcast_ref::<io::Error>().map(|e| e.kind()), Some(io::ErrorKind::ConnectionReset));
    let joined_again = Failure::caused(Stage::Tls, Class::Unavailable, format!("each leg failed: {text}"), root);
    let line = joined(&joined_again);
    assert_eq!(line, "each leg failed: the edge at h:1: the handshake: reset by peer");
    assert_eq!(line, joined_again.chain());
    let plain = Failure::caused(Stage::Dns, Class::Unavailable, "a", Failure::new(Stage::Dns, Class::Unavailable, "b"));
    let (text, root) = plain.into_root();
    assert_eq!((text.as_str(), root.is_none()), ("a: b", true));
}
