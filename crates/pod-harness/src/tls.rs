//! A TLS ClientHello, its JA3 and its JA4 fingerprint.
//!
//! The parse reads the first TLS handshake record of a capture. It follows
//! RFC 8446 for the layout and the JA3 and JA4 specifications for the
//! fingerprints. A GREASE value (RFC 8701) is left out of each list and
//! never counted.

use crate::error::{Error, Result};

/// The content type of a TLS handshake record.
const HANDSHAKE: u8 = 0x16;
/// The content type of a TLS change cipher spec record.
const CHANGE_CIPHER_SPEC: u8 = 0x14;
/// The content type of a TLS alert record.
const ALERT: u8 = 0x15;
/// The content type of a TLS application data record.
const APPLICATION_DATA: u8 = 0x17;

/// The fields of a ClientHello that the fingerprints use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientHello {
    pub legacy_version: u16,
    pub ciphers: Vec<u16>,
    pub extensions: Vec<u16>,
    pub groups: Vec<u16>,
    pub point_formats: Vec<u8>,
    pub signature_algorithms: Vec<u16>,
    pub supported_versions: Vec<u16>,
    pub sni: Option<String>,
    /// Each ALPN protocol as its bytes. A protocol is not text: a byte
    /// that is not UTF-8 is part of the JA4 fingerprint.
    pub alpn: Vec<Vec<u8>>,
}

impl ClientHello {
    /// The first record of `bytes` as a ClientHello. Bytes after the first
    /// handshake message are ignored, so a padded record reads.
    pub fn parse_record(bytes: &[u8]) -> Result<ClientHello> {
        if bytes.len() < 5 {
            return Err(Error::new("a TLS record is at least 5 bytes"));
        }
        match bytes[0] {
            HANDSHAKE => {}
            CHANGE_CIPHER_SPEC | ALERT | APPLICATION_DATA => {
                return Err(Error::new("the first record is not a TLS handshake"));
            }
            other => return Err(Error::new(format!("0x{other:02x} is not a TLS content type"))),
        }
        let length = u16::from_be_bytes([bytes[3], bytes[4]]) as usize;
        if bytes.len() < 5 + length {
            return Err(Error::new(format!(
                "the TLS record names {length} bytes and the capture holds {}",
                bytes.len().saturating_sub(5)
            )));
        }
        let payload = &bytes[5..5 + length];
        if payload.len() < 4 {
            return Err(Error::new("the handshake header is at least 4 bytes"));
        }
        if payload[0] != 0x01 {
            return Err(Error::new(format!("handshake type 0x{:02x} is not a ClientHello", payload[0])));
        }
        let body_length = u32::from_be_bytes([0, payload[1], payload[2], payload[3]]) as usize;
        if payload.len() < 4 + body_length {
            return Err(Error::new("the ClientHello body is shorter than its length"));
        }
        ClientHello::parse_body(&payload[4..4 + body_length])
    }

    fn parse_body(body: &[u8]) -> Result<ClientHello> {
        let mut cursor = Cursor::new(body);
        let legacy_version = cursor.u16()?;
        cursor.skip(32)?;
        let session = cursor.u8()? as usize;
        cursor.skip(session)?;
        let cipher_length = cursor.u16()? as usize;
        if !cipher_length.is_multiple_of(2) {
            return Err(Error::new("the cipher suite list has an odd length"));
        }
        let mut ciphers = Vec::with_capacity(cipher_length / 2);
        for _ in 0..cipher_length / 2 {
            ciphers.push(cursor.u16()?);
        }
        let compression = cursor.u8()? as usize;
        cursor.skip(compression)?;
        let mut hello = ClientHello { legacy_version, ciphers, ..ClientHello::default() };
        if cursor.remaining() == 0 {
            return Ok(hello);
        }
        let extensions_length = cursor.u16()? as usize;
        if cursor.remaining() < extensions_length {
            return Err(Error::new("the extension list is shorter than its length"));
        }
        let mut extensions = Cursor::new(cursor.take(extensions_length)?);
        while extensions.remaining() > 0 {
            let kind = extensions.u16()?;
            let length = extensions.u16()? as usize;
            let data = extensions.take(length)?;
            hello.extensions.push(kind);
            hello.read_extension(kind, data)?;
        }
        Ok(hello)
    }

    fn read_extension(&mut self, kind: u16, data: &[u8]) -> Result<()> {
        match kind {
            0x0000 => self.sni = Some(read_sni(data)?),
            0x0010 => self.alpn = read_alpn(data)?,
            0x000a => self.groups = read_u16_list(data)?,
            0x000b => self.point_formats = read_u8_list(data)?,
            0x000d => self.signature_algorithms = read_u16_list(data)?,
            0x002b => {
                self.supported_versions =
                    read_u8_list(data)?.as_chunks::<2>().0.iter().map(|pair| u16::from_be_bytes(*pair)).collect()
            }
            _ => {}
        }
        Ok(())
    }

    /// The JA3 string: version, ciphers, extensions, groups and point
    /// formats, in the order that they appear, with no GREASE value.
    pub fn ja3_string(&self) -> String {
        let list = |values: &[u16]| {
            values.iter().filter(|v| !is_grease(**v)).map(u16::to_string).collect::<Vec<_>>().join("-")
        };
        let points = self.point_formats.iter().map(u8::to_string).collect::<Vec<_>>().join("-");
        format!(
            "{},{},{},{},{}",
            self.legacy_version,
            list(&self.ciphers),
            list(&self.extensions),
            list(&self.groups),
            points
        )
    }

    /// The MD5 (RFC 1321) of the JA3 string, in lower case hexadecimal.
    pub fn ja3(&self) -> String {
        format!("{:x}", md5::compute(self.ja3_string().as_bytes()))
    }

    /// The JA4 fingerprint, as `a_b_c`.
    pub fn ja4(&self) -> String {
        format!("{}_{}_{}", self.ja4_a(), self.ja4_b(), self.ja4_c())
    }

    /// The readable part: version, SNI, counts and ALPN.
    pub fn ja4_a(&self) -> String {
        let version = self.supported_versions.iter().copied().filter(|v| !is_grease(*v)).max();
        let code = version_code(version.unwrap_or(self.legacy_version));
        let sni = if self.sni.as_deref().is_some_and(|s| !s.is_empty()) { 'd' } else { 'i' };
        let ciphers = self.ciphers.iter().filter(|v| !is_grease(**v)).count().min(99);
        let extensions = self.extensions.iter().filter(|v| !is_grease(**v)).count().min(99);
        let alpn = alpn_token(self.alpn.first().map(Vec::as_slice));
        format!("t{code}{sni}{ciphers:02}{extensions:02}{alpn}")
    }

    /// The truncated SHA-256 of the sorted cipher list.
    pub fn ja4_b(&self) -> String {
        let mut ciphers: Vec<u16> = self.ciphers.iter().copied().filter(|v| !is_grease(*v)).collect();
        if ciphers.is_empty() {
            return "000000000000".to_string();
        }
        ciphers.sort_unstable();
        let text = ciphers.iter().map(|v| format!("{v:04x}")).collect::<Vec<_>>().join(",");
        truncated_sha256(&text)
    }

    /// The truncated SHA-256 of the sorted extensions, less SNI and ALPN,
    /// followed by the signature algorithms in their order.
    pub fn ja4_c(&self) -> String {
        let mut extensions: Vec<u16> =
            self.extensions.iter().copied().filter(|v| !is_grease(*v) && *v != 0x0000 && *v != 0x0010).collect();
        if extensions.is_empty() {
            return "000000000000".to_string();
        }
        extensions.sort_unstable();
        let mut text = extensions.iter().map(|v| format!("{v:04x}")).collect::<Vec<_>>().join(",");
        let signatures: Vec<u16> = self.signature_algorithms.iter().copied().filter(|v| !is_grease(*v)).collect();
        if !signatures.is_empty() {
            text.push('_');
            text.push_str(&signatures.iter().map(|v| format!("{v:04x}")).collect::<Vec<_>>().join(","));
        }
        truncated_sha256(&text)
    }
}

/// A GREASE value: two equal bytes, the low nibble 0xa (RFC 8701).
pub fn is_grease(value: u16) -> bool {
    (value >> 8) == (value & 0xff) && (value & 0x0f) == 0x0a
}

/// The two characters of a JA4 version.
pub fn version_code(version: u16) -> &'static str {
    match version {
        0x0304 => "13",
        0x0303 => "12",
        0x0302 => "11",
        0x0301 => "10",
        0x0300 => "s3",
        0x0002 => "s2",
        0xfeff => "d1",
        0xfefd => "d2",
        0xfefc => "d3",
        _ => "00",
    }
}

/// The two characters of the first ALPN value. Bytes that are not
/// alphanumeric give the first and last character of their hex form.
pub fn alpn_token(value: Option<&[u8]>) -> String {
    let Some(bytes) = value else {
        return "00".to_string();
    };
    if bytes.is_empty() {
        return "00".to_string();
    }
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    if first.is_ascii_alphanumeric() && last.is_ascii_alphanumeric() {
        return format!("{}{}", first as char, last as char);
    }
    let hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let first = hex.chars().next().unwrap_or('0');
    let last = hex.chars().last().unwrap_or('0');
    format!("{first}{last}")
}

fn truncated_sha256(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(text.as_bytes());
    crate::capture::encode_hex(&digest)[..12].to_string()
}

fn read_sni(data: &[u8]) -> Result<String> {
    let mut cursor = Cursor::new(data);
    let length = cursor.u16()? as usize;
    let mut names = Cursor::new(cursor.take(length)?);
    let kind = names.u8()?;
    if kind != 0 {
        return Err(Error::new("the first SNI name is not a host name"));
    }
    let length = names.u16()? as usize;
    Ok(String::from_utf8_lossy(names.take(length)?).into_owned())
}

fn read_alpn(data: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut cursor = Cursor::new(data);
    let length = cursor.u16()? as usize;
    let mut list = Cursor::new(cursor.take(length)?);
    let mut out = Vec::new();
    while list.remaining() > 0 {
        let length = list.u8()? as usize;
        out.push(list.take(length)?.to_vec());
    }
    Ok(out)
}

fn read_u16_list(data: &[u8]) -> Result<Vec<u16>> {
    let mut cursor = Cursor::new(data);
    let length = cursor.u16()? as usize;
    if !length.is_multiple_of(2) {
        return Err(Error::new("a list of 16-bit values has an odd length"));
    }
    let mut list = Cursor::new(cursor.take(length)?);
    let mut out = Vec::new();
    while list.remaining() > 0 {
        out.push(list.u16()?);
    }
    Ok(out)
}

fn read_u8_list(data: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = Cursor::new(data);
    let length = cursor.u8()? as usize;
    Ok(cursor.take(length)?.to_vec())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Cursor<'a> {
        Cursor { bytes, at: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        if self.remaining() < count {
            return Err(Error::new(format!("a field names {count} bytes and {} remain", self.remaining())));
        }
        let out = &self.bytes[self.at..self.at + count];
        self.at += count;
        Ok(out)
    }

    fn skip(&mut self, count: usize) -> Result<()> {
        self.take(count).map(|_| ())
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ClientHello with the published lists of the JA4 example: 15 cipher
    /// suites, 16 extensions, the SNI and the ALPN `h2`.
    fn published_hello() -> ClientHello {
        ClientHello {
            legacy_version: 0x0303,
            ciphers: vec![
                0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013, 0xc014, 0x009c, 0x009d,
                0x002f, 0x0035,
            ],
            extensions: vec![
                0x001b, 0x0000, 0x0033, 0x0010, 0x4469, 0x0017, 0x002d, 0x000d, 0x0005, 0x0023, 0x0012, 0x002b, 0xff01,
                0x000b, 0x000a, 0x0015,
            ],
            groups: vec![0x001d, 0x0017],
            point_formats: vec![0],
            signature_algorithms: vec![0x0403, 0x0804, 0x0401, 0x0503, 0x0805, 0x0501, 0x0806, 0x0601],
            supported_versions: vec![0x0304],
            sni: Some("example.com".to_string()),
            alpn: vec![b"h2".to_vec()],
        }
    }

    fn record(hello: &ClientHello) -> Vec<u8> {
        let mut ext = Vec::new();
        for &kind in &hello.extensions {
            let data = match kind {
                0x0000 => {
                    let name = hello.sni.as_deref().unwrap_or("").as_bytes();
                    let mut d = Vec::new();
                    d.extend_from_slice(&((name.len() + 3) as u16).to_be_bytes());
                    d.push(0);
                    d.extend_from_slice(&(name.len() as u16).to_be_bytes());
                    d.extend_from_slice(name);
                    d
                }
                0x0010 => {
                    let name = hello.alpn.first().map(Vec::as_slice).unwrap_or(b"");
                    let mut d = Vec::new();
                    d.extend_from_slice(&((name.len() + 1) as u16).to_be_bytes());
                    d.push(name.len() as u8);
                    d.extend_from_slice(name);
                    d
                }
                0x000a => {
                    let mut d = Vec::new();
                    d.extend_from_slice(&(hello.groups.len() as u16 * 2).to_be_bytes());
                    for g in &hello.groups {
                        d.extend_from_slice(&g.to_be_bytes());
                    }
                    d
                }
                0x000b => {
                    let mut d = vec![hello.point_formats.len() as u8];
                    d.extend_from_slice(&hello.point_formats);
                    d
                }
                0x000d => {
                    let mut d = Vec::new();
                    d.extend_from_slice(&(hello.signature_algorithms.len() as u16 * 2).to_be_bytes());
                    for s in &hello.signature_algorithms {
                        d.extend_from_slice(&s.to_be_bytes());
                    }
                    d
                }
                0x002b => {
                    let mut d = vec![hello.supported_versions.len() as u8 * 2];
                    for v in &hello.supported_versions {
                        d.extend_from_slice(&v.to_be_bytes());
                    }
                    d
                }
                _ => Vec::new(),
            };
            ext.extend_from_slice(&kind.to_be_bytes());
            ext.extend_from_slice(&(data.len() as u16).to_be_bytes());
            ext.extend_from_slice(&data);
        }
        let mut body = Vec::new();
        body.extend_from_slice(&hello.legacy_version.to_be_bytes());
        body.extend_from_slice(&[0x11; 32]);
        body.push(0);
        body.extend_from_slice(&(hello.ciphers.len() as u16 * 2).to_be_bytes());
        for c in &hello.ciphers {
            body.extend_from_slice(&c.to_be_bytes());
        }
        body.push(1);
        body.push(0);
        body.extend_from_slice(&(ext.len() as u16).to_be_bytes());
        body.extend_from_slice(&ext);
        let mut handshake = vec![0x01];
        let len = body.len() as u32;
        handshake.extend_from_slice(&[(len >> 16) as u8, (len >> 8) as u8, len as u8]);
        handshake.extend_from_slice(&body);
        let mut out = vec![0x16, 0x03, 0x01];
        out.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
        out.extend_from_slice(&handshake);
        out
    }

    #[test]
    fn the_published_ja4_example_is_reproduced() {
        // JA4 specification, "Example": t13d1516h2_8daaf6152771_e5627efa2ab1.
        // The inputs are the 15 ciphers, the 16 extensions and the 8
        // signature algorithms of that example, in its order.
        let hello = published_hello();
        assert_eq!(hello.ja4(), "t13d1516h2_8daaf6152771_e5627efa2ab1");
        assert_eq!(hello.ja4_b(), "8daaf6152771");
        assert_eq!(hello.ja4_c(), "e5627efa2ab1");
    }

    #[test]
    fn the_ja3_string_and_its_md5_cover_the_specification() {
        // The JA3 inputs, from the JA3 specification: version 771, the
        // ciphers in order, the extensions in order, the groups and the
        // point formats, as decimal values with no GREASE.
        // 771,4865-4866-4867-49195-49199-49196-49200-52393-52392-49171-
        // 49172-156-157-47-53,27-0-51-16-17513-23-45-13-5-35-18-43-
        // 65281-11-10-21,29-23,0
        // The expected MD5 is the MD5 of that exact string, computed with
        // `printf '%s' '...' | md5sum` (RFC 1321), not with this crate.
        let hello = published_hello();
        assert_eq!(
            hello.ja3_string(),
            "771,4865-4866-4867-49195-49199-49196-49200-52393-52392-49171-49172-156-157-47-53,27-0-51-16-17513-23-45-13-5-35-18-43-65281-11-10-21,29-23,0"
        );
        assert_eq!(hello.ja3(), "c7240d90981bb4a546a859dd3bd5457c");
    }

    #[test]
    fn the_parse_reads_the_fields_of_a_record() {
        let hello = published_hello();
        let bytes = record(&hello);
        let parsed = ClientHello::parse_record(&bytes).unwrap();
        assert_eq!(parsed, hello);
        assert_eq!(parsed.sni.as_deref(), Some("example.com"));
        assert_eq!(parsed.alpn, vec![b"h2".to_vec()]);
    }

    #[test]
    fn grease_is_not_counted_and_not_hashed() {
        let mut hello = published_hello();
        hello.ciphers.insert(0, 0x0a0a);
        hello.extensions.insert(0, 0x1a1a);
        hello.groups.insert(0, 0x2a2a);
        assert_eq!(hello.ja4_a(), "t13d1516h2");
        assert!(hello.ja3_string().starts_with("771,4865-"));
        assert!(!hello.ja3_string().contains("2570"));
        // The published hash is of the lists with no GREASE.
        assert_eq!(hello.ja4_b(), "8daaf6152771");
    }

    #[test]
    fn the_alpn_token_uses_the_first_and_last_character() {
        assert_eq!(alpn_token(Some(b"h2")), "h2");
        assert_eq!(alpn_token(Some(b"http/1.1")), "h1");
        assert_eq!(alpn_token(Some(b"x")), "xx");
        assert_eq!(alpn_token(None), "00");
        assert_eq!(alpn_token(Some(b"")), "00");
        assert_eq!(alpn_token(Some(&[0xab])), "ab");
        assert_eq!(alpn_token(Some(&[0xab, 0xcd])), "ad");
    }

    #[test]
    fn a_malformed_record_is_an_error() {
        assert!(ClientHello::parse_record(&[]).is_err());
        assert!(ClientHello::parse_record(&[0x17, 0x03, 0x03, 0, 1, 0]).is_err());
        assert!(ClientHello::parse_record(&[0x16, 0x03, 0x01, 0, 5, 0x01, 0, 0, 0, 0]).is_err());
        let mut bytes = record(&published_hello());
        bytes.truncate(20);
        assert!(ClientHello::parse_record(&bytes).is_err());
    }
}
