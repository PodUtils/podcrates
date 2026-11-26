//! The accept side's parser against the published requests of
//! `pod_testkit::vectors::TROJAN` and against each way to fail.

use pod_testkit::vectors::TROJAN;

use super::*;
use crate::trojan::request::request;

fn password() -> Password {
    Password::new("password").unwrap()
}

/// A request for `host` that the client writes.
fn made(host: &str) -> Vec<u8> {
    request(&password(), host, 22).unwrap()
}

#[test]
fn each_published_request_parses_back_and_each_prefix_is_too_short() {
    for vector in TROJAN.iter() {
        let password = Password::new(vector.password).unwrap();
        let got = parse(vector.request, &password).unwrap().expect(vector.name);
        assert_eq!(
            got,
            Request {
                command: Command::Connect,
                host: vector.host.into(),
                port: vector.port,
                used: vector.request.len()
            },
            "{}",
            vector.name
        );
        for end in 0..vector.request.len() {
            assert_eq!(parse(&vector.request[..end], &password), Ok(None), "{} at {end}", vector.name);
        }
        let mut more = vector.request.to_vec();
        more.extend_from_slice(b"payload");
        assert_eq!(parse(&more, &password).unwrap().unwrap().used, vector.request.len());
    }
}

#[test]
fn the_client_s_request_of_each_address_form_parses_back() {
    for host in ["h.example", "192.0.2.7", "[2001:db8::1]", &"a".repeat(255)] {
        let got = parse(&made(host), &password()).unwrap().unwrap();
        assert_eq!(got.host, host.trim_matches(['[', ']']));
        assert_eq!((got.port, got.command), (22, Command::Connect));
    }
}

#[test]
fn udp_associate_is_a_command_by_name() {
    let mut bytes = made("h.example");
    bytes[58] = UDP_ASSOCIATE;
    assert_eq!(parse(&bytes, &password()).unwrap().unwrap().command, Command::UdpAssociate);
}

#[test]
fn another_password_is_a_wrong_password_and_not_a_trojan_request_is_not() {
    let other = Password::new("another").unwrap();
    assert_eq!(parse(&made("h.example"), &other), Err(RequestError::WrongPassword));
    assert_eq!(parse(&made("h.example")[..56], &other), Err(RequestError::WrongPassword));
    let mut one = made("h.example");
    one[55] ^= 1;
    assert_eq!(parse(&one, &password()), Err(RequestError::WrongPassword));
    let http = b"GET / HTTP/1.1\r\nHost: example.com\r\nUser-Agent: curl\r\n\r\n";
    assert_eq!(parse(http, &password()), Err(RequestError::NotTrojan(Malformed::Hash)));
}

fn malformed(edit: impl FnOnce(&mut Vec<u8>)) -> Malformed {
    let mut bytes = made("h.example");
    edit(&mut bytes);
    match parse(&bytes, &password()) {
        Err(RequestError::NotTrojan(why)) => why,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_bad_command_a_bad_type_a_bad_name_and_a_missing_crlf_are_not_trojan() {
    assert_eq!(malformed(|b| b[58] = 0x02), Malformed::Command(2));
    assert_eq!(malformed(|b| b[59] = 0x05), Malformed::AddressType(5));
    assert_eq!(malformed(|b| b[60] = 0), Malformed::Name);
    assert_eq!(malformed(|b| b[61] = 0xff), Malformed::Name);
    assert_eq!(malformed(|b| b[57] = b'x'), Malformed::HashEnd);
    assert_eq!(malformed(|b| b[56] = b' '), Malformed::HashEnd);
    assert_eq!(malformed(|b| *b.last_mut().unwrap() = b'x'), Malformed::End);
    assert_eq!(malformed(|b| b.truncate(b.len() - 2)), Malformed::End);
}

#[test]
fn a_request_holds_at_most_max_request_bytes() {
    let longest = made(&"a".repeat(255));
    assert_eq!(longest.len(), MAX_REQUEST);
    assert_eq!(parse(&longest, &password()).unwrap().unwrap().used, MAX_REQUEST);
}

#[test]
fn the_comparison_of_the_hash_checks_each_byte_and_the_length() {
    let password = password();
    let hex = *password.hex();
    assert!(password.matches(&hex));
    assert!(!password.matches(&hex[..55]));
    for at in 0..56 {
        let mut other = hex;
        other[at] ^= 1;
        assert!(!password.matches(&other), "byte {at}");
    }
}
