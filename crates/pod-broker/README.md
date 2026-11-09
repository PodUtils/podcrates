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

T-099 made this crate. T-100 adds the TLS end with a CA for each job, the
placeholder credentials and the scrub of answers, on the seam
`TunnelEnd`.

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
`podnet pod -x http+unix:PATH`, or a forwarder of a TCP port that the host
gives), or on a TCP address on loopback that the host gives to
`serve_tcp`. It sends its token as the password of Basic (the user is the
job id or empty) or as `Bearer TOKEN`.

## API

| Item | What |
| --- | --- |
| `Policy`, `Mode`, `Rule` | the policy of a job, version 1: `from_json`, `from_toml`, `from_value`, `read` (TOML for `.toml`, else JSON), `compile` |
| `Compiled`, `Verdict` | a checked policy: `verdict(target, path)` gives `Allow(rule)`, `PathsOnly` or `Deny(rule)`; `allows_path` for a TLS end |
| `DenyList` | nets, addresses and names: `fixed()`, `host_addresses(..)`, `parse(text, prefix)`, `read(path)`, `check_ip`, `check_name`, `check_host`; `deny::check_port` |
| `target::{parse_host, parse_authority, parse_absolute, normal_path}` | the strict reading of a target (400 on an ambiguous one) |
| `Broker::new(settings, resolve, dial)` | a broker; `with_audit`, `with_tunnel_end` |
| `Broker::load(Load)`, `drop_job(job)`, `list()` | the jobs; `Load { job, token: Token, ttl, policy }`, `JobInfo` |
| `Broker::handle(io)`, `serve_tcp`, `serve_unix` | one connection, and the listeners of the jobs |
| `Broker::control_line(line)` | the answer to one line of the control |
| `control::{bind_unix, serve_control}` | a Unix socket with a mode, and the control on it (Unix only) |
| `upstream(app, proxy, resolve)` | the way out: a `pod_proxy::Dialer` through `proxy` (HTTP CONNECT with Basic or Digest, or SOCKS5), or direct |
| `Audit`, `Line`, `JsonLines`, `Tracing`, `Memory` | the audit sink and its line |
| `TunnelEnd`, `Tunnel`, `Splice`, `Counts` | the inside of an allowed tunnel; the seam of T-100 |
| `Token` | a job token: wiped when dropped, shown by no format |

The failures are `pod_core::Failure`. A bad policy, job id, token or TTL
is `Class::Usage` (exit 64); a refusal of a deny or of the policy is
`Class::Policy` in the audit line.

## The policy

`schemas/broker-policy.v1.schema.json` is the JSON Schema. The TOML form
has the same keys.

```json
{
  "version": 1,
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
| 421 | the `Host` and the CONNECT target or the URL differ (T-100 adds the SNI) | usage |
| 403 | a deny or the policy refused the target | policy |
| 502 | the resolve, the upstream proxy or the far end failed | of the failure |
| 504 | the resolve or the far end did not answer in time | timeout |

A plain request goes to the far end in origin form with its own `Host`,
with no `Proxy-Authorization` and no hop-by-hop header, with
`Connection: close`; the broker reads no second request on the
connection.

## The control

A Unix socket of mode 0600 (`control::bind_unix`); a peer of another user
is refused. One JSON request on each line, one answer on each line, each
answer with `"v": 1`. An answer never holds a token.

```text
{"op":"load","job":"J","token":"T","ttl_s":600,"policy":{"version":1,"mode":"open"}}
{"v":1,"ok":true,"op":"load","job":"J","mode":"open","rules":0,"expires_in_s":600}
{"op":"drop","job":"J"}
{"v":1,"ok":true,"op":"drop","job":"J","dropped":true}
{"op":"list"}
{"v":1,"ok":true,"op":"list","jobs":[{"job":"J","mode":"open","rules":0,"expires_in_s":590,"open":1}]}
{"v":1,"ok":false,"class":"usage","error":"..."}
```

A job id has 1 to 128 of `A-Z a-z 0-9 . _ : -`. A token has 16 to 4096
printable ASCII characters with no space; the broker keeps only its
SHA-256, and one token belongs to one job. A TTL is from 1 s to 7 days.

## The audit

One line for each connection and each `load` and `drop`, in the form of
`schemas/log.v1.schema.json`:

```json
{"time":"2026-11-09T12:00:00.000Z","level":"info","event":"broker.connection",
 "fields":{"job":"J","kind":"connect","host":"example.com","port":443,"addr":"93.184.215.14:443",
           "decision":"allow","rule":"open","status":200,"up":517,"down":4096,"ms":120}}
```

`kind` is `connect`, `http` or `request` (no target was read). A refusal
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

## Sources

New code (T-099). The vocabulary of the answers is PodUtils/podvm#184;
the reader of a request's head and the body reader are `pod-proxy`'s. No
file is imported, so the crate has no `SOURCES` file.
