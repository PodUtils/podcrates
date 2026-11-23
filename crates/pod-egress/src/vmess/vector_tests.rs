//! VEC-2 and VEC-3: the request of VMess's AEAD form against the fixture of
//! `pod-testkit::vectors`. The fixture's bytes come from the published
//! algorithm built in Go; the codec here must make them again, byte for
//! byte. The `alterId` of the link stands for a form of the header, and a
//! value above 0 is the MD5 form, which this client does not send: the
//! fixture's header has no M bit, and the link is refused by name.

use base64::Engine as _;

use super::body::Cipher;
use super::header::{self, Keys};
use super::{kdf, link};
use pod_testkit::vectors::{self, Vmess, VmessCodec};

/// The codec under test.
struct Ours;

impl Ours {
    fn keys(fixture: &Vmess) -> Keys {
        Keys::new(&fixture.keys)
    }
}

impl VmessCodec for Ours {
    fn plain(&self, fixture: &Vmess) -> Vec<u8> {
        header::request(&Ours::keys(fixture), Cipher::Aes128Gcm, fixture.host, fixture.port, fixture.padding)
            .expect("a target of the fixture")
    }

    fn sealed(&self, fixture: &Vmess) -> Vec<u8> {
        let plain = self.plain(fixture);
        header::seal(&kdf::cmd_key(&fixture.id), &plain, fixture.unix, fixture.auth_random, fixture.nonce)
            .expect("a header of the fixture")
    }

    /// The `alterId` that the header stands for: its option byte's M bit is
    /// the MD5 form, whose `aid` is above 0. The AEAD form sets no M.
    fn alter_id(&self, fixture: &Vmess) -> u32 {
        let plain = self.plain(fixture);
        // Version, the IV and the key, the answer's byte, then the option.
        if plain[34] & 0x04 != 0 { 64 } else { 0 }
    }

    fn refuses_alter_id(&self, aid: u32) -> Option<String> {
        let id: String = vectors::VMESS.id.iter().map(|b| format!("{b:02x}")).collect();
        let json = format!(
            r#"{{"add":"n.example","port":"443","id":"{id}","aid":"{aid}","scy":"auto","net":"ws","tls":"tls"}}"#
        );
        let link = format!("vmess://{}", base64::engine::general_purpose::STANDARD.encode(json));
        link::Server::parse(&link).err()
    }
}

#[test]
fn the_published_vmess_fixture_is_the_request_that_this_codec_makes() {
    assert_eq!(vectors::check_vmess(&Ours).unwrap(), 4);
}

/// The fixture's own pieces, so that a mistake in one of them is said here
/// and not read as a codec's: its auth ID is the one that the command key
/// seals at its clock, and its wire is its pieces in order.
#[test]
fn the_fixture_s_pieces_are_the_ones_that_its_wire_holds() {
    let fixture = &vectors::VMESS;
    let cmd_key = kdf::cmd_key(&fixture.id);
    assert_eq!(cmd_key, fixture.cmd_key, "the command key is MD5 of the ID and the form's constant");
    assert_eq!(kdf::auth_id(&cmd_key, fixture.unix, fixture.auth_random), fixture.auth[..16], "the auth ID");
    let wire = [fixture.auth, fixture.sealed_len, &fixture.nonce[..], fixture.sealed].concat();
    assert_eq!(wire, fixture.wire, "the wire is the auth ID, the sealed length, the nonce and the header");
}
