//! A `rustls::crypto::CryptoProvider` in pure Rust (feature `pure`), which
//! a binary gives to the configurations of this crate when it must build
//! with no C compiler.
//!
//! **The reason this module exists at all.** `ring` and `aws-lc-sys` both
//! need `cc`, and both fail under `CC=/nonexistent` (measured, each in
//! isolation). `rustls-rustcrypto` does not resolve at any version, so there
//! is **no** built-in pure-Rust provider to name.
//!
//! **A provider is not a capability until it has exchanged a certificate
//! with a real peer.** `tests/live_handshake.rs` completes a handshake with a
//! live relay, with a verified chain and hostname; `tests/tls12.rs` with
//! OpenSSL's `s_server` for each TLS 1.2 suite. Each part is checked against
//! published vectors (`tests/crypto_vectors.rs`, `tests/quic_keys.rs`,
//! `hpke_tests.rs`).
//!
//! RSA (feature `rsa`) verifies only, with public keys: RUSTSEC-2023-0071
//! concerns the operations of `rsa` 0.9 with a private key.

pub mod aead;
pub mod hash;
pub mod hkdf;
pub mod hmac;
// HPKE for Encrypted Client Hello, which rustls offers only from aws-lc.
pub mod hpke;
pub mod kx;
pub mod quic;
pub mod random;
#[cfg(feature = "rsa")]
pub mod rsa_sig;
pub mod sign;
pub mod suites;
pub mod tls12;

pub use suites::provider;

#[cfg(test)]
mod tests {
    use zeroize::ZeroizeOnDrop;

    fn wipes_its_key_when_dropped<T: ZeroizeOnDrop>() {}

    /// Each cipher that this provider builds from a session's key wipes the key,
    /// or its schedule, when it is dropped. RustCrypto's crates do so only with
    /// their `zeroize` feature (chacha20poly1305 since 0.11), so a lost feature
    /// is a build error here, not a key left in freed memory.
    #[test]
    fn each_cipher_wipes_its_key_when_dropped() {
        wipes_its_key_when_dropped::<chacha20poly1305::ChaCha20Poly1305>();
        wipes_its_key_when_dropped::<aes_gcm::Aes128Gcm>();
        wipes_its_key_when_dropped::<aes_gcm::Aes256Gcm>();
        wipes_its_key_when_dropped::<aes::Aes128>();
        wipes_its_key_when_dropped::<aes::Aes256>();
    }
}
