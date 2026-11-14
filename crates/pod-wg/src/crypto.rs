//! The other primitives: X25519 (curve25519-dalek), ChaCha20-Poly1305
//! (ring), the TAI64N timestamp of an initiation, and random bytes from the
//! caller's [`Entropy`].

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use pod_core::entropy::Entropy;
use ring::aead;
use zeroize::Zeroize;

use crate::Error;

pub(crate) fn x25519(private: &[u8; 32], public: &[u8; 32]) -> [u8; 32] {
    curve25519_dalek::MontgomeryPoint(*public).mul_clamped(*private).to_bytes()
}

pub(crate) fn x25519_base(private: &[u8; 32]) -> [u8; 32] {
    curve25519_dalek::MontgomeryPoint::mul_base_clamped(*private).to_bytes()
}

fn nonce(counter: u64) -> aead::Nonce {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&counter.to_le_bytes());
    aead::Nonce::assume_unique_for_key(n)
}

fn key(key: &[u8; 32]) -> aead::LessSafeKey {
    aead::LessSafeKey::new(aead::UnboundKey::new(&aead::CHACHA20_POLY1305, key).expect("a key of 32 bytes"))
}

/// ChaCha20-Poly1305 of `plain` under `counter`, with the tag appended.
pub(crate) fn seal(k: &[u8; 32], counter: u64, plain: &[u8], ad: &[u8]) -> Vec<u8> {
    let mut out = plain.to_vec();
    // ring fails a seal only past 256 GiB of input; a datagram is far less.
    key(k).seal_in_place_append_tag(nonce(counter), aead::Aad::from(ad), &mut out).expect("ChaCha20-Poly1305 seals");
    out
}

/// The plain bytes of `sealed`, or `None` when it does not authenticate.
pub(crate) fn open(k: &[u8; 32], counter: u64, sealed: &[u8], ad: &[u8]) -> Option<Vec<u8>> {
    let mut buf = sealed.to_vec();
    let plain = key(k).open_in_place(nonce(counter), aead::Aad::from(ad), &mut buf).ok()?;
    Some(plain.to_vec())
}

/// HChaCha20 (draft-irtf-cfrg-xchacha, section 2.2): the subkey of
/// XChaCha20 for a key and the first 16 bytes of a nonce of 24. ring has no
/// XChaCha20, so it is here; ring's ChaCha20-Poly1305 does the rest.
fn hchacha20(key: &[u8; 32], nonce: &[u8; 16]) -> [u8; 32] {
    let mut s = [0u32; 16];
    s[..4].copy_from_slice(&[0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574]);
    for (i, w) in s[4..12].iter_mut().enumerate() {
        *w = u32::from_le_bytes(key[i * 4..i * 4 + 4].try_into().expect("4 bytes"));
    }
    for (i, w) in s[12..].iter_mut().enumerate() {
        *w = u32::from_le_bytes(nonce[i * 4..i * 4 + 4].try_into().expect("4 bytes"));
    }
    fn quarter(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
        s[a] = s[a].wrapping_add(s[b]);
        s[d] = (s[d] ^ s[a]).rotate_left(16);
        s[c] = s[c].wrapping_add(s[d]);
        s[b] = (s[b] ^ s[c]).rotate_left(12);
        s[a] = s[a].wrapping_add(s[b]);
        s[d] = (s[d] ^ s[a]).rotate_left(8);
        s[c] = s[c].wrapping_add(s[d]);
        s[b] = (s[b] ^ s[c]).rotate_left(7);
    }
    for _ in 0..10 {
        quarter(&mut s, 0, 4, 8, 12);
        quarter(&mut s, 1, 5, 9, 13);
        quarter(&mut s, 2, 6, 10, 14);
        quarter(&mut s, 3, 7, 11, 15);
        quarter(&mut s, 0, 5, 10, 15);
        quarter(&mut s, 1, 6, 11, 12);
        quarter(&mut s, 2, 7, 8, 13);
        quarter(&mut s, 3, 4, 9, 14);
    }
    let mut out = [0u8; 32];
    for (i, w) in s[..4].iter().chain(&s[12..]).enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

/// The subkey and the nonce of ChaCha20-Poly1305 for XChaCha20-Poly1305.
fn xkey(k: &[u8; 32], nonce: &[u8; 24]) -> ([u8; 32], aead::Nonce) {
    let subkey = hchacha20(k, nonce[..16].try_into().expect("16 bytes"));
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&nonce[16..]);
    (subkey, aead::Nonce::assume_unique_for_key(n))
}

/// XChaCha20-Poly1305 of `plain`, with the tag appended: the seal of a
/// cookie, which only a responder makes.
pub(crate) fn xseal(k: &[u8; 32], nonce: &[u8; 24], plain: &[u8], ad: &[u8]) -> Vec<u8> {
    let (mut subkey, n) = xkey(k, nonce);
    let mut out = plain.to_vec();
    key(&subkey).seal_in_place_append_tag(n, aead::Aad::from(ad), &mut out).expect("ChaCha20-Poly1305 seals");
    subkey.zeroize();
    out
}

/// The plain bytes of an XChaCha20-Poly1305 `sealed`, or `None` when it
/// does not authenticate.
pub(crate) fn xopen(k: &[u8; 32], nonce: &[u8; 24], sealed: &[u8], ad: &[u8]) -> Option<Vec<u8>> {
    let (mut subkey, n) = xkey(k, nonce);
    let mut buf = sealed.to_vec();
    let plain = key(&subkey).open_in_place(n, aead::Aad::from(ad), &mut buf).ok().map(|p| p.to_vec());
    subkey.zeroize();
    plain
}

/// The TAI64N label of a time.
pub(crate) fn tai64n(time: SystemTime) -> [u8; 12] {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let mut out = [0u8; 12];
    out[..8].copy_from_slice(&(since.as_secs() + (1u64 << 62) + 10).to_be_bytes());
    out[8..].copy_from_slice(&since.subsec_nanos().to_be_bytes());
    out
}

/// The timestamps of the initiations of one initiator. The responder takes
/// an initiation only when its timestamp is greater than each one before
/// (the WireGuard paper, section 5.1). A stamp is the wall clock at the
/// start plus the monotonic time since then, so a step back of the wall
/// clock does not move it back; and each stamp is past the last one, also
/// for two initiations at one instant.
#[derive(Clone, Debug)]
pub struct Stamps {
    wall: SystemTime,
    start: Instant,
    last: Option<[u8; 12]>,
}

impl Stamps {
    /// Stamps that start at the wall time `wall`, read at the instant
    /// `start`.
    pub fn new(wall: SystemTime, start: Instant) -> Stamps {
        Stamps { wall, start, last: None }
    }

    /// The timestamp of an initiation at `now`.
    pub fn next(&mut self, now: Instant) -> [u8; 12] {
        let mut stamp = tai64n(self.wall + now.saturating_duration_since(self.start));
        if let Some(last) = self.last
            && stamp <= last
        {
            stamp = after(last);
        }
        self.last = Some(stamp);
        stamp
    }
}

/// The TAI64N label one nanosecond after `stamp`.
fn after(stamp: [u8; 12]) -> [u8; 12] {
    let secs = u64::from_be_bytes(stamp[..8].try_into().expect("8 bytes"));
    let nanos = u32::from_be_bytes(stamp[8..].try_into().expect("4 bytes"));
    let (secs, nanos) = if nanos + 1 >= 1_000_000_000 { (secs + 1, 0) } else { (secs, nanos + 1) };
    let mut out = [0u8; 12];
    out[..8].copy_from_slice(&secs.to_be_bytes());
    out[8..].copy_from_slice(&nanos.to_be_bytes());
    out
}

/// `N` random bytes. A source that fails is an error, never weaker bytes.
pub(crate) fn random<const N: usize>(entropy: &mut dyn Entropy) -> Result<[u8; N], Error> {
    let mut b = [0u8; N];
    entropy.fill(&mut b).map_err(|_| Error::NoRandom)?;
    Ok(b)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// A timestamp never goes back: not at one instant, not for an instant
    /// before the last one, and not when the wall clock steps back (the
    /// stamps read it once).
    #[test]
    fn stamps_only_go_forward() {
        let start = Instant::now();
        let mut stamps = Stamps::new(UNIX_EPOCH + Duration::from_secs(1_800_000_000), start);
        let first = stamps.next(start);
        let again = stamps.next(start);
        assert!(again > first, "{again:?} after {first:?}");
        let later = stamps.next(start + Duration::from_secs(5));
        assert!(later > again);
        assert_eq!(later[..8], tai64n(UNIX_EPOCH + Duration::from_secs(1_800_000_005))[..8]);
        let earlier = stamps.next(start);
        assert!(earlier > later, "an instant before the last one");
        // A nanosecond count at its top carries into the seconds. The stamp
        // is made from its bytes: the clock of Windows counts in steps of
        // 100 ns, so a `SystemTime` cannot hold 999_999_999 ns there.
        let mut top = tai64n(UNIX_EPOCH + Duration::from_secs(7));
        top[8..].copy_from_slice(&999_999_999u32.to_be_bytes());
        assert_eq!(after(top), tai64n(UNIX_EPOCH + Duration::from_secs(8)));
    }

    /// The vector of HChaCha20 of draft-irtf-cfrg-xchacha, section 2.2.1.
    #[test]
    fn hchacha20_vector() {
        let key: [u8; 32] = std::array::from_fn(|i| i as u8);
        let nonce = crate::blake2s::hex("000000090000004a0000000031415927");
        let subkey = hchacha20(&key, nonce[..].try_into().unwrap());
        assert_eq!(
            subkey.to_vec(),
            crate::blake2s::hex("82413b4227b27bfed30e42508a877d73a0f9e4d58a74a853c12ec41326d3ecdc")
        );
    }

    /// XChaCha20-Poly1305 over ring is the XChaCha20Poly1305 of RustCrypto,
    /// both ways, with associated data; a changed byte does not open.
    #[test]
    fn xchacha20poly1305_is_the_one_of_rustcrypto() {
        use chacha20poly1305::aead::{Aead, KeyInit, Payload};
        let key: [u8; 32] = std::array::from_fn(|i| 0x80 + i as u8);
        let nonce: [u8; 24] = std::array::from_fn(|i| 0x40 + i as u8);
        let other = chacha20poly1305::XChaCha20Poly1305::new(&key.into());
        for len in [0usize, 1, 16, 63, 64, 65, 114] {
            let plain: Vec<u8> = (0..len).map(|i| i as u8).collect();
            let sealed = xseal(&key, &nonce, &plain, b"ad");
            let theirs = other.encrypt(&nonce.into(), Payload { msg: &plain, aad: b"ad" }).unwrap();
            assert_eq!(sealed, theirs, "{len} bytes");
            assert_eq!(xopen(&key, &nonce, &theirs, b"ad"), Some(plain));
            let mut bad = sealed.clone();
            bad[0] ^= 1;
            assert_eq!(xopen(&key, &nonce, &bad, b"ad"), None);
            assert_eq!(xopen(&key, &nonce, &sealed, b"other"), None);
        }
    }

    #[test]
    fn a_dry_entropy_is_an_error() {
        struct Dry;
        impl Entropy for Dry {
            fn fill(&mut self, _: &mut [u8]) -> Result<(), pod_core::entropy::NoRandom> {
                Err(pod_core::entropy::NoRandom)
            }
        }
        assert!(matches!(random::<4>(&mut Dry), Err(Error::NoRandom)));
    }
}
