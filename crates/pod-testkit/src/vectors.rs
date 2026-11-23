//! Conformance vectors: the bytes that a published construction makes of
//! fixed inputs, and a runner that checks a codec against them. A round trip
//! through our own client and our own server proves that the two agree; it
//! proves nothing about the protocol, because a byte that both ends read
//! wrongly is still a byte that both ends read. A vector says instead what
//! the specification says, and a codec that answers anything else does not
//! speak the protocol.
//!
//! Each vector carries its source in [`source`](Shadowsocks::source). None
//! of them came from a capture of a reference client: the Shadowsocks
//! answers are the published construction sealed with OpenSSL (Node's
//! crypto) and keyed by RFC 5869's HKDF-SHA1 in Python's `hmac`, Trojan's
//! hash is Python's `hashlib.sha224` and the rest of its request is the
//! published layout, and the VMess fixture is the published algorithm built
//! in Go's own crypto packages, the language of the reference server. A
//! capture from a reference client is still owed.
//!
//! The runner takes the codec as a trait, so that this kit holds no
//! dependency on a crate of podcrates and each road is checked the same way.

/// One known answer of Shadowsocks's AEAD form (SIP004).
#[derive(Debug, Clone, Copy)]
pub struct Shadowsocks {
    /// The method, as a link names it.
    pub method: &'static str,
    /// The master key: EVP_BytesToKey of the password.
    pub master_key: &'static [u8],
    /// The session's salt.
    pub salt: &'static [u8],
    /// HKDF-SHA1 of the master key and the salt, info `ss-subkey`.
    pub subkey: &'static [u8],
    /// The address of `example.com:22` and `hello`.
    pub plain: &'static [u8],
    /// The salt and the two sealed pieces of the plain text.
    pub wire: &'static [u8],
    /// Where the answer comes from.
    pub source: &'static str,
}

/// SIP004's answers for the three AEAD methods that this client sends. The
/// salt is 0, 1, 2, ... up to the method's length, the password is
/// `foobar`, and the plain text is the address of `example.com:22` and
/// `hello`. The master key is MD5's EVP_BytesToKey in Python's `hashlib`,
/// the subkey is RFC 5869's HKDF-SHA1 in its `hmac`, and OpenSSL (Node's
/// crypto) sealed the pieces with AES-256-GCM, AES-128-GCM and
/// ChaCha20-Poly1305.
pub const SHADOWSOCKS: [Shadowsocks; 3] = [
    Shadowsocks {
        method: "chacha20-ietf-poly1305",
        master_key: b"8X\xf6\"0\xac<\x91_0\x0cfC\x12\xc6?V\x83xR\x96\x14\xd2-\xdbI#}/`\xbf\xdf",
        salt: b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f",
        subkey: b"\xc4\xf0\xe9\x81\x83H\xb2\xf3\x01\x88\xd8+7\xa4\xcd\xdc\x9f^\xa51\x07\x0e\xc6r%\x16\x02\x09\xfa\xffW<",
        plain: b"\x03\x0bexample.com\x00\x16hello",
        wire: b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f]\x83*5xVh\x9bi[6x*\xb3\xf14x\xc3\xac\x89\xd4$\x11\x94\x9f\xff-\xe8\xa5\xe2c\xa0$\xc9\x05-\xa5\xe83\xea\x0b\xa2a\xff\x89\xebA\xf4!\\\x15\xc2t\xc0",
        source: "SIP004, sealed with OpenSSL's ChaCha20-Poly1305; the subkey by RFC 5869's HKDF-SHA1",
    },
    Shadowsocks {
        method: "aes-256-gcm",
        master_key: b"8X\xf6\"0\xac<\x91_0\x0cfC\x12\xc6?V\x83xR\x96\x14\xd2-\xdbI#}/`\xbf\xdf",
        salt: b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f",
        subkey: b"\xc4\xf0\xe9\x81\x83H\xb2\xf3\x01\x88\xd8+7\xa4\xcd\xdc\x9f^\xa51\x07\x0e\xc6r%\x16\x02\x09\xfa\xffW<",
        plain: b"\x03\x0bexample.com\x00\x16hello",
        wire: b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f&\xe8\x1bK\xc8\x0fM\xa1\x07\x13\xd8V\xe6\xe2\x1c\x18\xa2\xa8$\x18\x15\xb0L-\xd9\x12Q\xe2\x82\xd8X\x99o'-]{\xfa\x19\x1b\x00j8\xdaJ\x12(\x01\xfb\xe1\xdb\x93\x16\xe4",
        source: "SIP004, sealed with OpenSSL's AES-256-GCM; the subkey by RFC 5869's HKDF-SHA1",
    },
    Shadowsocks {
        method: "aes-128-gcm",
        master_key: b"8X\xf6\"0\xac<\x91_0\x0cfC\x12\xc6?",
        salt: b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f",
        subkey: b"\xe5\x9e\x94V\x99\xe8i\x91D\xc32\xb9\xe6A\xefe",
        plain: b"\x03\x0bexample.com\x00\x16hello",
        wire: b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\xf8Z\x8e\xf0\xca)\x88\x82\xae,\x98i\x9e%\xfb\xbc\x1do<\x0f\xc8\xa2-\xdaI\xf5F*X^\xb7d\xe6)\x98$A\xed)\x08\x95\xab]W\x98\x1e{\xc90\xc1\xc5tL\xe9",
        source: "SIP004, sealed with OpenSSL's AES-128-GCM; the subkey by RFC 5869's HKDF-SHA1",
    },
];

/// What a Shadowsocks codec must do for the runner.
pub trait ShadowsocksCodec {
    /// HKDF-SHA1 of the master key and the salt, info `ss-subkey`.
    fn subkey(&self, master: &[u8], salt: &[u8]) -> Vec<u8>;
    /// The salt and the sealed pieces of `plain`, as one stream.
    fn seal(&self, method: &str, master: &[u8], salt: &[u8], plain: &[u8]) -> Vec<u8>;
    /// The plain text of `wire`, or `None` at the first piece that does not
    /// open.
    fn open(&self, method: &str, master: &[u8], wire: &[u8]) -> Option<Vec<u8>>;
}

/// Each published answer against `codec`: its subkey, its sealed bytes and
/// its opening. The count of checks comes back, or the first answer that the
/// codec did not match, by name.
pub fn check_shadowsocks(codec: &dyn ShadowsocksCodec) -> Result<usize, String> {
    let mut checks = 0;
    for answer in SHADOWSOCKS.iter() {
        let name = answer.method;
        if codec.subkey(answer.master_key, answer.salt) != answer.subkey {
            return Err(format!("{name}: the subkey is not the published HKDF-SHA1 of the salt"));
        }
        checks += 1;
        if codec.seal(name, answer.master_key, answer.salt, answer.plain) != answer.wire {
            return Err(format!("{name}: the sealed first chunk is not the published one"));
        }
        checks += 1;
        if codec.open(name, answer.master_key, answer.wire).as_deref() != Some(answer.plain) {
            return Err(format!("{name}: the published stream did not open to its plain text"));
        }
        checks += 1;
    }
    Ok(checks)
}

/// One known answer of Trojan's request (`docs/protocol.md`): the password's
/// SHA-224 in hex, CRLF, the command, the address, the port and CRLF.
#[derive(Debug, Clone, Copy)]
pub struct Trojan {
    /// The name of the case.
    pub name: &'static str,
    pub password: &'static str,
    pub host: &'static str,
    pub port: u16,
    /// The request, whole.
    pub request: &'static [u8],
    pub source: &'static str,
}

/// Trojan's answers: the published layout, and the hash that Python's
/// `hashlib.sha224` gives.
pub const TROJAN: [Trojan; 3] = [
    Trojan {
        name: "a name",
        password: "password",
        host: "h.example",
        port: 22,
        request: b"d63dc919e201d7bc4c825630d2cf25fdc93d4b2f0d46706d29038d01\r\n\x01\x03\x09h.example\x00\x16\r\n",
        source: "Trojan's request layout, and SHA-224 of the password by Python's hashlib",
    },
    Trojan {
        name: "an IPv4 address",
        password: "password",
        host: "192.0.2.7",
        port: 443,
        request: b"d63dc919e201d7bc4c825630d2cf25fdc93d4b2f0d46706d29038d01\r\n\x01\x01\xc0\x00\x02\x07\x01\xbb\r\n",
        source: "Trojan's request layout, and SHA-224 of the password by Python's hashlib",
    },
    Trojan {
        name: "an IPv6 address",
        password: "password",
        host: "2001:db8::1",
        port: 22,
        request: b"d63dc919e201d7bc4c825630d2cf25fdc93d4b2f0d46706d29038d01\r\n\x01\x04\x20\x01\x0d\xb8\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x01\x00\x16\r\n",
        source: "Trojan's request layout, and SHA-224 of the password by Python's hashlib",
    },
];

/// What a Trojan codec must do for the runner.
pub trait TrojanCodec {
    fn request(&self, password: &str, host: &str, port: u16) -> Result<Vec<u8>, String>;
}

/// Each published request against `codec`.
pub fn check_trojan(codec: &dyn TrojanCodec) -> Result<usize, String> {
    let mut checks = 0;
    for answer in TROJAN.iter() {
        match codec.request(answer.password, answer.host, answer.port) {
            Ok(request) if request == answer.request => checks += 1,
            Ok(_) => return Err(format!("Trojan, {}: the request is not the published one", answer.name)),
            Err(why) => return Err(format!("Trojan, {}: {why}", answer.name)),
        }
    }
    Ok(checks)
}

/// One known answer of VMess's AEAD request header: the keys, the clock and
/// the target, the request in clear, and the bytes that go on the wire.
#[derive(Debug, Clone, Copy)]
pub struct Vmess {
    /// The user's ID, its 16 bytes.
    pub id: [u8; 16],
    /// MD5 of the ID and the form's constant.
    pub cmd_key: [u8; 16],
    /// The body's keys, the answer's byte: 33 random bytes.
    pub keys: [u8; 33],
    /// The auth ID's clock, in seconds.
    pub unix: u64,
    /// The auth ID's 4 random bytes.
    pub auth_random: [u8; 4],
    /// The connection's nonce, 8 bytes.
    pub nonce: [u8; 8],
    /// The target's name, and its port.
    pub host: &'static str,
    pub port: u16,
    /// The header's padding, at most 15 bytes.
    pub padding: &'static [u8],
    /// The header in clear, with its FNV-1a.
    pub plain: &'static [u8],
    /// The auth ID alone.
    pub auth: &'static [u8],
    /// The sealed length of the header, 2 bytes and a tag.
    pub sealed_len: &'static [u8],
    /// The sealed header.
    pub sealed: &'static [u8],
    /// The wire: the auth ID, the sealed length, the nonce and the sealed
    /// header.
    pub wire: &'static [u8],
    /// The `alterId` of the link that the fixture stands for: the AEAD form
    /// is 0, and each other value is the MD5 form, whose header differs.
    pub alter_id: u32,
    pub source: &'static str,
}

/// The fixture, built in Go from the published algorithm
/// (`aead/kdf.go`, `aead/authid.go`, `encoding/client.go`) with Go's own
/// crypto packages: the ID, the keys, the clock and the target are fixed
/// above. It is not a capture of a reference client.
pub const VMESS: Vmess = Vmess {
    id: [0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44, 0x00, 0x00],
    cmd_key: *b"\x90\xffq\x1f\xb4\xdb\x8a\xf1\xa5\xbd\x9b\x8e,\xce\x10W",
    keys: *b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f ",
    unix: 1_700_000_000,
    auth_random: [0x21, 0x22, 0x23, 0x24],
    nonce: [0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38],
    host: "www.example.com",
    port: 443,
    padding: b"\x01\x02\x03\x04\x05\x06\x07",
    plain: b"\x01\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f \x01s\x00\x01\x01\xbb\x02\x0fwww.example.com\x01\x02\x03\x04\x05\x06\x071D\x7f\xd4",
    auth: b"Y\x7fo\xe7\x7f\x09+3\x93\x9f\xe5\xfa\x02\x0cb\x09",
    sealed_len: b"^\xe8\xeb\xe9\xab\x8eJ\xc0'\xdd.\xbf.\x88\xae\xd1\xde\x8a",
    sealed: b"\xda\xea\x9e:\xbeA.\xe4Z\xe9p\xf0\"uX_\x9d\xe1\xdc\x89\xe1\xc1T\xfb\x1e\x02\xfaM,I\xf6\xe4\xed\x99]\x94^\x05\xaeJ\x1d\xc7\xbe>\xd4\x07\xc7\x01\xee\xfd\x07\xc2\xdf\xe2\x05\x8e\xf5&\xce\xf2\x05\x9b\x94\x7f\xb5\x93\xf2\xf7=\x92\x0b\x09\x93\xa8\x8e\xa3&\x04_U\xd5\xcb?\x13",
    wire: b"Y\x7fo\xe7\x7f\x09+3\x93\x9f\xe5\xfa\x02\x0cb\x09^\xe8\xeb\xe9\xab\x8eJ\xc0'\xdd.\xbf.\x88\xae\xd1\xde\x8a12345678\xda\xea\x9e:\xbeA.\xe4Z\xe9p\xf0\"uX_\x9d\xe1\xdc\x89\xe1\xc1T\xfb\x1e\x02\xfaM,I\xf6\xe4\xed\x99]\x94^\x05\xaeJ\x1d\xc7\xbe>\xd4\x07\xc7\x01\xee\xfd\x07\xc2\xdf\xe2\x05\x8e\xf5&\xce\xf2\x05\x9b\x94\x7f\xb5\x93\xf2\xf7=\x92\x0b\x09\x93\xa8\x8e\xa3&\x04_U\xd5\xcb?\x13",
    alter_id: 0,
    source: "the published algorithm, built in Go with crypto/aes, crypto/cipher, crypto/hmac and hash/crc32",
};

/// What a VMess codec must do for the runner.
pub trait VmessCodec {
    /// The fixture's header in clear.
    fn plain(&self, fixture: &Vmess) -> Vec<u8>;
    /// The fixture's bytes on the wire.
    fn sealed(&self, fixture: &Vmess) -> Vec<u8>;
    /// The `alterId` that the codec's header stands for: the AEAD form is 0.
    fn alter_id(&self, fixture: &Vmess) -> u32;
    /// The reason that the codec refuses a link whose `aid` is `aid`, if it
    /// does.
    fn refuses_alter_id(&self, aid: u32) -> Option<String>;
}

/// The fixture against `codec`: the header, the wire, and the `alterId`
/// rule. Both implementations of the field differ, and a link of another
/// `aid` is the MD5 form, whose header this client does not send.
pub fn check_vmess(codec: &dyn VmessCodec) -> Result<usize, String> {
    let fixture = &VMESS;
    let mut checks = 0;
    if codec.plain(fixture) != fixture.plain {
        return Err("VMess: the header in clear is not the fixture's".into());
    }
    checks += 1;
    if codec.sealed(fixture) != fixture.wire {
        return Err("VMess: the sealed request is not the fixture's".into());
    }
    checks += 1;
    let alter_id = codec.alter_id(fixture);
    if alter_id != fixture.alter_id {
        return Err(format!("VMess: alterId {alter_id} is not the fixture's, which is {}", fixture.alter_id));
    }
    checks += 1;
    if fixture.alter_id != 0 {
        return Err("VMess: the fixture is the AEAD form, whose alterId is 0".into());
    }
    match codec.refuses_alter_id(64) {
        Some(why) if !why.is_empty() => checks += 1,
        _ => return Err("VMess: a link with alterId 64 was not refused, and its header is another form".into()),
    }
    Ok(checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A codec that answers each vector with something else is refused by
    /// name: the runner is not a check that always passes.
    #[test]
    fn the_runner_refuses_a_codec_that_does_not_match_a_published_answer() {
        struct Wrong;
        impl ShadowsocksCodec for Wrong {
            fn subkey(&self, _master: &[u8], salt: &[u8]) -> Vec<u8> {
                salt.to_vec()
            }
            fn seal(&self, _method: &str, _master: &[u8], salt: &[u8], plain: &[u8]) -> Vec<u8> {
                [salt, plain].concat()
            }
            fn open(&self, _method: &str, _master: &[u8], wire: &[u8]) -> Option<Vec<u8>> {
                Some(wire.to_vec())
            }
        }
        let refused = check_shadowsocks(&Wrong).unwrap_err();
        assert!(refused.starts_with("chacha20-ietf-poly1305: the subkey"), "{refused}");
        struct NoneOfIt;
        impl TrojanCodec for NoneOfIt {
            fn request(&self, _password: &str, _host: &str, port: u16) -> Result<Vec<u8>, String> {
                Ok(port.to_be_bytes().to_vec())
            }
        }
        let refused = check_trojan(&NoneOfIt).unwrap_err();
        assert!(refused.contains("a name: the request is not the published one"), "{refused}");
        struct NoAlterId;
        impl VmessCodec for NoAlterId {
            fn plain(&self, fixture: &Vmess) -> Vec<u8> {
                fixture.plain.to_vec()
            }
            fn sealed(&self, fixture: &Vmess) -> Vec<u8> {
                fixture.wire.to_vec()
            }
            fn alter_id(&self, _fixture: &Vmess) -> u32 {
                64
            }
            fn refuses_alter_id(&self, _aid: u32) -> Option<String> {
                None
            }
        }
        let refused = check_vmess(&NoAlterId).unwrap_err();
        assert!(refused.contains("alterId 64"), "{refused}");
    }

    /// The data is what its source says: each vector's own fields agree with
    /// each other, so a typo in one of them fails here and not in a codec.
    #[test]
    fn the_vectors_agree_with_themselves() {
        for answer in SHADOWSOCKS.iter() {
            assert_eq!(answer.master_key.len(), answer.salt.len(), "{}", answer.method);
            assert_eq!(answer.subkey.len(), answer.master_key.len(), "{}", answer.method);
            assert_eq!(answer.wire.len(), answer.salt.len() + 18 + answer.plain.len() + 16, "{}", answer.method);
            assert!(answer.wire.starts_with(answer.salt), "{}", answer.method);
            assert!(!answer.source.is_empty());
        }
        for answer in TROJAN.iter() {
            assert_eq!(&answer.request[..56], b"d63dc919e201d7bc4c825630d2cf25fdc93d4b2f0d46706d29038d01");
            assert_eq!(&answer.request[56..59], b"\r\n\x01", "{}: the command is CONNECT", answer.name);
            assert!(answer.request.ends_with(b"\r\n"), "{}", answer.name);
            let port = u16::from_be_bytes([
                answer.request[answer.request.len() - 4],
                answer.request[answer.request.len() - 3],
            ]);
            assert_eq!(port, answer.port, "{}", answer.name);
        }
        let fixture = &VMESS;
        assert_eq!(fixture.auth.len(), 16);
        assert_eq!(fixture.sealed_len.len(), 18);
        assert_eq!(fixture.sealed.len(), 16 + fixture.plain.len());
        assert_eq!(fixture.wire.len(), 16 + 18 + 8 + fixture.sealed.len());
        assert!(fixture.wire.starts_with(fixture.auth));
        assert_eq!(fixture.alter_id, 0);
        assert_eq!(&fixture.plain[..9], &[1, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17]);
    }
}
