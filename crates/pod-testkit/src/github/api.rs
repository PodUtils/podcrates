//! The REST API of GitHub that an App and its runners need: the App JWT to
//! an installation token, and the runners of a repository with that token.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use super::{
    APP_ID, INSTALLATION_ID, Runner, Shared, State, body, credential, error, hook, json_answer, key, labels, now,
    rfc3339,
};
use crate::stand::{Answer, Request};

/// The longest life of an App JWT that GitHub takes: ten minutes.
const JWT_LIFE: u64 = 600;
/// The clock skew of an `iat` in the future that the fake takes.
const SKEW: u64 = 60;
/// The life of an installation token.
const TOKEN_LIFE: u64 = 3600;

/// The answer of a route of the API, or none for another path.
pub(super) fn route(shared: &Shared, req: &Request, parts: &[&str]) -> Option<Answer> {
    let answer = match (req.method.as_str(), parts) {
        ("GET", ["repos", owner, repo, "installation"]) => app(shared, req).map(|()| installation(shared, owner, repo)),
        ("POST", ["app", "installations", id, "access_tokens"]) => {
            app(shared, req).and_then(|()| access_token(shared, id))
        }
        ("POST", ["repos", owner, repo, "actions", "runners", "generate-jitconfig"]) => {
            installed(shared, req).and_then(|()| jitconfig(shared, &format!("{owner}/{repo}"), &body(req)))
        }
        ("GET", ["repos", owner, repo, "actions", "runners"]) => installed(shared, req).map(|()| {
            let st = shared.lock();
            let full = format!("{owner}/{repo}");
            let runners: Vec<Value> =
                st.runners.iter().filter(|r| r.repo == full && !r.gone).map(|r| runner_json(&st, r)).collect();
            json_answer(200, &json!({ "total_count": runners.len(), "runners": runners }))
        }),
        ("DELETE", ["repos", owner, repo, "actions", "runners", id]) => {
            installed(shared, req).and_then(|()| remove(shared, &format!("{owner}/{repo}"), id))
        }
        ("GET", ["repos", owner, repo, "actions", "jobs", id]) => installed(shared, req).and_then(|()| {
            let st = shared.lock();
            let full = format!("{owner}/{repo}");
            let job = st
                .jobs
                .iter()
                .find(|j| j.repo == full && id.parse() == Ok(j.id))
                .ok_or_else(|| error(404, "Not Found"))?;
            Ok(json_answer(200, &hook::job_json(&shared.base, &st, job)))
        }),
        _ => return None,
    };
    Some(answer.unwrap_or_else(|refusal| refusal))
}

/// The App JWT of the request: signed with the made-up key, of [`APP_ID`],
/// alive now, and alive for ten minutes at most.
fn app(shared: &Shared, req: &Request) -> Result<(), Answer> {
    let refused = |why: &str| error(401, why);
    let token = credential(req).ok_or_else(|| refused("A JSON web token could not be decoded"))?;
    let claims = shared.key.verify(token).ok_or_else(|| refused("A JSON web token could not be decoded"))?;
    let iss = claims["iss"].as_u64().or_else(|| claims["iss"].as_str().and_then(|s| s.parse().ok()));
    if iss != Some(APP_ID) {
        return Err(refused("The 'iss' claim of the JWT is not the id of this App"));
    }
    let (Some(iat), Some(exp)) = (claims["iat"].as_u64(), claims["exp"].as_u64()) else {
        return Err(refused("The JWT has no 'iat' or no 'exp' claim"));
    };
    let now = now();
    if exp <= now {
        return Err(refused("The JWT expired"));
    }
    if iat > now + SKEW || exp > iat + JWT_LIFE {
        return Err(refused("The 'exp' claim of the JWT is too far in the future"));
    }
    Ok(())
}

/// An installation token that the fake gave.
fn installed(shared: &Shared, req: &Request) -> Result<(), Answer> {
    let token = credential(req).unwrap_or_default();
    match shared.lock().tokens.iter().any(|t| key::same(t, token)) {
        true => Ok(()),
        false => Err(error(401, "Bad credentials")),
    }
}

/// The one installation holds each repository.
fn installation(shared: &Shared, owner: &str, repo: &str) -> Answer {
    json_answer(
        200,
        &json!({
            "id": INSTALLATION_ID, "app_id": APP_ID, "target_type": "Organization",
            "account": { "login": owner, "type": "Organization" }, "repository_selection": "all",
            "access_tokens_url": format!("{}app/installations/{INSTALLATION_ID}/access_tokens", shared.base),
            "html_url": format!("https://github.com/{owner}/{repo}"),
            "permissions": { "administration": "write", "actions": "read", "metadata": "read" },
        }),
    )
}

fn access_token(shared: &Shared, id: &str) -> Result<Answer, Answer> {
    if id.parse() != Ok(INSTALLATION_ID) {
        return Err(error(404, "Not Found"));
    }
    // No prefix of a real token: a scanner of secrets finds no GitHub token.
    let token = format!("fake-installation-{}", key::random_hex(20));
    shared.lock().tokens.push(token.clone());
    Ok(json_answer(
        201,
        &json!({
            "token": token, "expires_at": rfc3339(now() + TOKEN_LIFE), "repository_selection": "all",
            "permissions": { "administration": "write", "actions": "read", "metadata": "read" },
        }),
    ))
}

/// A JIT runner of `repo`: its record and the three files of the runner,
/// each in base64 in one JSON map, the whole in base64.
fn jitconfig(shared: &Shared, repo: &str, req: &Value) -> Result<Answer, Answer> {
    let name = req["name"].as_str().filter(|n| !n.trim().is_empty());
    let (Some(name), Some(labels)) = (name, labels(&req["labels"]).filter(|l| !l.is_empty())) else {
        return Err(error(422, "Invalid request: the name and at least one label are required"));
    };
    let mut st = shared.lock();
    if st.runners.iter().any(|r| r.repo == repo && r.name == name && !r.gone) {
        return Err(error(409, "Already exists - A runner with the same name already exists"));
    }
    let id = st.next_id();
    let client_id = format!("00000000-0000-4000-8000-{id:012x}");
    let base = &shared.base;
    let runner = json!({
        "agentId": id, "agentName": name, "poolId": 1, "poolName": "Default",
        "serverUrl": base, "serverUrlV2": base, "useV2Flow": true,
        "gitHubUrl": format!("https://github.com/{repo}"),
        "workFolder": req["work_folder"].as_str().unwrap_or("_work"),
        "ephemeral": true, "disableUpdate": true,
    });
    let credentials = json!({ "scheme": "OAuth", "data": {
        "clientId": client_id, "authorizationUrl": format!("{base}_apis/oauth2/token"),
        "requireFipsCryptography": "False",
    } });
    let files = json!({
        ".runner": STANDARD.encode(runner.to_string()),
        ".credentials": STANDARD.encode(credentials.to_string()),
        ".credentials_rsaparams": STANDARD.encode(shared.key.rsaparams()),
    });
    let runner = Runner {
        id,
        name: name.to_string(),
        repo: repo.to_string(),
        labels,
        client_id,
        token: None,
        job: None,
        gone: false,
    };
    let record = runner_json(&st, &runner);
    st.runners.push(runner);
    Ok(json_answer(201, &json!({ "runner": record, "encoded_jit_config": STANDARD.encode(files.to_string()) })))
}

/// Remove a runner that has no job in progress.
fn remove(shared: &Shared, repo: &str, id: &str) -> Result<Answer, Answer> {
    let mut st = shared.lock();
    let busy = |st: &State, job: Option<u64>| {
        job.is_some_and(|j| st.jobs.iter().any(|x| x.id == j && x.completed_at.is_none()))
    };
    let Some(r) = st.runners.iter().find(|r| r.repo == repo && !r.gone && id.parse() == Ok(r.id)) else {
        return Err(error(404, "Not Found"));
    };
    if busy(&st, r.job) {
        return Err(error(422, "Bad request - Runner is currently running a job"));
    }
    let id = r.id;
    if let Some(r) = st.runner(id) {
        r.gone = true;
    }
    Ok(Answer::new(204, Vec::new()))
}

/// A runner as the REST API of GitHub gives it.
pub(super) fn runner_json(st: &State, r: &Runner) -> Value {
    let busy = r.job.is_some_and(|j| st.jobs.iter().any(|x| x.id == j && x.completed_at.is_none()));
    let labels: Vec<Value> =
        r.labels.iter().enumerate().map(|(i, l)| json!({ "id": i + 1, "name": l, "type": "custom" })).collect();
    json!({
        "id": r.id, "name": r.name, "os": "Linux", "status": if r.token.is_some() { "online" } else { "offline" },
        "busy": busy, "ephemeral": true, "runner_group_id": 1, "labels": labels,
    })
}
