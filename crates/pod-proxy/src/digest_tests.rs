use super::*;

#[test]
fn md5_vectors_of_rfc_1321() {
    assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(hex(&md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(
        hex(&md5(b"12345678901234567890123456789012345678901234567890123456789012345678901234567890")),
        "57edf4a22be3c955ac49da2e2107b67a"
    );
}

/// The example of RFC 7616 section 3.9.1.
#[test]
fn digest_responses_of_rfc_7616() {
    let header = "Digest realm=\"http-auth@example.org\", qop=\"auth, auth-int\", algorithm=SHA-256, nonce=\"7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v\", opaque=\"FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS\"";
    let challenge = parse_challenge(header).unwrap();
    assert_eq!(challenge.qop.as_deref(), Some("auth"));
    let value = authorization(
        &challenge,
        "Mufasa",
        "Circle of Life",
        "GET",
        "/dir/index.html",
        "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ",
        1,
    );
    assert!(value.contains("response=\"753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1\""), "{value}");
    let md5_challenge = Challenge { algorithm: "MD5".into(), ..challenge };
    let value = authorization(
        &md5_challenge,
        "Mufasa",
        "Circle of Life",
        "GET",
        "/dir/index.html",
        "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ",
        1,
    );
    assert!(value.contains("response=\"8ca523f5e9506fed4657c9700eebdbec\""), "{value}");
    assert!(value.contains("nc=00000001"));
    assert!(value.contains("opaque=\"FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS\""));
}

/// The scheme has any case, and a value with more than one challenge gives
/// its Digest challenge.
#[test]
fn a_digest_challenge_among_others() {
    let lower = parse_challenge("digest realm=\"x\", nonce=\"n\", qop=\"auth\"").unwrap();
    assert_eq!((lower.realm.as_str(), lower.nonce.as_str()), ("x", "n"));
    assert_eq!(lower.qop.as_deref(), Some("auth"));
    let both =
        parse_challenge("Basic realm=\"a, b\", Digest realm=\"x\", nonce=\"n\", qop=\"auth,auth-int\", algorithm=MD5")
            .unwrap();
    assert_eq!((both.realm.as_str(), both.nonce.as_str(), both.algorithm.as_str()), ("x", "n", "MD5"));
    assert_eq!(both.qop.as_deref(), Some("auth"));
    let after = parse_challenge("NTLM, Negotiate, DIGEST nonce=\"n2\", realm=\"y\"").unwrap();
    assert_eq!((after.realm.as_str(), after.nonce.as_str()), ("y", "n2"));
    // The parameters of the Digest challenge stop at the next scheme.
    let before = parse_challenge("Digest realm=\"x\", nonce=\"n\", Basic realm=\"z\"").unwrap();
    assert_eq!(before.realm, "x");
}

#[test]
fn challenges_that_podnet_does_not_take() {
    assert!(parse_challenge("Basic realm=\"x\"").is_none());
    assert!(parse_challenge("Digest realm=\"x\", nonce=\"n\", algorithm=SHA-512-256").is_none());
    assert!(parse_challenge("Digest realm=\"x\"").is_none(), "no nonce");
}

/// The challenge of a head is the first that is answered; a client nonce
/// is 16 bytes of the source in hex, and a source that gives none is an
/// error.
#[test]
fn the_challenge_of_a_head_and_the_client_nonce() {
    let head = "HTTP/1.1 407 x\r\nProxy-Authenticate: Digest realm=\"r\", algorithm=SHA-512-256, nonce=\"a\"\r\n\
                proxy-authenticate: NTLM, digest nonce=\"b\", realm=\"s\"\r\n\r\n";
    let found = challenge_in(head).expect("the second header");
    assert_eq!((found.nonce.as_str(), found.realm.as_str()), ("b", "s"));
    assert_eq!(challenge_in("HTTP/1.1 407 x\r\nProxy-Authenticate: Basic realm=\"r\"\r\n\r\n"), None);

    struct Count(u8);
    impl Entropy for Count {
        fn fill(&mut self, buf: &mut [u8]) -> Result<(), NoRandom> {
            for b in buf {
                *b = self.0;
                self.0 += 1;
            }
            Ok(())
        }
    }
    assert_eq!(cnonce(&mut Count(0)).unwrap(), "000102030405060708090a0b0c0d0e0f");
    struct Dry;
    impl Entropy for Dry {
        fn fill(&mut self, _: &mut [u8]) -> Result<(), NoRandom> {
            Err(NoRandom)
        }
    }
    assert_eq!(cnonce(&mut Dry), Err(NoRandom));
}
