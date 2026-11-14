# pod-services

The relays and the services of other parties that the PodUtils binaries
use: the owner's relay, DNS over HTTPS, plain DNS, STUN, DERP and the DERP
map, iroh's relays and pkarr, Piping, the magic-wormhole mailboxes and
transit, OpenTunnel, the edge of Cloudflare's tunnels and its quick
tunnels, WARP, ACME, the public forwarders, Pinggy, GitHub's SSH host
keys, the OCI registries with the images that podvm pins, the guest
images of podvm with their sums, the GSRN relay of gsocket, the TCP
relays of ZeroTier and of Tox, the public MQTT brokers on port 443, the
public hosts of temporary files, and the management API and the relay
protocol of Microsoft Dev Tunnels.
Each service is one data file, and each value of a file has its source
and the date when it was read.

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
| `scheme` | `https`, `wss`, `wisp+wss`, `ssh`, `ssh+tls`, `stun`, `udp`, `tls`, `quic` | a URI scheme in lower case |
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
| `unit` | the unit of a number: `ms`, `s`, `min`, `h`, `bytes` | optional word |
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
new kind is one implementation there. The tests of `pod-services-watch`
check the arguments of each watch against this table (`schema.rs`,
T-324). The kinds that the files name now:

| Kind | Arguments | What it checks |
| --- | --- | --- |
| `http-json` | `url`, `pointer`; `fact`, or `facts` with `members`, or `member` with `role` (with no `role`: each endpoint), or `key` with `fact-prefix`, `member`, `port-member`, `true-members` and `url-member` (the last three if any) and `role`, if any | the document has a JSON value at the pointer; it equals the fact, its members equal the facts, or its members of the name `member` hold the host of each endpoint of the role (more hosts in the document are no finding); with `key`, the value is a list, and each endpoint of the role is the item whose member `key` is the text of the fact `fact-prefix` + NAME: a member of `member` (a text or a list) holds its host, the member `port-member` (a number or a list) holds its port, and each member of `true-members` is `true` (more items with the port and the true members are counted, no finding); with `url-member`, the URL in that member of each item (`relay://HOST:PORT/?id=ID` of Syncthing's pool) gives the members `host`, `port` and one for each parameter of its query |
| `http-status` | `endpoint` or `role`, `path`, `status` | a GET answers with the status |
| `http-text` | `url` or `endpoint`, `contains` | the answer to a GET of the URL, or of the URL of the endpoint, holds the text (the document of a fact, also on another host than the service) |
| `tls-name` | `endpoint` or `role` | a TLS handshake verifies the name |
| `tls-names` | `endpoint` or `role` | the certificate at each address names the host and the address |
| `tls-roots` | `repo`, `path`, `tag`, `fact` | the certificates of the file of the repository at the tag of the fact `tag` are the fact's |
| `dns-srv` | `fact`, `role` | the SRV record of the fact names the hosts and the port of the role |
| `dns-addresses` | `role`, `facts` | the address records of the hosts of the role hold each address of the facts |
| `doh-answer` | `form`, `name`, `type` | each endpoint answers a query in the form |
| `stun-binding` | `endpoint` | a Binding request gets a Binding response |
| `websocket-upgrade` | `endpoint`; `protocol`, if any | the upgrade answers 101; with `protocol`, the upgrade offers that subprotocol (`mqtt` for an MQTT broker) |
| `forward-banner` | `template`, `target` | a forward to the endpoint of the service `target` gives its SSH banner |
| `proxy-banner` | `fact`, `target` | a session through the share link of the fact gives the SSH banner of the service `target` |
| `ssh-banner` | `endpoint` | the server sends an SSH banner |
| `transit-relay` | `endpoint` (scheme `tcp`) | two connections of one random token (`please relay TOKEN for side SIDE`) each get `ok`, and the bytes that the first sends come out of the second: magic-wormhole's transit relay |
| `forward-url` | `endpoint`; `port` (80 if none), `seconds` (45 if none), `prove` (true if none) | a session with the forward relay (a key made for the run, the fact `user` or the word `pod-services-watch`, a remote forward of the port, a shell) gives a banner with a URL that the rule of the relay accepts ("SSH forward relays"); with `prove`, a GET of the URL reaches the forward of the session: the client answers it with a word of the run |
| `ssh-host-key` | `endpoint`, `fact` | the SSH host key of the endpoint has the SHA-256 fingerprint of the fact |
| `github-ssh-keys` | `url`, `pointer`, `fact` | GitHub's published fingerprints equal the fact |
| `github-release` | `repo`, `fact`, `max-age-days` | the newest release; the age of the pinned tag |
| `github-files` | `repo`, `tag`, `fact` | the SHA-256 of each file of the fact at the tag and at the newest release |
| `github-file` | `repo`, `path`; `endpoint` or `facts` | the file on the default branch holds the values: each value of each fact, or the addresses of the endpoint (its host when it has none) |
| `crate-file` | `crate`, `path`; `endpoint` or `role` | the file of the newest release of the crate holds the hosts |
| `rdap` | `domain`, `ahead-days` (30) | the registration of the domain ends `ahead-days` or more after the run (a domain of the owner gives 90) |
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
| `forward::Relay` (`of`, `service`, `endpoints`, `user`, `url_rule`, `session_limit`, `keepalive`) | a service read as an SSH forward relay ("SSH forward relays"); `of` gives none for another service |
| `forward::UrlRule` (`matches`, `skips`; `accepts`, `pick`) | the rule of the URL of a tunnel: each text of `matches`, no text of `skips` |
| `forward::Banner` (`new`, `push`, `finish`, `url`, `held`, `MOST_HELD`) | the reader of a banner in chunks: the first URL that the rule accepts |
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
| `github-ssh` | `ssh-host` | `ssh`, `ssh-443` (`ssh.github.com:443`) | `host-key-sha256` (the same keys on both ports) |
| `github-actions` | `ci-service` | `github`, `api`, `broker`, `runner-auth`, `results-receiver` (role `control`), `codeload`, `release-assets` (role `download`) | `full-domains`, `wildcard-domains` (the `actions_inbound` domains of GitHub's meta API, each watched by `http-json`), `multi-tenant` (the wildcards that hold the content of other parties) |
| `github-actions-runner` | `runner-release` | `releases` | `runner-tag` (the pin of `actions/runner`; `github-release` fails past 30 days, GitHub's rule of updates) |
| `cloudflare-doh`, `google-doh` | `doh` | `v4-1`, `v4-2` (role `v4`), `v6-1`, `v6-2` (role `v6`) | `json-path`, `wire-path`, `http2-only` |
| `quad9-doh` | `doh` | `v4-1`, `v4-2` (role `v4`) | `wire-path`, `http2-only` (no `json-path`: no JSON form) |
| `cloudflare-dns` | `dns` | `udp-1`, `udp-2` (role `udp`), `trace` | `whoami-name`, `check-name` |
| `cloudflare-stun`, `google-stun` | `stun` | `udp` | |
| `tailcat-derp` | `derp` | `new-york`, `san-francisco`, `frankfurt`, `tokyo` (role `region`) | `probe-path` |
| `tailscale-derp` | `derp` | one for each server, its role the code of its region (`nyc-1`, ...: 95 in 28 regions) | |
| `tailscale-derp-map` | `derp-map` | `map` | |
| `n0-iroh-relay` | `iroh-relay` | `na-east`, `na-west`, `eu`, `ap` (role `relay`) | |
| `n0-pkarr` | `pkarr` | `relay` | |
| `ppng-piping` | `piping` | `server` | |
| `onrender-piping` | `piping` | `server` | `streams-request-body`, `idle-stop-minutes` |
| `magic-wormhole-mailbox`, `leastauthority-mailbox` | `mailbox` | `mailbox` | |
| `leastauthority-transit` | `transit` | `relay` (`wss`, port 443) | |
| `magic-wormhole-transit` | `transit` | `relay` (`tcp`, `transit.magic-wormhole.io:4001`: magic-wormhole's default transit relay, the handshake of `transit_server.py`) | |
| `opentunnel` | `opentunnel` | `api` | `tunnel-zone` |
| `cloudflare-tunnel-edge` | `tunnel-edge` | `h2-region1`, `h2-region2` (role `h2`), `quic-region1`, `quic-region2` (role `quic`) | `srv-name`, `srv-target-zone`, `static-addresses`, `probe-address`, `warp-probe-address`, `roots` (`files/cloudflare-edge-roots.pem`), `roots-first-end`, `cloudflared-repository`, `cloudflared-tag`, `cloudflared-commit`, `cloudflared-released`, `cloudflared-files` |
| `trycloudflare` | `quick-tunnel` | `api` | `tunnel-zone` |
| `cloudflare-warp` | `warp` | `api`, `api-second` (role `api`), `masque-v4`, `masque-v6` (role `masque`), `masque-l4` | `connect-uri`, `user-agent`, `api-version`, `client-version`, `usque-api-version`, `usque-client-version`, `terms-url` |
| `letsencrypt-acme` | `acme` | `directory` | |
| `mudlet-forwarder`, `ssheasy-forwarder`, `mudix-forwarder`, `wisp-forwarder` | `forwarder` | `forwarder` | `word`, `template`, `note` |
| `barry-far-list`, `mahdibland-list`, `v2go-list` | `proxy-list` | `list` (https, the raw file of a public list of share links) | `note` |
| `node-*` (8) | `proxy-node` | `node` (the host of the link, port 443) | `link` (a `vless://` or `trojan://` line of a public list; the share link is public, not a secret) |
| `pinggy` | `ssh-tunnel` | `ssh`, `ssh-tls`, `ssh-tls-alt` | `user`, `url-match`, `url-skip`, `keepalive-seconds` ("SSH forward relays"), `host-key-sha256`, `tunnel-zone`, `free-tunnel-minutes` |
| `serveo` | `ssh-tunnel` | `ssh-443`, `ssh` | `url-match` (`.serveousercontent.com`), `url-skip`, `auth` (`publickey`, `keyboard-interactive`: an anonymous session answers it empty), `note` (the private alias), `keepalive-seconds`, `host-key-sha256` (Ed25519) |
| `ssh-j` | `ssh-tunnel` | `ssh` | `host-key-sha256` (read by podssh's live run); no URL: no fact of a forward relay |
| `tuns-sh` | `ssh-tunnel` | `ssh` (`tuns.sh:22`) | `host-key-sha256`, `account-required` (true), `note` (the URL form `{user}-NAME.tuns.sh`); no URL rule: data alone (T-375) |
| `qsocket-qsrn` | `qsr-relay` | `relay` (`relay.qsocket.io:443`, TLS) | `secret-scheme` (`qs://`), `cipher`, `note` (a candidate: no path both ways proven); no rank (T-376) |
| `gosuda-portal` | `portal-relay` | `gosunuts`, `dawnfullstack` (role `relay`) | `identity`, `note` (a candidate: the public URL of a lease failed TLS); no rank (T-376) |
| `localhost-run` | `ssh-tunnel` | `ssh` | `user` (`nokey`: a free tunnel with no key), `url-match`, `keepalive-seconds`, `host-key-sha256` (read by podssh's live run) |
| `sandhole` | `ssh-tunnel` | `ssh-443` (plain SSH on the HTTPS port), `ssh` | `url-match`, `remote-port` (80: port 0 is refused), `host-key-sha256`, `tunnel-zone` |
| `openbare-xyz` | `bare-node` | `bare` (`https://openbare.xyz/bare/`, a TompHTTP Bare v3 server) | `service`, `protocol`, `websocket-upgrade` (500: no WebSocket passes the edge) |
| `localtunnel` | `localtunnel` | `api` | `tunnel-zone` |
| `bore` | `tcp-tunnel` | `control` (`tcp://bore.pub:7835`) | `control-port`, `tunnel-hours` |
| `smee-io` | `smee` | `server` (`https://smee.io`: a POST to `/CHANNEL` comes out of a GET of `/CHANNEL` as server-sent events) | `routes` (the routes of `lib/server.js`) |
| `hostc` | `hostc` | `api` (`https://hostc.dev`) | `tunnel-zone` (`hostc.app`), `protocol-version` (5) |
| `zerotier-tcp-relay` | `tcp-relay` | `tcp` (`204.80.128.1:443`, anycast; records like TLS, no TLS) | `relay` (`ZT_TCP_FALLBACK_RELAY` of ZeroTierOne) |
| `tox-tcp-relay` | `tcp-relay` | `node-KEY8` (5: each node of the list of Tox with 443 in `tcp_ports` and `status_tcp` true, scheme `tcp`, its IPv4 literal, port 443; the name holds the first eight digits of its key; in the order of the keys, C83) | `public-key-node-KEY8` (the node's public key, 64 hex digits, to which the handshake of the TCP relay boxes) |
| `syncthing-relay` | `syncthing-relay` | `relay-ID7` (25: each relay of the pool of Syncthing on port 443 with an IP literal that was up 7 days or more at the read, scheme `tls`; the name holds the first seven characters of its device ID; in the order of the device IDs, C104) | `device-id-relay-ID7` (the relay's device ID, as its URL gives it; podssh pins the TLS of the relay to it, PodUtils/podssh@T-571) |
| `croc` | `relay` | `bridge` (`wss://getcroc.com/ws`), `bridge-relay-0` (the watch: `?relay=0&port=9009`) | `relays`, `relay-port`, `password` (croc's public default) |
| `docker-hub-registry` | `oci-registry` | `registry` | `domain`, `aliases`, `official-prefix`, `credential-key`, `token-realm`, `images`, `experiment-images` |
| `ecr-public-registry` | `oci-registry` | `registry` | `token-realm`, `images`, `guest-userland`, `aarch64-image`, `registry-image` |
| `ghcr-registry` | `oci-registry` | `registry` | `token-realm`, `anonymous-quota`, `images` |
| `gcr-registry` | `oci-registry` | `registry` (`gcr.io`) | `token-realm`, `images` (`distroless/static-debian12`) |
| `mcr-registry` | `oci-registry` | `registry` (`mcr.microsoft.com`) | `images` (`playwright`; no token: `/v2/` answers 200) |
| `mirror-gcr-registry` | `oci-registry` | `registry` (`mirror.gcr.io`) | `mirror-of` (`docker-hub-registry`), `token-realm`, `images` (the pin of `guest-userland`) |
| `quay-registry` | `oci-registry` | `registry` | `token-realm`, `images` |
| `opensuse-registry` | `oci-registry` | `registry` | `images` (no token: `/v2/` answers 200) |
| `pypi-files` | `package-files` | `files` | `probe-path` |
| `alpine-netboot` | `guest-image` | `netboot`, `archive` | `release`, `vmlinuz-virt-sha256`, `modloop-virt-sha256`, `archive-sha256`, `archive-bytes`, `kernel-release`, `module-failover-sha256`, `module-net-failover-sha256`, `module-virtio-net-sha256` (the NIC modules in `boot/initramfs-virt`) |
| `cloudflare-workerd-npm` | `tool` | `archive` (`registry.npmjs.org`) | `version`, `archive-sha256`, `archive-bytes`, `archive-integrity`, `bin-member`, `bin-sha256`, `bin-bytes` (the npm package of workerd; podvm's profile `workerd-test`) |
| `wasmtime-release` | `tool` | `archive` (`github.com`) | `version`, `archive-sha256`, `archive-bytes`, `bin-member`, `bin-sha256`, `bin-bytes` (the release archive of wasmtime; podvm's profile `wasmtime-wasi`) |
| `shiftr-mqtt` | `mqtt-broker` | `broker` (`wss`, port 443: `public.cloud.shiftr.io` at `/`); the documents that the watches read (role `document`: `try`, `limits`) | `user`, `password` (its public login, `public` and `public`), `payload-bytes-most`, `topic-chars-most`, `client-id-chars-most`, `subscriptions-most`, `keep-alive-seconds-most`; the meeting points of `pod_wormhole::mqtt` (PodUtils/podssh@T-737) |
| `tmpfiles-org` (rank 1), `litterbox` (rank 2; not on by default: Catbox refuses an anonymous upload from the network of a cloud; C285), `temp-sh` (rank 3), `null-pointer-0x0` (0x0.st; no rank and not on by default: its terms refuse automated uploads, and it refused GitHub runners; C220, C282) | `file-host` | `upload` (`https`, port 443, the path of the upload form); the documents that the watches read (role `document`) | `file-field`, `fields` (`name=value`, the shortest life), `life-seconds`, `answer` (`text` or `json`), `answer-pointer` (with `json`: a JSON pointer), `serves` (the hosts of the URLs of the answer), `fetch-segment` (a path segment after the host, or empty), `fetch-link-prefix` (where the page at the URL links the file), `fetch-method` (`GET` or `POST`), `delete` (`none` or `token-form`; with `token-form`: `delete-token-header`, `delete-token-field`, `delete-fields`), `upload-bytes-most`, `uploads-per-hour` (where published), `one-person`, `automated-uploads-refused`, `on-by-default`, `user-agent-names-program` (if asked), `note`; the hosts of the pieces of PodUtils/podssh@T-738 ("Public file hosts") |
| `yggdrasil-wss-peer` | `mesh-peer` | one for each peer (`wss`, port 443) | `urls` (the peers, sorted; PodUtils/podssh@T-735) |
| `hyperdht-bootstrap` | `dht-bootstrap` | `node1` to `node3` (role `node`, scheme `udp`, port 49737, each with its address) | `nodes` (`IP@NAME:PORT`, as HyperDHT takes them; PodUtils/podssh@T-736), `addresses` |
| `freedos-image` | `guest-image` | `zip` | `sha256`, `zip-bytes`, `image-member`, `image-bytes` |
| `microsoft-validationos` | `guest-image` | `iso` | `iso-bytes`, `vhdx-lba`, `sector-bytes`, `vhdx-bytes`, `vhdx-sha256`, `vhdx-magic` |
| `hackerschoice-gsr` | `gsr-relay` | `default` (scheme `tcp`; the shard letter goes in front of the host) | `shard-count`, `addr-key`, `srp-key`, `srp-password-hex-chars`, `srp-user`, `srp-group`, `cipher`, `secret-prefix`, `secret-suffix`, `secret-max-bytes`, `protocol-major`, `protocol-minor`, `ping-interval-ms`, `idle-timeout-ms` (the facts of gsocket, read by `pod-gsr`) |
| `microsoft-devtunnel` | `devtunnel` | `management` (`https://global.rel.tunnels.api.visualstudio.com`; the relay URI of a tunnel is in its connect information) | `api-version`, `tunnels-path`, `token-scope`, `cluster-host-rule`, `user-schemes`, `relay-scheme`, `subprotocol`, `ssh-user`, `ssh-auth`, `channel-type`, `connect-host` (the values of the vendor's client, read by `pod-devtunnel`, T-378); `relay-calls`, `management-calls`, `user-scheme-calls`, `endpoint-fields`, `relay-endpoint-fields`, `tunnel-fields` (the words of the vendor's code that the `github-file` watches check) |

An image fact is `REPOSITORY[:TAG]@sha256:DIGEST`, with no host: the host
is the endpoint `registry`.

## SSH forward relays

A forward relay takes a remote forward (`ssh -R`) with no account, and
writes the URL of the tunnel in the text of a session channel, its banner
(podnet's `podnet ssh-relay`, PodUtils/podnet#455). It is a service of the
kind `ssh-tunnel` with the fact `url-match` or `url-skip`, and an endpoint
of plain SSH. `forward::Relay::of(service)` reads it; the code knows no
relay, and an overlay file of the same facts is a relay of the user's own.

| Fact | Type | What | `Relay` |
| --- | --- | --- | --- |
| `url-match` | texts | the URL of the tunnel holds each text | `url_rule().matches` |
| `url-skip` | texts | the URL of the tunnel holds none of the texts (a dashboard) | `url_rule().skips` |
| `user` | text | the SSH user of a free tunnel; none: the relay takes any user | `user()` |
| `free-tunnel-minutes` | int, `min` | the relay ends a free tunnel after this time; none: no source states a limit | `session_limit()` |
| `keepalive-seconds` | int, `s` | the keepalive of the relay's own command; none: no source states one | `keepalive()` |
| `remote-port` | int | the port of the remote forward that the relay takes (a relay that refuses port 0); none: the consumer's choice | `remote_port()` |

The endpoints of the scheme `ssh` are plain SSH on TCP, in the order to
try (`endpoints()`): an endpoint on port 443 comes first. An endpoint of
`ssh+tls` is SSH inside TLS, which `Relay` does not give. A value that no
source states is not in the file.

| Relay | Plain SSH | `user` | `url-match` | `url-skip` | `free-tunnel-minutes` | `keepalive-seconds` |
| --- | --- | --- | --- | --- | --- | --- |
| `pinggy` | `ssh` (443) | `http` (the type of the tunnel) | `https://`, `.run.pinggy-free.link` | `dashboard.pinggy.io` | 60 | 30 |
| `serveo` | `ssh-443` (443), `ssh` (22) | any | `https://`, `.serveousercontent.com` | `console.serveo.net` | | 30 |
| `localhost-run` | `ssh` (22) | `nokey` | `https://`, `.lhr.life` | | | 60 |
| `sandhole` | `ssh-443` (443), `ssh` (22) | any | `https://`, `.demo.sandhole.com.br` | | | |

`ssh-j` is of the kind `ssh-tunnel` too, but it gives no URL (a jump and
a forward by name): it has none of these facts. `tuns-sh` gives a URL, but
no banner of it was read and a session needs an account: it has no URL
rule either (T-375). `tests/forward.rs` holds each service of the kind to
one of the two lists.

Each relay has a watch `forward-url` on its first endpoint to try: a
session as podnet opens one, and its banner through `forward::Banner`
with the rule of the file. A banner with no URL that the rule accepts is a
finding on `url-match`. Then a GET of the URL proves it (T-267): an answer
with no word of the run (a page of the relay) is a finding on `url-match`
too. A watch `http-text` holds the document of the zone
of `url-match` (`pinggy`, `serveo`, `localhost-run`): a document that moves
the zone is a finding.

| Relay | `forward-url` | The document of the zone |
| --- | --- | --- |
| `pinggy` | `ssh`, port 0 (`-R0:localhost:8000` of its document) | `https://pinggy.io/docs/`: `.run.pinggy-free.link` |
| `serveo` | `ssh-443` | `https://serveo.net/docs/`: "myapp.serveousercontent.com" (the placeholder of a reserved hostname) |
| `localhost-run` | `ssh`, the user `nokey` | `https://localhost.run/`: `.lhr.life` |
| `sandhole` | `ssh-443` | none: the zone comes from podssh's runs |

The rule is the one of podnet's flags `--url-match` and `--url-skip`: a
URL is the tunnel's when it holds each text of `url-match` and no text of
`url-skip`. `forward::Banner` reads the banner as the session channel
gives it, in chunks:

```rust
let relay = pod_services::forward::Relay::of(pod_services::get("pinggy").unwrap()).unwrap();
let mut banner = pod_services::forward::Banner::new(relay.url_rule());
for chunk in channel_data {
    if let Some(url) = banner.push(&chunk) {
        return Ok(url.to_string());
    }
}
banner.finish().map(str::to_string) // the channel closed: the held word is whole
```

* A URL is one word. A word ends at a space, an end of line or another
  control byte (also the escape of a color, as Sandhole writes it). The
  reader holds the word that a chunk cut, and no more of its line.
* The URL of a word starts at `http://` or `https://`; the marks of a
  sentence at its end (`.`, `,`, `)`, `]`, `>`, a quote) are not part of
  it, and it has a host after the scheme.
* A word of more than `Banner::MOST_HELD` (2048) bytes is no URL: the
  reader passes over it to its end, so a banner with no end of line holds
  no more than that.
* After the first URL that the rule accepts, `push` gives it again and
  reads no more.

## Public file hosts

A service of the kind `file-host` is a public host of temporary files
with no account: the last of the last fallbacks of podssh's resumable
layer (PodUtils/podssh@T-738, PodUtils/podssh#220), one sealed piece for
each upload. podcrates makes no upload: the facts come from each host's
pages, from podssh's reading of 2026-11-12 where a page does not show
them (the source names PodUtils/podssh#220), and from the live runs of
podssh and of other parties on GitHub (PodUtils/podssh#230, T-528). The
live step of `live.yml` proves each form (`pod_services_watch::file_host`).
The table of the kind is
`pod_services_watch::schema::kind_facts("file-host")`: the endpoint and
the facts that each file needs, the ones it can have, and their types.

* An upload is a multipart POST to the endpoint `upload` with each field
  of `fields` and the file in the field `file-field`. The life that the
  fields give is `life-seconds`.
* The answer is the URL as text (`answer` `text`), or JSON with the URL at
  the pointer `answer-pointer` (`json`). Its host is one of `serves`.
* The file is a request of `fetch-method` to the URL of the answer, with
  `fetch-segment` (when not empty) as the first segment of its path.
* With `fetch-link-prefix`, the URL of the answer is a page. The file is a
  GET of the first link in the page that starts with the prefix and ends
  with the path of the URL of the answer. tmpfiles.org gives a signed link
  of a short life (`https://tmpfiles.org/dl/EXPIRY.SIGNATURE/ID/NAME`):
  take it from the page just before each download. Its `/dl/ID/NAME`
  answers 302 to the page.
* `delete` `none`: the file lives out its life. `token-form`: a POST to
  the URL of the file with the field `delete-token-field` (the value of
  the header `delete-token-header` of the answer) and `delete-fields`.
* `upload-bytes-most` is the lower reading of a limit that the page gives
  in GB (10^9 or 2^30 bytes). `uploads-per-hour` is there where the host
  publishes it.
* `automated-uploads-refused` is true when the terms refuse uploads by a
  program; such a host has no rank and a `note` (C220).
* `on-by-default` false: a road takes the host only when its user names
  it, and the `note` says why (0x0.st: its terms, and the 429 or 503 that
  it gave to GitHub runners; C282).
  `user-agent-names-program` is true when the host asks for a User-Agent
  that names the program.

The watches read the pages of the facts (`http-text`) and the status of a
GET of the upload form (`http-status`: 405, 412). 0x0.st answers 500 to
automated reads: its watch is `tls-name` alone. No watch uploads: the
live step of `live.yml` does, each Thursday and by hand (T-528, L37 of
`docs/live-calls.md`).

The state on GitHub runners (2026-11-13, PodUtils/podssh#230): temp.sh
works. tmpfiles.org works with the link of its page (live.yml run
43448958418). litterbox answers 412 "Invalid uploader" to the fields of
its API page and to those of its upload form: Catbox refuses an anonymous
upload from the network of a cloud, so it is not on by default (call
C285). 0x0.st was busy at once.

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
| `src/relay.rs` the match on `dashboard.pinggy.io` | [x] `pinggy` fact `url-skip` (T-224; it was `dashboard-host`) |
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
* `tests/api.rs` (20): by id, by kind and by rank, the roles, the
  accessors, a source and a date for each value, the watches, and the
  values of single services.
* `tests/actions.rs` (3): the hosts of GitHub Actions and the pin of the
  official runner, each with its source and its watch.
* `tests/forward.rs` (5): each service of the kind `ssh-tunnel` is a
  forward relay or is named as not one; each forward relay has the facts
  of its reader, of their types, from a document or a measurement, 443
  first; the values of each relay; the rule of each relay picks the URL of
  the banner that its source shows, in chunks of each size, and refuses
  the other URLs of the service.
* `tests/banner.rs` (10): the reader `forward::Banner`: a URL that two
  chunks cut at each byte, the dashboard skipped, each text of match and
  no text of skip, a line with no URL, a banner of 1 MB with no end of
  line held to the bound, a URL of the bound and a longer one, the escapes
  and marks around a URL, the end of the banner, the first URL kept, bytes
  that are not UTF-8.
* `tests/public_carriers.rs` (6): the rules of the kinds `wisp-public`, `proxy-list` and `proxy-node`: a source with a date for each value, port 443, no refused suffix, no VMess, the forms of the share links, one `word` for each forwarder.
* `tests/file_host.rs` (4): the services of the kind `file-host` in
  podssh's order; each fact of the kind, of its type and form; the
  endpoint `upload` on https and 443; a source with a date for each value
  (a page, podssh's reading, or a live run), and no measurement; no rank,
  not on by default and a note for a host whose terms refuse automated
  uploads; a link prefix of a host that the host serves; the values of
  each host.
* `tests/yggdrasil.rs` (2): the peers of `yggdrasil-wss-peer`: each a
  `wss://` URL on 443, the list sorted, each URL an endpoint of the file.
* `tests/hyperdht.rs` (2): the nodes of `hyperdht-bootstrap`: each text
  `IP@NAME:PORT`, each an endpoint of the file, each address in the fact
  of the watch `dns-addresses`.
* `tests/tox.rs` (3): the relays of `tox-tcp-relay`: each an IP literal
  on 443 with a key of 64 hex digits whose head is in its name, the
  order of the keys, and the watch that holds each endpoint to the list.
* `tests/syncthing.rs` (3): the relays of `syncthing-relay`: each an IP
  literal on 443 with a device ID of eight groups of seven whose head is in
  its name, the order of the device IDs, and the watch that holds each
  endpoint to the pool.
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
* `tests/podvm.rs` (10): the constants of podvm come back from the files
  (`reference.rs`, `images.rs`, `workload.rs`, `371-validationos-stream.sh`);
  each registry answers at `/v2/`; each image is a repository and a
  SHA-256 digest; each sum of a guest image is a SHA-256; the registry of
  playwright holds podvm's pin; the npm archive of workerd and the release
  archive of wasmtime hold podvm's pins. podvm has no test of these values to move.
* The plant `scripts/plants/services-no-source` (group `services`): a
  service file whose fact names no source; the build must fail with the
  file and the field.
* The plant `scripts/plants/services-file-host-fact-missing` (group
  `services`): a file host with no fact `one-person`;
  `tests/file_host.rs` must fail.
* The plants `scripts/plants/services-forward-word-not-held` (the reader
  ends a word at the end of each chunk, so a cut URL is lost) and
  `scripts/plants/services-forward-held-unbounded` (no bound on the held
  word) (group `services`): `tests/banner.rs` must fail.
* The watches of the files: `crates/pod-services-watch` (T-050) checks
  each one against the live service; its tests and one plant for each
  watch kind (`scripts/plants/services-*`) are there.

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
| the hosts of the file hosts of `podssh-relay`'s `filehost` (PodUtils/podssh@T-738, new) | `of_kind("file-host")`: `endpoint("upload").url()`, `text("file-field")`, `texts("fields")`, `text("answer")`, `text("answer-pointer")`, `texts("serves")`, `text("fetch-segment")`, `text("fetch-link-prefix")`, `text("fetch-method")`, `text("delete")` and its facts, `int("upload-bytes-most")`, `bool("automated-uploads-refused")`, `bool("on-by-default")` ("Public file hosts") |
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
| `scripts/check-upstream.py` (`upstream.yml`) | the watches `github-release`, `github-files` and `tls-roots` of `cloudflare-tunnel-edge` in `services.yml` of podcrates; the script leaves podssh with its pin file `quick/cloudflared-pin.txt` (T-151) |
| `scripts/check-domain.py` (the step of `relay-watch.yml`) | the watch `rdap` of `ajam-relay` in `services.yml` of podcrates; the script and the step leave podssh with T-151 |
| `pinggy::dashboard()` (`text("dashboard-host")`) | `get("pinggy")...texts("url-skip")` (T-224 moved the fact) |
| `ssh_http::Word::{user, ports}`, `ssh_http::name::Lines` | can read `forward::Relay` (`user()`, the ports of `endpoints()`) and `forward::Banner`; the word `tunnel` of Sandhole stays podssh's (no source states a user) |
| a relay, a resolver or a server of the user's own (`PODSSH_RELAY`, `PODSSH_STUN`, ...) | the variables stay; a consumer can also lay a file (`Catalog::lay`, feature `overlay`) |

podnet:

| Old | New |
| --- | --- |
| `ws_relay::DEFAULT_RELAY` | `pod_services::get("ajam-relay").unwrap().endpoint("default").unwrap().url()` |
| `relay_pool::DEFAULT_HOST`, `SEED` | `...endpoint("default").host()`, `...endpoints_of("seed").map(Endpoint::host)` |
| `relay::DEFAULT_RELAY`, the default of `transport::ssh::SshRelay` | `get("pinggy")...endpoint("ssh").authority()` |
| the match on `dashboard.pinggy.io`, the default of `relay::UrlRule` | `get("pinggy")...texts("url-skip")` (T-224 moved `dashboard-host` into it) |
| `relay::RelayConfig::default()`: `address`, `username`; the defaults of the flags of `podnet ssh-relay` (`--server`, `--user`, `--url-match`, `--url-skip`) and its `KEEPALIVE` | `forward::Relay::of(get(ID))`: `endpoints()` (each `authority()`, tried in order), `user()` (none: podnet's own word), `url_rule()`, `keepalive()`, `session_limit()`; a flag stays an override |
| `relay::UrlRule` (`hold`, `skip`), `UrlRule::pick` | `forward::UrlRule` (`matches`, `skips`), `forward::UrlRule::pick` |
| the read of the banner chunk by chunk in `relay::open` (`ChannelMsg::Data`; `Eof`, `Close`) | `forward::Banner::push` for each chunk, `forward::Banner::finish` at the end of the channel |
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
| `tools/check-upstream.py` `REPO` | `cloudflare-tunnel-edge` fact `cloudflared-repository`; the check is the watches `github-release`, `github-files` and `tls-roots` of `cloudflare-tunnel-edge` in `services.yml` of podcrates; the script leaves podnet with its pin file (T-151) |
| `tools/edge-facts.sh`, the step "Edge facts" of `live.yml` | `scripts/services/edge-facts.sh` of podcrates, which `services.yml` runs each day (T-050; podnet took it at `0b945a3`, PodUtils/podnet#448) |
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
