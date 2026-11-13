//! The wire of the DNS simplex carrier: the frames up in the labels of a
//! query name, the frames down in the bytes of TXT records. One datagram of
//! the ARQ goes up as `session(4) || counter(4) || payload`, base32 of RFC
//! 4648 (lower case, no padding) in labels of at most 63 characters under
//! the zone; the counter makes each name unique, so a resolver on the path
//! caches no answer (call C274). The payload is the ARQ datagram, or no byte
//! for a poll. The bytes down ride in TXT records, one datagram for each
//! record, with no base32 (call C275).

/// The lower-case alphabet of base32 (RFC 4648, section 6).
const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// The longest label (RFC 1035, section 3.1).
const LABEL: usize = 63;

/// The longest name in its wire form, its length octets and the root
/// included.
const MAX_NAME: usize = 255;

/// The bytes of the header of each query: the session and the counter.
pub(crate) const HEADER: usize = 8;

/// base32 of `bytes`, lower case, no padding.
fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    for chunk in bytes.chunks(5) {
        let mut buf = [0u8; 5];
        buf[..chunk.len()].copy_from_slice(chunk);
        let bits = u64::from_be_bytes([0, 0, 0, buf[0], buf[1], buf[2], buf[3], buf[4]]);
        let chars = (chunk.len() * 8).div_ceil(5);
        for i in 0..chars {
            let shift = 35 - 5 * i;
            out.push(ALPHABET[((bits >> shift) & 0x1f) as usize] as char);
        }
    }
    out
}

/// The bytes of a base32 string (lower or upper case, no padding). A
/// character that is not of the alphabet is an error.
fn base32_decode(text: &str) -> Result<Vec<u8>, String> {
    let value = |c: u8| -> Result<u64, String> {
        match c {
            b'a'..=b'z' => Ok(u64::from(c - b'a')),
            b'A'..=b'Z' => Ok(u64::from(c - b'A')),
            b'2'..=b'7' => Ok(u64::from(c - b'2' + 26)),
            _ => Err(format!("a base32 name holds the byte 0x{c:02x}")),
        }
    };
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    for chunk in text.as_bytes().chunks(8) {
        let mut bits = 0u64;
        for &c in chunk {
            bits = (bits << 5) | value(c)?;
        }
        let have = chunk.len() * 5;
        bits <<= 40 - have;
        for i in 0..have / 8 {
            out.push((bits >> (32 - 8 * i)) as u8);
        }
    }
    Ok(out)
}

/// The wire length of `zone` as a name (its labels, their length octets and
/// the root). A zone whose labels are not of RFC 1035 is an error.
fn zone_wire(zone: &str) -> Result<usize, String> {
    let zone = zone.trim_end_matches('.');
    if zone.is_empty() {
        return Err("the zone is empty".into());
    }
    let mut wire = 1;
    for label in zone.split('.') {
        if label.is_empty() || label.len() > LABEL {
            return Err(format!("the zone has the label {label:?}: give 1 to {LABEL} bytes"));
        }
        wire += 1 + label.len();
    }
    Ok(wire)
}

/// The most base32 characters that fit under a zone of `zone_wire` bytes,
/// with their length octets, in a name of [`MAX_NAME`] bytes.
fn max_chars(zone_wire: usize) -> usize {
    let avail = MAX_NAME.saturating_sub(zone_wire);
    let mut best = 0;
    let mut labels = 1;
    while labels < avail {
        best = best.max((LABEL * labels).min(avail - labels));
        labels += 1;
    }
    best
}

/// The largest payload of one datagram up under `zone`: the base32 budget
/// of the name less the header. A zone that leaves no room is an error.
pub(crate) fn payload_mtu(zone: &str) -> Result<usize, String> {
    let chars = max_chars(zone_wire(zone)?);
    let blob = chars * 5 / 8;
    blob.checked_sub(HEADER)
        .filter(|m| *m > 0)
        .ok_or_else(|| format!("the zone {zone:?} is too long for a DNS carrier"))
}

/// The query name for one datagram up: `session || counter || payload` as
/// base32 labels under `zone`. A payload over [`payload_mtu`] is an error.
pub(crate) fn encode_query(zone: &str, session: u32, counter: u32, payload: &[u8]) -> Result<String, String> {
    if payload.len() > payload_mtu(zone)? {
        return Err(format!("a datagram of {} bytes, over the carrier unit", payload.len()));
    }
    let mut blob = Vec::with_capacity(HEADER + payload.len());
    blob.extend_from_slice(&session.to_be_bytes());
    blob.extend_from_slice(&counter.to_be_bytes());
    blob.extend_from_slice(payload);
    let chars = base32_encode(&blob);
    let mut name = String::new();
    for label in chars.as_bytes().chunks(LABEL) {
        if !name.is_empty() {
            name.push('.');
        }
        name.push_str(std::str::from_utf8(label).expect("base32 is ASCII"));
    }
    name.push('.');
    name.push_str(zone.trim_end_matches('.'));
    Ok(name)
}

/// The session, the counter and the payload of a query `name` under `zone`.
/// A name that is not under the zone, or whose labels are no base32 of a
/// header, is an error.
pub(crate) fn decode_query(zone: &str, name: &str) -> Result<(u32, u32, Vec<u8>), String> {
    // DNS names are case insensitive, and a resolver may send mixed case.
    let zone = zone.trim_end_matches('.').to_ascii_lowercase();
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    let under = name
        .strip_suffix(zone.as_str())
        .and_then(|p| p.strip_suffix('.'))
        .ok_or_else(|| format!("the name {name:?} is not under the zone {zone:?}"))?;
    let chars: String = under.split('.').collect();
    let blob = base32_decode(&chars)?;
    if blob.len() < HEADER {
        return Err(format!("a query of {} bytes, under the header of {HEADER}", blob.len()));
    }
    let session = u32::from_be_bytes(blob[0..4].try_into().expect("four bytes"));
    let counter = u32::from_be_bytes(blob[4..8].try_into().expect("four bytes"));
    Ok((session, counter, blob[HEADER..].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_round_trips_and_is_case_insensitive() {
        for n in 0..40usize {
            let bytes: Vec<u8> = (0..n).map(|i| (i as u32 * 97 + 3) as u8).collect();
            let text = base32_encode(&bytes);
            assert!(text.bytes().all(|c| ALPHABET.contains(&c)));
            assert_eq!(base32_decode(&text).unwrap(), bytes, "n {n}");
            assert_eq!(base32_decode(&text.to_uppercase()).unwrap(), bytes, "upper n {n}");
        }
        assert!(base32_decode("ab0").is_err());
    }

    #[test]
    fn a_query_round_trips_under_its_zone() {
        let zone = "l.test";
        let mtu = payload_mtu(zone).unwrap();
        assert!(mtu >= 41, "{mtu}");
        let payload: Vec<u8> = (0..mtu as u32).map(|i| i as u8).collect();
        let name = encode_query(zone, 0x1234_5678, 42, &payload).unwrap();
        assert!(name.ends_with(".l.test") && name.len() <= 253);
        for label in name.split('.') {
            assert!(label.len() <= LABEL, "{label:?}");
        }
        assert_eq!(decode_query(zone, &name.to_uppercase()).unwrap(), (0x1234_5678, 42, payload));
        assert!(encode_query(zone, 1, 1, &vec![0u8; mtu + 1]).is_err());
        assert!(decode_query("other.test", &name).is_err());
        // A poll has no payload.
        let poll = encode_query(zone, 9, 1, &[]).unwrap();
        assert_eq!(decode_query(zone, &poll).unwrap(), (9, 1, Vec::new()));
    }

    #[test]
    fn a_long_zone_leaves_no_room() {
        let zone = (0..4).map(|_| "a".repeat(60)).collect::<Vec<_>>().join(".");
        assert!(payload_mtu(&zone).is_err(), "{zone}");
    }
}
