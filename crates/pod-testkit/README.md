# pod-testkit

The test kit of podcrates: a dev-dependency of each crate, and of a
consumer that wants it. It is never a dependency of a release binary.

## API

* `need_loopback!()` and `need_loopback!(value)`: a test that needs TCP or
  UDP on 127.0.0.1 returns at once, with a line `skipping, REASON` on
  stderr, on a host that refuses `bind(AF_INET)` or has no loopback.
* `need_localhost!()`: the same for a host that does not resolve
  `localhost`.
* `need_userns!()`: the same for a host that refuses a user and a mount
  namespace with a tmpfs mount in them (Linux only; elsewhere the test
  skips).
* `need_tool!(TOOL)`: the same for a host with no `TOOL` that can execute.
  `TOOL` is a path (`"/bin/sh"`) or a name in `PATH` (`"qemu-img"`).
* `need_root!()`: the same for a process whose effective user is not
  root (Linux only; elsewhere the test skips).
* `host::loopback()`, `host::localhost()`, `host::userns()`: the probes,
  each run once for each test binary. `host::tool(TOOL)` and
  `host::root()` look each time.
* `elf::static_exit0()`: a static ELF of x86_64 or aarch64 (one
  `PT_LOAD`, no `PT_INTERP`) that calls `exit_group(0)`, for a test of an
  exec with no tool of the host; `None` on another architecture.
  `elf::is_dynamic(bytes)`: a 64-bit ELF with `PT_INTERP`. Linux only.
* `tamper::flip(path, at)`: one bit of the byte `at` of a file changed
  (the last byte for an `at` past the end), for the test of a check that
  must refuse one changed byte.

A stand-in of this kit speaks bytes and HTTP only, and takes no type of a
pod crate, so that each crate can dev-depend on the kit. The feature
`githome` is the one exception: it speaks the source wire of `pod-job`.

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podnet | `test_host::need_loopback!()` (`tests/test_host/mod.rs`, `#[path]`) | `pod_testkit::need_loopback!()` |
| podnet | `test_host::need_localhost!()` | `pod_testkit::need_localhost!()` |
| podvm | a test that forks, calls `unshare(CLONE_NEWUSER\|CLONE_NEWNS)` and skips on exit 2 | `pod_testkit::need_userns!()` first |

## The terminal oracle (feature `screen`)

`pod_testkit::screen::Screen`: a model of a VT terminal (vt100) that reads
the bytes of a program and tells what a person sees: each row, the
cursor, the colors and the attributes of a cell. `pod-term` checks the
model against tmux (`crates/pod-term/tests/calibration.rs`).

| Consumer | Old path | New path |
| --- | --- | --- |
| podssh | `#[path = "../../podssh-terminal/tests/screen/mod.rs"] mod screen;` (tests of podssh-ws, podssh-ssh, podssh-cli) | `use pod_testkit::screen;` with `pod-testkit = { git = ..., tag = ..., features = ["screen"] }` as a dev-dependency |

## The stand-ins (feature `stand`)

`pod_testkit::stand`: servers on the loopback for the offline tests of a
client or a watch. Each runs on threads, keeps the bytes that each
connection sent (`Stand::seen`), and lives until the end of the test
binary.

| Item | What |
| --- | --- |
| `stand::http(handler)` | an HTTP/1.1 server: each request (`Request`: `method`, `target`, `headers`, `path()`, `header(name)`) gets the `Answer` (`status`, `headers`, `body`) of the handler, with `Content-Length`, kept alive; a `101` writes the body after the head and keeps the connection (a WebSocket's frames) |
| `stand::raw(serve)` | a TCP server that gives each connection to `serve` |
| `stand::udp(answer)` | a UDP server that answers each datagram with what `answer` gives |
| `Stand::http_over(handler, wrap)`, `Stand::raw_over(serve, wrap)` | the same over the caller's stream: `wrap` turns each TCP connection into it (TLS with rustls's `StreamOwned`); the kit itself has no TLS |
| `Stand::addr()`, `Stand::port()`, `Stand::seen()` | the address; the bytes of each connection that ended |

`crates/pod-services-watch/tests/watches/` uses each of them.

## The fake GitHub (feature `github`)

`pod_testkit::github::FakeGithub` and its static binary `pod-fake-github`:
a GitHub on one TCP address for the CI of a consumer that drives a GitHub
App and its runners with no account. Each release attaches
`pod-fake-github-x86_64-unknown-linux-musl` and
`pod-fake-github-aarch64-unknown-linux-musl` with their lines in
`SHA256SUMS`. Build it with
`cargo build --release -p pod-testkit --features github --bin pod-fake-github`.

`pod-fake-github [--listen ADDRESS] [--hook URL]` prints its base URL
(`http://127.0.0.1:PORT/`) on the first line of stdout, then serves until
it is killed. Each credential is made up: the RSA key of
`src/github/made-up-key.pem` (PKCS#1, the form of the key of an App), the
webhook secret `pod-fake-github-made-up-webhook-secret`, and the random
tokens of each run.

| Part | Routes |
| --- | --- |
| The App | an RS256 JWT of the made-up key with `iss` 1, alive now and for ten minutes at most: `GET /repos/OWNER/REPO/installation` (installation 1 holds each repository), `POST /app/installations/1/access_tokens` (201, a token for one hour) |
| The runners, with the installation token | `POST /repos/OWNER/REPO/actions/runners/generate-jitconfig` (`name`, `labels`; 201 with `runner` and `encoded_jit_config`, the base64 of a JSON map of `.runner`, `.credentials` and `.credentials_rsaparams`, each in base64; 409 for a name in use), `GET .../actions/runners`, `DELETE .../actions/runners/ID`, `GET .../actions/jobs/ID` |
| The broker of each runner | `POST /_apis/oauth2/token` (a client assertion of the made-up key whose `sub` is the client id of a runner), `POST` and `DELETE /session`, `GET /message` (a long poll of 2 s), `POST /acquirejob`, `/renewjob`, `/completejob`, the results service of the logs under `/twirp/`; the routes that the official runner calls with `--jitconfig`, for a job of `run:` steps |
| The webhooks | `workflow_job` with `queued`, `in_progress` (with `runner_id` and `runner_name`) and `completed` (with `conclusion`), in the order of the events, signed with `X-Hub-Signature-256` (HMAC-SHA256 of the webhook secret) |
| The control API | `GET /_fake/app` (the App id, the installation id, the key in PEM, the webhook secret), `PUT /_fake/hook` (`url`, `secret`), `POST /_fake/jobs` (`repository`, `name`, `labels`, `steps`), `GET /_fake/jobs[/ID]`, `POST /_fake/jobs/ID/assign` (`runner_id`), `GET /_fake/runners`, `GET /_fake/deliveries` |

A runner takes the queued job that `assign` gave it, else the oldest
queued job of its repository with each label of the job, as GitHub gives
a job to any idle runner with its labels. Each runner is ephemeral: it
takes one job. The conclusion of the runner (`succeeded`, `failed`,
`cancelled`, `skipped`) becomes the conclusion of GitHub (`success`,
`failure`, `cancelled`, `skipped`). `crates/pod-testkit/tests/github.rs`
runs the binary: two jobs of one repository with the same labels, the
second job on the runner minted for the first, and both conclusions.

`pod_testkit::github::client` is the App side for a test or another fake:
`app_key` (the key of `GET /_fake/app`), `installation_token`, and
`jit_runner` (a runner id and its three files, decoded from
`encoded_jit_config`).

| Consumer | Old path | New path |
| --- | --- | --- |
| githome | no fake GitHub (G-30) | `pod-fake-github` of the release, pinned by its SHA-256 |
| podrunner | `crates/podrunner-broker-fake` (the broker of a JIT runner) | the routes of `pod-fake-github` for a job of `run:` steps |

## The fake of githome's source (feature `githome`)

`pod_testkit::githome::FakeGithome` and its static binary
`pod-fake-githome`: a source of jobs of githome for the CI of podrunner, on
the source wire of `pod_job::source` (podrunner
`docs/contracts/source-driver.md`). Each release attaches
`pod-fake-githome-x86_64-unknown-linux-musl` and
`pod-fake-githome-aarch64-unknown-linux-musl` with their lines in
`SHA256SUMS`. Build it with
`cargo build --release -p pod-testkit --features githome --bin pod-fake-githome`.
The feature `githome` takes the feature `github`.

`pod-fake-githome serve [--listen ADDRESS] [--github URL]` prints its base URL
(`http://127.0.0.1:PORT/`) on the first line of stdout, then holds the
state until it is killed. `pod-fake-githome source [--server URL]` is the
program of a `[[source]]` table of the kind `program`: it reads one
request line on stdin and writes one reply line on stdout. It finds
`serve` by `--server` or by the variable `POD_FAKE_GITHOME`. Each start
of `source` is a new session: one host of podrunner. With no `--github`,
each credential of `mint` is made up (`.runner`, `.credentials` with the
client id `pod-fake-githome-made-up-N`, `.credentials_rsaparams`) and
reaches no broker.

`--github URL` links `serve` to a `pod-fake-github` at the base URL `URL`.
podrunner names that address in its `[egress]`. A unit under an egress
hold has a loopback of its own, so the runner of the unit does not reach
the loopback of the host: when the runner runs in such a unit,
`pod-fake-github` listens on an address of the host that is not the
loopback (PodUtils/podrunner#329):

* `POST /_fake/jobs` also queues the job in the fake GitHub, through its
  control API: the repository of the offer, its labels (`self-hosted` when
  it has none), and the scripts of `steps` (the default `run:` step when
  there are none). `GET /_fake/jobs/ID` names that job as `github_job_id`.
  A job that the fake GitHub refuses is not queued (502).
* `mint` of a claimed job acts as the App: it reads the made-up key at
  `GET /_fake/app` of the fake GitHub, gets an installation token, calls
  `generate-jitconfig` with the runner name of the claim and the labels of
  the job, gives the job to that runner (`POST /_fake/jobs/ID/assign`), and
  answers the three files of the decoded `encoded_jit_config`. The official
  runner of these files takes exactly this job from the broker of the fake
  GitHub. A step that fails is `credential-unavailable`. The record `mints`
  names the runner as `github_runner_id`.
* The report of podrunner is in `GET /_fake/reports` of githome, and the
  conclusion of the runner is in `GET /_fake/jobs/ID` of the fake GitHub.

| Op | The rule |
| --- | --- |
| `hello` | `source` `githome` and `operator_controlled` true (the control API changes both); the host facts are kept |
| `next` | the control events of the session, then up to `capacity` queued jobs that fit the host facts (the arch, KVM, a container) and that the session does not hold or refused; with nothing to give, a wait of `wait_s`, 10 s at most |
| `ack` | a job that the session holds |
| `claim` | a held job that no session claimed: else `already-claimed`, or `unknown-job` for a job that ended |
| `mint` | the claim of the session for this runner name (`not-claimed`); one credential for each claim (`credential-already-issued`); a mint error of the control API, or `source-revoked` |
| `report`, `assigned` | the claim of the session (`not-claimed`); `assigned` gives `keep` with the trust facts of the real job, or `end` `not-of-this-source` for another owner |
| `audit` | each line |
| `withdraw` | a held job that the session did not claim (`already-claimed`); a reason other than `wait-deadline`, `cancelled` or `conformance` keeps the job from the session |
| `release` | a held job; no report, or `exit_reason` `lost`, queues the job again; a `refusal_code` queues it for the other sessions; another report ends it |

A job that the session does not hold is `unknown-job`, and a line that is
not of the wire is `bad-request`. A queued job goes to each session until
one claims it. A run of podrunner's `source-conformance` ends two jobs
(its tests 7 and 15 report and release them): queue three jobs, and two
more for each further run.

| Route of the control API | What |
| --- | --- |
| `POST /_fake/jobs` | the fields of a `job-offer.v1`, each optional, with `repository` (`OWNER/REPO`) and `tier` (`trusted` or `untrusted`), which give the trust inputs, and with `--github` the scripts of `steps`; 201 with the job |
| `GET /_fake/jobs[/ID]` | each job: `status` (`queued`, `claimed`, `reported`, `done`, `cancelled`), `offer`, `claim`, `minted`, `refused_by`, `github_job_id` |
| `POST /_fake/jobs/ID/cancel`, `/started` (`runner_name`), `/completed` (`result`) | the event of the forge to each session that holds the job |
| `POST /_fake/jobs/ID/mint-error` | `code`: the next `mint` of the job fails with it |
| `GET`, `PUT /_fake/settings` | `source`, `operator_controlled`, `unavailable`, `revoked` |
| `GET /_fake/NAME` | the records `reports`, `refusals`, `withdrawals`, `releases`, `audits`, `mints`, `assignments`, `hosts`, in order |

`crates/pod-testkit/tests/githome.rs` runs the binary: the 15 steps of
`source-conformance`, one job from its offer to its report, the
refusals with two hosts, and one job through a linked `pod-fake-github`
from the mint to the conclusion of its runner.

| Consumer | Old path | New path |
| --- | --- | --- |
| podrunner | no source of githome for its CI (PodUtils/podrunner@T-509, G-38) | `pod-fake-githome` of the release, pinned by its SHA-256, with `source-conformance --json` and a job through `serve` |
