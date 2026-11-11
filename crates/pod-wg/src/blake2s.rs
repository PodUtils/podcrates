//! BLAKE2s (RFC 7693), its HMAC and the KDF of WireGuard. ring has no
//! BLAKE2s, so it is here, with the vectors of the RFC and of the BLAKE2
//! reference.

const IV: [u32; 8] = [0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19];

const SIGMA: [[usize; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

fn compress(h: &mut [u32; 8], block: &[u8; 64], t: u64, last: bool) {
    let mut m = [0u32; 16];
    for (i, w) in m.iter_mut().enumerate() {
        *w = u32::from_le_bytes(block[i * 4..i * 4 + 4].try_into().expect("4 bytes"));
    }
    let mut v = [0u32; 16];
    v[..8].copy_from_slice(h);
    v[8..].copy_from_slice(&IV);
    v[12] ^= t as u32;
    v[13] ^= (t >> 32) as u32;
    if last {
        v[14] = !v[14];
    }
    fn g(v: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, x: u32, y: u32) {
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
        v[d] = (v[d] ^ v[a]).rotate_right(16);
        v[c] = v[c].wrapping_add(v[d]);
        v[b] = (v[b] ^ v[c]).rotate_right(12);
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
        v[d] = (v[d] ^ v[a]).rotate_right(8);
        v[c] = v[c].wrapping_add(v[d]);
        v[b] = (v[b] ^ v[c]).rotate_right(7);
    }
    for s in &SIGMA {
        g(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
        g(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
        g(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
        g(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
        g(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
        g(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
        g(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
        g(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
    }
    for i in 0..8 {
        h[i] ^= v[i] ^ v[i + 8];
    }
}

/// BLAKE2s with an output of `out_len` bytes (1 to 32) and a key of at most
/// 32 bytes, over the parts of `data` in order.
///
/// # Panics
///
/// When `out_len` or the key is out of its range: each caller gives a
/// constant.
pub(crate) fn blake2s(out_len: usize, key: &[u8], data: &[&[u8]]) -> Vec<u8> {
    assert!((1..=32).contains(&out_len) && key.len() <= 32);
    let mut h = IV;
    h[0] ^= 0x0101_0000 ^ ((key.len() as u32) << 8) ^ out_len as u32;
    let mut buf = Vec::with_capacity(64 + data.iter().map(|d| d.len()).sum::<usize>());
    if !key.is_empty() {
        let mut block = [0u8; 64];
        block[..key.len()].copy_from_slice(key);
        buf.extend_from_slice(&block);
    }
    for d in data {
        buf.extend_from_slice(d);
    }
    let mut t: u64 = 0;
    let mut chunks = buf.chunks(64).peekable();
    if chunks.peek().is_none() {
        compress(&mut h, &[0u8; 64], 0, true);
    }
    while let Some(chunk) = chunks.next() {
        let mut block = [0u8; 64];
        block[..chunk.len()].copy_from_slice(chunk);
        t += chunk.len() as u64;
        compress(&mut h, &block, t, chunks.peek().is_none());
    }
    let mut out = Vec::with_capacity(32);
    for w in h {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out.truncate(out_len);
    out
}

/// The HASH of WireGuard: BLAKE2s-256 with no key.
pub(crate) fn hash(data: &[&[u8]]) -> [u8; 32] {
    blake2s(32, &[], data).try_into().expect("32 bytes")
}

/// The MAC of WireGuard: keyed BLAKE2s-128.
pub(crate) fn mac(key: &[u8], data: &[&[u8]]) -> [u8; 16] {
    blake2s(16, key, data).try_into().expect("16 bytes")
}

/// HMAC with BLAKE2s-256 (a block of 64 bytes).
pub(crate) fn hmac(key: &[u8], data: &[&[u8]]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&hash(&[key]));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    let mut parts: Vec<&[u8]> = vec![&ipad];
    parts.extend_from_slice(data);
    let inner = hash(&parts);
    hash(&[&opad, &inner])
}

/// The KDF of WireGuard: `N` keys of 32 bytes from a chaining key and an
/// input.
pub(crate) fn kdf<const N: usize>(key: &[u8; 32], input: &[u8]) -> [[u8; 32]; N] {
    let t0 = hmac(key, &[input]);
    let mut out = [[0u8; 32]; N];
    let mut prev: Vec<u8> = Vec::new();
    for (i, o) in out.iter_mut().enumerate() {
        let t = hmac(&t0, &[&prev, &[i as u8 + 1]]);
        *o = t;
        prev = t.to_vec();
    }
    out
}

#[cfg(test)]
pub(crate) fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blake2s_vectors() {
        // RFC 7693, appendix B.
        assert_eq!(
            blake2s(32, &[], &[b"abc"]),
            hex("508c5e8c327c14e2e1a72ba34eeb452f37458b209ed63a294d999b4c86675982")
        );
        // The keyed vector of the BLAKE2 reference (key 00..1f, no input).
        let key: Vec<u8> = (0u8..32).collect();
        assert_eq!(blake2s(32, &key, &[]), hex("48a8997da407876b3d79c0d92325ad3b89cbb754d86ab71aee047ad345fd2c49"));
        // ... and with the input 00..3f (64 bytes: one whole block).
        let input: Vec<u8> = (0u8..64).collect();
        assert_eq!(
            blake2s(32, &key, &[&input]),
            hex("8975b0577fd35566d750b362b0897a26c399136df07bababbde6203ff2954ed4")
        );
        // The parts are one input.
        assert_eq!(blake2s(32, &[], &[b"a", b"bc"]), blake2s(32, &[], &[b"abc"]));
    }

    /// HMAC-BLAKE2s, the KDF, a long input and a keyed MAC of 16 bytes, as
    /// Python's hashlib and hmac compute them.
    #[test]
    fn hmac_and_kdf_vectors() {
        let key: [u8; 32] = (0u8..32).collect::<Vec<_>>().try_into().unwrap();
        let t0 = hmac(&key, &[b"WireGuard"]);
        assert_eq!(t0.to_vec(), hex("5415924543576b3f5ee867c94e1aab70586d33b32fae3fccd2c635b3e1d7198d"));
        let [k1, k2, k3] = kdf::<3>(&key, b"WireGuard");
        assert_eq!(k1.to_vec(), hex("7e615ce2573f4139b960d562ec463daf7164bd86a0a64c0adc374ff2655769f3"));
        assert_eq!(k2.to_vec(), hex("42dbbd4438e101cd2c2ea8d05ed6716bf87489aea47ece2f06358835670130bf"));
        assert_eq!(k3.to_vec(), hex("5c58c1d9808a5bfdf8f38f6487248985c04f4b6d0fe427516935a0f6903b0157"));
        let long: Vec<u8> = (0..768).map(|i| i as u8).collect();
        assert_eq!(blake2s(32, &[], &[&long]), hex("b928a17862e211e99759ba8819280803a914cd3dee7c5d711a3b5185aa96a7b3"));
        assert_eq!(blake2s(16, &[0u8; 32], &[b"mac1 input"]), hex("e3df99b695c09da1151bb0fd6cd4ea5b"));
    }
}
