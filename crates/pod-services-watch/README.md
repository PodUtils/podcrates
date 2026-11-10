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
cargo run -q -p pod-services-watch -- [--service ID]... [--kind KIND]... [--file PATH]...
```

* `--service ID`, `--kind KIND`: only these services, only these kinds.
* `--file PATH`: lay a service file of the same schema over the built-in
  set (`Catalog::lay`), to check a new file before its commit.
* The proxy comes from the variables of the environment (`HTTPS_PROXY`,
  `NO_PROXY`, and the others of `pod_proxy::Environment`).
  `GITHUB_TOKEN`, when set, goes to GitHub's API alone, never to another
  host and never to the output.
* UDP (`stun-binding`) goes direct: a proxy carries TCP alone.

One line for each watch, then a count:

| Line | Means |
| --- | --- |
| `holds SERVICE KIND: what it saw` | the live service is as the file says |
| `MOVED SERVICE KIND: services/SERVICE.toml FACT is WAS; the service gives NOW (source: ..., read DATE)` | a fact of the file moved: read the source, then change the file with a new `read` date |
| `FAILED SERVICE KIND: why` | the check could not hold: no answer, or an answer of another kind |
| `skipped SERVICE KIND: why` | the watch cannot speak a form of the service (a transport of a proxy link); no pass and no failure |

Exit 0: each watch holds or is skipped. Exit 1: a watch found a fact that
moved, or failed. Exit 2: a fault of the command line or of a file.

## The library

| Item | What |
| --- | --- |
| `Watch` (`kind`, `check`) | one watch kind |
| `kinds()`, `find(word)` | each watch kind; the kind of a word |
| `run(service, watch, net)` | one check, as a `Report`; a kind that the crate does not know is a failure |
| `Outcome` (`seen`, `findings`, `skipped`), `Finding` (`Moved`, `Failed`) | the result of one check |
| `Report` (`lines`, `holds`) | the lines of one watch |
| `Net` (`context`, `dial`, `trust`, `client`, `dns`, `services`, `bases`, `github_token`, `today`, `limit`), `Bases` (`public`, `at`) | the way out of each check: the runner fills it, a test gives its stand-ins |
| `args::Args` | the arguments of a watch and the values that they name |

The library reads no variable and no file, and prints nothing.

## The watch kinds

`crates/pod-services/README.md`, "The schema", has the arguments of each
kind. Each kind goes through the dial of `pod-proxy` and the TLS of
`pod-tls`; the DNS kinds ask the resolvers of DNS over HTTPS of `pod-dns`.

| Kind | Module | How |
| --- | --- | --- |
| `http-json`, `http-status`, `http-text`, `github-ssh-keys` | `http.rs` | a GET; a JSON pointer; with `member`, each host of the role is in the document, and more hosts there are counted, not found |
| `http-sha256` | `sha.rs` | a streamed GET (a `Range` for a range of sectors); the length is the `Content-Length` or the total of `Content-Range` |
| `tls-name`, `tls-names` | `tls.rs` | a verified handshake; for `tls-names`, one for the host and one for the address, at each address |
| `dns-srv`, `dns-addresses`, `doh-answer` | `dns.rs` | DNS over HTTPS (`pod_dns::Way`); `doh-answer` asks each resolver of the service in the one form |
| `stun-binding` | `stun.rs` | a Binding request over UDP (`pod_proxy::stun`), three tries |
| `websocket-upgrade`, `forward-banner` | `ws.rs` | the upgrade of `pod-ws`; a forwarder's target in the query, in a first text frame (`{"Host":...,"Port":...}`), or in a Wisp CONNECT |
| `proxy-banner` | `proxy.rs` | Trojan and VLESS over TCP, with or without TLS; another transport (XHTTP) is skipped |
| `ssh-banner`, `ssh-host-key` | `ssh.rs` | the banner; the host key of the reply of curve25519-sha256, one exchange for each kind of key (Ed25519, ECDSA, RSA). The watch reads the key that the server offers; it does not check a signature |
| `github-release`, `github-files`, `github-file`, `tls-roots` | `github.rs` | GitHub's API (the newest release, the release of a tag) and its raw files (at a tag, or at `HEAD`) |
| `crate-file` | `crate_file.rs` | the newest stable version on crates.io, its `.crate` archive, the member file |
| `rdap` | `rdap.rs` | IANA's bootstrap, then the registry's server, else `rdap.org`; the `expiration` event must be 90 days or more away |
| `oci-ping`, `oci-digest` | `oci.rs` | `GET /v2/` and its `Bearer` challenge; an anonymous token for each repository; a `HEAD` of each manifest by its digest; a tag that moved is reported, and is no failure |

A new kind: one async function `(service, watch, net) -> Checked`, its
line `kind!(Name, "word", module::function)` and its place in `kinds()`
in `src/lib.rs`, its test in `tests/watches/`, its plant in
`scripts/plants/` (group `services`), and its row in the README of
`pod-services`. The test `data::each_watch_of_the_data_has_its_kind`
fails when the data names a kind with no implementation, or when a kind
has no watch in the data.

## Tests

`tests/watches/` (one binary): each kind against a stand-in of
`pod-testkit` (feature `stand`) on the loopback, over TLS with the test
certificates of `pod-tls`: a file that the stand-in matches holds, and a
file that it does not match gives its finding.

## Plants

`scripts/plants/services-*.plant` (group `services`): one for each watch
kind. Each plant makes its check blind to one change, and the test of the
kind must fail. `sh scripts/plant-each.sh --group services` runs them.
