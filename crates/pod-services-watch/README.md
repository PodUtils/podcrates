# pod-services-watch

The watches of `pod-services`. Each `[[watch]]` of a data file names a
watch kind and its arguments; each kind is one implementation of the trait
`Watch` here. The runner checks each watch against the live service and
prints one line for each. The workflow `services.yml` of podcrates runs it
each day and by hand. The crate is a tool of podcrates, as `pod-todo`:
`publish = false`, and no consumer takes it.

The rulings: `docs/decisions.md`, "Services crate", "Services: the
design"; the design: `docs/design.md`, section 4, "`pod-services`".

## The runner

```sh
cargo run -q -p pod-services-watch -- [--service ID]... [--kind KIND]... [--skip-kind KIND]... [--file PATH]...
    [--no-answer-was PATH] [--no-answer-now PATH]
```

* `--service ID`, `--kind KIND`: only these services, only these kinds.
* `--skip-kind KIND`: not this kind. A run of `services.yml` by hand
  that names no service and no kind skips `forward-url`: the daily run
  opens the one session with each relay (call C89).
* `--file PATH`: lay a service file of the same schema over the built-in
  set (`Catalog::lay`), to check a new file before its commit.
* The proxy comes from the variables of the environment (`HTTPS_PROXY`,
  `NO_PROXY`, and the others of `pod_proxy::Environment`).
  `GITHUB_TOKEN`, when set, goes to GitHub's API alone, never to another
  host and never to the output.
* UDP (`stun-binding`) goes direct: a proxy carries TCP alone.
* `--no-answer-was PATH`: the list of the watches with no answer of the
  last scheduled run (call C153); a missing file is no state. A watch
  whose findings are each "no answer" and that the list does not name is
  the first one: its lines say `FAILED (first no answer)`, and it does not
  fail the run. `--no-answer-now PATH` writes the list of this run, one
  watch on each line (`SERVICE KIND N`, N the place of the watch in its
  service, from 1); it needs `--no-answer-was`. `services.yml` gives both
  on a scheduled run and keeps the list in the cache of its job. A run by
  hand gives neither, and each finding fails it.

One line for each watch, then a count:

| Line | Means |
| --- | --- |
| `holds SERVICE KIND: what it saw` | the live service is as the file says |
| `MOVED SERVICE KIND: services/SERVICE.toml FACT is WAS; the service gives NOW (source: ..., read DATE)` | a fact of the file moved: read the source, then change the file with a new `read` date |
| `FAILED SERVICE KIND: why` | the check could not hold: no answer, or an answer of another kind |
| `FAILED (first no answer) SERVICE KIND: why` | the host gave no answer, and the last scheduled run had an answer (or no state); no failure of the run |
| `skipped SERVICE KIND: why` | the watch cannot speak a form of the service (a transport of a proxy link); no pass and no failure |

Exit 0: each watch holds, is skipped, or is the first with no answer.
Exit 1: a watch found a fact that moved, or failed. Exit 2: a fault of the
command line or of a file.

"No answer" is a class that the way out gives where the failure is made,
never a parse of words: the dial's error (a connect that timed out, no
route, a connect refused or reset), or a TLS handshake that the network
ended (a reset, a close, a time limit) before the first byte of the
server. Through a proxy, the proxy answers: its refusal is a failure of
another kind.

## The library

| Item | What |
| --- | --- |
| `Watch` (`kind`, `check`) | one watch kind |
| `kinds()`, `find(word)` | each watch kind; the kind of a word |
| `run(service, watch, net)` | one check, as a `Report`; a kind that the crate does not know is a failure |
| `Outcome` (`seen`, `findings`, `skipped`, `no_answer`), `Finding` (`Moved`, `Failed`, `NoAnswer`; `prefixed`) | the result of one check |
| `Report` (`lines`, `holds`, `mark_first`, `is_first`) | the lines of one watch |
| `answer::State` (`parse`, `key`, `note`, `now`, `text`) | the rule of call C153: the list of the watches with no answer of the last scheduled run, and of this run |
| `Net` (`context`, `dial`, `trust`, `client`, `dns`, `services`, `bases`, `github_token`, `today`, `limit`), `Bases` (`public`, `at`) | the way out of each check: the runner fills it, a test gives its stand-ins |
| `args::Args` | the arguments of a watch and the values that they name |
| `schema::arguments(kind)`, `schema::faults(service, watch, is_service)` | the arguments of each watch kind, and the faults of one watch |
| `schema::kind_facts(kind)` (`KindFacts`: `endpoints`, `need`, `optional`, `words`, `when`), `schema::service_faults(service)` | the endpoints and the facts of a service kind with a table (`file-host`, T-416), and the faults of one service: a missing endpoint or fact, a type, a word, a fact that the kind does not name, a fact that a word needs |

The library reads no variable and no file, and prints nothing.

## The watch kinds

`crates/pod-services/README.md`, "The schema", has the arguments of each
kind. Each kind goes through the dial of `pod-proxy` and the TLS of
`pod-tls`; the DNS kinds ask the resolvers of DNS over HTTPS of `pod-dns`.

| Kind | Module | How |
| --- | --- | --- |
| `http-json`, `http-status`, `http-text`, `github-ssh-keys` | `http.rs` | a GET; a JSON pointer; with `member`, each host of the role is in the document, and more hosts there are counted, not found; with `key`, each endpoint is the item of the list whose member `key` is its fact `fact-prefix` + NAME, with its host in a member of `member`, its port in `port-member` and `true` in each member of `true-members`; more items with the port and the true members are counted, not found; with `url-member`, the URL in that member of each item adds the members `host`, `port` and each parameter of its query (Syncthing's pool). A host compares in lower case, an IP address in its one text form |
| `http-sha256` | `sha.rs` | a streamed GET (a `Range` for a range of sectors); the length is the `Content-Length` or the total of `Content-Range` |
| `tls-name`, `tls-names` | `tls.rs` | a verified handshake; for `tls-names`, one for the host and one for the address, at each address; an IPv6 address is not checked on a host with no route for IPv6 (`Net::ipv6`, which the runner probes) |
| `dns-srv`, `dns-addresses`, `doh-answer` | `dns.rs` | DNS over HTTPS (`pod_dns::Way`); `doh-answer` asks each resolver of the service in the one form, each IPv6 one only with a route for IPv6 |
| `stun-binding` | `stun.rs` | a Binding request over UDP (`pod_proxy::stun`), three tries |
| `websocket-upgrade`, `forward-banner` | `ws.rs` | the upgrade of `pod-ws`, which offers the subprotocol of `protocol`; a forwarder's target in the query, in a first text frame (`{"Host":...,"Port":...}`), or in a Wisp CONNECT |
| `proxy-banner` | `proxy.rs`, `xhttp.rs` | Trojan and VLESS over TCP (`tcp`, `raw`), and VLESS over XHTTP (`xhttp`, `splithttp`) in the mode `packet-up`, with or without TLS. XHTTP follows the client of Xray-core at `c43ee98ec7c5a9d34b32cedf891b4c8707e9c8b0`: a streamed GET of `PATH/SESSION` for the downloads, a POST of `PATH/SESSION/SEQ` for each upload (the VLESS request in the upload of sequence 0), the padding `x_padding` in the query of a `Referer`, a random UUID as the session id; `auto` is `packet-up`. TLS offers the `alpn` of the link, else `h2` and `http/1.1`; the protocol that the server chose gives HTTP/1.1 (a connection for the GET, one for the POST) or HTTP/2 (two streams of one connection); with no choice, the version of Xray's client. Skipped: another transport or security, the XHTTP modes `stream-up` and `stream-one`, HTTP/3, the XHTTP `extra` settings, a VLESS `flow` or `encryption` |
| `forward-url` | `forward.rs` | an SSH session of russh (call C88) over the dial, within 45 s (`seconds`): the authentication with an Ed25519 key made for the run, then with no key, then keyboard-interactive with an empty answer to each prompt (Serveo's anonymous login; T-272); the user of the fact `user`, else `pod-services-watch`; a remote forward (`tcpip-forward`) of `localhost` and the port (`port`, 80 if none) to no local server; a session channel with a shell, and no terminal, as podnet's `ssh-relay`; the data of the shell through `pod_services::forward::Banner` with the rule of the service. It reports the host of the URL. A banner with other URLs and none that the rule accepts is a fact `url-match` that moved; a banner with no URL is a failure. Then, unless `prove = false`, a GET of the URL through the dial within 20 s (T-267): the client takes each channel `forwarded-tcpip` and answers its request with a word of the run (from the fingerprint of the key of the run); an answer with no word is a fact `url-match` that moved. The close follows the URL, the end of the shell or the bound. It accepts each host key: `ssh-host-key` checks the key, and the session sends no secret |
| `ssh-banner`, `ssh-host-key` | `ssh.rs` | the banner, after the banner of the client (a server can wait for it); the host key of the reply of curve25519-sha256, one exchange for each kind of key (Ed25519, ECDSA, RSA). The watch reads the key that the server offers; it does not check a signature |
| `github-release`, `github-files`, `github-file`, `tls-roots` | `github.rs` | GitHub's API (the newest release, the release of a tag) and its raw files (at a tag, or at `HEAD`) |
| `crate-file` | `crate_file.rs` | the newest stable version on crates.io, its `.crate` archive, the member file |
| `rdap` | `rdap.rs` | IANA's bootstrap, then the registry's server, else `rdap.org`; the `expiration` event must be `ahead-days` (30) or more away; a domain of the owner gives 90; under 90 days the run shows the end, with no finding; with no record, the failure names the answer of each server |
| `oci-ping`, `oci-digest` | `oci.rs` | `GET /v2/` and its `Bearer` challenge; an anonymous token for each repository; a `HEAD` of each manifest by its digest (404 is a pin that moved; 429 gets one more try after its `Retry-After`, at most 30 s); a tag that moved is reported, and is no failure |

A new kind: one async function `(service, watch, net) -> Checked`, its
line `kind!(Name, "word", module::function)` and its place in `kinds()`
in `src/lib.rs`, its arguments in `src/schema.rs`, its test in
`tests/watches/`, its plant in `scripts/plants/` (group `services`), and
its row in the README of `pod-services`. The test
`data::each_watch_of_the_data_has_its_kind` fails when the data names a
kind with no implementation, or when a kind has no watch in the data.
`data::each_watch_of_the_data_has_the_arguments_of_its_kind` fails on a
missing or an unknown argument, and on a name of an endpoint, a role, a
fact or a service that the data does not have (`schema::faults`). A run
checks the same before the network.

A service kind can have a table of its facts (`schema::kind_facts`; the
kind `file-host` has one). `data::each_service_has_the_facts_of_its_kind`
fails when a built-in service of such a kind misses an endpoint or a fact
of the table, has a fact of another type or a word that the table does
not name, or has a fact that the table does not name. A run checks the
same before the network: the watches of such a service fail with the
faults, so a file of a consumer (`--file`) is checked too.

## Tests

`tests/watches/answer.rs`: a refused connect and a handshake that ends
before a byte are no answer, a handshake after a byte of the server is a
failure of another kind, and the runner on a closed port of the loopback
passes with no state (`first no answer`) and fails with the state that
names the watch.

`tests/watches/` (one binary): each kind against a stand-in of
`pod-testkit` (feature `stand`) on the loopback, over TLS with the test
certificates of `pod-tls`: a file that the stand-in matches holds, and a
file that it does not match gives its finding. `tests/watches/xhttp.rs`
has an XHTTP node on HTTP/1.1 (a stand-in of `pod-testkit`) and one on
HTTP/2 (the server of `h2`). Each node checks each request as the server
of Xray does: the host, the path and its query, the padding, the session
id, and the sequence number of each upload. `tests/watches/forward.rs` has
a forward relay of russh's server: it takes a key, no key, or an empty
keyboard-interactive login, a remote
forward and a shell, writes its banner in chunks that cut a URL, and tells
the test what it saw (the user, the key, the forward, the shell, the end
of the session). With a front, its banner names an HTTPS front for
`stand.test` that gives each request to the client through a channel
`forwarded-tcpip`, or answers with a page of its own (T-267).

## Plants

`scripts/plants/services-*.plant` (group `services`): one or more for
each watch kind. Each plant makes its check blind to one change, and the test of the
kind must fail. `sh scripts/plant-each.sh --group services` runs them.
