//! The resources of the stand-in CA (RFC 8555, section 7): the directory,
//! the nonces, the accounts with their contacts; the orders, their
//! authorizations and challenges, the finalization and the certificates are
//! in the module `orders`. Each answer to a POST carries a new nonce, a refusal too.

mod orders;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use rustls::crypto::CryptoProvider;
use serde_json::{Value, json};

use super::ca::Ca;
use super::serve::{Answer, Asked};
use super::signed::{self, Keys, Problem, Signer};
use super::{Plan, Seen, lock};

/// The path of the directory.
pub const DIRECTORY: &str = "/dir";
const NEW_NONCE: &str = "/nonce";
const NEW_ACCOUNT: &str = "/new-acct";
const NEW_ORDER: &str = "/new-order";
const ACCOUNT: &str = "/acct/";
/// The terms that a new account agrees to.
const TERMS: &str = "/terms";
/// The type of each JWS POST (RFC 8555, section 6.2).
const JOSE: &str = "application/jose+json";

/// The state of the stand-in.
pub struct State {
    /// `https://127.0.0.1:PORT`.
    pub base: String,
    pub plan: Plan,
    pub seen: Seen,
    pub ca: Ca,
    nonces: HashSet<String>,
    bad_nonces_left: usize,
    accounts: Vec<Account>,
    orders: orders::Orders,
}

/// An account: the point of its key, the JWK of it, its contacts.
struct Account {
    point: Vec<u8>,
    x: String,
    y: String,
    contacts: Vec<String>,
}

impl State {
    pub fn new(base: String, plan: Plan, ca: Ca) -> State {
        let bad_nonces_left = plan.bad_nonces;
        State {
            base,
            plan,
            seen: Seen::default(),
            ca,
            nonces: HashSet::new(),
            bad_nonces_left,
            accounts: Vec::new(),
            orders: orders::Orders::default(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// A new nonce of 128 random bits, in base64url.
    fn nonce(&mut self) -> String {
        let nonce = crate::jws::b64url(&random::<16>());
        self.nonces.insert(nonce.clone());
        nonce
    }

    /// The number of the account at `url`, from 1.
    fn account_number(&self, url: &str) -> Option<usize> {
        let number: usize = url.strip_prefix(&self.url(ACCOUNT))?.parse().ok()?;
        (1..=self.accounts.len()).contains(&number).then_some(number)
    }

    /// The thumbprint of the key of account `number` (RFC 7638).
    fn thumbprint(&self, number: usize) -> String {
        let account = &self.accounts[number - 1];
        crate::jws::thumbprint(&account.x, &account.y)
    }

    /// The point of the key of account `number`.
    fn point(&self, number: usize) -> Vec<u8> {
        self.accounts[number - 1].point.clone()
    }

    fn refused(&mut self, problem: &Problem) -> Answer {
        self.seen.refusals.push(format!("{}: {}", problem.kind, problem.detail));
        let body = json!({
            "type": format!("urn:ietf:params:acme:error:{}", problem.kind),
            "detail": problem.detail,
            "status": problem.status,
        });
        let mut answer = Answer::json(problem.status, &body);
        answer.headers = vec![("Content-Type", "application/problem+json".to_string())];
        answer
    }
}

impl Keys for State {
    fn account(&self, kid: &str) -> Option<(usize, Vec<u8>)> {
        let number = self.account_number(kid)?;
        Some((number, self.point(number)))
    }

    fn take_nonce(&mut self, nonce: &str) -> bool {
        self.nonces.remove(nonce)
    }
}

/// The answer to `asked`.
pub fn answer(state: &Arc<Mutex<State>>, provider: &Arc<CryptoProvider>, asked: &Asked) -> Answer {
    let mut guard = lock(state);
    let path = asked.target.split('?').next().unwrap_or_default().to_string();
    match (asked.method.as_str(), path.as_str()) {
        ("GET", DIRECTORY) => directory(&guard),
        ("GET" | "HEAD", NEW_NONCE) => {
            let status = if asked.method == "GET" { 204 } else { 200 };
            let nonce = guard.nonce();
            Answer { status, headers: vec![("Replay-Nonce", nonce)], body: Vec::new() }
        }
        ("POST", _) => {
            let mut answer = post(&mut guard, state, provider, &path, asked);
            let nonce = guard.nonce();
            answer.headers.push(("Replay-Nonce", nonce));
            answer.headers.push(("Link", format!("<{}>;rel=\"index\"", guard.url(DIRECTORY))));
            answer
        }
        _ => guard.refused(&Problem::new("malformed", 404, format!("no resource {} {path}", asked.method))),
    }
}

fn directory(state: &State) -> Answer {
    Answer::json(
        200,
        &json!({
            "newNonce": state.url(NEW_NONCE),
            "newAccount": state.url(NEW_ACCOUNT),
            "newOrder": state.url(NEW_ORDER),
            "revokeCert": state.url("/revoke-cert"),
            "keyChange": state.url("/key-change"),
            "meta": {"termsOfService": state.url(TERMS), "externalAccountRequired": false},
        }),
    )
}

/// A JWS POST to `path`: checked, then given to its resource.
fn post(
    state: &mut State,
    shared: &Arc<Mutex<State>>,
    provider: &Arc<CryptoProvider>,
    path: &str,
    asked: &Asked,
) -> Answer {
    if asked.kind.as_deref() != Some(JOSE) {
        let problem = Problem::new("malformed", 415, format!("a POST is of the type {JOSE}"));
        return state.refused(&problem);
    }
    let url = state.url(path);
    let signed = match signed::check(&asked.body, &url, path == NEW_ACCOUNT, state) {
        Ok(signed) => signed,
        Err(problem) => return state.refused(&problem),
    };
    if state.bad_nonces_left > 0 {
        // A good nonce refused on purpose, as Pebble refuses some: the
        // client must send the request again with a new one.
        state.bad_nonces_left -= 1;
        state.seen.bad_nonces += 1;
        return state.refused(&Problem::new("badNonce", 400, "the stand-in refuses this nonce on purpose"));
    }
    let result = match (path, signed.signer) {
        (NEW_ACCOUNT, Signer::Key { point, x, y }) => new_account(state, point, x, y, signed.payload),
        (_, Signer::Account(number)) if path.starts_with(ACCOUNT) => account(state, number, path, signed.payload),
        (_, Signer::Account(number)) => orders::post(state, shared, provider, number, path, signed.payload),
        (_, Signer::Key { .. }) => Err(Problem::malformed("a request after newAccount names its account by kid")),
    };
    match result {
        Ok(answer) => answer,
        Err(problem) => state.refused(&problem),
    }
}

/// newAccount (RFC 8555, section 7.3): the account of the key when it has
/// one (200), else a new one (201) that agrees to the terms, with its
/// contacts checked.
fn new_account(
    state: &mut State,
    point: Vec<u8>,
    x: String,
    y: String,
    payload: Option<Value>,
) -> Result<Answer, Problem> {
    let payload = payload.ok_or_else(|| Problem::malformed("newAccount has a payload"))?;
    if let Some(at) = state.accounts.iter().position(|a| a.point == point) {
        let number = at + 1;
        return Ok(account_answer(state, number, 200));
    }
    if payload["onlyReturnExisting"] == true {
        return Err(Problem::new("accountDoesNotExist", 400, "no account has this key"));
    }
    if payload["termsOfServiceAgreed"] != true {
        return Err(Problem::malformed("a new account must agree to the terms of service"));
    }
    let contacts = contacts(state, &payload)?;
    state.seen.accounts.push(contacts.clone());
    state.accounts.push(Account { point, x, y, contacts });
    Ok(account_answer(state, state.accounts.len(), 201))
}

/// A POST to an account: a POST-as-GET of it, or an update of its contacts
/// (RFC 8555, section 7.3.2), by the account alone.
fn account(state: &mut State, number: usize, path: &str, payload: Option<Value>) -> Result<Answer, Problem> {
    if state.account_number(&state.url(path)) != Some(number) {
        return Err(Problem::unauthorized("an account is read and changed by its own key alone"));
    }
    if let Some(payload) = payload
        && payload.get("contact").is_some()
    {
        let contacts = contacts(state, &payload)?;
        state.seen.accounts[number - 1] = contacts.clone();
        state.accounts[number - 1].contacts = contacts;
    }
    Ok(account_answer(state, number, 200))
}

fn account_answer(state: &State, number: usize, status: u16) -> Answer {
    let url = state.url(&format!("{ACCOUNT}{number}"));
    let body = json!({
        "status": "valid",
        "contact": state.accounts[number - 1].contacts,
        "orders": format!("{url}/orders"),
    });
    Answer::json(status, &body).header("Location", url)
}

/// The contacts of a payload, each checked as Boulder checks one: a
/// `mailto:` URI (`unsupportedContact` for another scheme) of one address
/// with a domain, no header and no escape (`invalidContact`), and not one
/// of [`Plan::refused_contacts`] (`invalidContact`).
fn contacts(state: &State, payload: &Value) -> Result<Vec<String>, Problem> {
    let Some(list) = payload.get("contact") else { return Ok(Vec::new()) };
    let list = list.as_array().ok_or_else(|| Problem::malformed("contact is not an array"))?;
    let mut out = Vec::new();
    for item in list {
        let contact = item.as_str().ok_or_else(|| Problem::malformed("a contact is not a string"))?;
        let invalid = |why: &str| Problem::new("invalidContact", 400, format!("the contact {contact:?}: {why}"));
        let Some(address) = contact.strip_prefix("mailto:") else {
            let scheme = contact.split(':').next().unwrap_or_default();
            return Err(Problem::new(
                "unsupportedContact",
                400,
                format!("the contact scheme {scheme:?} is not supported"),
            ));
        };
        if address.contains([',', '?', '%']) || address.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(invalid("give one address, with no header and no escape"));
        }
        let domain = match address.split_once('@') {
            Some((local, domain)) if !local.is_empty() && !domain.contains('@') => domain,
            _ => return Err(invalid("the address is not LOCAL@DOMAIN")),
        };
        if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
            return Err(invalid("the domain is not a public DNS name"));
        }
        if state.plan.refused_contacts.iter().any(|r| r == contact) {
            return Err(invalid("the CA refuses this address"));
        }
        out.push(contact.to_string());
    }
    Ok(out)
}

/// `N` random bytes from the system.
fn random<const N: usize>() -> [u8; N] {
    use pod_core::entropy::Entropy as _;
    let mut bytes = [0u8; N];
    pod_core::entropy::OsEntropy.fill(&mut bytes).expect("the system's random bytes");
    bytes
}
