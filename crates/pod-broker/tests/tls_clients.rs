//! The clients of a job through the TLS end (T-973): curl, git over HTTPS,
//! Python (urllib, and requests where it imports), Node.js and cargo, each
//! with the bundle of `ca::write_bundle` and the variables of `ca::env`,
//! through a broker on 127.0.0.1 to a far end that ends TLS with a leaf of
//! its own CA and names the `Authorization` that it got. The far end gets
//! the value; the client reads the placeholder. With no variable of the
//! bundle, each client refuses the leaf of the job. With the bundle, a host
//! that no credential names keeps the roots of the host and gets the
//! placeholder as it is.
//!
//! A client that this host does not have skips with the reason.
//! `POD_BROKER_TEST_CLIENTS=required` makes a skip a failure
//! (`scripts/ci/broker-clients-tests.sh`). `POD_BROKER_TEST_PYTHON`,
//! `POD_BROKER_TEST_NODE`, `POD_BROKER_TEST_GIT` and `POD_BROKER_TEST_CURL`
//! name a program in place of the one in `PATH`. Unix only: the clients
//! run with a clean environment and `/dev/null` as the git configuration.
#![cfg(unix)]

mod common;
mod tls_common;

mod clients;

use base64::Engine as _;
use clients::{Lab, Run, git, need, registry};
use tls_common::{HOST, OTHER, PH, SECRET};

/// Each script reads its URL from `JOB_URL` and the placeholder from
/// `JOB_TOKEN`, and prints the status, the header `X-Seen` and the body.
const PYTHON_URLLIB: &str = r#"
import os, urllib.request
req = urllib.request.Request(os.environ["JOB_URL"], headers={"Authorization": "Bearer " + os.environ["JOB_TOKEN"]})
with urllib.request.urlopen(req, timeout=20) as r:
    print(r.status, "X-Seen: " + r.headers.get("X-Seen"))
    print(r.read().decode())
"#;

const PYTHON_REQUESTS: &str = r#"
import os, requests
r = requests.get(os.environ["JOB_URL"], headers={"Authorization": "Bearer " + os.environ["JOB_TOKEN"]}, timeout=20)
print(r.status_code, "X-Seen: " + r.headers.get("X-Seen"))
print(r.text)
"#;

/// Node.js before 22.21 has no proxy of the environment: the script sends
/// the CONNECT itself, then its TLS takes the default trust, which
/// `NODE_EXTRA_CA_CERTS` extends. Each `Host` names the target, as a proxy
/// agent sends it: `http` with no agent writes the port 80.
const NODE: &str = r#"
const http = require("http"), tls = require("tls");
const proxy = new URL(process.env.HTTPS_PROXY), url = new URL(process.env.JOB_URL);
const auth = Buffer.from(decodeURIComponent(proxy.username) + ":" + decodeURIComponent(proxy.password)).toString("base64");
const connect = http.request({ host: proxy.hostname, port: proxy.port, method: "CONNECT", path: url.hostname + ":443",
  headers: { Host: url.hostname + ":443", "Proxy-Authorization": "Basic " + auth } });
connect.on("connect", (res, socket) => {
  if (res.statusCode !== 200) { console.error("CONNECT", res.statusCode); process.exit(3); }
  const secure = tls.connect({ socket, servername: url.hostname, ALPNProtocols: ["http/1.1"] });
  secure.on("error", (e) => { console.error("tls:", e.code || e.message); process.exit(4); });
  const req = http.request({ createConnection: () => secure, host: url.hostname, path: url.pathname,
    headers: { Host: url.hostname, Authorization: "Bearer " + process.env.JOB_TOKEN, Connection: "close" } }, (r) => {
    let body = "";
    r.on("data", (d) => (body += d));
    r.on("end", () => { console.log(r.statusCode, "X-Seen: " + r.headers["x-seen"]); console.log(body); process.exit(0); });
  });
  req.on("error", (e) => { console.error("request:", e.code || e.message); process.exit(5); });
  req.end();
});
connect.on("error", (e) => { console.error("connect:", e.message); process.exit(6); });
connect.end();
"#;

/// The `fetch` of Node.js with `--use-env-proxy`: the proxy of
/// `HTTPS_PROXY`, as a job with Node.js 22.21, 24.5 or later uses it.
const NODE_FETCH: &str = r#"
fetch(process.env.JOB_URL, { headers: { Authorization: "Bearer " + process.env.JOB_TOKEN } })
  .then(async (r) => { console.log(r.status, "X-Seen: " + r.headers.get("x-seen")); console.log(await r.text()); })
  .catch((e) => { console.error("fetch:", e.message, e.cause && (e.cause.code || e.cause.message)); process.exit(2); });
"#;

fn b64(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

fn url(host: &str) -> String {
    format!("https://{host}/user")
}

/// With no variable of the bundle, the client refuses the leaf and the far
/// end reads no request.
fn refused_with_no_bundle(lab: &Lab, ran: &Run, client: &str) {
    assert_ne!(ran.code, Some(0), "{client} with no bundle: {ran}");
    assert!(lab.far.seen().is_empty(), "{client} with no bundle: {}", lab.far.seen());
    println!("clients: {client} with no bundle refused the leaf");
}

/// The far end got `got` as a bearer token, and the client read only the
/// placeholder, in the header (`X-Seen`) and in the body (`echo=`).
fn bearer_seen(lab: &Lab, ran: &Run, got: &str, client: &str) {
    assert_eq!(ran.code, Some(0), "{client}: {ran}");
    assert!(lab.far.seen().contains(&format!("authorization: Bearer {got}")), "{client}: {}", lab.far.seen());
    let header = format!("x-seen: bearer {}", PH.to_ascii_lowercase());
    assert!(ran.stdout.to_ascii_lowercase().contains(&header), "{client}: {ran}");
    assert!(ran.stdout.contains(&format!("echo=Bearer {PH}")), "{client}: {ran}");
    assert!(!ran.all().contains(SECRET), "{client}: {ran}");
}

/// The three runs of a client with a bearer token: no bundle to the host of
/// the credential; the bundle to the host of the credential (the swap); the
/// bundle to a host that no credential names (the roots, no swap). `args`
/// gives the arguments for a URL.
async fn bearer_legs(lab: &Lab, program: &str, args: impl Fn(&str) -> Vec<String>, client: &str) {
    let run = |host: &str, bundle: bool| {
        let url = url(host);
        let args = args(&url);
        async move {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            lab.run(program, &args, &[("JOB_URL", &url)], bundle).await
        }
    };
    let ran = run(HOST, false).await;
    refused_with_no_bundle(lab, &ran, client);
    let ran = run(HOST, true).await;
    bearer_seen(lab, &ran, SECRET, client);
    assert!(!lab.far.seen().contains(PH), "{client}: {}", lab.far.seen());
    println!("clients: {client} took the bundle, the far end got the value, the job read the placeholder");
    lab.far.seen.lock().unwrap().clear();
    let ran = run(OTHER, true).await;
    bearer_seen(lab, &ran, PH, client);
    println!("clients: {client} reached a host with no credential by the roots of the bundle, with no swap");
}

/// The same arguments for each URL: a script reads `JOB_URL`.
fn script(args: &[&str]) -> impl Fn(&str) -> Vec<String> {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    move |_| args.clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn curl_takes_the_bundle() {
    let Some(curl) = need("POD_BROKER_TEST_CURL", "curl") else { return };
    let lab = Lab::start("curl", clients::echo_auth).await;
    let auth = format!("Authorization: Bearer {PH}");
    // `-i`: curl prints the head of the answer with the body.
    let args = |url: &str| ["-q", "-sS", "-i", "-H", &auth, url].map(String::from).to_vec();
    bearer_legs(&lab, &curl, args, "curl").await;

    // A body over `max_body`, with `Expect: 100-continue`, which curl sends
    // for a body over 1 MiB: the broker answers 100, and the body passes.
    lab.far.seen.lock().unwrap().clear();
    let body = lab.dir.join("body");
    std::fs::write(&body, git::blob()).unwrap();
    let data = format!("@{}", body.display());
    let args = ["-q", "-sS", "-v", "-i", "-H", &auth, "--data-binary", &data, &url(HOST)];
    let ran = lab.run(&curl, &args, &[], true).await;
    bearer_seen(&lab, &ran, SECRET, "curl with a large body");
    assert!(ran.stderr.contains("Expect: 100-continue"), "curl with a large body: {ran}");
    assert!(lab.far.seen.lock().unwrap().len() > git::blob().len(), "curl with a large body: {ran}");
    println!("clients: curl sent a body over the bound of the swap, with Expect: 100-continue");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_clones_with_the_bundle_by_an_extra_header_and_by_a_token_in_the_url() {
    let Some(program) = need("POD_BROKER_TEST_GIT", "git") else { return };
    let lab = Lab::start("git", |dir| {
        git::repository(&program, &dir.join("far"));
        git::backend(&program, &dir.join("far"))
    })
    .await;
    // The header of actions/checkout, as it writes it: `AUTHORIZATION:
    // basic` and the base64 of `x-access-token:TOKEN`.
    let header = format!("http.extraHeader=AUTHORIZATION: basic {}", b64(&format!("x-access-token:{PH}")));
    let url = format!("https://{HOST}/owner/repo.git");
    let ran = lab.run(&program, &["-c", &header, "clone", "-q", &url, "none"], &[], false).await;
    refused_with_no_bundle(&lab, &ran, "git");
    let ran = lab.run(&program, &["-c", &header, "clone", "-q", &url, "one"], &[], true).await;
    let (real, sent) = (b64(&format!("x-access-token:{SECRET}")), b64(&format!("x-access-token:{PH}")));
    git_proven(&lab, &ran, "one", &real, &sent, "git with an extra header");

    // A token in the URL: git sends it after the 401 of the far end.
    lab.far.seen.lock().unwrap().clear();
    let url = format!("https://x-access-token:{PH}@{HOST}/owner/repo.git");
    let ran = lab.run(&program, &["clone", "-q", &url, "two"], &[], true).await;
    git_proven(&lab, &ran, "two", &real, &sent, "git with a token in the URL");
}

/// The clone in `out` holds the commit of the far end; the far end got the
/// value in Basic and the job read only the placeholder.
fn git_proven(lab: &Lab, ran: &Run, out: &str, real: &str, sent: &str, client: &str) {
    assert_eq!(ran.code, Some(0), "{client}: {ran}");
    let file = std::fs::read_to_string(lab.dir.join(out).join(git::FILE)).unwrap_or_default();
    assert_eq!(file, git::TEXT, "{client}: {ran}");
    assert!(std::fs::read(lab.dir.join(out).join(git::BLOB)).unwrap_or_default() == git::blob(), "{client}");
    assert!(lab.far.seen().contains(&format!(" {real}\r\n")), "{client}: {}", lab.far.seen());
    assert!(!lab.far.seen().contains(sent), "{client}: {}", lab.far.seen());
    // GIT_TRACE_CURL shows each header of each answer: the far end names
    // the value, and the job reads the placeholder.
    let trace = ran.stderr.to_ascii_lowercase();
    assert!(trace.contains(&format!("x-seen: basic {}", sent.to_ascii_lowercase())), "{client}: {ran}");
    assert!(!ran.all().contains(real) && !ran.all().contains(SECRET), "{client}: {ran}");
    println!("clients: {client} took the bundle, the far end got the value, the job read the placeholder");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn python_urllib_takes_the_bundle() {
    let Some(python) = need("POD_BROKER_TEST_PYTHON", "python3") else { return };
    let lab = Lab::start("urllib", clients::echo_auth).await;
    bearer_legs(&lab, &python, script(&["-I", "-c", PYTHON_URLLIB]), "python urllib").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn python_requests_takes_the_bundle() {
    let Some(python) = need("POD_BROKER_TEST_PYTHON", "python3") else { return };
    if let Err(why) = clients::python_imports(&python, "requests") {
        clients::skip(&why);
        return;
    }
    let lab = Lab::start("requests", clients::echo_auth).await;
    bearer_legs(&lab, &python, script(&["-I", "-c", PYTHON_REQUESTS]), "python requests").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn node_takes_the_bundle() {
    let Some(node) = need("POD_BROKER_TEST_NODE", "node") else { return };
    let lab = Lab::start("node", clients::echo_auth).await;
    bearer_legs(&lab, &node, script(&["-e", NODE]), "node").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn node_fetch_takes_the_bundle_through_the_proxy_of_the_environment() {
    let Some(node) = need("POD_BROKER_TEST_NODE", "node") else { return };
    if let Err(why) = clients::node_has_env_proxy(&node) {
        clients::skip(&why);
        return;
    }
    let lab = Lab::start("fetch", clients::echo_auth).await;
    bearer_legs(&lab, &node, script(&["--use-env-proxy", "-e", NODE_FETCH]), "node fetch").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cargo_resolves_from_a_registry_that_needs_a_token() {
    let Some(cargo) = need("CARGO", "cargo") else { return };
    let lab = Lab::start("cargo", registry::answer).await;
    registry::job(&lab.dir);
    let env = registry::env(&lab.dir, PH);
    let env: Vec<(&str, &str)> = env.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
    let args = ["generate-lockfile", "--manifest-path", "job/Cargo.toml"];
    let ran = lab.run(&cargo, &args, &env, false).await;
    refused_with_no_bundle(&lab, &ran, "cargo");
    let ran = lab.run(&cargo, &args, &env, true).await;
    assert_eq!(ran.code, Some(0), "cargo: {ran}");
    let lock = std::fs::read_to_string(lab.dir.join("job/Cargo.lock")).unwrap_or_default();
    assert!(lock.contains("name = \"foo\"\nversion = \"1.0.0\""), "cargo: {lock}");
    // Cargo sends the token as the whole value of `Authorization`.
    assert!(lab.far.seen().contains(&format!("authorization: {SECRET}")), "cargo: {}", lab.far.seen());
    assert!(!lab.far.seen().contains(PH), "cargo: {}", lab.far.seen());
    assert!(!ran.all().contains(SECRET) && !lock.contains(SECRET), "cargo: {ran}");
    println!("clients: cargo took the bundle, the far end got the value");
}
