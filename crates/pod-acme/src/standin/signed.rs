//! The check of a JWS POST, as a CA makes it (RFC 8555, sections 6.2 to
//! 6.5): the flattened JSON form and its three members alone, ES256, the
//! key (`jwk` for newAccount alone, else the `kid` of a known account), the
//! signature, a nonce that the stand-in gave and that no request used, and
//! the `url` of the request. The order of the checks is Pebble's: the
//! signature before the nonce, so that a forged request uses no nonce.

use base64::Engine as _;
use p256::ecdsa::signature::Verifier as _;
use serde_json::Value;

/// A problem of ACME: its type past `urn:ietf:params:acme:error:`, its
/// detail, its HTTP status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub kind: &'static str,
    pub detail: String,
    pub status: u16,
}

impl Problem {
    pub fn new(kind: &'static str, status: u16, detail: impl Into<String>) -> Problem {
        Problem { kind, detail: detail.into(), status }
    }

    pub fn malformed(detail: impl Into<String>) -> Problem {
        Problem::new("malformed", 400, detail)
    }

    pub fn unauthorized(detail: impl Into<String>) -> Problem {
        Problem::new("unauthorized", 403, detail)
    }
}

/// Who signed: a new key, given whole, or a known account by its number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signer {
    /// The point of a key given as a `jwk` (newAccount), and its x and y.
    Key {
        point: Vec<u8>,
        x: String,
        y: String,
    },
    Account(usize),
}

/// A JWS that passed each check.
#[derive(Debug, Clone)]
pub struct Signed {
    pub signer: Signer,
    /// The payload; `None` for the empty one of a POST-as-GET.
    pub payload: Option<Value>,
}

/// The facts of the stand-in that a check needs.
pub trait Keys {
    /// The point of the key of the account at `kid`, and its number.
    fn account(&self, kid: &str) -> Option<(usize, Vec<u8>)>;
    /// Take the nonce: true when the stand-in gave it and no request used it.
    fn take_nonce(&mut self, nonce: &str) -> bool;
}

/// Check the JWS `body` of a POST to `url`; `new_account` when `url` is
/// newAccount, whose key comes as a `jwk`.
pub fn check(body: &[u8], url: &str, new_account: bool, keys: &mut impl Keys) -> Result<Signed, Problem> {
    let jws: Value = serde_json::from_slice(body).map_err(|_| Problem::malformed("the body is not JSON"))?;
    let members = jws.as_object().ok_or_else(|| Problem::malformed("the body is not a JWS object"))?;
    if members.len() != 3 {
        return Err(Problem::malformed("a JWS has protected, payload and signature alone (RFC 8555, section 6.2)"));
    }
    let part = |name: &str| {
        jws[name].as_str().map(str::to_string).ok_or_else(|| Problem::malformed(format!("the JWS has no {name}")))
    };
    let (protected, payload, signature) = (part("protected")?, part("payload")?, part("signature")?);
    let header: Value = serde_json::from_slice(&b64(&protected)?)
        .map_err(|_| Problem::malformed("the protected header is not JSON"))?;
    if header["alg"] != "ES256" {
        return Err(Problem::new("badSignatureAlgorithm", 400, "the stand-in takes ES256 alone"));
    }
    let (signer, point) = match (header.get("jwk"), header.get("kid")) {
        (Some(_), Some(_)) => return Err(Problem::malformed("the header has jwk and kid: give one")),
        (Some(jwk), None) if new_account => jwk_key(jwk)?,
        (Some(_), None) => return Err(Problem::malformed("a request after newAccount names its account by kid")),
        (None, Some(_)) if new_account => return Err(Problem::malformed("newAccount takes the key as a jwk")),
        (None, Some(kid)) => {
            let kid = kid.as_str().ok_or_else(|| Problem::malformed("the kid is not a string"))?;
            let (number, point) = keys
                .account(kid)
                .ok_or_else(|| Problem::new("accountDoesNotExist", 400, format!("no account at {kid}")))?;
            (Signer::Account(number), point)
        }
        (None, None) => return Err(Problem::malformed("the header has no jwk and no kid")),
    };
    let verifying = p256::ecdsa::VerifyingKey::from_sec1_bytes(&point)
        .map_err(|_| Problem::malformed("the key is not a P-256 point"))?;
    let raw = b64(&signature)?;
    let fixed = p256::ecdsa::Signature::from_slice(&raw)
        .map_err(|_| Problem::malformed("the signature is not r and s of 32 bytes each"))?;
    if verifying.verify(format!("{protected}.{payload}").as_bytes(), &fixed).is_err() {
        return Err(Problem::malformed("the JWS signature does not verify"));
    }
    let nonce = header["nonce"].as_str().unwrap_or_default();
    if !keys.take_nonce(nonce) {
        return Err(Problem::new(
            "badNonce",
            400,
            format!("the nonce {nonce:?} is not one that the stand-in gave, or it was used"),
        ));
    }
    if header["url"].as_str() != Some(url) {
        return Err(Problem::unauthorized(format!("the url of the header is {}, not {url}", header["url"])));
    }
    let payload = match payload.as_str() {
        "" => None,
        text => Some(serde_json::from_slice(&b64(text)?).map_err(|_| Problem::malformed("the payload is not JSON"))?),
    };
    Ok(Signed { signer, payload })
}

/// The key of a `jwk`: EC, P-256, x and y of 32 bytes each.
fn jwk_key(jwk: &Value) -> Result<(Signer, Vec<u8>), Problem> {
    if jwk["kty"] != "EC" || jwk["crv"] != "P-256" {
        return Err(Problem::new("badPublicKey", 400, "the stand-in takes an EC key of P-256 alone"));
    }
    let coordinate = |name: &str| -> Result<(String, Vec<u8>), Problem> {
        let text =
            jwk[name].as_str().ok_or_else(|| Problem::new("badPublicKey", 400, format!("the jwk has no {name}")))?;
        let bytes = b64(text)?;
        if bytes.len() != 32 {
            return Err(Problem::new("badPublicKey", 400, format!("{name} of the jwk is not 32 bytes")));
        }
        Ok((text.to_string(), bytes))
    };
    let ((x, xb), (y, yb)) = (coordinate("x")?, coordinate("y")?);
    let mut point = vec![0x04];
    point.extend(xb);
    point.extend(yb);
    Ok((Signer::Key { point: point.clone(), x, y }, point))
}

/// Base64url with no padding, as JOSE writes each part.
fn b64(text: &str) -> Result<Vec<u8>, Problem> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| Problem::malformed("a part of the JWS is not base64url with no padding"))
}
