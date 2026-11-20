# pod-broker

The egress broker of a job: an HTTP proxy (CONNECT, and plain requests in
absolute form) that holds each job to its policy. A job proves itself
with its token in `Proxy-Authorization`. The broker reads the target
strictly, applies the fixed denies, the denies of the host and the policy
of the job, resolves a name once, checks each address, and dials only a
checked address, directly or through an upstream proxy. Each connection
writes one JSON audit line with no secret value. A job ends at `drop`, at
its TTL, or at a new `load` of its id, and each of its connections ends
with it.

The crate is a library (docs/decisions.md, C27). A product hosts it in a
thin `main`: it reads its own flags and variables, fills the settings,
gives the resolver, the way out and the audit sink, and binds the
listeners. The crate reads no variable, prints nothing, and writes its
diagnostics to `tracing`.

The TLS end (`TlsEnd`, T-100) gives each job a CA of its own and reads
the tunnels to the hosts of its credentials: the job holds a placeholder,
the far end gets the value of the secret, and each answer comes back with
the placeholder ("Placeholder credentials" below).

## Use

```rust
use std::sync::Arc;

let resolve: Arc<dyn pod_core::Resolve> = Arc::new(pod_dns::Resolver::new(dns_settings));
let settings = pod_broker::Settings {
    deny: pod_broker::DenyList::read("/etc/podrunner/deny.txt".as_ref())?,
    host_addresses,                     // the addresses of the host
    ..Default::default()
};
// The way out: an upstream proxy (HTTPS_PROXY, read by the product), or
// direct. Its resolver resolves the name of the proxy only.
let way_out = pod_broker::upstream("podrunner", proxy, Arc::new(pod_proxy::resolve::System::new()));
let audit = Arc::new(pod_broker::JsonLines::new(std::io::stderr()));
let broker = Arc::new(pod_broker::Broker::new(settings, resolve, way_out).with_audit(audit));

let control = pod_broker::control::bind_unix(&run_dir.join("control.sock"), 0o600)?;
let jobs = pod_broker::control::bind_unix(&run_dir.join("broker.sock"), 0o660)?;
tokio::spawn(pod_broker::control::serve_control(broker.clone(), control));
broker.serve_unix(jobs).await?;
```

A job reaches the broker on its Unix socket (for example through
`podnet pod -x http://unix:/PATH`, or a forwarder of a TCP port that the host
gives), or on a TCP address on loopback that the host gives to
`serve_tcp`. It sends its token as the password of Basic (the user is the
job id or empty) or as `Bearer TOKEN`.

A listener can also belong to one job (`serve_unix_job(job, listener)`,
`serve_tcp_job`): the socket of a unit is then its identity, and no token
reaches the unit. Each connection on it is the job's with no
`Proxy-Authorization`; a header of another job, or of no job, gets 407. A
reload of the job keeps the listener with the new policy; a drop or the
TTL ends it with each of its connections (the audit line
`broker.listener.end`). The host binds and removes the socket file.

### Placeholder credentials

The policy names a placeholder, the name of a secret and its hosts; the
host gives the value at the load. The broker needs its TLS end, with the
provider and the trust of the host for its TLS to the far ends:

```rust
let provider = Arc::new(pod_tls::crypto::provider());
let tls = pod_tls::Context::new(provider.clone(), pod_tls::Settings::new(pod_tls::Words::new("podrunner")));
let trust = tls.config_for(&pod_tls::Trust::Default)?;
let broker = pod_broker::Broker::new(settings, resolve, way_out)
    .with_tunnel_end(Arc::new(pod_broker::TlsEnd::new(provider, trust)));

let mut load = pod_broker::Load::new(job, token, ttl, policy); // the policy of the offer
load.secrets = pod_broker::Secrets::new().with("github", value);
let info = broker.load(load)?;
// The job trusts its CA: write the bundle and name it in its environment.
let ca_pem = info.ca_pem.expect("a CA for a job with a credential");
let roots = std::fs::read("/etc/ssl/certs/ca-certificates.crt")?;
pod_broker::ca::write_bundle(&unit_dir.join("ca.pem"), &ca_pem, &roots)?;
for (var, path) in pod_broker::ca::env(&unit_dir.join("ca.pem")) {
    command.env(var, path); // SSL_CERT_FILE, GIT_SSL_CAINFO, NODE_EXTRA_CA_CERTS, DENO_CERT, ...
}
// A JVM reads a trust store: the same certificates in PKCS #12, before the
// options of the job.
pod_broker::ca::write_trust_store(&unit_dir.join("ca.p12"), &ca_pem, &roots)?;
let java = pod_broker::ca::java_options(&unit_dir.join("ca.p12"), job_env.get("JAVA_TOOL_OPTIONS").map(String::as_str))?;
command.env("JAVA_TOOL_OPTIONS", java);
command.env("GITHUB_TOKEN", "PODCRED_GITHUB_TOKEN_0001"); // the placeholder
```

The TLS end reads a tunnel to a host that a credential names, or to a
host whose rules each have a path, when the job starts it with a TLS
handshake; each other tunnel passes as it is (docs/decisions.md, C812).
Inside the tunnel it checks the SNI and each `Host` against the CONNECT
target (421), the target in origin form (400) and each path (403); it
swaps each placeholder for its value in the places of its credential;
it opens its own TLS to the far end with the trust of the host; and it
puts the placeholder back in each answer, in a header or in the body, as
the value, its percent-encoded form, its form in JSON, its base64, and
the base64 of each `Basic` value that it wrote (C815). The ALPN is
`http/1.1` on each side.

| Bound (`tls_end::Bounds`) | What the TLS end does |
| --- | --- |
| a request body with a length up to `max_body` (1 MiB) | read whole, swapped, sent with its new length |
| a larger request body, or one with a `Content-Encoding` | passed with no swap; the audit note `body-large` or `body-encoded` |
| a chunked request body | swapped as it comes, sent in chunks |
| `Accept-Encoding` of a job with a secret | `identity` in its place |
| an answer with a `Content-Encoding` | 502 to the job, the note `answer-encoded` |
| an answer with a length up to `max_body` | scrubbed whole, sent with its new length |
| a larger, chunked or close-delimited answer | scrubbed as it comes, sent in chunks (HTTP/1.1) or to the close (HTTP/1.0) |
| `Expect: 100-continue` | the broker answers 100 itself |
| 101 | the bytes of the job pass; the bytes of the far end come back scrubbed |

The values never reach a line, an answer of the control or a text of an
error; `Secrets`, `Vault` and `JobCa` show no value in their debug form.

The job also needs the broker as its proxy: `HTTPS_PROXY` (and
`https_proxy`) with the job token as the password, unless the listener
belongs to the job. `tests/tls_clients.rs` runs the clients of a job
with the bundle, each variable of `ca::env` and `JAVA_TOOL_OPTIONS` of
`ca::java_options` (`scripts/ci/broker-clients-tests.sh` in CI). Each
client reads one variable or more of the list. A client with a variable in
the column "Only" refuses the leaf of the job when that variable is
missing and each other variable is set (a test holds it):

| Client | The variable that it reads | Only | Note |
| --- | --- | --- | --- |
| curl | `CURL_CA_BUNDLE`, `SSL_CERT_FILE` | | |
| git (GnuTLS or OpenSSL) | `GIT_SSL_CAINFO` | | the Basic of `http.extraHeader`, also as `AUTHORIZATION: basic` (actions/checkout), and a token in the URL after a 401 |
| Python `urllib`, `ssl` | `SSL_CERT_FILE` | yes | |
| Python `requests` | `REQUESTS_CA_BUNDLE`, `CURL_CA_BUNDLE` | | not `SSL_CERT_FILE` |
| pip 24.0 | `REQUESTS_CA_BUNDLE`, `CURL_CA_BUNDLE` | | `PIP_CERT` and `cert` of `pip.conf` come before them; a token in the URL of the index |
| Node.js | `NODE_EXTRA_CA_CERTS` | yes | `fetch` and `https` use `HTTPS_PROXY` only with `--use-env-proxy` or `NODE_USE_ENV_PROXY=1` (Node.js 22.21, 24.5 and later) |
| npm 10 | `NODE_EXTRA_CA_CERTS` | yes | `cafile` and `ca` of an `.npmrc` come before it; the `_authToken` of an `.npmrc` from the environment |
| cargo | `CARGO_HTTP_CAINFO`; on Linux also `SSL_CERT_FILE` | | the token of a registry is the whole value of `Authorization`; on macOS, cargo does not read `SSL_CERT_FILE` |
| Go (`crypto/x509`) | `SSL_CERT_FILE` on a Unix other than macOS | yes | also `SSL_CERT_DIR`; on macOS Go verifies with the keychains and reads no variable, so a Go job on macOS refuses the leaf of the job (its test skips there) |
| Java (each JVM) | `JAVA_TOOL_OPTIONS` of `ca::java_options` | yes | the trust store of `ca::write_trust_store`; a JVM reads no proxy of the environment |
| Deno 2.9 | `DENO_CERT`, `NODE_EXTRA_CA_CERTS` | both | `fetch` takes `HTTPS_PROXY` |
| Google Cloud CLI | `CLOUDSDK_CORE_CUSTOM_CA_CERTS_FILE` | yes | with a proxy of its properties, or a CA file of its installation, it reads no other variable |
| Hex of Elixir (`mix`) | `HEX_CACERTS_PATH` | yes | the key of `HEX_API_KEY` is the whole value of `Authorization`; `mix` ends with 0 after a refused leaf |

A client that sends a `Host` that is not the target gets 421 (C816): for
example, `http.request` of Node.js with no agent over a socket of the
tunnel writes `Host: NAME:80`. A host that no credential names gets its
tunnel as it is, and the job checks its leaf with the roots of the
bundle.

### An upstream that takes names only

Some proxy-only hosts have an upstream CONNECT proxy that refuses an IP
literal, or no resolver that works. There set `Settings::names_upstream`:
the broker checks the name and each address of its one resolve as
before, then sends the checked name to the upstream, once, in place of
the checked address. The audit line of the connection holds
`"sent":"name"` and the address that the check read. The upstream
resolves the name again and can reach another address, so ask it first:

```rust
match broker.check_upstream().await {
    pod_broker::UpstreamCheck::RefusesNonPublic => {} // the job can be untrusted
    _ => {} // hold `advisory` only: serve no untrusted job through it
}
```

`check_upstream` sends `Settings::canary` (default `localhost`, a name of
a non-public address) through the way out, and writes the audit line
`broker.upstream.canary`. The default stays the checked address
(docs/decisions.md, C28 and C51).

The load flags each allow rule under a suffix where anyone can register a
name or put content (`names::OPEN_SUFFIXES`: `github.io`,
`githubusercontent.com`, `pages.dev`, `workers.dev` and others): the
policy keeps the rule, `JobInfo::flagged` and the control answer
(`"flagged"`) name it, and the `broker.job.load` line is at `warn`.

## API

| Item | What |
| --- | --- |
| `Policy`, `Mode`, `Rule` | the policy of a job, version 1: `from_json`, `from_toml`, `from_value`, `read` (TOML for `.toml`, else JSON), `compile` |
| `Compiled`, `Verdict` | a checked policy: `verdict(target, path)` gives `Allow(rule)`, `PathsOnly` or `Deny(rule)`; `allows_path` for a TLS end |
| `Compiled::no_rule(target)`, `NoRule` | why no rule allows a target: `Host`, `Port(ports)` (the ports that the rules of the host take) or `Path`; `NoRule::words(target)` is the text of the 403 (T-609) |
| `DenyList` | nets, addresses and names: `fixed()`, `host_addresses(..)`, `parse(text, prefix)`, `read(path)`, `check_ip`, `check_name`, `check_host`; `deny::check_port` |
| `target::{parse_host, parse_authority, parse_absolute, normal_path}` | the strict reading of a target (400 on an ambiguous one) |
| `Broker::new(settings, resolve, dial)` | a broker; `with_audit`, `with_tunnel_end` |
| `Broker::load(Load)`, `drop_job(job)`, `list()` | the jobs; `Load { job, token: Token, ttl, policy }`, `JobInfo` |
| `Broker::handle(io)`, `serve_tcp`, `serve_unix` | one connection, and the listeners of the jobs |
| `Broker::handle_bound(io, job)`, `serve_unix_job(job, listener)`, `serve_tcp_job` | one connection, and a listener, of one job with no token (T-160); a job that is not loaded is refused (`NotFound`) |
| `Broker::control_line(line)` | the answer to one line of the control |
| `control::{bind_unix, serve_control}` | a Unix socket with a mode, and the control on it (Unix only) |
| `upstream(app, proxy, resolve)` | the way out: a `pod_proxy::Dialer` through `proxy` (HTTP CONNECT with Basic or Digest, or SOCKS5), or direct |
| `Settings::names_upstream`, `Settings::canary`, `Broker::check_upstream()`, `UpstreamCheck` | an upstream that takes names only, and its canary (T-161) |
| `names::{OPEN_SUFFIXES, open_suffix}`, `JobInfo::flagged`, `Compiled::flagged` | the allow rules under an open suffix (T-161) |
| `Audit`, `Line`, `JsonLines`, `Tracing`, `Memory` | the audit sink and its line |
| `TunnelEnd`, `Tunnel`, `Splice`, `Counts`, `Report` | the inside of an allowed tunnel, and what its end did for the audit line |
| `TlsEnd::new(provider, client_config)`, `with_bounds`, `tls_end::{Bounds, MAX_BODY}` | the TLS end (T-100) |
| `Credential`, `Place`, `credentials::Checked`, `Compiled::credentials`, `Compiled::needs_tls_end` | the placeholder credentials of a policy |
| `Secrets` (`new`, `with`), `Load::secrets`, `Load::new` | the values of the secrets, given at the load |
| `JobInfo::ca_pem`, `Broker::ca_pem(job)`, `ca::{JobCa, bundle, write_bundle, env}` | the CA of a job, its bundle and its variables |
| `Vault` | the CA and the secrets of one job |
| `inner::{check, read, Inner, Refused}`, `target::parse_origin` | the checks of a request inside a tunnel |
| `replace::{Replace, replace_text, percent, json_escaped}` | the swap and the scrub of a stream |
| `Token` | a job token: wiped when dropped, shown by no format |

The failures are `pod_core::Failure`. A bad policy, job id, token or TTL
is `Class::Usage` (exit 64); a refusal of a deny or of the policy is
`Class::Policy` in the audit line.

## The policy

`schemas/broker-policy.v1.schema.json` is the JSON Schema. It comes from
the type `Policy` (`src/schemas.rs` writes it and compares it, with
`schemas/log.v1.schema.json` from the audit `Line`; after a change of a
type: `POD_BROKER_WRITE_SCHEMAS=1 cargo test -p pod-broker --lib schemas`).
The feature `schema` gives the `JsonSchema` of the policy to the tests of a
consumer. The TOML form has the same keys.

```json
{
  "schema": 1,
  "mode": "allow",
  "allow": [
    { "host": "api.github.com", "path": "/repos/owner", "ports": [443] },
    { "host": "*.githubusercontent.com" },
    { "host": "203.0.113.9", "ports": [8443] }
  ],
  "deny": ["198.51.100.0/24", "*.ads.example"]
}
```

* `mode`: `allow` (only the targets of a rule) or `open` (each target
  that no deny holds; an open policy has no allow rule).
* `host`: a name, `*.NAME` (each name below `NAME` on whole labels, not
  `NAME` itself: `*.example.com` holds `a.example.com`, not
  `badexample.com`), or an address. A name matches in lower case with no
  dot at its end.
* `path`: a prefix on whole segments: `/v1/repos` holds `/v1/repos` and
  `/v1/repos/x`, not `/v1/reposx`. A plain request shows its path. A
  tunnel shows none: a CONNECT to a host whose rules each have a path is
  refused (`policy:path-needs-tls-end`) until a `TunnelEnd` that reads each
  request (T-100) checks the paths.
* `ports`: none or empty: each port.
* `deny`: extra denies, after the fixed ones.
* `credentials`: the placeholder credentials (T-100). Each has
  `placeholder` (16 to 256 of `A-Z a-z 0-9 _ . -`), `secret` (the name of a
  secret whose value the host gives at the load), `hosts` (as the `host`
  of a rule) and `in` (`header`, `basic`, `query`, `body`; none: `header`
  and `basic`). A credential needs the TLS end of the broker.

```json
{ "schema": 1, "mode": "open",
  "credentials": [ { "placeholder": "PODCRED_GITHUB_TOKEN_0001", "secret": "github",
                     "hosts": ["api.github.com", "*.githubusercontent.com"], "in": ["header", "basic"] } ] }
```

## The denies

No policy lifts a fixed deny. The broker checks the port, the host (a
name or an address), and each address of the one resolve. An IPv4
address inside an IPv6 address (`::ffff:a.b.c.d`, `::a.b.c.d`, NAT64
`64:ff9b::/96`, 6to4 `2002::/16`) is checked as the IPv4 address.

| Rule | Nets, names or ports |
| --- | --- |
| `fixed:metadata` | `169.254.169.254`, `169.254.170.2`, `168.63.129.16`, `100.100.100.200`, `fd00:ec2::254`; `metadata.google.internal`, `metadata.goog`, `metadata`, `instance-data`, `instance-data.ec2.internal` |
| `fixed:unspecified` | `0.0.0.0/8`, `::/128` |
| `fixed:private` | `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `fc00::/7` |
| `fixed:cgnat` | `100.64.0.0/10` |
| `fixed:loopback` | `127.0.0.0/8`, `::1/128`; `localhost`, `*.localhost` |
| `fixed:link-local` | `169.254.0.0/16`, `fe80::/10` |
| `fixed:special` | `192.0.0.0/24` |
| `fixed:site-local` | `fec0::/10` |
| `fixed:multicast` | `224.0.0.0/4`, `ff00::/8` |
| `fixed:reserved` | `240.0.0.0/4` (with `255.255.255.255`) |
| `fixed:smtp` | port 25 |
| `host:address` | each address in `Settings::host_addresses` |
| `file:LINE` | each line of the deny file of the host (`DenyList::read`: a net, an address, a name or `*.NAME`; `#` starts a comment) |
| `policy:deny[N]` | each extra deny of the policy |

## The answers

The words of PodUtils/podvm#184. Each answer closes the connection.

| Status | When | Class |
| --- | --- | --- |
| 200 | a CONNECT that the checks allowed; the tunnel follows | — |
| 400 | a malformed head; an ambiguous target: user information, a host in percent-encoding, an IPv4 address in another spelling (`2130706433`, `0177.0.0.1`, `127.1`), an IPv6 address with no brackets, a port out of range or with a leading zero, `%2e`, `%2f`, `%5c`, a dot segment, `//`, a fragment; a target in origin form; two `Host` headers; a body with two framings | usage |
| 408, 431, 505 | no head within `Settings::head_timeout`; a head over 16 KiB; a version of HTTP other than 1.x | timeout, usage |
| 407 | no `Proxy-Authorization`, a scheme other than Basic or Bearer, a token of no job, or a Basic user that is not the job of the token | refused |
| 421 | the `Host` and the CONNECT target or the URL differ; inside a tunnel of the TLS end, the SNI or a `Host` is not the CONNECT target | usage |
| 403 | a deny or the policy refused the target | policy |
| 502 | the resolve, the upstream proxy or the far end failed | of the failure |
| 504 | the resolve or the far end did not answer in time | timeout |

The text after the status names the cause (T-609):

* 403 of a deny: `denied by the rule fixed:private`.
* 403 of `policy:no-rule`: the half of the rules that failed, for example
  `denied by the rule policy:no-rule: the rules of the host example.com
  take the port 443, not 80`, or `no allow rule names the host
  example.com`.
* 502 of a refusal of the upstream proxy: its status by the RFC 9110
  name, the proxy and the target, for example `the upstream proxy
  http://proxy.example:3128 answered "403 Forbidden" to CONNECT
  192.0.2.10:443`. The reason phrase and the body of the proxy stay out.
* Another 502 or 504: the failure of the resolve or of the dial, and
  `(through the upstream proxy LABEL)` when the dial went through one.

No text holds a credential: a proxy label has none, and the broker
removes the user information of each URL in the text.

A plain request goes to the far end in origin form with its own `Host`,
with no `Proxy-Authorization` and no hop-by-hop header, with
`Connection: close`; the broker reads no second request on the
connection.

## The control

A Unix socket of mode 0600 (`control::bind_unix`); a peer of another user
is refused. One JSON request on each line, one answer on each line, each
answer with `"v": 1`. An answer never holds a token.

```text
{"op":"load","job":"J","token":"T","ttl_s":600,"policy":{"schema":1,"mode":"open"}}
{"v":1,"ok":true,"op":"load","job":"J","mode":"open","rules":0,"expires_in_s":600}
{"op":"drop","job":"J"}
{"v":1,"ok":true,"op":"drop","job":"J","dropped":true}
{"op":"list"}
{"v":1,"ok":true,"op":"list","jobs":[{"job":"J","mode":"open","rules":0,"expires_in_s":590,"open":1}]}
{"v":1,"ok":false,"class":"usage","error":"..."}
```

A `load` of a policy with credentials gives the values in `secrets`
(`{"NAME":"VALUE"}`), and its answer has `ca_pem`, the CA of the job.

A job id has 1 to 128 of `A-Z a-z 0-9 . _ : -`. A token has 16 to 4096
printable ASCII characters with no space; the broker keeps only its
SHA-256, and one token belongs to one job. A TTL is from 1 s to 7 days.

## The audit

One line for each connection and each `load` and `drop`, in the form of
`schemas/log.v1.schema.json`:

```json
{"schema":1,"time":"2026-11-09T12:00:00.000Z","level":"info","event":"broker.connection",
 "fields":{"job":"J","kind":"connect","host":"example.com","port":443,"addr":"93.184.215.14:443",
           "decision":"allow","rule":"open","status":200,"up":517,"down":4096,"ms":120}}
```

`kind` is `connect`, `http` or `request` (no target was read). A tunnel that
the TLS end read has `tls` (`end`), `requests`, `swaps`, `scrubs`, and
`notes` (`body-large`, `body-encoded`, `answer-encoded`, `sni-mismatch`,
`host-mismatch`, `path-denied`, `upstream-tls`, `not-tls`, `upgrade` and
the other words of a refusal); `broker.job.load` has `credentials` (the
names of the secrets) and `ca`. A refusal
has `decision` `deny`, its `rule` and `status`, and `stage` and `class`. A
connection that the end of its job cut has `end` `job-ended` and the
class `revoked`. A line never holds a token, a header, a path or a query.

## Adoption map

| Consumer | Use |
| --- | --- |
| podvm | `--net broker` (PodUtils/podvm@T-1665): the payload's egress goes to the Unix socket of a broker that podvm hosts; the 407, 403, 502 and 504 of podvm#184 |
| podssh | the local egress daemon of A-S-11 (`podssh egress serve --listen unix:PATH`): a thin `main` over `Broker`, with podssh's `pod_proxy` dialer and `pod_dns` resolver as the way out |
| podrunner (later) | a broker for each host; the `policy` object of the job source (githome G-04) goes to `load`; the audit lines go back to the source |
| cfb | the egress proxy of CFB-230 (A-X-05, A-V-03, A-N-13), with the TLS end of T-100 |
| githome (through podrunner) | G-35: the `credentials` of the `policy` of an offer; podrunner gives the values at the load and the bundle to the unit |

## Sources

New code (T-099, T-100). The vocabulary of the answers is
PodUtils/podvm#184; the reader of a request's head and the body reader
are `pod-proxy`'s; the TLS server and client are `pod-tls`'s; the P-256
keys and the PEM of the CA are `pod-acme`'s. No
file is imported, so the crate has no `SOURCES` file.
