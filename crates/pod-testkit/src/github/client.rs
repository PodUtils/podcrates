//! The App side of a fake GitHub: the calls that mint a JIT runner (the App
//! JWT, the installation token, `generate-jitconfig`) and the files of the
//! runner, decoded from `encoded_jit_config`. `pod-fake-githome serve
//! --github URL` mints with it, and a test acts as the App with it.

use std::io;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use super::{APP_ID, Key, now};
use crate::stand::call;

/// A JIT runner of a repository.
#[derive(Debug, Clone)]
pub struct JitRunner {
    /// The id of the runner in the fake.
    pub id: u64,
    /// The files of the runner (`.runner`, `.credentials`,
    /// `.credentials_rsaparams`): the name and the text of each, in the
    /// order of their names.
    pub files: Vec<(String, String)>,
}

/// One call with an optional bearer credential and a JSON body (none for
/// `Null`); the status and the JSON of the answer (`Null` for no JSON).
pub fn call_json(method: &str, url: &str, token: Option<&str>, body: &Value) -> io::Result<(u16, Value)> {
    let auth = token.map(|t| format!("Bearer {t}"));
    let mut headers = vec![("Content-Type", "application/json")];
    if let Some(a) = &auth {
        headers.push(("Authorization", a.as_str()));
    }
    let raw = if body.is_null() { Vec::new() } else { body.to_string().into_bytes() };
    let (status, out) = call(method, url, &headers, &raw)?;
    Ok((status, serde_json::from_slice(&out).unwrap_or(Value::Null)))
}

/// A call of [`call_json`] that must answer `want`: else the method, the
/// URL, the status and the answer as the error.
pub fn call_expect(method: &str, url: &str, token: Option<&str>, body: &Value, want: u16) -> Result<Value, String> {
    match call_json(method, url, token, body) {
        Ok((status, v)) if status == want => Ok(v),
        Ok((status, v)) => Err(format!("{method} {url}: status {status}: {v}")),
        Err(e) => Err(format!("{method} {url}: {e}")),
    }
}

/// The key of the App of the fake at `base` (`http://ADDR/`), from
/// `GET /_fake/app`.
pub fn app_key(base: &str) -> Result<Key, String> {
    let app = call_expect("GET", &format!("{base}_fake/app"), None, &Value::Null, 200)?;
    Key::from_pem(app["private_key"].as_str().unwrap_or_default())
}

/// The App with `key`: its JWT, the installation of `repo` (`OWNER/REPO`),
/// and a new installation token.
pub fn installation_token(base: &str, key: &Key, repo: &str) -> Result<String, String> {
    let t = now();
    let jwt = key.jwt(&json!({ "iat": t - 30, "exp": t + 540, "iss": APP_ID.to_string() }));
    let inst = call_expect("GET", &format!("{base}repos/{repo}/installation"), Some(&jwt), &Value::Null, 200)?;
    let url = inst["access_tokens_url"].as_str().ok_or("the installation has no access_tokens_url")?;
    let token = call_expect("POST", url, Some(&jwt), &Value::Null, 201)?;
    token["token"].as_str().map(str::to_string).ok_or_else(|| "the answer has no token".to_string())
}

/// A JIT runner of `repo` named `name` with `labels`, minted with the
/// installation token `token`.
pub fn jit_runner(base: &str, token: &str, repo: &str, name: &str, labels: &[String]) -> Result<JitRunner, String> {
    let url = format!("{base}repos/{repo}/actions/runners/generate-jitconfig");
    let body = json!({ "name": name, "runner_group_id": 1, "labels": labels });
    let v = call_expect("POST", &url, Some(token), &body, 201)?;
    let id = v["runner"]["id"].as_u64().ok_or("the answer has no runner id")?;
    let bad = |what: &str| format!("the encoded_jit_config is not {what}");
    let raw = STANDARD.decode(v["encoded_jit_config"].as_str().unwrap_or_default()).map_err(|_| bad("base64"))?;
    let Ok(Value::Object(map)) = serde_json::from_slice(&raw) else { return Err(bad("a JSON object")) };
    let mut files = Vec::with_capacity(map.len());
    for (name, value) in map {
        let bytes = STANDARD.decode(value.as_str().unwrap_or_default()).map_err(|_| bad("base64 in each file"))?;
        files.push((name, String::from_utf8(bytes).map_err(|_| bad("UTF-8 in each file"))?));
    }
    Ok(JitRunner { id, files })
}
