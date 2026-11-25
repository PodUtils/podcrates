//! Each check of a bearer, and each class of its refusals.

use super::*;

/// 2026-11-25, in seconds since the epoch.
const NOW: u64 = 1_795_564_800;

fn signer() -> Identity {
    Identity::from_seed(&[0x55; 32])
}

fn holder() -> PublicKey {
    Identity::from_seed(&[0x11; 32]).public()
}

fn channel(byte: u8) -> Binding {
    Binding::from_bytes([byte; 32])
}

fn takes_the_signer(key: &PublicKey) -> bool {
    *key == signer().public()
}

/// A bearer of the test's signer for the test's holder on channel `byte`.
fn bearer_of(byte: u8, iat: u64) -> Vec<u8> {
    mint(&signer(), holder(), &channel(byte), iat, [0x77; 32])
}

fn verify(bearers: &Bearers, blob: &[u8], binding: Option<&Binding>, now: u64) -> Result<Verified, Refused> {
    bearers.verify(blob, binding, &holder(), takes_the_signer, now)
}

#[test]
fn a_bearer_of_its_channel_is_taken_once() {
    let bearers = Bearers::default();
    let blob = bearer_of(1, NOW);
    assert_eq!(blob.len(), LEN);
    assert!(is_bearer(&blob));
    let verified = verify(&bearers, &blob, Some(&channel(1)), NOW + 1).unwrap();
    assert_eq!(verified, Verified { signer: signer().public(), iat: NOW, exp: NOW + LIFE });
    assert_eq!(verify(&bearers, &blob, Some(&channel(1)), NOW + 2), Err(Refused::Replayed));
}

#[test]
fn a_read_bearer_writes_back_to_its_bytes_from_its_fields() {
    let blob = bearer_of(1, NOW);
    let read = Bearer::read(&blob).unwrap();
    assert_eq!(read.to_bytes()[..], blob[..]);
    assert_eq!(read.to_bytes()[..SIGNED], read.signed[..]);
    // Each field writes to its own place: a change of one field changes
    // its bytes alone.
    let mut later = read.clone();
    later.exp += 1;
    let changed: Vec<usize> = (0..LEN).filter(|&i| later.to_bytes()[i] != blob[i]).collect();
    assert_eq!(changed, vec![MAGIC.len() + 64 + 15]);
}

#[test]
fn a_bearer_of_another_channel_is_refused_by_its_binding() {
    let bearers = Bearers::default();
    let blob = bearer_of(1, NOW);
    assert_eq!(verify(&bearers, &blob, Some(&channel(2)), NOW), Err(Refused::Binding));
    // The refusal recorded nothing: the bearer still holds on its own
    // channel.
    assert!(verify(&bearers, &blob, Some(&channel(1)), NOW).is_ok());
}

#[test]
fn a_bearer_with_no_binding_is_refused() {
    let bearers = Bearers::default();
    assert_eq!(verify(&bearers, &bearer_of(1, NOW), None, NOW), Err(Refused::Unbound));
}

#[test]
fn a_changed_jti_or_nonce_fails_the_signature_and_a_signed_wrong_jti_the_binding() {
    let bearers = Bearers::default();
    let blob = bearer_of(1, NOW);
    let jti_at = MAGIC.len() + 32 + 32 + 8 + 8 + 32;
    for at in [jti_at, jti_at - 1, jti_at + 31] {
        let mut changed = blob.clone();
        changed[at] ^= 1;
        assert_eq!(verify(&bearers, &changed, Some(&channel(1)), NOW), Err(Refused::Signature), "byte {at}");
    }
    // The signer signed a jti of another binding: the signature holds, the
    // binding does not.
    let unsigned = Unsigned::new(signer().public(), holder(), &channel(9), NOW, [0x77; 32]);
    let signed = unsigned.signed(&signer().sign(unsigned.message()));
    assert_eq!(verify(&bearers, &signed, Some(&channel(1)), NOW), Err(Refused::Binding));
}

#[test]
fn the_form_the_holder_and_the_signer_each_have_a_class() {
    let bearers = Bearers::default();
    let blob = bearer_of(1, NOW);
    let bound = Some(&channel(1));
    assert_eq!(verify(&bearers, &blob[..LEN - 1], bound, NOW), Err(Refused::Form));
    let mut longer = blob.clone();
    longer.push(0);
    assert_eq!(verify(&bearers, &longer, bound, NOW), Err(Refused::Form));
    let mut magic = blob.clone();
    magic[0] = 0;
    assert_eq!(verify(&bearers, &magic, bound, NOW), Err(Refused::Form));
    let other = Identity::from_seed(&[0x12; 32]).public();
    assert_eq!(bearers.verify(&blob, bound, &other, takes_the_signer, NOW), Err(Refused::Holder));
    assert_eq!(bearers.verify(&blob, bound, &holder(), |_| false, NOW), Err(Refused::Signer));
}

#[test]
fn the_life_and_the_clock_are_checked_with_the_skew() {
    let bearers = Bearers::default();
    let bound = Some(&channel(1));
    // A bearer made later than now and the skew, and one made at its edge.
    assert_eq!(verify(&bearers, &bearer_of(1, NOW + SKEW + 1), bound, NOW), Err(Refused::Early));
    assert!(verify(&Bearers::default(), &bearer_of(1, NOW + SKEW), bound, NOW).is_ok());
    // Its life and the skew ended, or not yet.
    let blob = bearer_of(1, NOW);
    assert_eq!(verify(&bearers, &blob, bound, NOW + LIFE + SKEW), Err(Refused::Expired));
    assert!(verify(&bearers, &blob, bound, NOW + LIFE + SKEW - 1).is_ok());
    // A signer that writes a longer life, or none.
    for exp in [NOW + LIFE + 1, NOW, NOW - 1] {
        let mut unsigned = Unsigned::new(signer().public(), holder(), &channel(1), NOW, [0x77; 32]);
        unsigned.message[MAGIC.len() + 72..MAGIC.len() + 80].copy_from_slice(&exp.to_be_bytes());
        let signed = unsigned.signed(&signer().sign(unsigned.message()));
        assert_eq!(verify(&bearers, &signed, bound, NOW), Err(Refused::Life), "exp {exp}");
    }
}

#[test]
fn the_record_forgets_a_jti_after_its_life_and_refuses_when_full() {
    let bearers = Bearers::with_limit(1);
    let bound = Some(&channel(1));
    assert!(verify(&bearers, &bearer_of(1, NOW), bound, NOW).is_ok());
    let second = mint(&signer(), holder(), &channel(1), NOW, [0x78; 32]);
    assert_eq!(verify(&bearers, &second, bound, NOW), Err(Refused::Full));
    // The first one's life and the skew ended: its entry goes, and the
    // record takes the next.
    let third = mint(&signer(), holder(), &channel(1), NOW + LIFE + SKEW, [0x79; 32]);
    assert!(verify(&bearers, &third, bound, NOW + LIFE + SKEW).is_ok());
}

#[test]
fn no_text_and_no_debug_output_holds_a_byte_of_the_bearer() {
    let blob = bearer_of(1, NOW);
    let read = Bearer::read(&blob).unwrap();
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let secrets = [hex(&read.nonce), hex(&read.jti), hex(&read.signature), hex(channel(1).bytes())];
    let bearers = Bearers::default();
    assert!(verify(&bearers, &blob, Some(&channel(1)), NOW).is_ok());
    let mut shown = vec![format!("{read:?}"), format!("{:?}", channel(1)), format!("{bearers:?}")];
    let all = [
        Refused::Unbound,
        Refused::Form,
        Refused::Holder,
        Refused::Signer,
        Refused::Signature,
        Refused::Life,
        Refused::Early,
        Refused::Expired,
        Refused::Binding,
        Refused::Replayed,
        Refused::Full,
    ];
    let names: std::collections::HashSet<&str> = all.iter().map(|r| r.name()).collect();
    assert_eq!(names.len(), all.len(), "each class has its own name");
    for refused in all {
        shown.push(refused.to_string());
        shown.push(format!("{refused:?}"));
        assert!(refused.to_string().contains(refused.name()), "{refused}");
    }
    for text in &shown {
        for secret in &secrets {
            assert!(!text.contains(&secret[..16]), "{text}");
        }
    }
}
