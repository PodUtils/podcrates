//! The orders of the stand-in (RFC 8555, sections 7.4 and 7.5): an order of
//! DNS names, an authorization for each with its challenges, TLS-ALPN-01
//! validated by a task of its own after the client's POST, the
//! finalization of a ready order with a CSR, and the chain. Each object is
//! read and changed by its own account alone.

use std::sync::{Arc, Mutex};

use base64::Engine as _;
use rustls::crypto::CryptoProvider;
use serde_json::{Value, json};

use super::{NEW_ORDER, State, random};
use crate::standin::serve::Answer;
use crate::standin::signed::Problem;
use crate::standin::{lock, validate};

const ORDER: &str = "/order/";
const AUTHZ: &str = "/authz/";
const CHALLENGE: &str = "/chal/";
/// The `http-01` challenge that the stand-in lists and does not validate.
const HTTP_CHALLENGE: &str = "/http-chal/";
const FINALIZE: &str = "/finalize/";
const CERT: &str = "/cert/";
/// How long an order and an authorization hold.
const EXPIRES: i64 = 7 * 86_400;

/// The orders, the authorizations and the chains, each by its number from 1.
#[derive(Default)]
pub struct Orders {
    orders: Vec<Order>,
    authzs: Vec<Authz>,
    chains: Vec<String>,
}

struct Order {
    account: usize,
    names: Vec<String>,
    authzs: Vec<usize>,
    /// The number of its chain, once finalized.
    chain: Option<usize>,
}

struct Authz {
    account: usize,
    name: String,
    token: String,
    status: Status,
    error: Option<Value>,
    validated: Option<i64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Pending,
    Processing,
    Valid,
    Invalid,
}

/// A POST of account `account` to `path`, past the check of its JWS.
pub(super) fn post(
    state: &mut State,
    shared: &Arc<Mutex<State>>,
    provider: &Arc<CryptoProvider>,
    account: usize,
    path: &str,
    payload: Option<Value>,
) -> Result<Answer, Problem> {
    if path == NEW_ORDER {
        return new_order(state, account, payload);
    }
    let number = |prefix: &str| path.strip_prefix(prefix).and_then(|n| n.parse::<usize>().ok()).filter(|n| *n >= 1);
    if let Some(n) = number(ORDER) {
        let order = own_order(state, account, n)?;
        no_payload(&payload)?;
        return Ok(order_answer(state, order, 200));
    }
    if let Some(n) = number(AUTHZ) {
        own_authz(state, account, n)?;
        no_payload(&payload)?;
        return Ok(authz_answer(state, n));
    }
    if let Some(n) = number(CHALLENGE) {
        own_authz(state, account, n)?;
        return challenge(state, shared, provider, n, payload);
    }
    if let Some(n) = number(HTTP_CHALLENGE) {
        own_authz(state, account, n)?;
        return Err(Problem::malformed("the stand-in validates tls-alpn-01 alone"));
    }
    if let Some(n) = number(FINALIZE) {
        let order = own_order(state, account, n)?;
        return finalize(state, order, payload);
    }
    if let Some(n) = number(CERT) {
        no_payload(&payload)?;
        let order = state.orders.orders.iter().find(|o| o.chain == Some(n) && o.account == account);
        let order =
            order.ok_or_else(|| Problem::unauthorized("a certificate is read by the account of its order alone"))?;
        let chain = state.orders.chains[order.chain.unwrap_or(n) - 1].clone();
        let headers = vec![("Content-Type", crate::client::PEM_CHAIN.to_string())];
        return Ok(Answer { status: 200, headers, body: chain.into_bytes() });
    }
    Err(Problem::new("malformed", 404, format!("no resource {path}")))
}

fn no_payload(payload: &Option<Value>) -> Result<(), Problem> {
    match payload {
        None => Ok(()),
        Some(_) => Err(Problem::malformed("a read is a POST-as-GET, with an empty payload")),
    }
}

/// The index of order `n` of `account`.
fn own_order(state: &State, account: usize, n: usize) -> Result<usize, Problem> {
    match state.orders.orders.get(n - 1) {
        Some(order) if order.account == account => Ok(n - 1),
        Some(_) => Err(Problem::unauthorized("an order is read and changed by its own account alone")),
        None => Err(Problem::new("malformed", 404, format!("no order {n}"))),
    }
}

fn own_authz(state: &State, account: usize, n: usize) -> Result<(), Problem> {
    match state.orders.authzs.get(n - 1) {
        Some(authz) if authz.account == account => Ok(()),
        Some(_) => Err(Problem::unauthorized("an authorization is read and changed by its own account alone")),
        None => Err(Problem::new("malformed", 404, format!("no authorization {n}"))),
    }
}

/// newOrder: DNS names alone, each a name that TLS-ALPN-01 can prove.
fn new_order(state: &mut State, account: usize, payload: Option<Value>) -> Result<Answer, Problem> {
    let payload = payload.ok_or_else(|| Problem::malformed("newOrder has a payload"))?;
    let identifiers = payload["identifiers"].as_array().filter(|l| !l.is_empty());
    let identifiers = identifiers.ok_or_else(|| Problem::malformed("the order has no identifiers"))?;
    let mut names = Vec::new();
    for identifier in identifiers {
        let value = identifier["value"].as_str().unwrap_or_default().to_ascii_lowercase();
        if identifier["type"] != "dns" {
            return Err(Problem::new(
                "unsupportedIdentifier",
                400,
                "the stand-in takes identifiers of the type dns alone",
            ));
        }
        if let Err(why) = dns_name(&value) {
            return Err(Problem::new("rejectedIdentifier", 400, format!("{value:?}: {why}")));
        }
        if !names.contains(&value) {
            names.push(value);
        }
    }
    let mut authzs = Vec::new();
    for name in &names {
        let token = crate::jws::b64url(&random::<16>());
        let authz = Authz { account, name: name.clone(), token, status: Status::Pending, error: None, validated: None };
        state.orders.authzs.push(authz);
        authzs.push(state.orders.authzs.len());
    }
    state.orders.orders.push(Order { account, names, authzs, chain: None });
    Ok(order_answer(state, state.orders.orders.len() - 1, 201))
}

/// A name that TLS-ALPN-01 can prove: DNS labels, two or more, no
/// wildcard, not an address.
fn dns_name(name: &str) -> Result<(), &'static str> {
    if name.starts_with("*.") {
        return Err("TLS-ALPN-01 cannot prove a wildcard name");
    }
    let labels: Vec<&str> = name.split('.').collect();
    let label_ok = |l: &&str| {
        (1..=63).contains(&l.len())
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !l.starts_with('-')
            && !l.ends_with('-')
    };
    if name.len() > 253 || labels.len() < 2 || !labels.iter().all(label_ok) {
        return Err("not a DNS name");
    }
    if labels.last().is_some_and(|l| l.bytes().all(|b| b.is_ascii_digit())) {
        return Err("an IP address is not a DNS name");
    }
    Ok(())
}

/// The status of order `index`, from its authorizations.
fn order_status(state: &State, index: usize) -> &'static str {
    let order = &state.orders.orders[index];
    if order.chain.is_some() {
        return "valid";
    }
    let statuses: Vec<Status> = order.authzs.iter().map(|a| state.orders.authzs[a - 1].status).collect();
    if statuses.contains(&Status::Invalid) {
        "invalid"
    } else if statuses.iter().all(|s| *s == Status::Valid) {
        "ready"
    } else {
        "pending"
    }
}

fn order_answer(state: &State, index: usize, status: u16) -> Answer {
    let order = &state.orders.orders[index];
    let n = index + 1;
    let url = state.url(&format!("{ORDER}{n}"));
    let order_state = order_status(state, index);
    let mut body = json!({
        "status": order_state,
        "expires": crate::utc(crate::order::now_secs() + EXPIRES),
        "identifiers": order.names.iter().map(|v| json!({"type": "dns", "value": v})).collect::<Vec<_>>(),
        "authorizations": order.authzs.iter().map(|a| state.url(&format!("{AUTHZ}{a}"))).collect::<Vec<_>>(),
        "finalize": state.url(&format!("{FINALIZE}{n}")),
    });
    if let Some(chain) = order.chain {
        body["certificate"] = Value::from(state.url(&format!("{CERT}{chain}")));
    }
    if order_state == "invalid" {
        let failed = order.authzs.iter().find_map(|a| state.orders.authzs[a - 1].error.clone());
        body["error"] = failed.unwrap_or(Value::Null);
    }
    let answer = Answer::json(status, &body).header("Location", url);
    if order_state == "pending" { answer.header("Retry-After", "1") } else { answer }
}

fn authz_answer(state: &State, n: usize) -> Answer {
    let authz = &state.orders.authzs[n - 1];
    let (status, challenge_status) = match authz.status {
        Status::Pending => ("pending", "pending"),
        Status::Processing => ("pending", "processing"),
        Status::Valid => ("valid", "valid"),
        Status::Invalid => ("invalid", "invalid"),
    };
    let mut tls = challenge_object(state, n, challenge_status);
    if let Some(error) = &authz.error {
        tls["error"] = error.clone();
    }
    let http = json!({
        "type": "http-01",
        "url": state.url(&format!("{HTTP_CHALLENGE}{n}")),
        "token": authz.token,
        "status": "pending",
    });
    let body = json!({
        "identifier": {"type": "dns", "value": authz.name},
        "status": status,
        "expires": crate::utc(crate::order::now_secs() + EXPIRES),
        "challenges": [http, tls],
    });
    let answer = Answer::json(200, &body);
    if status == "pending" { answer.header("Retry-After", "1") } else { answer }
}

fn challenge_object(state: &State, n: usize, status: &str) -> Value {
    let authz = &state.orders.authzs[n - 1];
    let mut challenge = json!({
        "type": "tls-alpn-01",
        "url": state.url(&format!("{CHALLENGE}{n}")),
        "token": authz.token,
        "status": status,
    });
    if let Some(at) = authz.validated {
        challenge["validated"] = Value::from(crate::utc(at));
    }
    challenge
}

/// The client's answer to the TLS-ALPN-01 challenge of authorization `n`
/// (RFC 8555, section 7.5.1): a payload `{}` starts the validation, in a
/// task of its own; a POST-as-GET reads the challenge.
fn challenge(
    state: &mut State,
    shared: &Arc<Mutex<State>>,
    provider: &Arc<CryptoProvider>,
    n: usize,
    payload: Option<Value>,
) -> Result<Answer, Problem> {
    let authz = &mut state.orders.authzs[n - 1];
    match &payload {
        Some(v) if v.as_object().is_some_and(|o| o.is_empty()) => {}
        Some(_) => return Err(Problem::malformed("the answer to a challenge is the empty object {}")),
        None => {}
    }
    if payload.is_some() && authz.status == Status::Pending {
        authz.status = Status::Processing;
        let (name, token, account) = (authz.name.clone(), authz.token.clone(), authz.account);
        let key_authorization = format!("{token}.{}", state.thumbprint(account));
        let (shared, provider, at) = (shared.clone(), provider.clone(), state.plan.validate_at);
        tokio::spawn(async move {
            let result = validate::validate(provider, at, &name, &key_authorization).await;
            let mut state = lock(&shared);
            let authz = &mut state.orders.authzs[n - 1];
            match &result {
                Ok(()) => {
                    authz.status = Status::Valid;
                    authz.validated = Some(crate::order::now_secs());
                }
                Err(why) => {
                    authz.status = Status::Invalid;
                    let detail = format!("TLS-ALPN-01 of {name}: {why}");
                    authz.error = Some(
                        json!({"type": "urn:ietf:params:acme:error:unauthorized", "detail": detail, "status": 403}),
                    );
                }
            }
            state.seen.validations.push((name, result));
        });
    }
    let status = match state.orders.authzs[n - 1].status {
        Status::Pending => "pending",
        Status::Processing => "processing",
        Status::Valid => "valid",
        Status::Invalid => "invalid",
    };
    let up = format!("<{}>;rel=\"up\"", state.url(&format!("{AUTHZ}{n}")));
    Ok(Answer::json(200, &challenge_object(state, n, status)).header("Link", up))
}

/// The finalization of a ready order with the CSR of its payload.
fn finalize(state: &mut State, index: usize, payload: Option<Value>) -> Result<Answer, Problem> {
    let status = order_status(state, index);
    if status != "ready" {
        return Err(Problem::new("orderNotReady", 403, format!("the order is {status}, not ready")));
    }
    let csr = payload
        .as_ref()
        .and_then(|p| p["csr"].as_str())
        .ok_or_else(|| Problem::malformed("the finalization has no csr"))?;
    let der = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(csr)
        .map_err(|_| Problem::malformed("the csr is not base64url with no padding"))?;
    let order = &state.orders.orders[index];
    let point = state.point(order.account);
    let chain =
        state.ca.issue(&der, &order.names, &point, state.plan.days).map_err(|why| Problem::new("badCSR", 400, why))?;
    state.seen.issued.push(order.names.clone());
    state.orders.chains.push(chain);
    state.orders.orders[index].chain = Some(state.orders.chains.len());
    Ok(order_answer(state, index, 200))
}
