//! `grants.json`: grants used once, OpenSSH user certificates of Ed25519
//! keys, as `pod-relay`'s `grant::Mint` signs them and `grant::Cert` reads
//! them; and bearers bound to one channel, as `grant::bearer::mint` signs
//! them and `grant::Bearers` checks them. Ed25519 signs deterministically,
//! so a seed and the fields give the bytes.

use pod_e2e::Binding;
use pod_identity::Identity;
use pod_relay::grant::{self, Bearer, Bearers, Cert, Mint, Refused, USER, bearer};

use crate::common::{array, bytes, hex, name, number, of_kind, strings, text, unhex};

#[test]
fn each_grant_signs_to_its_bytes_and_reads_back_whole() {
    for v in of_kind("grants", "grant") {
        let issuer = Identity::from_seed(&array(&v, "issuer_seed"));
        let key = Identity::from_seed(&array(&v, "key_seed")).public();
        let node = Identity::from_seed(&array(&v, "node_seed")).public();
        assert_eq!(issuer.public().hex(), text(&v, "issuer"), "{}", name(&v));
        assert_eq!(key.hex(), text(&v, "key"), "{}", name(&v));
        let principals = strings(&v, "principals");
        assert_eq!(principals, [grant::principal(&node, v["route"].as_str())], "{}", name(&v));
        let owned: Vec<(String, Vec<u8>)> = v["critical"]
            .as_array()
            .expect("critical")
            .iter()
            .map(|o| (o[0].as_str().expect("a name").to_string(), unhex(o[1].as_str().expect("data"))))
            .collect();
        let critical: Vec<(&str, &[u8])> = owned.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        let cert = Mint {
            nonce: array(&v, "nonce"),
            key,
            serial: number(&v, "serial"),
            key_id: text(&v, "key_id"),
            principals: &principals,
            valid_after: number(&v, "valid_after"),
            valid_before: number(&v, "valid_before"),
            critical: &critical,
        }
        .sign(&issuer);
        assert_eq!(hex(&cert.blob()), text(&v, "hex"), "{}", name(&v));
        assert_eq!(hex(&cert.digest()), text(&v, "digest"), "{}", name(&v));
        let read = Cert::from_blob(&bytes(&v, "hex")).unwrap_or_else(|e| panic!("{}: {e}", name(&v)));
        assert_eq!(read, cert, "{}", name(&v));
        assert!(read.signed_by_issuer(), "{}: its signature", name(&v));
        assert_eq!(read.kind, USER, "{}", name(&v));
        assert!(read.critical.iter().any(|(n, d)| n == grant::ONCE && d.is_empty()), "{}", name(&v));
        assert!(read.extensions.is_empty(), "{}", name(&v));
    }
}

#[test]
fn a_changed_grant_fails_its_signature_and_a_cut_one_its_form() {
    for v in of_kind("grants", "grant-forged") {
        let read = Cert::from_blob(&bytes(&v, "hex")).unwrap_or_else(|e| panic!("{}: {e}", name(&v)));
        assert_eq!(read.serial, number(&v, "serial"), "{}", name(&v));
        assert!(!read.signed_by_issuer(), "{}: a forged grant verified", name(&v));
    }
    for v in of_kind("grants", "grant-refused") {
        assert!(Cert::from_blob(&bytes(&v, "hex")).is_err(), "{}: read", name(&v));
    }
}

/// 2026-11-25, past the `iat` of each bearer of the vectors.
const LATER: u64 = 1_795_564_800 + 3_600;

#[test]
fn each_bearer_signs_to_its_bytes_and_holds_on_its_channel_alone() {
    for v in of_kind("grants", "bearer") {
        let signer = Identity::from_seed(&array(&v, "signer_seed"));
        let holder = Identity::from_seed(&array(&v, "holder_seed")).public();
        assert_eq!(signer.public().hex(), text(&v, "signer"), "{}", name(&v));
        assert_eq!(holder.hex(), text(&v, "holder"), "{}", name(&v));
        let binding = Binding::of_hash(&bytes(&v, "handshake_hash"));
        assert_eq!(hex(binding.bytes()), text(&v, "binding"), "{}", name(&v));
        let nonce = array(&v, "nonce");
        assert_eq!(hex(&bearer::jti(&binding, &nonce)), text(&v, "jti"), "{}", name(&v));
        let iat = number(&v, "iat");
        let blob = bearer::mint(&signer, holder, &binding, iat, nonce);
        assert_eq!(hex(&blob), text(&v, "hex"), "{}", name(&v));
        let read = Bearer::read(&blob).unwrap_or_else(|e| panic!("{}: {e}", name(&v)));
        assert_eq!((read.signer, read.holder, read.iat, read.exp), (signer.public(), holder, iat, number(&v, "exp")));
        let verified = Bearers::default().verify(&blob, Some(&binding), &holder, |_| true, iat);
        assert!(verified.is_ok(), "{}: {verified:?}", name(&v));
        let elsewhere = Binding::from_bytes([0; 32]);
        let refused = Bearers::default().verify(&blob, Some(&elsewhere), &holder, |_| true, iat);
        assert_eq!(refused, Err(Refused::Binding), "{}", name(&v));
        assert!(Bearers::default().verify(&blob, Some(&binding), &holder, |_| true, LATER).is_err());
    }
}

#[test]
fn each_refused_bearer_fails_the_check_that_its_vector_names() {
    for v in of_kind("grants", "bearer-refused") {
        let blob = bytes(&v, "hex");
        let binding = Binding::from_bytes(array(&v, "binding"));
        let (holder, now) = match Bearer::read(&blob) {
            Ok(read) => (read.holder, read.iat),
            Err(_) => (Identity::from_seed(&[0; 32]).public(), 0),
        };
        let refused = Bearers::default().verify(&blob, Some(&binding), &holder, |_| true, now);
        assert_eq!(refused.map(|_| ()).map_err(Refused::name), Err(text(&v, "reason")), "{}", name(&v));
    }
}
