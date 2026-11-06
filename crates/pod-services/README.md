# pod-services

The relays and the services of other parties that the PodUtils binaries
use: the owner's relay, DNS over HTTPS, plain DNS, STUN, DERP and the DERP
map, iroh's relays and pkarr, Piping, the magic-wormhole mailboxes and
transit, OpenTunnel, the edge of Cloudflare's tunnels and its quick
tunnels, WARP, ACME, the public forwarders, Pinggy, GitHub's SSH host
keys, the OCI registries with the images that podvm pins, and the guest
images of podvm with their sums. Each service is one data file, and each value of a file has its
source and the date when it was read.

The crate knows no provider in its code. A new relay or service is a new
file, with no change of code. `build.rs` checks each file against the
schema and compiles the set in, so a binary parses nothing at run time.
A consumer reads a service by its id or the services of a kind, and can
lay its own files of the same schema over the built-in set (feature
`overlay`). The crate reads no variable, no file and no directory, keeps
no process state and prints nothing.

The watches that each file names run in the workflows of podcrates
(`pod-services-watch`, T-050). The rulings: `docs/decisions.md`,
"Services crate", "Services: the own relay", "Services: the facts of
probes", "Services: the design", "Services: what stays"; the design:
`docs/design.md`, section 4, "`pod-services`".

## The schema

One TOML file for each service: `services/ID.toml`. The build fails on a
fault, with the file and the field, for example
`services/plant-relay.toml: fact "idle-timeout-ms": no source (...)`.

| Key | What | Rule |
| --- | --- | --- |
| `id` | the id of the service | a word; the name of its file (`ID.toml`); one file for each id |
| `kind` | what the service is: `relay`, `doh`, `stun`, `derp`, ... | a word: a new kind needs no change of the schema |
| `party` | who runs the service | one line |
| `about` | what the service is | one line |
| `[rank]` | the place of the service in the lists of its kind: `value` (a whole number from 0, the lower first) and `source` | optional; a service with no rank comes after each ranked one; the same rank goes by id |
| `[[endpoint]]` | where a consumer connects | zero or more |
| `[[fact]]` | a typed value of the service | zero or more; one endpoint or one fact at least |
| `[[watch]]` | a check of the live service against the file | one or more |

A word is a lower-case letter, then lower-case letters, digits and single
`-`, at most 64 bytes, with no `-` at the end.

`[[endpoint]]`:

| Key | What | Rule |
| --- | --- | --- |
| `name` | the name in its service | a word; once in a file |
| `role` | a group of endpoints (`v4`, `v6`, `seed`, `region`, `h2`, `quic`) | optional word |
| `scheme` | `https`, `wss`, `wisp+wss`, `ssh`, `stun`, `udp`, `tls`, `quic` | a URI scheme in lower case |
| `host` | a name (a last `.` is a name that is fully qualified), or an IP address with no brackets | required |
| `port` | the port | 1 to 65535; required |
| `path` | the path | optional; starts with `/`, no space |
| `addresses` | the addresses of the host, to dial with no resolver | optional; each one an IP address |
| `sni` | the TLS name when it is not the host | optional; a name |
| `alpn` | the ALPN words, in order; none for no ALPN | optional |
| `source` | the source of the endpoint | required |

`[[fact]]`:

| Key | What | Rule |
| --- | --- | --- |
| `name` | the name in its service | a word; once in a file |
| `value` | a text, an integer, a bool, a date (`2029-08-15`, no quotes), a list of texts or a list of integers | required, or `file` |
| `file` | a text file under `services/` whose text is the value (the edge roots) | the built-in set only: a fact of an overlay gives its text in `value` |
| `type` | `text`, `int`, `bool`, `date`, `texts`, `ints` | optional; the value must be of it; required for an empty list |
| `unit` | the unit of a number: `ms`, `bytes` | optional word |
| `source` | the source of the value | required |

`source`, a table:

| Key | What |
| --- | --- |
| `url` | a document (`https://` or `http://`) |
| `measured` | a measurement, with its method |
| `ruling` | a ruling of an owner or an operator, and where it is written |
| `read` | the date when the value was read at its source (a TOML date, no time) |
| `note` | optional: one line more (an earlier read, a mirror, the member of a document) |

A source has exactly one of `url`, `measured` and `ruling`, and `read`.
A key that the schema does not name is a fault: a mistyped key fails the
build, and is not a value that nobody reads.

`[[watch]]`: `kind` (a word) and its arguments (words, with a text, an
integer, a bool or a list as their value). The build checks only these
words: a watch kind is a plugin of `pod-services-watch` (T-050), and a
new kind is one implementation there. The kinds that the files name now:

| Kind | Arguments | What it checks |
| --- | --- | --- |
| `http-json` | `url`, `pointer`; `fact`, or `facts` with `members`, or `member` with `role`, if any | the document has a JSON value at the pointer; it equals the fact, its members equal the facts, or its objects' member holds the hosts of the role |
| `http-status` | `endpoint` or `role`, `path`, `status` | a GET answers with the status |
| `http-text` | `endpoint`, `contains` | the answer to a GET holds the text |
| `tls-name` | `endpoint` or `role` | a TLS handshake verifies the name |
| `tls-names` | `endpoint` or `role` | the certificate at each address names the host and the address |
| `tls-roots` | `repo`, `path`, `fact` | the certificates of the file of the repository at the pinned tag are the fact's |
| `dns-srv` | `fact`, `role` | the SRV record of the fact names the hosts and the port of the role |
| `dns-addresses` | `role`, `facts` | the address records of the hosts of the role hold each address of the facts |
| `doh-answer` | `form`, `name`, `type` | each endpoint answers a query in the form |
| `stun-binding` | `endpoint` | a Binding request gets a Binding response |
| `websocket-upgrade` | `endpoint` | the upgrade answers 101 |
| `forward-banner` | `template`, `target` | a forward to the endpoint of the service `target` gives its SSH banner |
| `ssh-banner` | `endpoint` | the server sends an SSH banner |
| `github-ssh-keys` | `url`, `pointer`, `fact` | GitHub's published fingerprints equal the fact |
| `github-release` | `repo`, `fact`, `max-age-days` | the newest release; the age of the pinned tag |
| `github-files` | `repo`, `tag`, `fact` | the SHA-256 of each file of the fact at the tag and at the newest release |
| `github-file` | `repo`, `path`; `endpoint` or `facts` | the file on the default branch holds the values |
| `crate-file` | `crate`, `path`; `endpoint` or `role` | the file of the newest release of the crate holds the hosts |
| `rdap` | `domain` | the registration of the domain does not end soon |
| `oci-ping` | `endpoint`; `fact`, if any | `GET /v2/` answers 200, or 401 with a `Bearer` challenge whose realm is `https://` and equals the fact |
| `oci-digest` | `endpoint`, `facts` | each `REPOSITORY[:TAG]@sha256:DIGEST` of the facts pulls by its digest; the tag that moved is reported |
| `http-sha256` | `endpoint`; `fact`, or `files` with `facts`; for a range `start`, `sector`, `bytes` (facts), and `length` (a fact) if any | the SHA-256 of the file (or of each file under the path, or of the range) equals the fact; the length equals `length` |

An example:

```toml
id = "example-relay"
kind = "relay"
party = "Example"
about = "A relay of the user's own"

[rank]
value = 0
source = { ruling = "the user's setting", read = 2026-11-06 }

[[endpoint]]
name = "default"
scheme = "https"
host = "relay.example"
port = 443
source = { url = "https://relay.example/relays.json", read = 2026-11-06 }

[[fact]]
name = "idle-timeout-ms"
value = 180000
unit = "ms"
source = { measured = "a session with no traffic closed after 180 s", read = 2026-11-06 }

[[watch]]
kind = "http-json"
url = "https://relay.example/relays.json"
pointer = "/limits"
```

## API

| Item | What |
| --- | --- |
| `get(id)` | the built-in `&'static Service` of the id, or none |
| `of_kind(kind)` | the built-in services of the kind, by rank and then by id |
| `all()` | each built-in service, by rank and then by id |
| `Service` (`id`, `kind`, `party`, `about`, `rank`, `endpoints`, `endpoint(name)`, `endpoints_of(role)`, `facts`, `fact(name)`, `text`, `int`, `bool`, `date`, `texts`, `ints`, `watches`) | one service; a typed accessor gives none when the fact is missing or of another type |
| `Endpoint` (`name`, `role`, `scheme`, `host`, `port`, `path`, `addresses`, `ip_addrs`, `socket_addrs`, `sni`, `server_name`, `alpn`, `authority`, `url`, `source`) | `server_name` is the SNI, else the host; `authority` is `host:port` (brackets for IPv6); `url` is `scheme://host[:port][path]`, with no port when it is the default of `https`, `wss` (443), `http` or `ws` (80); `socket_addrs` gives the addresses with the port, or the host when it is an address |
| `Fact` (`name`, `value`, `unit`, `source`), `Value` (`Text`, `Int`, `Bool`, `Date`, `Texts`, `Ints`; `as_text`, ...; `Display`) | a typed value |
| `Source` (`origin`, `read`, `note`), `Origin` (`Url`, `Measured`, `Ruling`), `Date` (`new`, `year`, `month`, `day`; `Display` as `YYYY-MM-DD`) | where a value comes from and when it was read |
| `Rank` (`value`, `source`), `Watch` (`kind`, `args`, `arg(name)`), `Arg` (`name`, `value`) | |
| `Catalog::new()`, `Catalog::get`, `Catalog::of_kind`, `Catalog::iter`, `Catalog::overlay` | the built-in set with the overlay of the consumer |
| `Catalog::lay(label, text)`, `Catalog::with_overlay(files)`, `OverlayError` (`label`, `field`, `why`, `class()`) | feature `overlay` |
| `Text`, `List<T>` | `Cow<'static, str>` and `Cow<'static, [T]>`: the built-in set borrows from the binary, an overlay owns its values |

## The overlay

The feature `overlay` adds `Catalog::lay`. The consumer reads its own
files (a setting of the consumer: rule 6 of `docs/design.md`, section 1)
and gives each text with a label for the errors:

```rust
let mut services = pod_services::Catalog::new();
for (path, text) in files_of_the_user {
    services.lay(&path, &text)?; // OverlayError: the label, the field and why; class Config
}
let relay = services.of_kind("relay").next();
```

* A file holds to the same schema as a built-in file, and the same check
  reads it (`src/schema.rs`). Its id need not be the name of its file.
* A file with the id of a built-in service replaces the whole service; a
  new id adds a service. A second file of one id in an overlay is an
  error.
* A fact of an overlay gives its text in `value`; `file` is refused.
* The built-in set does not change: `get`, `of_kind` and `all` give it
  alone.
* `OverlayError` shows the label, the field and why, each on one line
  (`pod_core::text::one_line`); its class is `pod_core::Class::Config`.

## Features

| Feature | What |
| --- | --- |
| none (default) | the built-in set, the API and `Catalog` with no overlay; no TOML at run time |
| `overlay` | `Catalog::lay` and `Catalog::with_overlay`; a TOML reader (the crate `toml`, pure Rust) at run time |

## The services

| Id | Kind | Endpoints | Facts |
| --- | --- | --- | --- |
| `ajam-relay` | `relay` | `default`; `seed-*` (role `seed`, 6) | `connect-timeout-ms`, `idle-timeout-ms`, `max-session-ms`, `max-session-bytes`, `max-frame-bytes`, `measured-version`, `signed-list-keys` |
| `ajam-ts-relay` | `ts-relay` | `default` | |
| `github-ssh` | `ssh-host` | `ssh` | `host-key-sha256` |
| `cloudflare-doh`, `google-doh` | `doh` | `v4-1`, `v4-2` (role `v4`), `v6-1`, `v6-2` (role `v6`) | `json-path`, `wire-path`, `http2-only` |
| `quad9-doh` | `doh` | `v4-1`, `v4-2` (role `v4`) | `wire-path`, `http2-only` (no `json-path`: no JSON form) |
| `cloudflare-dns` | `dns` | `udp-1`, `udp-2` (role `udp`), `trace` | `whoami-name`, `check-name` |
| `cloudflare-stun`, `google-stun` | `stun` | `udp` | |
| `tailcat-derp` | `derp` | `new-york`, `san-francisco`, `frankfurt`, `tokyo` (role `region`) | `probe-path` |
| `tailscale-derp-map` | `derp-map` | `map` | |
| `n0-iroh-relay` | `iroh-relay` | `na-east`, `na-west`, `eu`, `ap` (role `relay`) | |
| `n0-pkarr` | `pkarr` | `relay` | |
| `ppng-piping` | `piping` | `server` | |
| `magic-wormhole-mailbox`, `leastauthority-mailbox` | `mailbox` | `mailbox` | |
| `leastauthority-transit` | `transit` | `relay` | |
| `opentunnel` | `opentunnel` | `api` | `tunnel-zone` |
| `cloudflare-tunnel-edge` | `tunnel-edge` | `h2-region1`, `h2-region2` (role `h2`), `quic-region1`, `quic-region2` (role `quic`) | `srv-name`, `srv-target-zone`, `static-addresses`, `probe-address`, `warp-probe-address`, `roots` (`files/cloudflare-edge-roots.pem`), `roots-first-end`, `cloudflared-repository`, `cloudflared-tag`, `cloudflared-commit`, `cloudflared-released`, `cloudflared-files` |
| `trycloudflare` | `quick-tunnel` | `api` | `tunnel-zone` |
| `cloudflare-warp` | `warp` | `api`, `api-second` (role `api`), `masque-v4`, `masque-v6` (role `masque`), `masque-l4` | `connect-uri`, `user-agent`, `api-version`, `client-version`, `usque-api-version`, `usque-client-version`, `terms-url` |
| `letsencrypt-acme` | `acme` | `directory` | |
| `mudlet-forwarder`, `ssheasy-forwarder`, `mudix-forwarder`, `wisp-forwarder` | `forwarder` | `forwarder` | `word`, `template`, `note` |
| `pinggy` | `ssh-tunnel` | `ssh` | `dashboard-host` |
| `docker-hub-registry` | `oci-registry` | `registry` | `domain`, `aliases`, `official-prefix`, `credential-key`, `token-realm`, `images`, `experiment-images` |
| `ecr-public-registry` | `oci-registry` | `registry` | `token-realm`, `images`, `guest-userland`, `aarch64-image`, `registry-image` |
| `ghcr-registry` | `oci-registry` | `registry` | `token-realm`, `anonymous-quota`, `images` |
| `quay-registry` | `oci-registry` | `registry` | `token-realm`, `images` |
| `opensuse-registry` | `oci-registry` | `registry` | `images` (no token: `/v2/` answers 200) |
| `alpine-netboot` | `guest-image` | `netboot`, `archive` | `release`, `vmlinuz-virt-sha256`, `modloop-virt-sha256` |
| `freedos-image` | `guest-image` | `zip` | `sha256`, `zip-bytes`, `image-member`, `image-bytes` |
| `microsoft-validationos` | `guest-image` | `iso` | `iso-bytes`, `vhdx-lba`, `sector-bytes`, `vhdx-bytes`, `vhdx-sha256`, `vhdx-magic` |

An image fact is `REPOSITORY[:TAG]@sha256:DIGEST`, with no host: the host
is the endpoint `registry`.

## Where the consumers differed

| Value | podssh | podnet | Kept |
| --- | --- | --- | --- |
| The relay's host | `relay.rs` `DEFAULT_RELAY_HOST` | `ws_relay.rs` `DEFAULT_RELAY` (a URL) and `relay_pool.rs` `DEFAULT_HOST`: two constants | one endpoint, `ajam-relay` `default` |
| The seed of the pool | `pool.rs` `SEED`, from `/relays.json` of 2026-10-08 | `relay_pool.rs` `SEED`, the same six, measured on 2026-11-04 | the same six; read again in `/relays.json` on 2026-11-06, the measurement of podnet in the note |
| DoH | six resolvers by IPv4 address, with the names of their certificates, the JSON and the wire form, Quad9 over HTTP/2 alone; the operators take turns | eight by address (IPv4 and IPv6), the JSON form, Cloudflare then Google; two by name through a proxy | each address of both, with the names of the certificates (read again on 2026-11-06; the IPv6 addresses take the names that the certificates give them); the rank of the operators; each consumer keeps its order of the addresses (the adoption map) |
| The name of 8.8.4.4 | `dns.google.com` | `dns.google` (the server by name) | both: `google-doh` `v4-2` is `dns.google.com`, `v4-1` is `dns.google`; the certificate names both |
| The SRV of the edge | `quick/edge.rs` `SRV_NAME` | `edge_discovery.rs` and `cloudflare/mod.rs` `EDGE_SRV`: two constants | one fact, `srv-name` |
| The regions | `REGIONS` | `FIXED_TARGETS`, `tunnel.rs` `EDGE_REGION_NAME` | the endpoints of the role `h2` (and `quic`) |
| An address of the edge | none | `198.41.192.167` (`udp.rs`, `STATIC_EDGE_IPS`), `198.41.192.27` (`warp probe`) | both, as two facts (`probe-address`, `warp-probe-address`): each was an A record of `region1.v2.argotunnel.com` on 2026-11-06 |
| The edge roots | `quick/cloudflare-roots.pem` (the certificates, with a comment for each) | `cloudflare/cf-edge-roots.pem` (the certificates alone) | the same three certificates; podssh's form, with a header for 2026.10.7 |
| The pin of cloudflared | 2026.10.6 (`35b379a`, released 2026-10-29), with the sum of `features.go` of 2026.10.7 that podssh judged the same, and `connection/protocol.go` | 2026.10.7 (`872b241`, released 2026-10-30), with the schemas, the roots and seven source files | 2026.10.7: the union of the files, each sum read at 2026.10.7 on 2026-11-06 (`connection/protocol.go` is the same at both tags); podssh's sum of `features.go` of 2026.10.6 is not kept |
| The words of the release that the adapter speaks | `SPEAKS` = `the quick tunnel of cloudflared 2026.10.6` | `SPEAKS` = `cloudflared 2026.10.7` | the consumer writes its words from `cloudflared-tag` |
| The pair of versions of WARP's API | `v0a4471` and `a-6.35-4471` (usque) | `v0a5641` and `a-6.38.9-5641` | both pairs: `usque-api-version`, `usque-client-version` (usque's `internal/consts.go`, the same on 2026-11-06) and `api-version`, `client-version` (podnet's registrations); the API refuses a pair whose build numbers differ, so each consumer keeps its pair |
| The authority of the CONNECT-IP of WARP | `cloudflareaccess.com:443` | `https://cloudflareaccess.com` | usque's form, `connect-uri` = `https://cloudflareaccess.com`; podssh adds `:443` |
| The front door of MASQUE | `ENDPOINT_H2` `162.159.198.2:443` | `MASQUE_ADDRESS` `162.159.198.2`, `MASQUE_ADDRESS_V6` `2606:4700:103::2` | `masque-v4` and `masque-v6` |
| 1.1.1.1 on port 443 | `podssh-warp` `probe.rs` `TARGET_V4`, `TARGET_V6` | `warp/exit.rs` `TRACE_*`, the `warp probe` target | one endpoint, `cloudflare-dns` `trace` |
| 1.1.1.1 | | about 12 places (DoH, plain DNS, the trace, scripts) | `cloudflare-doh` `v4-1`, `cloudflare-dns` `udp-1` and `trace`; the scripts are T-050's |
| Pinggy | | `relay.rs`, `transport/ssh.rs`, `examples/edge_probe.rs` | one endpoint, `pinggy` `ssh` |

## Parity

The inventory of podssh (`docs/research/services/podssh.md`, A and E), at
`d3745ab`:

| Place | Here |
| --- | --- |
| `crates/podssh-relay/src/relay.rs` `DEFAULT_RELAY_HOST` | [x] `ajam-relay` endpoint `default` (`host`) |
| `relay.rs` `RELAY_IDLE_SECS`, `SESSION_BYTE_CAP`, `SESSION_TIME_CAP` | [x] `ajam-relay` facts `idle-timeout-ms`, `max-session-bytes`, `max-session-ms` |
| `relay.rs` `SESSION_BYTE_BUDGET`, `SESSION_TIME_BUDGET`, `RELAY_ENV`, `SESSION_BUDGET_ENV` | stay: podssh's budgets under the caps and its variables (pod-relay) |
| `relay.rs` the test `the_compiled_limits_are_the_facts_of_the_relays_limits` | [x] `tests/moved.rs`, the same name |
| `crates/podssh-relay/src/pool.rs` `SEED` | [x] `ajam-relay` endpoints of the role `seed` |
| `pool.rs` the test `the_seed_hosts_belong_to_the_default_deployment` | [x] `tests/moved.rs`, the same name (the rule of the parent domain written in the test; pod-relay keeps `same_deployment` and its tests) |
| `pool.rs` `STALE_AFTER_MS`, `MAX_BODY`, the path `/relays.json` | stay: the client's cache and the relay contract (pod-relay, T-047) |
| `crates/podssh-relay/src/pool/signed.rs` `BUILT_IN` | [x] `ajam-relay` fact `signed-list-keys` (empty) |
| `crates/podssh-ts/src/config.rs` `default_relay`, `crates/podssh-cli/src/ts.rs` the default of `--ts-relay` | [x] `ajam-ts-relay` endpoint `default` |
| `crates/podssh-cli/src/doctor/relay_checks.rs` `MEASURED_VERSION` | [x] `ajam-relay` fact `measured-version` |
| `relay_checks.rs` `FORWARD_HOST`, `GITHUB_KEYS` | [x] `github-ssh` endpoint `ssh`, fact `host-key-sha256` |
| `relay_checks.rs` the test `the_published_keys_are_well_formed_sha256_fingerprints` | [x] `tests/moved.rs`, the same name |
| `crates/podssh-ws/src/resolve.rs` `DOH_RESOLVERS` | [x] `cloudflare-doh`, `google-doh`, `quad9-doh`: `ip` the endpoint's `addresses`, `name` its `host`, `json` the fact `json-path`, `wire` the fact `wire-path`, `http2` the fact `http2-only` |
| `resolve.rs` the tests of the forms and of the names through a proxy | stay: they test the code of the resolver (pod-dns) |
| `crates/podssh-cli/src/doctor/nat.rs` `DEFAULT` | [x] `cloudflare-stun`, `google-stun`, endpoint `udp` (`authority()`) |
| `crates/podssh-relay/src/derp/servers.rs` `TAILCAT`, `PROBE_PATH` | [x] `tailcat-derp` endpoints of the role `region`, fact `probe-path` |
| `crates/podssh-cli/src/ts/probe.rs` `DERP_MAP_HOST`, `DERP_MAP_PATH` | [x] `tailscale-derp-map` endpoint `map` |
| `crates/podssh-iroh/src/relays.rs` `DEFAULT` | [x] `n0-iroh-relay` endpoints of the role `relay` (`url()` gives `https://use1-1.relay.n0.iroh.link.`) |
| `crates/podssh-iroh/tests/relays.rs` the check that `DEFAULT` is iroh's list | stays: it reads the crate iroh; podssh compares `n0-iroh-relay` with iroh there |
| `crates/podssh-iroh/src/pkarr.rs` `N0` (and the test that holds it to iroh's) | [x] `n0-pkarr` endpoint `relay` (`url()`); the test stays: it reads the crate iroh |
| `crates/podssh-relay/src/piping/server.rs` `DEFAULT_HOST`, `PORT` | [x] `ppng-piping` endpoint `server` |
| `crates/podssh-relay/src/code/mod.rs` `MAILBOXES` | [x] `of_kind("mailbox")`, endpoint `mailbox` (`url()`) |
| `crates/podssh-relay/src/transit/mod.rs` `DEFAULT_URL` | [x] `leastauthority-transit` endpoint `relay` (`url()`) |
| `crates/podssh-relay/src/opentunnel/mod.rs` `DEFAULT_API`, `PORT`, `Service::hosted` | [x] `opentunnel` endpoint `api` |
| `crates/podssh-cli/src/ssh/opentunnel.rs` `HOSTED` | [x] `opentunnel` fact `tunnel-zone` (`.` and the zone) |
| `crates/podssh-relay/src/quick/edge.rs` `SRV_NAME`, `REGIONS`, `PORT`, `SERVER_NAME`, `ROOTS_FIRST_END`, `CLOUDFLARE_ROOTS` | [x] `cloudflare-tunnel-edge` fact `srv-name`; the endpoints of the role `h2` (`host`, `port`, `sni`); facts `roots-first-end`, `roots` |
| `quick/edge.rs` the test `the_first_end_of_cloudflared_s_roots_is_the_day_that_doctor_names` | [x] `tests/moved.rs`, the same name |
| `quick/edge.rs` `DIAL_LIMIT`, the choice of the SRV targets and its tests | stay: the code of the adapter (pod-cloudflare) |
| `crates/podssh-relay/src/quick/api.rs` `HOST`, `PATH` | [x] `trycloudflare` endpoint `api` (`host`, `path`) |
| `quick/api.rs` `TIMEOUT`, `BUDGET` | stay: the code of the adapter (pod-cloudflare) |
| `crates/podssh-relay/src/quick/quic.rs` `SERVER_NAME`, `ALPN` | [x] `cloudflare-tunnel-edge` endpoints of the role `quic` (`sni`, `alpn`) |
| `quick/quic.rs` `HANDSHAKE_WAIT`, `SILENCE`, `KEEPALIVE` and the first packets | stay: cloudflared's parameters of QUIC, the protocol of the adapter (pod-cloudflare) |
| `crates/podssh-iroh/src/pkarr_record.rs` `QUICK_ZONE` | [x] `trycloudflare` fact `tunnel-zone` |
| `crates/podssh-relay/src/quick/cloudflared-pin.txt` | [x] `cloudflare-tunnel-edge` facts `cloudflared-tag`, `cloudflared-commit`, `cloudflared-released`, `cloudflared-files` (see "Where the consumers differed") |
| `crates/podssh-relay/src/quick/mod.rs` `SPEAKS` and the test `the_release_named_is_the_pin_of_the_table` | stay: the words of the adapter; it writes them from `cloudflared-tag`, and its test ties the words to that fact |
| `crates/podssh-relay/src/quick/cloudflare-roots.pem` | [x] `services/files/cloudflare-edge-roots.pem`, the fact `roots` |
| `crates/podssh-warp/src/lib.rs` `ENDPOINT_H2`, `PORT_H2`, `SERVER_NAME` | [x] `cloudflare-warp` endpoint `masque-v4` (`addresses`, `port`, `host`) |
| `podssh-warp/src/lib.rs` `AUTHORITY`, `TERMS_URL` | [x] `cloudflare-warp` facts `connect-uri` (podssh adds `:443`), `terms-url` |
| `podssh-warp/src/lib.rs` `MTU` | stays: usque's default of the tunnel (pod-warp) |
| `crates/podssh-warp/src/register.rs` `API_HOST`, `API_SECOND`, `API_HOSTS` | [x] `cloudflare-warp` endpoints of the role `api` |
| `register.rs` `API_VERSION`, `CLIENT_VERSION`, `USER_AGENT` | [x] `cloudflare-warp` facts `usque-api-version`, `usque-client-version`, `user-agent` |
| `register.rs` `CONTENT_TYPE`, `MODEL`, `LOCALE`, `KEY_TYPE` | stay: the protocol of the API's client (pod-warp) |
| `crates/podssh-warp/src/probe.rs` `TARGET_V4`, `TARGET_V6` | [x] `cloudflare-dns` endpoint `trace` (`socket_addrs()`) |
| `crates/podssh-ws/src/forwarder/builtin.rs` `BUILTIN` | [x] `of_kind("forwarder")`: `word`, `template`, `note`; the order is the rank |
| `builtin.rs` the test `each_built_in_template_is_one_of_its_form` | stays: it tests the parser of a template (pod-egress) on the data |
| `crates/podssh-ws/src/iap/mod.rs`, `crates/podssh-cli/src/ssh/cloud/google.rs`, `cloud/iap.rs` 35.235.240.0/20, `crates/podssh-ws/src/eice/mod.rs` | stay: the cloud endpoints of the user's own accounts (the ruling "Services: what stays") |
| `crates/podssh-cli/src/acme/mod.rs` `LETS_ENCRYPT` | [x] `letsencrypt-acme` endpoint `directory` (`url()`) |
| `acme/store.rs` the staging directory of a test | stays: a fixture of a test |
| The manual's text (`man/notes_public.rs`, `man/facts/roads.rs`, `man/facts/cloud.rs`, `man/notes_ssh.rs`, `man/notes_pair.rs`, `man/notes_doctor.rs`, `man/notes_deployments.rs`, `flags/derp.rs`, `flags/opentunnel.rs`, `flags/relay.rs`) | stays: podssh's text; it follows each move, and can print the values of `pod-services` |
| `crates/podssh-probe/facts/relay-limits.toml` | [x] `ajam-relay` facts `connect-timeout-ms`, `idle-timeout-ms`, `max-session-ms`, `max-session-bytes`, `max-frame-bytes` |
| `crates/podssh-probe/facts/relay-facts.toml`, `tests/spec/relay-spec-2026-10-03-r2.txt` | the relay contract (pod-relay, T-047) |
| `crates/podssh-cli/tests/fixtures/derpmap-default-2026-10-10.json` | stays: a fixture of a test |
| `scripts/zig-mirrors.txt` | the supply chain of CI (T-046) |
| `scripts/interop-baseline.txt` (`tcp.ts.relay.ajam.dev:443`, `github.com:22`) | stays: the record of podssh's interop, which names the values in its text |
| B (the code that measures or watches), C (the workflows), D (the scripts) | T-050 (the watches, the workflows, the scripts); the rank, the pool, the health and the doctor are code of the consumers (pod-relay and the consumers) |

The inventory of podnet (`docs/research/services/podnet.md`, 1 and 5), at
`380d9a4`:

| Place | Here |
| --- | --- |
| `src/ws_relay.rs` `DEFAULT_RELAY` | [x] `ajam-relay` endpoint `default` (`url()` gives `https://tcp.ssh.relay.ajam.dev`) |
| `src/relay_pool.rs` `DEFAULT_HOST`, `SEED` | [x] `ajam-relay` endpoint `default` (`host`), the endpoints of the role `seed` |
| `relay_pool.rs` the path `/relays.json`, `same_deployment`, `MAX_ALTERNATES`, `STALE_AFTER`, `MAX_BODY` | stay: the relay contract and the client (pod-relay, T-047) |
| `src/relay.rs` `DEFAULT_RELAY`, `src/transport/ssh.rs` the default address | [x] `pinggy` endpoint `ssh` (`authority()` gives `free.pinggy.io:443`) |
| `src/relay.rs` the match on `dashboard.pinggy.io` | [x] `pinggy` fact `dashboard-host` |
| `examples/edge_probe.rs` the defaults (`free.pinggy.io:443`, `region1.v2.argotunnel.com`) | stay: an example of podnet; it can read `pinggy` and `cloudflare-tunnel-edge` |
| The tests that repeat the relay's hosts (`relay_pool.rs`, `ws_relay.rs`, `tests/relay_reverse.rs`) | stay: tests of the code |
| `src/resolve.rs` `DOH_SERVERS` | [x] `cloudflare-doh`, `google-doh`: the URL is `https://` with the endpoint's address (in brackets for IPv6) and the fact `json-path`; the operator is the `party` |
| `src/resolve.rs` `DOH_SERVERS_BY_NAME` | [x] the `host` of the endpoint `v4-1` and the fact `json-path` of `cloudflare-doh` and `google-doh` |
| `src/resolve.rs` the test `the_servers_are_ip_addresses_of_two_operators` | [x] `tests/moved.rs`, the same name |
| `src/resolve.rs` `STEP_TIMEOUT`, `PINS_VARIABLE`, `by_family` and its tests | stay: the code and the settings of the resolver (pod-dns) |
| `src/edge_discovery.rs` `EDGE_SRV`, `src/cloudflare/mod.rs` `EDGE_SRV` | [x] `cloudflare-tunnel-edge` fact `srv-name` |
| `edge_discovery.rs` `STATIC_EDGE_IPS` | [x] `cloudflare-tunnel-edge` fact `static-addresses`; the port is the port of the role `h2` |
| `edge_discovery.rs` `FIXED_TARGETS` | [x] the `host` and the `port` of the endpoints of the role `h2` |
| `edge_discovery.rs` the rule that a target is under `argotunnel.com` | [x] the fact `srv-target-zone`; the rule stays in the code (pod-cloudflare) |
| `src/tunnel.rs` `EDGE_REGION_NAME` | [x] `cloudflare-tunnel-edge` endpoint `h2-region1` (`host`) |
| `src/cloudflare/mod.rs` `QUIC_SNI`, `H2_SNI`, `QUIC_ALPN` | [x] the `sni` and the `alpn` of the endpoints of the roles `quic` and `h2` |
| `src/cloudflare/mod.rs` the tests `quic_alpn_and_sni_are_different_strings`, `the_embedded_edge_roots_are_real_certificates` | [x] `tests/moved.rs`, the same names |
| `src/cloudflare/mod.rs` `SPEAKS` and the test `speaks_names_the_tag_of_the_pin` | stay: the words of the adapter; it writes them from `cloudflared-tag`, and its test ties the words to that fact |
| `src/cloudflare/cf-edge-roots.pem`, `CF_EDGE_ROOTS_PEM` | [x] `cloudflare-tunnel-edge` fact `roots` |
| `src/trust.rs` `EDGE_ROOTS_FIRST_END` and the test `the_first_end_of_the_edge_roots` | [x] the fact `roots-first-end`; `tests/moved.rs`, the same name |
| `src/udp.rs` `PROBE_TARGET` | [x] `cloudflare-tunnel-edge` fact `probe-address` and the port of the role `h2` |
| `src/main.rs` the default targets of `warp probe` (`1.1.1.1:443`, `198.41.192.27:7844`) | [x] `cloudflare-dns` endpoint `trace` (its IPv4 address and port); `cloudflare-tunnel-edge` fact `warp-probe-address` and the port of the role `h2` |
| `src/main.rs` the nameserver `1.1.1.1` of `warp run` in a raw pod | [x] `cloudflare-dns` endpoint `udp-1` (`host`) |
| `src/main.rs` the tests, the help text; `src/manual/facts.rs`, `src/manual/model.rs` | stay: text and tests of podnet; the text follows each move |
| `src/quicktunnel.rs` `DEFAULT_QUICK_SERVICE` | [x] `trycloudflare` endpoint `api`: `https://` and its `host` |
| `src/util/mod.rs` `validate_quick_hostname` (the suffix `.trycloudflare.com`) | [x] the fact `tunnel-zone` of `trycloudflare`; the rule stays in the code (pod-cloudflare) |
| `src/doctor.rs` the target `api` (`api.trycloudflare.com:443`) | [x] `trycloudflare` endpoint `api` (`authority()`) |
| `src/doctor.rs` the target `warp-api` (`api.cloudflareclient.com:443`) | [x] `cloudflare-warp` endpoint `api` (`authority()`) |
| `src/doctor.rs` `MASQUE_ADDRESS`, `MASQUE_ADDRESS_V6` | [x] `cloudflare-warp` endpoints `masque-v4`, `masque-v6` (`addresses`) |
| `src/fetch.rs`, `src/hop.rs` tests, `src/bug_report.rs` the redaction of `.trycloudflare.com`, `src/quicktunnel.rs` `REAL_BODY` | stay: tests, test data and code of podnet |
| `src/warp/register.rs` `API_BASE`, `API_SECOND` | [x] `cloudflare-warp` endpoints `api`, `api-second` (`url()`) |
| `src/warp/register.rs` `API_VARIABLE` | stays: a variable of podnet (`PODNET_WARP_API`) |
| `src/warp/register.rs` `API_VERSION`, `CLIENT_VERSION`, `USER_AGENT` | [x] `cloudflare-warp` facts `api-version`, `client-version`, `user-agent` |
| `src/warp/register.rs` `MASQUE_SNI`, `MASQUE_PORT`, `CONNECT_URI` | [x] `cloudflare-warp` endpoint `masque-v4` (`host`, `port`); fact `connect-uri` |
| `src/warp/l4.rs` `L4_SNI` | [x] `cloudflare-warp` endpoint `masque-l4` (`host`) |
| `src/warp/exit.rs` `TRACE_ADDRESS`, `TRACE_ADDRESS_V6`, `TRACE_NAME`, `/cdn-cgi/trace` | [x] `cloudflare-dns` endpoint `trace` (`addresses`, `host`, `path`) |
| `src/warp/exit.rs` the TXT of `whoami.cloudflare` in class CH | [x] `cloudflare-dns` fact `whoami-name` |
| `src/warp/proxy.rs` `TUNNEL_RESOLVER`, `CHECK_NAME` | [x] `cloudflare-dns` endpoint `udp-1` (`socket_addrs()`), fact `check-name` |
| `src/warp/route.rs` the fallback nameservers `1.1.1.1`, `1.0.0.1` | [x] `cloudflare-dns` endpoints of the role `udp` |
| `src/agent.rs` `1.1.1.1:53` | [x] `cloudflare-dns` endpoint `udp-1` (`authority()`) |
| The tests and the fuzz seeds with addresses of the edge and of WARP (`src/cloudflare/mod.rs`, `src/egress.rs`, `src/ws_relay.rs`, `src/fuzzing.rs`, `src/warp/*.rs`, `src/main.rs`) | stay: fixtures of tests |
| `src/state.rs` `candidates` | not a host: no move |
| 1.6: `src/update.rs`, `src/sigstore-fulcio.pem`, `tests/data/*`, `scripts/install.sh` | stay: the release identity of `podnet update` (the ruling "Services: what stays") |
| 1.7: the hosts of the build and of CI (ziglang.org, cargo-deny, ghcr.io), `scripts/pull-image.sh` | the supply chain of CI (T-046) |
| 1.7: `tools/ts-sshd.sh` | stays: a helper of a test, pinned |
| 1.7: `forge.web.pkgforge.dev` in `scripts/check-github-numbers.py` and `scripts/agent/*` | stays: the tools of podnet's own repository |
| 1.7: `tools/check-upstream.py` `REPO` | [x] `cloudflare-tunnel-edge` fact `cloudflared-repository`; the check is T-050's |
| The NTP packet of `live.yml` (`162.159.200.123:123`) | stays: a step of a live test (T-050 moves the live steps) |
| 5: `src/cloudflare/cloudflared-pin.txt` | [x] `cloudflare-tunnel-edge` facts `cloudflared-*` |
| 5: `src/cloudflare/schemas/*.capnp`, `tools/header-oracle.txt` | the protocol of the adapter (pod-cloudflare); the pin's `copy` lines hold their sums |
| 5: `scripts/box/report.txt` | stays: the facts of a box of a test |
| 2 (the code that measures or watches), 3 (the workflows), 4 (the scripts and the tools) | T-050; the doctor, the probes and the pool are code of the consumers (and of pod-relay, pod-cloudflare, pod-warp) |

The inventory of podvm (`docs/research/services/podvm.md`, H and I, and
podvm's own rows in `docs/research/services/podvm-inventory.md`), at
`0249527`:

| Place | Here |
| --- | --- |
| `crates/podvm-image/src/reference.rs` `DEFAULT_DOMAIN`, `DEFAULT_ENDPOINT`, `OFFICIAL_PREFIX`, the alias `index.docker.io` of `Reference::endpoint` (C01) | [x] `docker-hub-registry` facts `domain`, `official-prefix`, `aliases`; endpoint `registry` (`host`) |
| `reference.rs` `DEFAULT_TAG`, the parse of a reference and its tests | stay: the rules of a reference (pod-oci, T-056) |
| `crates/podvm/src/images/list.rs` and `images/login_extract.rs` (`images.rs` before podvm `0d4b943`) the default server of `podvm login` and `podvm logout`; `crates/podvm-image/src/credentials.rs` `server_host` (C05) | [x] `docker-hub-registry` fact `credential-key`; the files of the user's logins stay (pod-oci) |
| `crates/podvm-image/src/registry.rs` the realm of a token (C03) | stays: the realm comes from the registry's challenge (pod-oci); the fact `token-realm` of each registry is for the watch only |
| `registry.rs`, `transport.rs`, `tls.rs` (C02, C04, C06, C07) | stay: the client, the policy and the trust paths (pod-oci, pod-tls) |
| `crates/podvm-measure/src/workload.rs` `ALPINE_REF` (E06) | [x] `ecr-public-registry` fact `guest-userland` |
| `workload.rs` `KURL`, `KERNEL_SHA256` (E06) | [x] `alpine-netboot` endpoint `netboot` and fact `vmlinuz-virt-sha256` |
| `experiments/146`, `147`, `360`, `361`, `390`: `ALPINE_REF`, `KURL`, `KERNEL_SHA256` (E06) | [x] the same facts; the experiments read them from the files (a `scripts/services/` export, T-050) |
| `experiments/361-guest-usernet.sh` `MURL`, `MODLOOP_SHA256` (E06) | [x] `alpine-netboot` endpoint `netboot`, fact `modloop-virt-sha256` |
| `experiments/290-microvm.sh` the Alpine `v3.21` netboot with no sum (E07) | stays: a setting of an experiment with no pin |
| `experiments/369-windows-tcg-dos.sh` `FD_URL`, `FD_SHA256`, `FD_IMG_BYTES` (E04) | [x] `freedos-image` endpoint `zip`, facts `sha256`, `image-bytes` |
| `crates/podvm/src/windows/dos.rs`, `windows/doctor.rs` the text that names FreeDOS 1.4 (E03) | stays: podvm's text |
| `experiments/371-validationos-stream.sh` `URL`, `ISO_LEN`, `VHDX_LBA`, `VHDX_LEN`, `VHDX_SHA256`; `experiments/lib/kvm-guest-base.sh` `VHDX_LEN`, `VHDX_SHA256` (E05) | [x] `microsoft-validationos` endpoint `iso`, facts `iso-bytes`, `vhdx-lba`, `vhdx-bytes`, `vhdx-sha256` |
| `podvm windows fetch --url`, `--sha256`; the cache paths of the bases (E01, E02, E09) | stay: the user's own data |
| `scripts/common/distro-matrix.sh` `DISTRO_ROWS_NSSWITCH` (the hosts and the digests) | [x] `docker-hub-registry` fact `images`; the row names and the libc column stay in podvm |
| `distro-matrix.sh` `DISTRO_ROWS_M5` (the hosts and the digests) (F01) | [x] the facts `images` of `ecr-public-registry`, `ghcr-registry`, `quay-registry`, `opensuse-registry`; the row names and the libc column stay in podvm |
| `distro-matrix.sh` `distro_qualify` | stays: a rule of a reference (`docker-hub-registry` fact `official-prefix` holds its prefix) |
| `experiments/70`, `80`, `90`, `220`: `alpine@sha256:d9e853e8...` on Docker Hub (F02) | [x] `docker-hub-registry` fact `experiment-images` |
| `experiments/105`, `106`: `quay.io/fedora/fedora@sha256:e78cd1a6...` (F02) | [x] `quay-registry` fact `images` (the same digest as `fedora:42`) |
| `experiments/367-qemu-user-aarch64.sh` `ALPINE_AA64` (F05) | [x] `ecr-public-registry` fact `aarch64-image` |
| `experiments/280-insecure-registry.sh` `REGISTRY_IMG` (F05) | [x] `ecr-public-registry` fact `registry-image` |
| `experiments/150-image-acquisition.sh`, `270-multiarch-image.sh`: the choice of ghcr (no anonymous quota) (F03) | [x] `ghcr-registry` fact `anonymous-quota` |
| `experiments/150`, `270`, `crates/podvm/tests/detached_stdio.rs`: the default `ghcr.io/pkgforge-dev/archlinux:latest` with no digest (F03); the tags with no digest of F04 and F05 | stay: settings of the tests with no pin; the digest of that tag is in `ghcr-registry` `images` |
| `experiments/250-negative-tests.sh` `registry.opensuse.org/opensuse/leap:15.6` | [x] `opensuse-registry` fact `images` |
| `experiments/180-driver.Dockerfile`, `Dockerfile.target`: `debian:bookworm@sha256:6ebd97fa...` (F10) | stay: the base image of a build of a test (T-046) |
| The relay: `crates/podvm-ssh/src/mux.rs`, `experiments/387`, `389` (A01 to A10) | `ajam-relay` (no new endpoint); the contract is pod-relay's (T-047); podvm has no default relay |
| The release identity: `crates/podvm-release/` (B01 to B04) | stays: the ruling "Services: what stays" |
| The rootfs completion: `crates/podvm-complete/` (D01 to D06) | stays: the mirrors are the image's, the resolver is the host's; no host of another party |
| The supply chain: zig, trufflehog, the actions, zot, Nix, nixpkgs, dropbear, `rust:1.98.1-bookworm` (F06, F07, G, H) | the supply chain of CI (T-046) |
| `mine-repo.sh`, `check-github-numbers.py`, `doctor.sh` `example.com` (H07, I01, I02) | stay: the tools of podvm's own repository |

## Tests

* `src/schema_tests.rs` (12): each rule of the schema refuses its fault
  with the file and the field; the built-in files pass the same reader.
* `tests/api.rs` (9): by id, by kind and by rank, the roles, the
  accessors, a source and a date for each value, the watches.
* `tests/overlay.rs` (5, feature `overlay`): a file adds a service or
  replaces one, an error of the settings, one id once, no file.
* `tests/moved.rs` (8): the tests of the consumers that test these values,
  with their names: podssh
  `the_compiled_limits_are_the_facts_of_the_relays_limits`,
  `the_seed_hosts_belong_to_the_default_deployment`,
  `the_published_keys_are_well_formed_sha256_fingerprints`,
  `the_first_end_of_cloudflared_s_roots_is_the_day_that_doctor_names`;
  podnet `the_first_end_of_the_edge_roots`,
  `the_embedded_edge_roots_are_real_certificates`,
  `quic_alpn_and_sni_are_different_strings`,
  `the_servers_are_ip_addresses_of_two_operators`.
* `tests/podvm.rs` (6): the constants of podvm come back from the files
  (`reference.rs`, `images.rs`, `workload.rs`, `371-validationos-stream.sh`);
  each registry answers at `/v2/`; each image is a repository and a
  SHA-256 digest; each sum of a guest image is a SHA-256. podvm has no
  test of these values to move.
* The plant `scripts/plants/services-no-source` (group `services`): a
  service file whose fact names no source; the build must fail with the
  file and the field.

## Adoption map

podssh:

| Old | New |
| --- | --- |
| `podssh_relay::relay::DEFAULT_RELAY_HOST` | `pod_services::get("ajam-relay").unwrap().endpoint("default").unwrap().host()` |
| `relay::RELAY_IDLE_SECS` | `get("ajam-relay")...int("idle-timeout-ms") / 1000` |
| `relay::SESSION_BYTE_CAP` | `get("ajam-relay")...int("max-session-bytes")` |
| `relay::SESSION_TIME_CAP` | `Duration::from_millis(get("ajam-relay")...int("max-session-ms"))` |
| `podssh_ws::frame::FORWARD_MAX_FRAME` | `get("ajam-relay")...int("max-frame-bytes")` |
| `pool::SEED` | `get("ajam-relay")...endpoints_of("seed").map(Endpoint::host)` |
| `pool::signed::BUILT_IN` | `get("ajam-relay")...texts("signed-list-keys")` |
| `TsMode::default_relay()`, the default of `--ts-relay` | `get("ajam-ts-relay")...endpoint("default")`: `host()`, `port()` |
| `doctor::relay_checks::MEASURED_VERSION` | `get("ajam-relay")...text("measured-version")` |
| `relay_checks::FORWARD_HOST`, `GITHUB_KEYS` | `get("github-ssh")...endpoint("ssh").host()`, `texts("host-key-sha256")` |
| `podssh_ws::resolve::DOH_RESOLVERS` | `of_kind("doh")`: for each endpoint of the role `v4`, `Doh { ip: addresses()[0], name: host(), json: text("json-path"), wire: text("wire-path"), http2: bool("http2-only") }`; podssh keeps its order (Cloudflare and Google take turns, then Quad9) |
| `doctor::nat::DEFAULT` | `of_kind("stun").map(\|s\| s.endpoint("udp").authority())` |
| `derp::servers::TAILCAT`, `PROBE_PATH` | `get("tailcat-derp")...endpoints_of("region").map(Endpoint::host)`, `text("probe-path")` |
| `ts::probe::DERP_MAP_HOST`, `DERP_MAP_PATH` | `get("tailscale-derp-map")...endpoint("map")`: `host()`, `path()` |
| `podssh_iroh::relays::DEFAULT` | `get("n0-iroh-relay")...endpoints_of("relay").map(Endpoint::url)`; `tests/relays.rs` compares that list with iroh's |
| `podssh_iroh::pkarr::N0` | `get("n0-pkarr")...endpoint("relay").url()` |
| `piping::server::DEFAULT_HOST`, `PORT` | `get("ppng-piping")...endpoint("server")`: `host()`, `port()` |
| `code::MAILBOXES` | `of_kind("mailbox").map(\|s\| s.endpoint("mailbox").url())` |
| `transit::DEFAULT_URL` | `get("leastauthority-transit")...endpoint("relay").url()` |
| `opentunnel::DEFAULT_API`, `PORT`, `Service::hosted()` | `get("opentunnel")...endpoint("api")`: `url()`, `host()`, `port()` |
| `ssh::opentunnel::HOSTED` | `format!(".{}", get("opentunnel")...text("tunnel-zone"))` |
| `quick::edge::SRV_NAME`, `REGIONS`, `PORT`, `SERVER_NAME` | `get("cloudflare-tunnel-edge")...text("srv-name")`; `endpoints_of("h2")`: `host()`, `port()`, `sni()` |
| `quick::edge::ROOTS_FIRST_END`, `CLOUDFLARE_ROOTS` | `...date("roots-first-end").to_string()`, `...text("roots")` |
| `quick::quic::SERVER_NAME`, `ALPN` | `endpoints_of("quic")`: `sni()`, `alpn()` |
| `quick::api::HOST`, `PATH` | `get("trycloudflare")...endpoint("api")`: `host()`, `path()` |
| `podssh_iroh::pkarr_record::QUICK_ZONE` | `get("trycloudflare")...text("tunnel-zone")` |
| `quick::SPEAKS`, `quick/cloudflared-pin.txt` | `format!("the quick tunnel of cloudflared {}", ...text("cloudflared-tag"))`; the facts `cloudflared-*` |
| `podssh_warp::ENDPOINT_H2`, `PORT_H2`, `SERVER_NAME` | `get("cloudflare-warp")...endpoint("masque-v4")`: `socket_addrs()[0]`, `port()`, `server_name()` |
| `podssh_warp::AUTHORITY` | `format!("{}:443", ...text("connect-uri").strip_prefix("https://"))` |
| `podssh_warp::TERMS_URL` | `...text("terms-url")` |
| `register::API_HOST`, `API_SECOND`, `API_HOSTS` | `...endpoint("api").host()`, `...endpoint("api-second").host()`, `endpoints_of("api")` |
| `register::API_VERSION`, `CLIENT_VERSION`, `USER_AGENT` | `...text("usque-api-version")`, `...text("usque-client-version")`, `...text("user-agent")` |
| `podssh_warp::probe::TARGET_V4`, `TARGET_V6` | `get("cloudflare-dns")...endpoint("trace").socket_addrs()` |
| `podssh_ws::forwarder::builtin::BUILTIN` | `of_kind("forwarder")`: `Builtin { word: text("word"), template: text("template"), note: text("note") }` |
| `acme::LETS_ENCRYPT` | `get("letsencrypt-acme")...endpoint("directory").url()` |
| `crates/podssh-probe/facts/relay-limits.toml` | the facts of `ajam-relay`; `scripts/check-relay-limits.py` moves to the watch `http-json` (T-050) |
| a relay, a resolver or a server of the user's own (`PODSSH_RELAY`, `PODSSH_STUN`, ...) | the variables stay; a consumer can also lay a file (`Catalog::lay`, feature `overlay`) |

podnet:

| Old | New |
| --- | --- |
| `ws_relay::DEFAULT_RELAY` | `pod_services::get("ajam-relay").unwrap().endpoint("default").unwrap().url()` |
| `relay_pool::DEFAULT_HOST`, `SEED` | `...endpoint("default").host()`, `...endpoints_of("seed").map(Endpoint::host)` |
| `relay::DEFAULT_RELAY`, the default of `transport::ssh::SshRelay` | `get("pinggy")...endpoint("ssh").authority()` |
| the match on `dashboard.pinggy.io` | `get("pinggy")...text("dashboard-host")` |
| `resolve::DOH_SERVERS` | for `cloudflare-doh` then `google-doh`, each endpoint: `DohServer { operator: party(), url: format!("https://{}{}", address in brackets for IPv6, text("json-path")) }` |
| `resolve::DOH_SERVERS_BY_NAME` | for the same two: `format!("https://{}{}", endpoint("v4-1").host(), text("json-path"))` |
| `edge_discovery::EDGE_SRV`, `cloudflare::EDGE_SRV` | `get("cloudflare-tunnel-edge")...text("srv-name")` |
| `edge_discovery::STATIC_EDGE_IPS`, `STATIC_EDGE_PORT` | `...texts("static-addresses")`, the `port()` of `endpoints_of("h2")` |
| `edge_discovery::FIXED_TARGETS` | `endpoints_of("h2").map(\|e\| (e.host(), e.port()))` |
| the suffix `.argotunnel.com` of `checked_targets` | `format!(".{}", ...text("srv-target-zone"))` |
| `tunnel::EDGE_REGION_NAME` | `...endpoint("h2-region1").host()` |
| `cloudflare::QUIC_SNI`, `QUIC_ALPN`, `H2_SNI` | `endpoint("quic-region1")`: `sni()`, `alpn()[0]`; `endpoint("h2-region1").sni()` |
| `cloudflare::CF_EDGE_ROOTS_PEM` | `...text("roots").as_bytes()` |
| `trust::EDGE_ROOTS_FIRST_END` | `...date("roots-first-end").to_string()` |
| `cloudflare::SPEAKS`, `cloudflared-pin.txt` | `format!("cloudflared {}", ...text("cloudflared-tag"))`; the facts `cloudflared-*` (the `copy` lines name no local file: the adapter knows its copies) |
| `udp::PROBE_TARGET` | `format!("{}:{}", ...text("probe-address"), the port of the role h2)` |
| the default targets of `warp probe` | `get("cloudflare-dns")...endpoint("trace")` (`1.1.1.1:443`) and `...text("warp-probe-address")` with the port of the role `h2` |
| `quicktunnel::DEFAULT_QUICK_SERVICE` | `format!("https://{}", get("trycloudflare")...endpoint("api").host())` |
| `util::validate_quick_hostname` (its zone) | `get("trycloudflare")...text("tunnel-zone")` |
| `doctor::targets` `api`, `warp-api` | `get("trycloudflare")...endpoint("api")`, `get("cloudflare-warp")...endpoint("api")`: `host()`, `port()` |
| `doctor::MASQUE_ADDRESS`, `MASQUE_ADDRESS_V6` | `get("cloudflare-warp")...endpoint("masque-v4").addresses()[0]`, `endpoint("masque-v6").addresses()[0]` |
| `warp::register::API_BASE`, `API_SECOND` | `...endpoint("api").url()`, `...endpoint("api-second").url()` |
| `warp::register::API_VERSION`, `CLIENT_VERSION`, `USER_AGENT` | `...text("api-version")`, `...text("client-version")`, `...text("user-agent")` |
| `warp::register::MASQUE_SNI`, `MASQUE_PORT`, `CONNECT_URI` | `...endpoint("masque-v4")`: `server_name()`, `port()`; `...text("connect-uri")` |
| `warp::l4::L4_SNI` | `...endpoint("masque-l4").host()` |
| `warp::exit::TRACE_ADDRESS`, `TRACE_ADDRESS_V6`, `TRACE_NAME`, the path of the trace | `get("cloudflare-dns")...endpoint("trace")`: `ip_addrs()`, `host()`, `path()` |
| the name `whoami.cloudflare` of `warp::exit` | `get("cloudflare-dns")...text("whoami-name")` |
| `warp::proxy::TUNNEL_RESOLVER`, `CHECK_NAME` | `...endpoint("udp-1").socket_addrs()[0]`, `...text("check-name")` |
| the nameservers of `warp::route::nameservers` and of `warp run` | `...endpoints_of("udp").map(Endpoint::host)` |
| `agent::udp_dns` (`1.1.1.1:53`) | `...endpoint("udp-1").authority()` |
| `tools/check-upstream.py` `REPO` | `cloudflare-tunnel-edge` fact `cloudflared-repository`; the check moves to the watches (T-050) |
| a relay of the user's own (`PODNET_RELAY`, `PODNET_WARP_API`, `PODNET_EDGE`, `--resolve`) | the variables stay; a consumer can also lay a file (`Catalog::lay`, feature `overlay`) |

podvm:

| Old | New |
| --- | --- |
| `podvm_image::reference::DEFAULT_DOMAIN` | `pod_services::get("docker-hub-registry").unwrap().text("domain").unwrap()` |
| `reference::DEFAULT_ENDPOINT` | `get("docker-hub-registry")...endpoint("registry").unwrap().host()` |
| `reference::OFFICIAL_PREFIX` | `get("docker-hub-registry")...text("official-prefix")` |
| the alias `index.docker.io` in `Reference::endpoint` | `get("docker-hub-registry")...texts("aliases")` |
| the default server of `podvm login` and `podvm logout` | `get("docker-hub-registry")...text("credential-key")` |
| `podvm_measure::workload::ALPINE_REF` | `format!("{}/{}", get("ecr-public-registry")...endpoint("registry").host(), ...text("guest-userland"))` |
| `workload::KURL`, `KERNEL_SHA256` | `format!("{}vmlinuz-virt", get("alpine-netboot")...endpoint("netboot").url())`, `...text("vmlinuz-virt-sha256")` |
| `ALPINE_REF`, `KURL`, `KERNEL_SHA256`, `MURL`, `MODLOOP_SHA256` of `experiments/146`, `147`, `360`, `361`, `390` | the same reads, through the export of the files (T-050) |
| `FD_URL`, `FD_SHA256`, `FD_IMG_BYTES` of `experiments/369` | `get("freedos-image")...endpoint("zip").url()`, `text("sha256")`, `int("image-bytes")` |
| `URL`, `ISO_LEN`, `VHDX_LBA`, `VHDX_LEN`, `VHDX_SHA256` of `experiments/371` and `lib/kvm-guest-base.sh` | `get("microsoft-validationos")...endpoint("iso").url()`, `int("iso-bytes")`, `int("vhdx-lba")`, `int("vhdx-bytes")`, `text("vhdx-sha256")`; the offset is `vhdx-lba` times `sector-bytes` |
| the hosts and the digests of `DISTRO_ROWS_NSSWITCH` and `DISTRO_ROWS_M5` | `of_kind("oci-registry")`: for each, `endpoint("registry").host()` and each item of `texts("images")`; the row names and the libc column stay in `distro-matrix.sh` |
| the pins of `experiments/70`, `80`, `90`, `220`, `105`, `106`, `280`, `367` | `docker-hub-registry` `experiment-images`, `quay-registry` `images`, `ecr-public-registry` `registry-image`, `aarch64-image` |
| a registry, a login or a guest image of the user's own (`--insecure-registry`, `podvm login SERVER`, `windows fetch --url`) | the flags stay; a consumer can also lay a file (`Catalog::lay`, feature `overlay`) |

## Sources

`SOURCES` lists each file of podssh (at `d3745ab`), of podnet (at
`380d9a4`) and of podvm (at `0249527`) that held a moved value, with the service files that it
became. Only some lines of each file moved: a change of such a file may
change a value, and `scripts/check-sources.py` reports it.
