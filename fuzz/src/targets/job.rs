//! The fuzz targets of the records of `pod-job`.

use pod_job::JobOffer;

/// A job offer of a source (`pod_job::JobOffer`, serde and `check`; new in
/// `pod-job`, T-163): untrusted JSON from a forge. An offer that serde
/// reads encodes again and reads back to the same offer, with the same
/// verdict of `check`; the check of a `github-scaleset` source refuses
/// each offer that the other check refuses, and each offer with a payload.
pub fn job_offer(data: &[u8]) {
    let Ok(offer) = serde_json::from_slice::<JobOffer>(data) else { return };
    let any = offer.check(false);
    let scaleset = offer.check(true);
    if any.is_err() || offer.payload.is_some() {
        assert!(scaleset.is_err(), "{offer:?}");
    }
    let text = serde_json::to_vec(&offer).expect("an offer encodes");
    let back: JobOffer = serde_json::from_slice(&text).expect("an encoded offer reads");
    assert_eq!(back.check(false), any);
    assert_eq!(back.check(true), scaleset);
    // serde_json reads a float with no exact round trip (its feature
    // `float_roundtrip` is off), so a number of the policy can change in its
    // last digit. The policy is data for the broker; its keys stay.
    assert_eq!(back.policy.keys().collect::<Vec<_>>(), offer.policy.keys().collect::<Vec<_>>());
    let (mut back, mut offer) = (back, offer);
    back.policy.clear();
    offer.policy.clear();
    assert_eq!(back, offer);
}
