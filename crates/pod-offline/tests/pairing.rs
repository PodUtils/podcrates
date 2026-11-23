//! The tests of pairing by an eight-word code: the exchange through a
//! spool, a single use, the offer's window, and the binding of the paired
//! key to the identity.

mod common;

use std::time::Duration;

use pod_identity::Identity;
use pod_offline::pairing::{self, CODE_BITS, Code, Giver, Offer, Paired, UsedCodes, WORDS, WORDS_COUNT};
use pod_offline::spool::Spool;

#[test]
fn a_code_pairs_two_identities_through_a_spool_and_a_second_use_is_refused() {
    let scratch = common::Scratch::new("pairing-spool");
    let giver = Identity::from_seed(&[51; 32]);
    let taker = Identity::from_seed(&[52; 32]);
    let code = Code::from_entropy(&[4, 9, 17, 33, 5, 6, 7, 8]);
    let spool = Spool::open(scratch.join("spool")).expect("a spool");
    let store = UsedCodes::open(scratch.join("used")).expect("a store");

    let offer = Offer::make(&giver, &code);
    spool.put(&giver, 1, &offer.encode()).expect("the offer");
    let entry = spool.receive().expect("a read").expect("the offer");
    let offer = Offer::decode(&entry.body).expect("the offer back");
    assert!(offer.verify(&code));
    spool.consume(&entry).expect("the consume");

    let paired = pairing::take(&taker, &store, &offer, &code).expect("the pairing");
    assert!(paired.verify(&code));
    assert_eq!(paired.identity, taker.public());
    assert_eq!(paired.peer, giver.public());
    spool.put(&taker, 2, &paired.encode()).expect("the record");

    let entry = spool.receive().expect("a read").expect("the record");
    let paired = Paired::decode(&entry.body).expect("the record back");
    assert!(paired.verify(&code));
    assert_eq!(paired.peer, giver.public());
    spool.consume(&entry).expect("the consume");

    let reused = UsedCodes::open(scratch.join("used")).expect("the same store");
    let failed = pairing::take(&taker, &reused, &offer, &code).expect_err("a second use");
    assert_eq!(failed.name(), "code-used");
}

#[test]
fn the_paired_key_is_bound_to_the_identity() {
    let scratch = common::Scratch::new("pairing-bind");
    let giver = Identity::from_seed(&[53; 32]);
    let taker = Identity::from_seed(&[54; 32]);
    let other = Identity::from_seed(&[55; 32]);
    let code = Code::from_entropy(&[7; WORDS_COUNT]);
    let store = UsedCodes::open(scratch.join("used")).expect("a store");
    let offer = Offer::make(&giver, &code);
    let paired = pairing::take(&taker, &store, &offer, &code).expect("the pairing");
    assert!(paired.verify(&code));
    assert_eq!(Paired::decode(&paired.encode()).expect("the text"), paired);

    let mut changed_peer = paired.clone();
    changed_peer.peer = other.public();
    assert!(!changed_peer.verify(&code), "a changed peer key still verified");

    let mut changed_identity = paired.clone();
    changed_identity.identity = other.public();
    assert!(!changed_identity.verify(&code), "a changed identity still verified");

    let mut changed_proof = paired.clone();
    changed_proof.proof[3] ^= 0x01;
    assert!(!changed_proof.verify(&code));

    let wrong = Code::from_entropy(&[8; WORDS_COUNT]);
    assert!(!paired.verify(&wrong));
}

#[test]
fn an_offer_with_another_code_or_key_is_refused() {
    let giver = Identity::from_seed(&[56; 32]);
    let code = Code::from_entropy(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let offer = Offer::make(&giver, &code);
    assert!(offer.verify(&code));
    assert!(!offer.verify(&Code::from_entropy(&[1, 2, 3, 4, 5, 6, 7, 9])));

    let mut changed = offer.clone();
    changed.identity = Identity::from_seed(&[57; 32]).public();
    assert!(!changed.verify(&code));
    assert_eq!(Offer::decode(&offer.encode()).expect("the text"), offer);
}

#[test]
fn a_code_carries_at_least_48_bits_of_entropy() {
    // The floor is checked when the test is compiled, so a shorter code
    // cannot pass this test by accident.
    const { assert!(CODE_BITS >= 48, "the documented code is at least 48 bits") };
    assert_eq!(CODE_BITS, WORDS_COUNT * 6);
    assert_eq!(WORDS.len(), 64, "each word is six bits");
    let low = Code::from_entropy(&[0; WORDS_COUNT]);
    let high = Code::from_entropy(&[0xff; WORDS_COUNT]);
    assert_ne!(low, high);
    assert_eq!(high.text().split('-').count(), WORDS_COUNT);
    // Each of the eight positions takes all 64 words with the others fixed,
    // so the code is the whole 64^8 space and not a smaller image.
    for at in 0..WORDS_COUNT {
        let mut texts = std::collections::BTreeSet::new();
        for value in 0..64u8 {
            let mut bytes = [0u8; WORDS_COUNT];
            bytes[at] = value;
            texts.insert(Code::from_entropy(&bytes).text());
        }
        assert_eq!(texts.len(), 64, "byte {at} does not reach all 64 words");
    }
}

#[test]
fn a_wrong_code_is_refused_before_a_claim() {
    let scratch = common::Scratch::new("pairing-wrong");
    let giver = Identity::from_seed(&[58; 32]);
    let taker = Identity::from_seed(&[59; 32]);
    let code = Code::from_entropy(&[1, 3, 5, 7, 9, 11, 13, 15]);
    let wrong = Code::from_entropy(&[1, 3, 5, 7, 9, 11, 13, 16]);
    let store = UsedCodes::open(scratch.join("used")).expect("a store");
    let offer = Offer::make(&giver, &code);
    let failed = pairing::take(&taker, &store, &offer, &wrong).expect_err("a wrong code");
    assert_eq!(failed.name(), "proof");
    assert!(!store.used(&code.hash()), "a wrong code claimed the real one");
}

#[test]
fn the_giver_claims_an_offer_once_and_refuses_a_second_claim() {
    let scratch = common::Scratch::new("pairing-giver");
    let giver_id = Identity::from_seed(&[61; 32]);
    let taker = Identity::from_seed(&[62; 32]);
    let code = Code::from_entropy(&[2, 4, 6, 8, 10, 12, 14, 16]);
    let giver = Giver::open(scratch.join("giver"), Duration::from_secs(600)).expect("a giver");
    let offer = giver.offer(&giver_id, &code).expect("an offer");
    assert!(offer.verify(&code));
    let store = UsedCodes::open(scratch.join("used")).expect("a store");
    let now = offer.issued + 1;
    let paired = pairing::take_at(&taker, &store, &offer, &code, now).expect("the pairing");
    giver.accept(&code, &paired, now).expect("the first claim");
    let failed = giver.accept(&code, &paired, now).expect_err("a second claim");
    assert_eq!(failed.name(), "code-used");
    // A second offer of the same code is refused as well: it was issued.
    let again = giver.offer(&giver_id, &code).expect_err("a second issue");
    assert_eq!(again.name(), "code-used");
}

#[test]
fn an_expired_offer_is_refused_by_name() {
    let scratch = common::Scratch::new("pairing-expiry");
    let giver_id = Identity::from_seed(&[63; 32]);
    let taker = Identity::from_seed(&[64; 32]);
    let code = Code::from_entropy(&[3, 5, 7, 9, 11, 13, 15, 17]);
    let store = UsedCodes::open(scratch.join("used")).expect("a store");
    let offer = Offer::make_at(&giver_id, &code, 1_000, Duration::from_secs(10));
    assert!(offer.verify(&code));
    assert!(!offer.expired(1_009));
    let late = pairing::take_at(&taker, &store, &offer, &code, 1_010).expect_err("an expired offer");
    assert_eq!(late.name(), "code-expired");

    // The giver refuses a claim past the expiry of the offer it issued.
    let giver = Giver::open(scratch.join("giver"), Duration::from_secs(600)).expect("a giver");
    let issued = giver.offer(&giver_id, &code).expect("an offer");
    let paired = pairing::take_at(&taker, &store, &issued, &code, issued.issued + 1).expect("the pairing");
    let expired = giver.accept(&code, &paired, issued.expires + 1).expect_err("an expired claim");
    assert_eq!(expired.name(), "code-expired");
}
