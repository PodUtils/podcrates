//! The clients of a job through the TLS end (T-973, T-980): curl, git over
//! HTTPS, Python (urllib, requests and pip), Node.js, npm, cargo, Go, Java,
//! Deno, the Google Cloud CLI and Hex of Elixir, each with the bundle of
//! `ca::write_bundle`, the variables of `ca::env` and `JAVA_TOOL_OPTIONS` of
//! `ca::java_options`, through a broker on 127.0.0.1 to a far end that ends
//! TLS with a leaf of its own CA and names the `Authorization` that it got.
//! The far end gets the value; the client reads the placeholder. With no
//! variable of the bundle, each client refuses the leaf of the job; a
//! client that reads one variable alone refuses it also with each other
//! variable. With the bundle, a host that no credential names keeps the
//! roots of the host and gets the placeholder as it is.
//!
//! A client that this host does not have skips with the reason.
//! `POD_BROKER_TEST_CLIENTS=required` makes a skip a failure
//! (`scripts/ci/broker-clients-tests.sh`). `POD_BROKER_TEST_PYTHON`,
//! `POD_BROKER_TEST_NODE`, `POD_BROKER_TEST_GIT`, `POD_BROKER_TEST_CURL`,
//! `POD_BROKER_TEST_GO`, `POD_BROKER_TEST_JAVA`, `POD_BROKER_TEST_DENO`,
//! `POD_BROKER_TEST_NPM`, `POD_BROKER_TEST_GCLOUD` and `POD_BROKER_TEST_MIX`
//! name a program in place of the one in `PATH`; `POD_BROKER_TEST_MIX_HOME`
//! names the `MIX_HOME` with Hex. Unix only: the clients run with a clean
//! environment and `/dev/null` as the git configuration.
#![cfg(unix)]

mod common;
mod tls_common;

mod clients;

use base64::Engine as _;
use clients::{Lab, Run, Trust, far, git, need, registry, sources};
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
/// gives the arguments for a URL. With `own`, a fourth run before the swap
/// has each variable but `own`: the client reads no other, so it refuses.
async fn bearer_legs(
    lab: &Lab,
    program: &str,
    args: impl Fn(&str) -> Vec<String>,
    client: &str,
    own: Option<&'static [&'static str]>,
) {
    let run = |host: &str, trust: Trust| {
        let url = url(host);
        let args = args(&url);
        async move {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            lab.run_trusting(program, &args, &[("JOB_URL", &url)], trust).await
        }
    };
    let ran = run(HOST, Trust::None).await;
    refused_with_no_bundle(lab, &ran, client);
    if let Some(var) = own {
        let ran = run(HOST, Trust::Without(var)).await;
        refused_without(lab, &ran, client, var);
    }
    let ran = run(HOST, Trust::All).await;
    bearer_seen(lab, &ran, SECRET, client);
    assert!(!lab.far.seen().contains(PH), "{client}: {}", lab.far.seen());
    println!("clients: {client} took the bundle, the far end got the value, the job read the placeholder");
    lab.far.seen.lock().unwrap().clear();
    let ran = run(OTHER, Trust::All).await;
    bearer_seen(lab, &ran, PH, client);
    println!("clients: {client} reached a host with no credential by the roots of the bundle, with no swap");
}

/// With each variable but those of `vars`, the client refuses the leaf, and
/// the far end reads no request.
fn refused_without(lab: &Lab, ran: &Run, client: &str, vars: &[&str]) {
    let vars = vars.join(" and ");
    assert_ne!(ran.code, Some(0), "{client} with no {vars}: {ran}");
    assert!(lab.far.seen().is_empty(), "{client} with no {vars}: {}", lab.far.seen());
    println!("clients: {client} with no {vars} refused the leaf");
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
    bearer_legs(&lab, &curl, args, "curl", None).await;

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
    bearer_legs(&lab, &python, script(&["-I", "-c", PYTHON_URLLIB]), "python urllib", Some(&["SSL_CERT_FILE"])).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn python_requests_takes_the_bundle() {
    let Some(python) = need("POD_BROKER_TEST_PYTHON", "python3") else { return };
    if let Err(why) = clients::python_imports(&python, "requests") {
        clients::skip(&why);
        return;
    }
    let lab = Lab::start("requests", clients::echo_auth).await;
    bearer_legs(&lab, &python, script(&["-I", "-c", PYTHON_REQUESTS]), "python requests", None).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn node_takes_the_bundle() {
    let Some(node) = need("POD_BROKER_TEST_NODE", "node") else { return };
    let lab = Lab::start("node", clients::echo_auth).await;
    bearer_legs(&lab, &node, script(&["-e", NODE]), "node", Some(clients::node_own(&node))).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn node_fetch_takes_the_bundle_through_the_proxy_of_the_environment() {
    let Some(node) = need("POD_BROKER_TEST_NODE", "node") else { return };
    if let Err(why) = clients::node_has_env_proxy(&node) {
        clients::skip(&why);
        return;
    }
    let lab = Lab::start("fetch", clients::echo_auth).await;
    bearer_legs(&lab, &node, script(&["--use-env-proxy", "-e", NODE_FETCH]), "node fetch", None).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cargo_resolves_from_a_registry_that_needs_a_token() {
    let Some(cargo) = registry::cargo() else { return };
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
    // `CARGO_HTTP_CAINFO` alone: the one variable that cargo reads on macOS.
    std::fs::remove_file(lab.dir.join("job/Cargo.lock")).unwrap();
    let bundle = lab.bundle.to_string_lossy().into_owned();
    let env: Vec<(&str, &str)> = env.iter().copied().chain([("CARGO_HTTP_CAINFO", bundle.as_str())]).collect();
    let ran = lab.run(&cargo, &args, &env, false).await;
    assert_eq!(ran.code, Some(0), "cargo with CARGO_HTTP_CAINFO alone: {ran}");
    println!("clients: cargo took the bundle by CARGO_HTTP_CAINFO alone");
}

/// Go reads `SSL_CERT_FILE` on a Unix other than macOS only: on macOS
/// `crypto/x509` verifies with the Security framework and the keychains
/// (`root_darwin.go`; `root_unix.go` does not build there), so no variable
/// gives Go the CA of the job.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn go_takes_the_bundle_by_ssl_cert_file() {
    if cfg!(target_os = "macos") {
        // A limit of Go, not a client that the host lacks: no failure, also
        // when each client is required.
        pod_testkit::host::skip_line(
            "Go on macOS verifies with the keychains and reads no SSL_CERT_FILE or SSL_CERT_DIR",
        );
        return;
    }
    let Some(go) = need("POD_BROKER_TEST_GO", "go") else { return };
    let lab = Lab::start("go", clients::echo_auth).await;
    let program = sources::build_go(&go, &lab.dir).unwrap_or_else(|why| panic!("{why}"));
    bearer_legs(&lab, &program.to_string_lossy(), script(&[]), "go", Some(&["SSL_CERT_FILE"])).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn java_takes_the_trust_store_of_java_tool_options() {
    let Some(java) = need("POD_BROKER_TEST_JAVA", "java") else { return };
    let lab = Lab::start("java", clients::echo_auth).await;
    let source = sources::write(&lab.dir, "Client.java", sources::JAVA);
    let source = source.to_string_lossy().into_owned();
    bearer_legs(&lab, &java, script(&[&source]), "java", Some(&["JAVA_TOOL_OPTIONS"])).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deno_takes_the_bundle_by_deno_cert() {
    let Some(deno) = need("POD_BROKER_TEST_DENO", "deno") else { return };
    let lab = Lab::start("deno", clients::echo_auth).await;
    bearer_legs(
        &lab,
        &deno,
        script(&["eval", "--quiet", sources::DENO]),
        "deno",
        Some(&["DENO_CERT", "NODE_EXTRA_CA_CERTS"]),
    )
    .await;
}

/// A package client: its program and variables (`{host}` in a value is the
/// host of the run), the variables that it alone reads (`own`), the value of
/// `Authorization` for a token (`sent`) and what the job prints for the
/// placeholder (`shown`).
struct Package<'a> {
    client: &'a str,
    program: &'a str,
    env: Vec<(String, String)>,
    own: Option<&'static [&'static str]>,
    sent: fn(&str) -> String,
    shown: fn(&str) -> String,
}

/// The runs of a package client to the host of the credential with no
/// bundle and with the bundle (with `own`, also with each variable but
/// `own`), then to a host with no credential; `args` gives the arguments
/// for a host. The far end got `sent` of the secret, and of the placeholder
/// on the host with no credential; the job printed `shown`.
async fn package_legs(lab: &Lab, p: &Package<'_>, args: impl Fn(&str) -> Vec<String>) {
    let run = |host: &str, trust: Trust| {
        let args = args(host);
        let env: Vec<(String, String)> = p.env.iter().map(|(n, v)| (n.clone(), v.replace("{host}", host))).collect();
        async move {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let env: Vec<(&str, &str)> = env.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
            lab.run_trusting(p.program, &args, &env, trust).await
        }
    };
    let client = p.client;
    // mix ends with 0 after a refused leaf: a run that does not print
    // `shown` is a refusal.
    let refused = |mut ran: Run| {
        if !ran.stdout.contains(&(p.shown)(PH)) && ran.code == Some(0) {
            ran.code = Some(1);
        }
        ran
    };
    refused_with_no_bundle(lab, &refused(run(HOST, Trust::None).await), client);
    if let Some(vars) = p.own {
        refused_without(lab, &refused(run(HOST, Trust::Without(vars)).await), client, vars);
    }
    for (host, value) in [(HOST, SECRET), (OTHER, PH)] {
        lab.far.seen.lock().unwrap().clear();
        let ran = run(host, Trust::All).await;
        assert_eq!(ran.code, Some(0), "{client} to {host}: {ran}");
        let seen = lab.far.seen();
        assert!(seen.contains(&format!("authorization: {}\r\n", (p.sent)(value))), "{client} to {host}: {seen}");
        assert!(host == OTHER || !seen.contains(PH), "{client}: {seen}");
        assert!(ran.stdout.contains(&(p.shown)(PH)), "{client} to {host}: {ran}");
        assert!(!ran.all().contains(SECRET), "{client} to {host}: {ran}");
    }
    println!("clients: {client} took the bundle, the far end got the value, the job read the placeholder");
    println!("clients: {client} reached a host with no credential by the roots of the bundle, with no swap");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pip_reads_an_index_that_needs_a_token() {
    let Some(python) = need("POD_BROKER_TEST_PYTHON", "python3") else { return };
    if let Err(why) = clients::python_imports(&python, "pip") {
        clients::skip(&why);
        return;
    }
    let lab = Lab::start("pip", far::pip).await;
    let p = Package {
        client: "pip",
        program: &python,
        env: vec![("PIP_NO_INPUT".into(), "1".into())],
        own: None,
        sent: |token| format!("Basic {}", b64(&format!("__token__:{token}"))),
        shown: |_| "foo (1.0.0)".into(),
    };
    package_legs(&lab, &p, |host| {
        let index = format!("https://__token__:{PH}@{host}/simple/");
        let args = ["-I", "-m", "pip", "index", "versions", "foo", "--index-url", &index, "--no-cache-dir"];
        let more = ["--disable-pip-version-check", "--retries", "0", "--timeout", "20"];
        args.iter().chain(&more).map(|a| a.to_string()).collect()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn npm_reads_a_registry_that_needs_a_token() {
    let Some(npm) = need("POD_BROKER_TEST_NPM", "npm") else { return };
    let lab = Lab::start("npm", far::npm).await;
    // The line of actions/setup-node: npm reads the token from the
    // environment of the job.
    let npmrc = format!("//{HOST}/:_authToken=${{JOB_TOKEN}}\n//{OTHER}/:_authToken=${{JOB_TOKEN}}\n");
    std::fs::write(lab.dir.join("home/.npmrc"), npmrc).unwrap();
    let p = Package {
        client: "npm",
        program: &npm,
        env: vec![("npm_config_update_notifier".into(), "false".into())],
        // npm runs the `node` of `PATH`.
        own: Some(clients::node_own("node")),
        sent: |token| format!("Bearer {token}"),
        shown: |token| format!("Bearer {token}"),
    };
    package_legs(&lab, &p, |host| {
        let registry = format!("https://{host}/");
        ["view", "foo", "description", "--registry", &registry, "--fetch-retries", "0"].map(String::from).to_vec()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gcloud_takes_the_bundle_by_its_custom_ca_file() {
    let Some(gcloud) = need("POD_BROKER_TEST_GCLOUD", "gcloud") else { return };
    let lab = Lab::start("gcloud", far::gcloud).await;
    let port = lab.proxy.rsplit(':').next().unwrap().to_string();
    let config = lab.dir.join("home/gcloud").to_string_lossy().into_owned();
    // The proxy as properties: an installation of the CLI can name a proxy
    // of its own, and a property comes before `HTTPS_PROXY`.
    let env = [
        ("CLOUDSDK_CONFIG", config.as_str()),
        ("CLOUDSDK_API_ENDPOINT_OVERRIDES_CLOUDRESOURCEMANAGER", "https://{host}/"),
        ("CLOUDSDK_AUTH_ACCESS_TOKEN", PH),
        ("CLOUDSDK_CORE_DISABLE_PROMPTS", "1"),
        ("CLOUDSDK_COMPONENT_MANAGER_DISABLE_UPDATE_CHECK", "1"),
        ("CLOUDSDK_CORE_DISABLE_USAGE_REPORTING", "1"),
        ("CLOUDSDK_PROXY_TYPE", "http"),
        ("CLOUDSDK_PROXY_ADDRESS", "127.0.0.1"),
        ("CLOUDSDK_PROXY_PORT", &port),
        ("CLOUDSDK_PROXY_USERNAME", "job1"),
        ("CLOUDSDK_PROXY_PASSWORD", common::TOKEN),
    ];
    let p = Package {
        client: "gcloud",
        program: &gcloud,
        env: env.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect(),
        own: Some(&["CLOUDSDK_CORE_CUSTOM_CA_CERTS_FILE"]),
        sent: |token| format!("Bearer {token}"),
        shown: |token| format!("Bearer {token}"),
    };
    package_legs(&lab, &p, |_| {
        ["projects", "describe", "job-project", "--format=value(name)"].map(String::from).to_vec()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hex_takes_the_bundle_by_hex_cacerts_path() {
    let Some(mix) = need("POD_BROKER_TEST_MIX", "mix") else { return };
    let mix_home = match clients::mix_home_with_hex() {
        Ok(home) => home,
        Err(why) => return clients::skip(&why),
    };
    let lab = Lab::start("hex", far::hex).await;
    let hex_home = lab.dir.join("home/hex").to_string_lossy().into_owned();
    let env = [
        ("MIX_HOME", mix_home.as_str()),
        ("HEX_HOME", hex_home.as_str()),
        ("HEX_API_URL", "https://{host}/api"),
        ("HEX_API_KEY", PH),
        ("ELIXIR_ERL_OPTIONS", "+fnu"),
    ];
    let p = Package {
        client: "hex",
        program: &mix,
        env: env.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect(),
        own: Some(&["HEX_CACERTS_PATH"]),
        sent: |token| token.to_string(),
        shown: |_| "job-user".into(),
    };
    package_legs(&lab, &p, |_| ["hex.user", "whoami"].map(String::from).to_vec()).await;
}
