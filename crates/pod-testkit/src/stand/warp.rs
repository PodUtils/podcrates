//! A stand-in of WARP's registration API, from podssh's test of the
//! registration: a device at `POST /reg`, its MASQUE key at
//! `PATCH /reg/ID`, its record at `GET /reg/ID`, its account at
//! `GET` and `PUT /reg/ID/account`, its removal at `DELETE /reg/ID`, each
//! but the first with the device's token. The record gives the front door
//! and its key that the test sets, as the API gives a device its edge; a
//! test can change the key, as WARP's front door showed a new device
//! another key than its record's.
//!
//! The handler goes to [`super::http`] (plain HTTP on the loopback) or
//! [`super::Stand::http_over`] (with the caller's TLS). The tokens are
//! made-up values of the stand-in.

use std::sync::{Arc, Mutex};

use super::{Answer, Request, lock};

/// One device of the stand-in.
#[derive(Debug, Clone)]
struct Device {
    id: String,
    token: String,
    /// The enrolled key: the base64 SPKI of the last `PATCH`, or the X25519
    /// key of the `POST`.
    key: String,
    tunnel_type: String,
    license: String,
}

#[derive(Debug, Default)]
struct State {
    devices: Vec<Device>,
    /// The `PUBLIC KEY` PEM of the front door in each record.
    peer_pem: String,
    /// The front doors of each record, `ADDR:0` and `[ADDR]:0`.
    v4: String,
    v6: String,
    /// The status that answers each `POST` in place of a device.
    post_status: Option<u16>,
    /// Each request: its method, its path and its body.
    asked: Vec<(String, String, String)>,
}

/// The stand-in API.
#[derive(Debug, Clone, Default)]
pub struct Api {
    state: Arc<Mutex<State>>,
}

/// A string as a JSON string.
fn json(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The string value of `"name":"..."` in a JSON body that a client wrote
/// with no space around the colon.
fn field(body: &str, name: &str) -> Option<String> {
    let start = body.find(&format!("\"{name}\":\""))? + name.len() + 4;
    let end = body[start..].find('"')?;
    Some(body[start..start + end].to_string())
}

impl Api {
    /// A stand-in whose records name the front door `v4` and `v6` (each an
    /// address, or empty) with the key `peer_pem`.
    pub fn new(peer_pem: &str, v4: &str, v6: &str) -> Api {
        let state = State {
            peer_pem: peer_pem.to_string(),
            v4: if v4.is_empty() { String::new() } else { format!("{v4}:0") },
            v6: if v6.is_empty() { String::new() } else { format!("[{v6}]:0") },
            ..State::default()
        };
        Api { state: Arc::new(Mutex::new(state)) }
    }

    /// The key of the front door that each record gives from now on.
    pub fn serve_key(&self, peer_pem: &str) {
        lock(&self.state).peer_pem = peer_pem.to_string();
    }

    /// Answer each `POST` with `status` in place of a device (`None`: a
    /// device).
    pub fn fail_post(&self, status: Option<u16>) {
        lock(&self.state).post_status = status;
    }

    /// The requests of `method` that it took.
    pub fn asked(&self, method: &str) -> usize {
        lock(&self.state).asked.iter().filter(|(m, _, _)| m == method).count()
    }

    /// The bodies of the requests of `method` that it took, in order.
    pub fn bodies(&self, method: &str) -> Vec<String> {
        lock(&self.state).asked.iter().filter(|(m, _, _)| m == method).map(|(_, _, b)| b.clone()).collect()
    }

    /// The devices that it holds.
    pub fn devices(&self) -> usize {
        lock(&self.state).devices.len()
    }

    /// The enrolled keys (base64 SPKI) of the devices of the tunnel type
    /// `masque`.
    pub fn enrolled(&self) -> Vec<String> {
        let state = lock(&self.state);
        state.devices.iter().filter(|d| d.tunnel_type == "masque").map(|d| d.key.clone()).collect()
    }

    /// The handler of the stand-in.
    pub fn handler(&self) -> impl Fn(&Request) -> Answer + Send + Sync + 'static {
        let api = self.clone();
        move |request| api.answer(request)
    }

    fn record(state: &State, device: &Device, with_token: bool) -> String {
        let token = if with_token { format!(",\"token\":{}", json(&device.token)) } else { String::new() };
        format!(
            "{{\"id\":{id},\"key\":{key},\"key_type\":\"secp256r1\",\"tunnel_type\":{tt}{token},\
             \"account\":{{\"id\":\"account-{id_raw}\",\"account_type\":\"free\",\"license\":{lic}}},\
             \"config\":{{\"client_id\":\"AQID\",\"peers\":[{{\"public_key\":{pem},\
             \"endpoint\":{{\"v4\":{v4},\"v6\":{v6}}}}}],\
             \"interface\":{{\"addresses\":{{\"v4\":\"172.16.0.2\",\"v6\":\"2606:4700:110:8000::2\"}}}}}}}}",
            id = json(&device.id),
            id_raw = device.id,
            key = json(&device.key),
            tt = json(&device.tunnel_type),
            lic = json(&device.license),
            pem = json(&state.peer_pem),
            v4 = json(&state.v4),
            v6 = json(&state.v6),
        )
    }

    fn answer(&self, request: &Request) -> Answer {
        let mut state = lock(&self.state);
        let path = request.path().to_string();
        let body = String::from_utf8_lossy(&request.body).into_owned();
        state.asked.push((request.method.clone(), path.clone(), body.clone()));
        let json_answer =
            |status: u16, text: String| Answer::new(status, text).header("Content-Type", "application/json");
        // The path after the version: /v0a.../reg[/ID[/account]].
        let mut parts = path.split('/').skip(2);
        if parts.next() != Some("reg") {
            return Answer::new(404, "no such path");
        }
        let id = parts.next().map(str::to_string);
        let account = parts.next() == Some("account");
        let Some(id) = id else {
            if request.method != "POST" {
                return Answer::new(405, "POST /reg");
            }
            if let Some(status) = state.post_status {
                return Answer::new(status, "error code: 1015");
            }
            let n = state.devices.len() + 1;
            let device = Device {
                id: format!("stand-in-device-{n}"),
                token: format!("stand-in-token-{n}"),
                key: field(&body, "key").unwrap_or_default(),
                tunnel_type: field(&body, "tunnel_type").unwrap_or_else(|| "wireguard".into()),
                license: format!("stand-in-license-{n}"),
            };
            let text = Api::record(&state, &device, true);
            state.devices.push(device);
            return json_answer(200, text);
        };
        let bearer = request.header("authorization").and_then(|v| v.strip_prefix("Bearer ")).map(str::to_string);
        let Some(at) = state.devices.iter().position(|d| d.id == id) else {
            return Answer::new(404, "no such device");
        };
        if bearer.as_deref() != Some(state.devices[at].token.as_str()) {
            return Answer::new(401, "no token");
        }
        match (request.method.as_str(), account) {
            ("PATCH", false) => {
                if let Some(key) = field(&body, "key") {
                    state.devices[at].key = key;
                }
                if let Some(kind) = field(&body, "tunnel_type") {
                    state.devices[at].tunnel_type = kind;
                }
                json_answer(200, Api::record(&state, &state.devices[at], false))
            }
            ("GET", false) => json_answer(200, Api::record(&state, &state.devices[at], false)),
            ("DELETE", false) => {
                state.devices.remove(at);
                Answer::new(204, "")
            }
            ("GET", true) => {
                let lic = json(&state.devices[at].license);
                json_answer(200, format!("{{\"account_type\":\"free\",\"premium_data\":0,\"license\":{lic}}}"))
            }
            ("PUT", true) => match field(&body, "license") {
                Some(license) if license.len() >= 8 => {
                    state.devices[at].license = license;
                    let lic = json(&state.devices[at].license);
                    json_answer(200, format!("{{\"account_type\":\"limited\",\"license\":{lic}}}"))
                }
                _ => Answer::new(400, "bad license"),
            },
            _ => Answer::new(405, "no such method"),
        }
    }
}
