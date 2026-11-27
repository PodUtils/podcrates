# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md` (the
procedures, the tools and the rules of the work), `docs/design.md` and
`docs/decisions.md`. Keep this page current: remove a line when its work
is done. Git and the record keep finished work.

## The first steps of the next session

1. Start (`docs/hosted-sessions.md`, "Start"). The Claude Code hook runs
   `scripts/session-start.sh`; in another harness, run it by hand, then
   `sh scripts/cloud-setup.sh` on a new host and `. ./scripts/cloud-env.sh`
   in each shell. Read `.work/watch.log` and `.work/runs.log` after each
   step. Give the work to workers by the role table and the brief of
   `docs/hosted-sessions.md`, "Lead and workers". Work until each task is
   done or blocked on a thing out of the control of the session (the
   ruling "Work until done").
2. Coordinate the five sessions by COORD issues alone (the rulings
   "Coordinator" and "COORD only"). COORD-136 (#227) gives the order of
   each session; route each ask of podrunner first, and edit #227 when the
   order changes. COORD-157 (#263, T-1387) asks each repository for one
   `docs/hosted-sessions.md` and one role table: record each answer on its
   body, and close it when the four sessions answered.
3. Answer the open COORD issues `(to podcrates)`. Map each new one in the
   same commit.
   - The hand-off of `v0.86.0` is COORD-158 (#272). It answers
     PodUtils/podvm#667; PodUtils/podnet#753 to #756, #758 and #776;
     PodUtils/podssh#359 to #363 and #367. Record each take on its body
     (the "Takes" lines, as #262), and close it when each consumer names the
     tag (call C780). The takes of podnet (`144a5ee`,
     PodUtils/podnet#778), podrunner (`879af093`, PodUtils/podrunner#366)
     and podvm (`71a15d3` in `ef541f1`, PodUtils/podvm#690) are verified.
     The take of podrunner is on the body of #272; the takes of podnet and
     podvm are in a comment, and go to the body at the close. COORD-159 (#273) is closed: podvm fixed its
     frozen `ext.rs` at `846e733` (PodUtils/podvm#687). COORD-157 (#263): podnet,
     podvm and podrunner took the role table; it waits for podssh.
   - On `main` after `v0.86.0`: PodUtils/podvm#675 (COORD-256, T-1424) and
     PodUtils/podvm#677 (COORD-258, T-1425); answer each with the next tag.
   - On `main` after `v0.86.0`: PodUtils/podssh#358 (COORD-148, T-1390);
     PodUtils/podvm#678 (COORD-259, T-1426, T-1427); PodUtils/podvm#680
     (COORD-260, T-1500); PodUtils/podnet#781 (COORD-136, T-1507, T-1508);
     PodUtils/podnet#783 (COORD-138, T-1516).
   - Done on `main`: PodUtils/podnet#779 (T-1431, T-1432) and ask 2 of
     PodUtils/podnet#782 (T-1432). Done on `main`: PodUtils/podnet#780
     (T-1505, T-1506). Done on `main`: PodUtils/podnet#782
     (T-1525 to T-1527; ask 5 is T-1528, open). Done on `main`:
     PodUtils/podnet#785 (T-1531, T-1532). Done on `main`:
     PodUtils/podrunner#367 (COORD-140, T-1529) and PodUtils/podrunner#373
     (COORD-145, T-1530); answer each with the next tag. The answer to
     #373 tells podrunner to read `jobServiceContainers` (the name of
     `AgentJobRequestMessage`), not `serviceContainers`, and that the
     step logs come from `GET /_fake/jobs/ID/logs` of `pod-fake-github`
     (a lane reads `github_job_id` of the githome job). The owner deletes
     `wip/gh373` (the proxy refuses a delete).
   - Open asks:
     PodUtils/podvm#668 (COORD-251) is T-1391; PodUtils/podssh#357
     (COORD-147) asks T-1380, T-1381 and T-1383; PodUtils/podssh#349
     (COORD-139) is T-1169.
   - Answered, waiting for the take of the consumer: PodUtils/podssh#340,
     #349, #351 to #353, #355, #356; PodUtils/podvm#657 (T-1193 to
     T-1196). podrunner took PodUtils/podrunner#352 (T-1362) and #353
     (T-1370) at `3391edc` and `f2716a1`; its session closes them.
   - Mapped, no step now: PodUtils/podssh#350 (T-1191, T-1192);
     PodUtils/podvm#659 (T-1205), #661 (T-1213); PodUtils/podnet#709
     (T-1209).
4. Read the runs of the head of `main` (`ci`, `plants`, `adoption`,
   `secrets`, `deny`, `issue-sync`). A new red run needs an entry before
   other work. The open red runs and the runs to read:
   - `nightly.yml` 45603454952 at `cd7cb5a`: T-1444 (`pod-core`,
     `files::temp` on Windows: `PermissionDenied` at the name of a
     directory) and T-1445 (`pod-tpm`, `tests/socket.rs` on macOS: the
     time limits of a closed Unix stream, `pod_core::sockopt`) are done on
     `main`; the next nightly is their proof (no run on Windows or macOS
     here). T-1446 (`pod-warp`, a
     socket path over 104 bytes on macOS) and T-1447 (the doc tests of
     `pod-h2`) are fixed on `main`. `riscv64` and `loongarch64` fail in
     `pod-iroh` (T-916, T-938, the ruling "Emulated nightly jobs"). T-1359
     and T-1360 held. Open: T-789, T-766, T-1044.
   - `ci.yml`: T-1325 closes with the times of five runs of the two macOS
     rows of `static` (limit 60 minutes): read the next `ci` runs.
   - `services.yml`: read the next scheduled run (T-1415 reopens on a
     second 5xx of `magic-wormhole-mailbox`; T-1096 reopens when
     `freedos-image` times out again). `live.yml`: T-963 (partial); T-1504 (the upload of temp.sh
     answered 500 on 2026-11-26: read the next scheduled run).
   - `upstream.yml` (job `sources`): read the next daily run.
   - The dry run of `publish.yml` on the tag `v0.86.0` (45786405631):
     read it (step 5 of "A release").
   - A plant that a run misses once with no change of its crate runs its
     test five times in its `.plant` (as `link-quic-drop-closes-at-once`).
5. The work order of `TODO/PROGRESS.md`: the next tag (T-1389, T-1392 to
   T-1394), then T-106 (the CI of githome and cfb), then the asks of the
   consumers. `docs/research/hard-problems.md` has the hard open problems
   (H-01 to H-18); T-978 (partial) holds the findings that no other entry
   has.
6. Rules of the session that the documents do not repeat:
   - The channel of cfb and githome is #223. podcrates keeps one reply
     comment to each there (cfb: 6970315549; githome: 6970683028) and
     edits it, with the UTC time of each update. cfb is offline until the
     CI of podrunner and githome is complete; cfb's A-X-18 (T-106,
     partial) waits for its answer on #223.
   - A step of githome's answers that podrunner writes and that differs
     from `pod-fake-githome` or `pod-fake-github` is an entry here.
   - A pull request from outside the team gets CI runs with the state
     `action_required`. Review its code before you approve or dispatch
     its CI (the ruling "Strangers").

## Release: v0.86.0

`v0.86.0` (`22d1a75`, `docs/releases/v0.86.0.md`) is released: the GitHub
release, the signed `SHA256SUMS` and `SHA256SUMS.sigstore.json`, 14 assets
and the images of `ghcr.io/podutils` (each index passes `gh attestation
verify`). `ci` 45779678814 at `22d1a75`, the dry run of `release.yml`
45784561978 and the run on the tag 45785761766 passed. It carries the
tmpfs of `pod-enter` (T-1389), the asks of podnet #753 to #758 and #776
(T-1392 to T-1395, T-1404, T-1416, T-1474, call C973), the asks of podssh
#359 to #363 and #367 (T-1396, T-1399 to T-1412, T-1421 to T-1423), the
fixes of the nightly (T-1444 to T-1447) and the Dependabot updates
(T-1414, with `incompatible-rust-versions = "allow"`). The notes name each
breaking change with the files of each consumer.

The hand-off is COORD-158 (#272). The hand-off COORD-156 (#262) of
`v0.85.0` is closed: each consumer took it.

### On `main` after `v0.86.0` (for the next tag)

- T-1424 (`pod-hostprobe`, PodUtils/podvm#675): `Settings::emulator`
  (`Option<PathBuf>`) and `Settings::with_emulator`; the machine legs run
  the emulator at that path. Breaking: `podvm-probe` builds `Settings`
  with each field and adds `emulator: None`.
- T-1500 (`pod-guest`, PodUtils/podvm#680): the import of podvm's writer
  of an empty ext4 disk (`crates/podvm/src/machine/guest/disk/ext.rs` at
  `9c1edb5`): `pod_guest::ext4::write_empty(&file, bytes, &Names)`,
  `Names::random(label)`, `Layout::of(bytes)`, `MIN_BYTES` 64 MiB,
  `MAX_BYTES` 16 TiB less a block. A fix of the import: a superblock copy
  past group 65535 records 65535, as `mke2fs` does (podvm's frozen copy
  records the low 16 bits: tell podvm). A size that is not a whole number
  of 4 KiB blocks is refused (podvm cut it down). `e2fsck` 1.47.0 passes a
  wrong free count of the superblock alone: the plant and the test check
  the counts of the group descriptors. podvm: depend on `pod-guest` in
  `crates/podvm/Cargo.toml`, call `Layout::of` in `refusal` and
  `write_empty(&file, bytes, &Names::random("podvm-disk")?)` in `make`
  (no `set_len` before it), and remove `ext.rs`.
- T-1507, T-1508 (`pod-cloudflare`, PodUtils/podnet#781): `Capture::route`
  (`Route { rule, hostname, host }`, `non_exhaustive`) before the head of a
  request that an ingress rule took, and `Capture::fault` (`Fault::
  RequestChunking`) before `end`; both have default bodies, so podnet
  builds with no change. A failure of the chunk decoder is not a refusal
  of the sink: the sink gets `fault` and one `end`. `Dechunk` refuses a
  chunk size over `i64::MAX` (`ffffffffffffffff` was taken as a size).
- T-1431 (`pod-proxy`, PodUtils/podnet#779 ask 1): `ProxyKind::Https`
  (`https://[USER[:PASS]@]HOST[:PORT]`, port 443), `Dialer::with_proxy_tls`
  and `ProxyTls`, `pod_proxy::hop::tls` (moved from `pod_egress::via`, one
  copy), `Stream::Tls`, `blocking::Stream::Tls`. With no proxy TLS given an
  `https` proxy fails with `BadProxy`; `Dialer::tcp` refuses a TLS stream.
  Breaking for podvm: `crates/podvm-ssh/src/transport/egress.rs` matches
  `blocking::Stream` with no wildcard: add a `Stream::Tls` arm. Each
  consumer adds `.with_proxy_tls(ctx, Trust::Default)` to take an `https`
  proxy (podnet `src/egress.rs` and `podnet-core/src/connect.rs`; podvm
  `mux.rs`, `egress.rs`; podssh `podssh-ws/src/dial.rs`, `resolve.rs`;
  podrunner `podrunner-egress/src/dns.rs`). T-1433 (`pod-iroh` gives iroh
  an `https` proxy by its address) and T-1434 (`pod-relay` dials a reverse
  target with `Dialer::tcp` alone) are open.
- T-1432 (`pod-egress`, PodUtils/podnet#779 ask 2 and #782 ask 2): the
  feature `standin` exports the stand-ins of `via`, wstunnel, VLESS (ws,
  xhttp h2 and HTTP/1.1), Trojan, Wisp (v1, v2) and Shadowsocks (with the
  feature `shadowsocks`); a consumer takes them in `[dev-dependencies]`.
- T-1505 (`pod-acme`, PodUtils/podnet#780 ask 1): `Contact` (`parse`,
  `mailto`; a bad contact is refused by name before any request) and
  `Acme::with_contacts`; `newAccount` sends them, and an account that
  exists gets an update. A refused contact is `Failed::Ca` with the
  contact in its words. podnet: `--acme-email` as
  `acme::open(..)?.with_contacts(vec![Contact::mailto(email)?])`.
- T-1506 (`pod-acme`, PodUtils/podnet#780 ask 2): the feature `standin`:
  `standin::Standin` (`start(provider, Plan)`, `directory`, `trust`,
  `http`, `seen`), a CA that checks the JWS, the nonces, the account of
  each object, the CSR and the TLS-ALPN-01 certificate of RFC 8737. No
  build change for podnet.
- T-1525 (`pod-egress`, PodUtils/podnet#782 ask 1): `Wisp::is_open()` and
  `wisp::Reopening`, a `Dial` that opens a new connection on the next dial
  after the old one ended (3 tries with a backoff, in the deadline; a
  refused credential is tried once). podnet can drop its `Held`.
- T-1527 (`pod-egress`, PodUtils/podnet#782 ask 4): `wisp::Login` (zeroed on
  drop, never shown) and the field `Connect::login`. Breaking: podnet's
  literal of `Connect` (`src/networks/servers/wisp.rs`) adds
  `login: None`. A login over `ws://` to a host that is not the loopback is
  refused. A refused credential (0xc0 to 0xc2) is stage `Relay` now, not
  `Origin`.
- T-1526 (`pod-egress`, PodUtils/podnet#782 ask 3): `shadowsocks::key_file`
  (an `ss://` link, SIP008 JSON or Outline YAML in a private file), the
  `Display` of a `Node` shows no password and no path, Outline's salt
  prefix (`Node::with_prefix`; it was refused in YAML and JSON and dropped
  from an `ss://` link), and YAML merge keys. T-1528 (a wait for the answer
  of a VLESS server) is open with its decision.
- T-1517 to T-1521 (the ports of `upstream.yml` 45838753274):
  `pod_core::text::file_words`, and the lines of `pod_core::files` and of
  `pod-unit` show no " (os error N)"; `pod-guest` `ext4` gains
  `Layout::fit`, `write_heads`, `Counts`, `bitmap` and the layout methods
  of podvm's change at `a00e3a4` (`write_empty` gives the same bytes);
  `pod-gzip` `SOURCES` has the `taken` line of podvm's take at `04c8b7f4`;
  no change of the fakes of `pod-testkit`. T-1522 (podvm's writer of a
  read-only ext4 disk of an image tree stays in podvm) is open.
- T-1513 (`pod-services`): DERP region 24 (Honolulu, `hnl-1` to `hnl-3`)
  is gone from Tailscale's map, and `derp14e` (`ams-4`) is new: 93
  endpoints in 27 regions (podnet and podssh read the set). The watch
  `github-release` fails by age only when a newer release exists (the pin
  `v2.338.2` is the newest).
- T-1516 (`pod-derp`, PodUtils/podnet#783): the derper tests read
  `POD_DERP_TEST_SERVER` and `POD_DERP_TEST_CA`.
- T-1426 (`pod-oci`, PodUtils/podvm#678 ask 1): `Reference::parse` bounds
  the name to 255 characters (`NAME_MAX`) and a tag to 128 (`TAG_MAX`),
  refuses a tag that starts with `.` or `-`, and refuses a path component
  outside docker's grammar (`-a`, `a..b`); each error is `Class::Usage`
  and cuts the input to 64 characters. No build change.
- T-1427 (`pod-enter`, PodUtils/podvm#678 ask 2): `WorkdirMade { ByDocker,
  NotMade }` and `check_workdir_as(root, workdir, image, made)`; the clause
  "docker creates it" is said for `ByDocker` alone. No `Plan` field. podvm:
  add `check_workdir_as, WorkdirMade` to the re-export of
  `crates/podvm-enter/src/lib.rs`, and call it with `WorkdirMade::NotMade`
  in `crates/podvm/src/exec/command.rs` and `crates/podvm/src/exec/enter.rs`.
  T-1428 (a file at the working directory reads as absent) is open.
- T-1390 (`pod-term`, PodUtils/podssh#358): `Transfer::set_size(cols,
  rows)`, `pane_chunk`, `CHUNK_FLOOR` (256), `SPARE_ROWS` (2): a `DATA`
  frame takes at most `rows - 2` lines; each end sends its pane cap in the
  `chunk` of `HELO` and takes the smaller cap; a lost `DATA` goes again
  built from its first `CHUNK_FLOOR` bytes. The codec stays version 2 (the
  `chunk` of `HELO` was a cap already), so old and new ends pair. podssh:
  in `crates/podssh-ssh/src/zmodem/far.rs` call `codec.set_size(cols,
  rows)` at the start and at each resize in place of `set_width`, drop
  `PACE` and the parts of `put`, and name the cap in `docs/terminal.md`.
- T-1488 (`pod-link`): the loopback, UDP and split lines show no
  " (os error N)" (`pod_core::text::io_words`).
- T-1473 (`pod-relay`), breaking: `grant::openssh_public` is removed. podssh
  (`crates/podssh-cli/src/grants.rs`, three places): `grant::openssh_public(&k)`
  becomes `k.openssh()` (`pod_identity::PublicKey::openssh`, the same type).
  `grant::parse_public` reads with `PublicKey::from_openssh`.
- T-1480 (`pod-core`): `pod_core::abort::AbortOnDrop` (feature `async`) in
  place of the copies of `pod-relay`, `pod-cloudflare` and `pod-warp`.
- T-1425 (`pod-oci`, PodUtils/podvm#677): `spec::Config::rootfs`,
  `spec::RootFs`, `spec::check_layers`, `MAX_LAYERS`. No build change.
- T-1529 (`pod-job`, PodUtils/podrunner#367): `UnitFields::limits`
  (`pod_job::UnitLimits`: `memory_bytes`, `cpus`, `pids`, `disk_bytes`,
  each `Option<u64>`). Each literal of `UnitFields` adds `limits: None`;
  the schema id stays `run-report.v1`.
- T-1531 (`pod-egress`, PodUtils/podnet#785): `trojan::accept::parse`
  (the server side of a Trojan request: `Request`, `Command`,
  `RequestError`, `Malformed`, `MAX_REQUEST`) and
  `trojan::Password::matches` (constant time). The features `fuzzing`
  and `standin` turn on `trojan`.
- T-1532 (`pod-egress`, PodUtils/podnet#785): `shadowsocks::Decoder::salt`
  and the docs of the accept side of the codec.
- T-1530 (`pod-testkit`, PodUtils/podrunner#373): `POST /_fake/jobs` of
  `pod-fake-github` takes `container` and `services`; the job message
  carries `jobContainer` and `jobServiceContainers` as template tokens
  (`null` when absent); `pod-fake-githome serve --github` passes them
  through; `GET /_fake/jobs/ID/logs` gives the step logs of a job that
  ended (404 for no such job, 409 for a job that did not end). No build
  change.
- T-1506 (`pod-acme`, feature `standin`): the TLS-ALPN-01 verifier of the
  stand-in takes only the certificate of the challenge (a pinned
  verifier); the error of a refused certificate is the words of the check.

### What the next hand-off carries

- The takes of `v0.86.0` (#272), and the answer of podssh to COORD-157
  (#263).
- podvm: T-1424, T-1425 (on `main`); T-1391 (a static `mkfs.erofs`,
  PodUtils/podvm#668); T-1430; T-1321.
- podnet: T-502 (PodUtils/podnet#760); T-1450 to T-1452; T-1343
  (partial); T-1377, T-1378.
- podssh: T-1390 on `main`; T-1413 (optional); T-1169,
  T-1103, T-1340, T-1380, T-1381, T-1383.
- podcrates: T-1463 (a check that stops
  a `to_string()` of a failure), T-1460, T-1453, T-1484 (iroh 1.5).

## Unfinished work

- `origin/wip/t1073` is merged into `main` (T-1073, T-1070). The proxy
  refuses `git push origin --delete`, so the owner deletes the branch on
  GitHub (`TODO/PROGRESS.md`, "Owner actions").

## What waits

- podvm: T-330 (its pair commands). T-207 (`pod-netlab`) does not wait for
  podvm: podvm closed PodUtils/podvm#260 and took the offline part; T-345 to
  T-347 (the probe program, the world and the run on a kernel) are open, and
  podvm's lab guest (PodUtils/podvm@T-1662 to PodUtils/podvm@T-1664) takes
  them when they are done.
- podswarm: its asks wait until its session starts, but for T-444, T-447
  and T-448 (the ruling "podswarm").
- `pod-git` (T-217 to T-220) is blocked (`PodUtils/podforge`, deferred).
  The rest of M8 waits (the ruling "Priority"); the entries for the CI of
  githome and cfb are in the work order.
- T-386 (the live run of `pod-devtunnel`) waits for the owner's token and
  tunnel (`TODO/PROGRESS.md`, "Owner actions").
