//! The PEM bundles of trust: a file of `CERTIFICATE` blocks, read with a
//! small reader of this crate.
//!
//! **A missing bundle is an error, with the path in the message.** It is
//! never a silent fallback to a compiled-in set and never an empty store: an
//! empty `RootCertStore` rejects every chain, and it does so with a message
//! that reads like a network fault.
//!
//! **A bundle next to a binary is found from the binary's own path, never
//! from `argv[0]`.** `argv[0]` is where the binary was *launched from*, not
//! where it *is*: a binary started through `PATH`, through a symlink in
//! `/usr/bin`, or by an absolute path from another directory resolves a
//! different bundle in each case. `std::env::current_exe` reads the path
//! through the platform's own mechanism on each target, and resolves
//! symlinks on Linux. The consumer finds the path and gives it
//! ([`crate::Settings::extra_bundle`]).

use std::path::Path;

use rustls_pki_types::CertificateDer;

/// **A missing file and an unparseable file are different failures** and a
/// doctor reports the path in both. The path is in the error because a
/// deployment that cannot find its trust store cannot be diagnosed from "TLS
/// failed".
pub fn load_bundle(path: &Path) -> Result<Vec<CertificateDer<'static>>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_bundle(path, &bytes)
}

/// **The count is checked, not dropped.** "Loaded 0 certificates" is a
/// different failure from "could not read the file", and a bundle that parses
/// to nothing is a configuration error that must never read as a loaded
/// bundle — an empty `RootCertStore` would reject every chain with a message
/// that looks like a network fault.
pub fn parse_bundle(path: &Path, bytes: &[u8]) -> Result<Vec<CertificateDer<'static>>, String> {
    let certs = pem_certificates(bytes).map_err(|why| format!("{}: {why}", path.display()))?;
    if certs.is_empty() {
        return Err(format!("{}: parsed as PEM but held no CERTIFICATE block", path.display()));
    }
    Ok(certs)
}

/// **A deliberately small PEM reader, written here rather than pulled in.**
/// A CA bundle is a sequence of `-----BEGIN CERTIFICATE-----` / base64 /
/// `-----END CERTIFICATE-----` blocks and nothing else: a dependency for
/// forty lines is not worth what a build pulls in. The decoder is asserted
/// byte-exactly against a known certificate in the tests.
///
/// It accepts ONLY `CERTIFICATE` blocks. A `PRIVATE KEY` block in a trust
/// bundle is a key in a file that is world-readable, and a parser that
/// silently ignored it would hide that.
pub fn pem_certificates(bytes: &[u8]) -> Result<Vec<CertificateDer<'static>>, String> {
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";

    let text = std::str::from_utf8(bytes).map_err(|_| "a PEM bundle must be UTF-8 text")?;
    let mut out = Vec::new();
    // **`\r` is stripped from every line before decoding.** A PEM file
    // written on Windows carries CRLF, and `body.trim()` removes it only at the
    // two ends of the block: each interior line keeps its `\r`, and the
    // decoder then refuses the whole bundle as "not valid base64".
    let normalised = text.replace("\r\n", "\n");
    let mut rest = normalised.as_str();

    while let Some(after_begin) = rest.find(BEGIN) {
        let body_start = after_begin + BEGIN.len();
        let Some(rel_end) = rest[body_start..].find(END) else {
            return Err("a BEGIN CERTIFICATE block has no matching END".into());
        };
        let body_end = body_start + rel_end;
        let body = &rest[body_start..body_end];

        // **Whitespace is removed before decoding.** A PEM body is one
        // base64 string wrapped at 64 columns, so it arrives with embedded
        // newlines, which the decoder refuses as "not valid base64".
        let der = base64_decode(&strip_whitespace(body))?;
        if der.is_empty() {
            return Err("a CERTIFICATE block is empty".into());
        }
        out.push(CertificateDer::from(der));
        rest = &rest[body_end + END.len()..];
    }
    Ok(out)
}

/// Every ASCII space character, and nothing else. Not `split_whitespace`,
/// which would also treat a non-ASCII byte as whitespace and let a mangled
/// bundle decode to something plausible.
fn strip_whitespace(input: &str) -> String {
    input.chars().filter(|c| !matches!(c, ' ' | '\t' | '\r' | '\n' | '\x0b' | '\x0c')).collect()
}

/// Standard base64 with padding, rejecting anything else. A trust bundle is
/// machine-generated, so an input that needs a lenient decoder is an input
/// that is not a trust bundle.
fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    const INVALID: &str = "not valid base64";
    if !input.len().is_multiple_of(4) {
        return Err(INVALID.into());
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    for chunk in input.as_bytes().as_chunks::<4>().0 {
        let hi = base64_value(chunk[0]).ok_or(INVALID)?;
        let mid = base64_value(chunk[1]).ok_or(INVALID)?;

        out.push((hi << 2) | (mid >> 4));

        // **The padding is checked BEFORE the sextets it pads are read.** A
        // decoder that reads all four characters first refuses each block
        // that holds `=`: each block whose length is not a multiple of three,
        // most real certificates.
        if chunk[2] == b'=' {
            // `xx==` is the last block of the input: padding may not appear
            // anywhere else, and a `=` here followed by more data is a bundle
            // that is not base64.
            if chunk[3] != b'=' {
                return Err(INVALID.into());
            }
            continue;
        }
        let lo = base64_value(chunk[2]).ok_or(INVALID)?;
        out.push((mid << 4) | (lo >> 2));

        if chunk[3] == b'=' {
            continue;
        }
        let last = base64_value(chunk[3]).ok_or(INVALID)?;
        out.push((lo << 6) | last);
    }
    Ok(out)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}
