# Plan: run GitHub CI (and other CI) on any machine

Status: research result, not a ruling. Nothing here changes a ruling of the owner.
Date of the research: 2026-11-10. GitHub issue: the tracker of this plan.

## 1. The question and the short answer

The owner asks for the tool that runs GitHub (and other) CI on any machine.

The short answer:

- The tool is a new project, `podrunner`. The owner already reserved it (A-X-01 of GitHub #35, G-01 to G-12 of #36). Its repository is empty.
- `podrunner` is a thin, static Rust launcher. It asks GitHub for a JIT runner config, starts one fresh unit for each job, and runs the official `actions/runner` once inside the unit. It reports and destroys the unit.
- The unit is a podvm payload. podvm gives the unit on hosts that deny root, namespaces, mounts and ptrace. podnet and pod-proxy give the way out. pod-guest/podvm machine tier gives a microVM for untrusted jobs.
- The one missing piece that blocks `container:`, `services:` and Docker actions is a Docker-compatible socket on top of podvm. Proposed name: `pod-dockerapi` (a crate in podcrates, hosted by `podrunner`). With it, the official runner, GitLab Runner (docker executor), the Gitea/Forgejo runner, nektos/act and testcontainers all work on a host with no docker daemon.
- A native Rust runner (a new set of crates `pod-actions-*`) is a later, conditional phase. Build it only if the measurements of phases 0 to 3 show that the .NET runner cannot run on a target host (32-bit, riscv64, loongarch64, ppc64le, Windows) or that its fidelity or size is not acceptable.

Where it fits the ruling "Priority" (2026-11-09): phases 0 to 2 are mostly work in podvm and podnet (their own records, already routed: A-V-04, A-V-06, A-V-09, A-V-11). That work is first priority. The new project and the new crates wait for the owner to lift the wait. See section 9.

## 2. What a CI runner needs from the host (facts)

### 2.1 GitHub Actions, official runner (`actions/runner`)

Sources are listed in section 11. Facts marked "verify" come from memory or one secondary source, and need a measurement.

- License MIT. Language C# (.NET). It ships a listener (`Runner.Listener`) and a worker (`Runner.Worker`), and it downloads Node runtimes (node20, node24) for JavaScript actions. Platforms: Linux x64, arm, arm64; macOS; Windows. It is not a static binary: .NET needs glibc (or a musl build), ICU (or `DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1`) and OpenSSL. Whether a musl build is shipped: unknown.
- Network: outbound TCP 443 only, to `github.com`, `api.github.com`, `*.actions.githubusercontent.com`, `codeload.github.com`, `results-receiver.actions.githubusercontent.com`, `*.blob.core.windows.net`, `ghcr.io` and others (GitHub docs, "Self-hosted runners reference"). No inbound port.
- Registration has two forms:
  - Registration token (`POST /repos/{o}/{r}/actions/runners/registration-token`, or `/orgs/{org}/...`; the token expires after 1 hour). The runner makes an RSA key pair, sends the public key, and gets a `clientId`.
  - JIT config (`POST /repos/{o}/{r}/actions/runners/generate-jitconfig`, or `/orgs/...`; body `name`, `runner_group_id`, `labels` (1 to 100), optional `work_folder`; answer 201 with `encoded_jit_config`). The caller needs admin on the repository or the organisation. A JIT runner is ephemeral: it takes one job.
- The JIT config holds `AgentId`, `AgentName`, `PoolId`, `ServerUrl`, `ServerUrlV2` (`https://broker.actions.githubusercontent.com/`), `GitHubUrl`, `WorkFolder`, `Ephemeral`, `DisableUpdate`, `UseV2Flow`, an RSA private key and an authorization URL (Depot article). Verify: the blob is a base64 map of the files `.runner`, `.credentials` and `.credentials_rsaparams`.
- Auth: the runner signs a JWT with its RSA key and sends it to the authorization URL. The answer is an OAuth token (Bearer).
- Broker flow (the V2 flow):
  - `POST {ServerUrlV2}/sessions` with `sessionId`, `ownerName` and `agent {id, name, version, osDescription, ephemeral, status, provisioningState}`. An old `version` gets 400. The answer has `sessionId`.
  - `GET {ServerUrlV2}/message?sessionId=..&status=Online&runnerVersion=..&os=..&architecture=..&disableUpdate=..` is a long poll of up to 50 s. No job gives 202 and an empty body.
  - A job message has type `RunnerJobRequest`. Its body holds `runner_request_id`, `run_service_url` and `billing_owner_id`.
  - Within about 2 minutes: `POST {run_service_url}/acquirejob` (`jobMessageId`, `runnerOS`, `billingOwnerId`). The answer is the full job message (steps, contexts, secrets, endpoints, tokens). The `planId` is in `x-plan-id`.
  - About each minute: `POST {run_service_url}/renewjob` (`planId`, `jobId`); the answer has `lockedUntil` (about 10 minutes ahead).
  - Complete job, log upload, step status, annotations, summaries, artifacts and cache: not covered by the sources I could read. Verify by capture (section 10).
- The old (V1) flow used the Azure DevOps `/sessions` and message queue with RSA-encrypted job messages (auth.md of the runner repository). The V1 flow is being replaced. The runner can need an update within 30 days of a release, or GitHub stops queuing jobs to it (`--disableupdate` does not remove that limit).
- Step types in the job message:
  - `run:` steps: a shell script on the host or in the job container. Shell choice: `bash`, `sh`, `pwsh`, `python`, `cmd` and others.
  - JavaScript actions: `node20` or `node24` runtime, started by the runner with its own downloaded Node. A pre, main and post step.
  - Composite actions: a list of steps with their own contexts.
  - Docker container actions: `docker build` or `docker pull`, then `docker create` and `docker start` with the workspace mounted.
  - `container:` jobs and `services:`: the runner calls the `docker` CLI: `network create`, `create`, `start`, `exec`, `inspect` (for the health state), `logs`, `rm`, `network rm`. GitHub states that these need a Linux machine with Docker installed.
  - Expressions (`${{ }}`), `if:` conditions, matrix, `needs` and the YAML parse are mostly done by GitHub before the job message. Verify the exact split; the runner still evaluates step expressions and conditions.
- Cache and artifact: `actions/cache` and `actions/upload-artifact` (v4) talk to GitHub's results service with `ACTIONS_RESULTS_URL` and `ACTIONS_RUNTIME_TOKEN` from the job message. They need no part of the runner except these variables. Verify.

### 2.2 Alternative runners and CI systems

| Project | License | Language | What it gives | Static build |
| --- | --- | --- | --- | --- |
| `ChristopherHX/github-act-runner` | MIT | Go (1.24) | Same protocol as `actions/runner`, built on a fork of act. Works on more OS (FreeBSD). Needs `node` on PATH and a Docker daemon (`DOCKER_HOST`) for containers and services. Gaps: `actions/cache` cannot save, no annotations or problem matchers, weaker secret masking | Not stated; Go with `CGO_ENABLED=0` normally gives a static binary (verify) |
| `nektos/act` | MIT | Go | Runs a workflow locally through the Docker API. Docker is required | Same as above |
| `actions/scaleset` | MIT | Go | The official client for custom autoscalers: scale-set sessions, long poll, `GenerateJitRunnerConfig`. Public preview. It starts the official runner as a process, container or VM | Same as above |
| Gitea `act_runner` | MIT (verify) | Go | Runner of Gitea Actions (act based) | Same |
| Forgejo `runner` | GPL-3.0-or-later | Go | Runner of Forgejo; Docker, Podman, LXC | A GPL program may run as a separate process payload; do not link or copy it into 0BSD crates |
| GitLab Runner | MIT | Go | Executors: shell, docker, custom, kubernetes, instance, docker-autoscaler. The custom executor runs four scripts: `config_exec`, `prepare_exec`, `run_exec`, `cleanup_exec` | Same |
| Woodpecker agent | Apache-2.0 (verify) | Go | gRPC to the server; Docker or kubernetes backends | Same |

Facts about `github-act-runner` and act come from their README pages. The statement that Go builds static: general knowledge, verify with `file` on a build.

### 2.3 What a runner host needs, in short

1. Outbound HTTPS to the forge and to the blob and results hosts, by a proxy when the host has no direct way out.
2. A writable work directory with enough space.
3. A glibc or musl userland with `bash`, `git`, `tar`, `curl`, and Node for JavaScript actions.
4. A container engine for `container:`, `services:` and Docker actions.
5. Often root (`sudo apt-get install` in a workflow) and a real network (ports of services on `localhost`).
6. A fresh state for each job, for isolation.

## 3. Obstacles on the PodUtils target hosts and the piece that removes each

| Obstacle | Piece | State | Gap |
| --- | --- | --- | --- |
| No docker daemon, no root, no user namespace, no mount, no chroot | podvm chroot, namespace, userland (LD_PRELOAD interposer), supervise and machine tiers; `pod-enter`, `pod-supervise`, `pod-extract`, `pod-rootfs`, `pod-oci`, `pod-hostprobe` | Taken by podvm up to `pod-oci`; `pod-extract` and `pod-rootfs` wait for the take | A-V-04: a glibc Node job on a chroot-denied host is open (measured failures on beta.12; raw `statx` of libuv passes the interposer, T-1697) |
| Runner credential must not be readable from a step | podvm `run --secret src=,target=,mode=0400,once` | Filed as podvm T-1667-area entry (cli.md), P2; verb is refused today | G-03, A-V-11. The interposer is not a security boundary; a raw syscall can bypass the read-once rule |
| `container:` and `services:` need `docker network`, `exec`, `-v`, labels, health | podvm parity table: `network`, `volume`, `-v`, `--network`, `-p` are `None`; `exec` is `Degraded` (re-entry, not a running container); `--label` is a stub; no daemon | Open | A Docker Engine API subset over podvm. See `pod-dockerapi` (section 5) |
| Service ports (`redis:6379`) | The chroot tier shares the host network namespace, so a port is a host port | Works for one job on one host | Port clash between slots. Use the machine tier (microVM) for a job with services, or a loopback address for each slot (`127.x.y.z`; verify that the host allows it) |
| Proxy-only egress | podnet and `pod-proxy` (HTTP CONNECT, SOCKS5, `NO_PROXY`), `podnet pod` (seccomp backend that sends all traffic of a program to a proxy, no root), `pod-broker` (egress policy, audit) | `pod-broker` T-099 done; TLS end T-100 open | Node's `fetch` and many tools ignore `HTTPS_PROXY`; use `podnet pod` or `pod-broker` as the only way out (A-V-02, T-109) |
| No inbound port | The runner is outbound only (long poll over 443). podssh/`pod-relay` give reach to the host for an operator | Exists | none |
| Process supervision, hard timeout, destroy in 60 s | `pod-supervise` (launcher, pidfd, lock) | Taken by podvm | A-V-07: a run timeout that kills the tree, peak RSS and CPU (open) |
| A full VM for an untrusted fork PR; Windows or DOS guest | `pod-guest`, podvm machine tier (QEMU, KVM or TCG), A-V-08 `pod-microvm` (M7) | `pod-guest` taken; microVM from an OCI image planned (T-062) | KVM is free (2026-11-09); a Windows guest needs an operator image |
| Images for jobs (`ubuntu`, `node`, a golden image) | `pod-oci`, `pod-extract`, `pod-rootfs` | Exists | A golden "runner image" with the runner, git, Node, `sudo` shim |
| Credentials and policy of a job | `pod-broker` (job token, deny list, audit, placeholders after T-100) | T-099 done | T-100 (TLS end with a CA for each job) |
| Ephemeral hosts, 32-bit and non-x86 hosts | podnet, podssh and podvm already ship static binaries for x86_64, aarch64, armv7, riscv64, i686, loongarch64, powerpc64le | Exists | The official runner runs on x64, arm, arm64 only. For other targets only a native runner works (phase 5) |
| Windows hosts | podssh has Windows builds; podnet is Linux only | Partial | Out of scope for phases 0 to 4. A Windows job runs in a Windows guest (`pod-guest`) |

`pod-broker` is an egress broker, not a credential vault. It holds a job policy and a token for one job and writes an audit line. It swaps placeholder credentials only after T-100.

## 4. The design choice

### Option A: the official runner inside a podvm unit, started by a thin static launcher (recommended first)

- Fidelity: full, because it is the real runner. Every action, every shell, cache, artifact, summary, annotation and OIDC works as on GitHub.
- Effort: S to M for `run:` and JavaScript steps. The container features need `pod-dockerapi` (L).
- Maintenance: low. The runner updates itself or the image is rebuilt each month (30-day rule).
- Static rule: `podrunner`, `podvm` and `pod-dockerapi` are static. The runner is a payload in an OCI image, as Node is. This is a reading of the rule that the owner must confirm (Q2).
- Cost: needs glibc and the .NET runtime in the image; runs only on x64, arm and arm64.

### Option B: a native static Rust runner that speaks the runner protocol

- Fidelity: lower and always behind. The Go project github-act-runner shows the cost: no cache save, no annotations, weaker masking after years of work. The protocol has no public spec; GitHub changes it (V1 to broker, version checks).
- Effort: L to XL (about 15 to 25 kLoC): the protocol client, the job message model, the expression engine and the contexts, the file commands (`GITHUB_ENV`, `GITHUB_OUTPUT`, `GITHUB_PATH`, `GITHUB_STATE`, `GITHUB_STEP_SUMMARY`), the commands in output (`::add-mask::` and others), action download and the composite and post-step logic, the log and result upload, Node and Docker actions.
- Gain: static on all targets, no .NET, no Node for `run:` steps, one process model, full control of the container backend (no `docker` CLI), and a possible local `run workflow` mode.
- Risk: pretends to be a runner of a given version (the `version` field of the session). This is a terms and breakage risk (section 8).

### Option C: only a launcher, with no change of the isolation (the ARC and scaleset model)

This is Option A without podvm: the launcher starts the runner in a container with a real docker daemon. It does not work on the target hosts. It is the right shape for the launcher on a host that has docker, and `podrunner` keeps it as one backend (`unit = "docker"`).

### Option D: vendor `github-act-runner` (MIT, Go) and patch it to use podvm

- Effort: M. Gain: a runner that is static and runs on many targets sooner than a Rust rewrite.
- Cost: a second language in the tree; Go code to maintain; known fidelity gaps (cache save, masking); the Docker Go SDK needs a daemon API, so it also needs `pod-dockerapi`. A fork would live in `vendor/` of `podrunner` with its patches (rule C12). MIT needs its notice in `THIRD_PARTY`.
- Decision: keep it as the fallback for phase 5 if the Rust rewrite is not approved.

### Recommendation

Do A now, with `pod-dockerapi` as the key addition, and keep B and D as a conditional phase 5 that the measurements decide. Reasons:

1. Option A is what the owner already routed (A-X-01 "run `actions/runner` once in a fresh unit", A-V-04 "an `actions/runner` image answers `./config.sh --help`").
2. `pod-dockerapi` is not a cost of A alone. It also serves GitLab Runner, the Gitea/Forgejo runners, nektos/act, Woodpecker, testcontainers and plain `docker` users, so it is the real "other CI" answer. It makes the choice of runner a matter of taste.
3. Fidelity first. A native runner can come after the owner sees where the official one fails.
4. The `pod-broker` TLS end (T-100) can record the real runner protocol. That gives the spec that Option B needs, as a by-product.

Rejected:

- Option B first: no spec, high and continuing cost, terms risk. Rejected as a start; kept as phase 5.
- Option C alone: needs a docker daemon; does not serve the target hosts.
- Writing a docker daemon replacement inside podvm itself: podvm decided "no daemon". A per-job socket served by `podrunner` is not a host daemon. See Q3.
- Switching to Incus or Firecracker as the unit (the interim of cfb): needs root or a KVM group, not the target hosts. Kept only as a backend for hosts that have them.

## 5. The pieces

Naming: `pod-` names are reserved on crates.io by `check-crates-io-names.py`. Each new crate needs a row in `docs/design.md` section 4 and in `README.md`, `check-design-paths.py` and the name check. The owner allows crates for the new projects when a consumer needs them ("Priority" ruling).

| # | Piece | New or extension | Where | What | Depends on | Size | Proof |
| --- | --- | --- | --- | --- | --- | --- | --- |
| P1 | Measure the official runner under podvm | Extension of podvm | podvm record: A-V-04 (T-1643 to T-1649) | `podvm run` of an image with `actions/runner`: `config.sh --help`, then `run.sh --jitconfig` on a host that denies chroot and namespaces; glibc, ICU invariant mode, `statx` fix (T-1697) | podvm | M | Live: a JIT runner runs a `run: echo` job of a test repository, exit 0 |
| P2 | `--secret ...,once` | Extension of podvm | podvm `run/parse.rs`, `parity.rs`, `pod-enter` path map | The JIT files at a target path, mode 0400, read once, no argv, no env | podvm T-1653 | M | `experiments/427-secret-flag.sh`; tests `secret_*` (already written in the entry) |
| P3 | `-v HOST:GUEST` through the path map, `:ro` | Extension of podvm | podvm, `pod-enter` `Names`/path map | Workspace and cache directories in a unit | podvm | M | A-V-06 test: write through `-v`, `:ro` gives `EROFS` |
| P4 | Run report and labels | Extension of podvm | podvm `run`, `ps` | `--report r.json`, `--label`, `ps --filter label=` | pod-supervise | M | A-V-09 `jq .boundary` |
| P5 | Limits and kill tree | Extension of podvm and `pod-supervise` | `pod-supervise` (T-060 family) | `--ulimit`, a timeout that kills the whole tree, peak RSS and CPU | pod-supervise | M | A-V-07 test |
| P6 | `pod-dockerapi` | New crate | podcrates `crates/pod-dockerapi` | Docker Engine API subset on a Unix socket, one socket for each job: `_ping`, `version`, `info`, `images/create` (pull through `pod-oci`), `containers/create|start|wait|logs|inspect|json|kill|delete|exec`, `exec/start`, `networks/create|delete|connect` (a name table, no real network; host aliases through `--add-host`), `volumes`, the labels, health state from the `HEALTHCHECK` of the image. A back end trait with a podvm back end (`podvm run` as a child) and a machine-tier back end | `pod-core`, `pod-supervise`, `pod-oci`, `pod-hostprobe`; podvm CLI or crates | L | Run the real `docker` CLI (static, Apache-2.0) against it: `docker run --rm alpine true`, `exec`, `network create`, `inspect` health; then a `container:` job and a `services: redis` job of the official runner |
| P7 | Egress hold for a job | Extension of existing | `pod-broker` T-100 (TLS end), podvm A-V-02 (T-109), `podnet pod` | All job traffic goes to the broker; the CA of the job in `SSL_CERT_FILE`, `NODE_EXTRA_CA_CERTS`, `GIT_SSL_CAINFO` | `pod-broker`, `pod-tls` | L (T-100) | T-100 tests; live run with a proxy-only host |
| P8 | `podrunner` | New project (own repository, empty today) | `PodUtils/podrunner` | The launcher: config (`[[source]]`, slots, labels, `unit` kind), the JIT mint over `pod-proxy`/`pod-tls`, the slot manager, the unit start, the report, `status --json`, `drain`. Job-source trait (GitHub JIT is one; a forge is another, G-01). Thin `main`; hosts `pod-broker` and `pod-dockerapi` (rule C27) | P1 to P7, `pod-control` (T-103), `pod-unit` (T-102) | L | Acceptance of A-X-01: 20 jobs, fresh unit, kill gives destroy in 60 s, burst of 10, cache hit, credential not readable |
| P9 | Fake job source | Extension of `pod-testkit` | `pod-testkit` stand-ins (T-106) | A stand-in GitHub (`generate-jitconfig`, broker `sessions`, `message`, `acquirejob`, `renewjob`) and a stand-in forge, on loopback | `pod-testkit` | M | G-01 acceptance offline |
| P10 | `pod-unit`, `pod-control`, `pod-release` | New crates, already planned | M8: T-102, T-103, T-104 | Service unit, control API, signed releases of `podrunner` | `pod-core` | M each | their entries |
| P11 | Golden images | Extension of `images/` of podrunner | `podrunner image build` | Runner, git, Node, `sudo` shim, tool cache, pinned by digest, with SBOM and signature | `pod-oci`, T-104 | M | G-11 |
| P12 | GitLab Runner back end | Extension of `podrunner` | custom executor scripts | `config_exec`, `prepare_exec`, `run_exec`, `cleanup_exec` call `podvm run` or the P6 socket | P6, P8 | S | A GitLab job (shell and docker executor) of a test project |
| P13 | Forgejo/Gitea back end | Extension of `podrunner` (source kind) | job-source trait | Run the Gitea `act_runner` (MIT) or the Forgejo runner (GPL, separate process only) in the unit against P6 | P6, P8 | S to M | A Forgejo test instance job (live workflow) |
| P14 | Native runner | New crates, conditional | `pod-actions-proto`, `pod-actions-expr`, `pod-actions-exec` (names proposed) | See section 6 phase 5 | `pod-core`, `pod-proxy`, `pod-tls`, `pod-supervise`, P6 or direct container back end | XL | Same acceptance tests as A, against the same test repository |

Notes:

- P1 to P5 are tasks of the podvm record. Podcrates does not edit podvm. Podcrates ports the generic parts into crates (rule: `pod-enter`, `pod-supervise`) when podvm hands them over.
- P6: the crate is a library; the socket and CLI live in the host (rule C27). It must not claim a feature that podvm does not give. Each missing feature answers with a Docker error and a named reason, as podvm's parity table does.
- P6 semantics to decide: a "container" is a podvm record; `exec` is a fresh re-entry (podvm `exec` is `Degraded`: it shares only the filesystem, not the processes). A service container that a later `exec` must see needs the same root directory. This works for the chroot tier. In the machine tier the exec goes through the guest agent.
- P6 network model: job containers and service containers share the host network (chroot tier). The name of a service resolves to a loopback address of the slot. Parallel slots need distinct loopback addresses or the machine tier. Measure.

## 6. Phases

### Phase 0: measure (no new code in podcrates)

Goal: know what the official runner does under podvm on a hostile host.

- [ ] Capture the runner protocol: run the official runner against a test repository through `pod-broker` with a TLS end (needs T-100) or through a CA-injecting proxy on a machine the owner controls. Record `sessions`, `message`, `acquirejob`, `renewjob`, the completion calls, the log and result upload, and the results (cache, artifact) hosts. Store the redacted capture in `docs/research/ci/`.
- [ ] Read `actions/runner` sources: `src/Runner.Listener` (`Runner.cs`, `BrokerMessageListener`, JIT config parse), `src/Sdk/DTWebApi`, `src/Runner.Worker` (`ContainerOperationProvider`, `ActionRunner`, `FileCommandManager`). Write the exact JIT blob format and the env names (`ACTIONS_RUNNER_INPUT_JITCONFIG`).
- [ ] Measure: the runner image under `podvm run` with the chroot, namespace, userland tiers (podvm A-V-04). Record each failure with its entry and refusal.
- [ ] Measure: ICU invariant mode, `DOTNET_*` variables, the self-contained runner on glibc and on musl; does a musl build of the runner run (Alpine) so that a static podvm payload with a musl image works.
- [ ] Measure: JavaScript actions (`actions/checkout`, `actions/setup-node`, `actions/cache`, `actions/upload-artifact`) under the proxy-only host; Node ignores `HTTPS_PROXY` (test `NODE_USE_ENV_PROXY`, `--use-env-proxy` of new Node, or `podnet pod`).
- [ ] Measure: where the job reads `.credentials` of the runner (G-03) and what `once` can protect.
- [ ] Measure: the 30-day update rule (`--disableupdate`) and the `version` field check of the session.
- [ ] Read the GitHub terms (section 8) and ask the owner Q5.

### Phase 1: `run:` steps and JavaScript actions on a host with no root

Goal: a GitHub job with `run:` and JS actions runs on a no-root host, one job at a time.

- [ ] podvm: A-V-04 (Node job), P2 (`--secret once`), P3 (`-v`), P4 (report, labels), P5 (limits) in the podvm record.
- [ ] `pod-supervise` and `pod-enter` fixes that come from the measurements (as ports from podvm).
- [ ] Create `PodUtils/podrunner` skeleton (needs the owner to lift the wait): static musl, `podrunner serve --config`, `status --json`, `drain`.
- [ ] JIT mint: `generate-jitconfig` through `pod-proxy` and `pod-tls`; credential file with mode 0600; never in argv or logs.
- [ ] Slot manager: N slots, label match, hard timeout, a fresh unit for each job, destroy and clean.
- [ ] Unit backend `podvm`: `podvm run --rm --secret ... IMAGE ./run.sh`.
- [ ] Unit backend `docker` (hosts with docker) and `host` (no isolation, for a dedicated machine).
- [ ] Report for each job (JSON, schema in `schemas/`).
- [ ] Live test in `live.yml` style on GitHub: a test repository, 20 jobs, each in a fresh unit.
- [ ] Release `podrunner` as a beta (betas are allowed; no crates.io).

### Phase 2: egress hold, caches, policy

- [ ] `pod-broker` T-100 (TLS end, job CA, placeholders, scrub).
- [ ] `podrunner` hosts `pod-broker` and gives each job a token and a policy (open with fixed denies, or allow-list).
- [ ] Caches for each trust level (G-07): toolchain, npm, pnpm; a fork PR gets a read-only cache.
- [ ] `podnet` or `pod-proxy` for the runner host's own egress through a proxy; test on a proxy-only host.
- [ ] Control API and unit file: T-102 (`pod-unit`), T-103 (`pod-control`).
- [ ] Signed release and index: T-104.

### Phase 3: `container:`, `services:` and Docker actions (`pod-dockerapi`)

- [ ] Study entry: list the docker CLI calls of the runner (`ContainerOperationProvider`) and of GitLab Runner, act_runner and testcontainers. Record each as a verb, a flag and the podvm answer.
- [ ] Crate `pod-dockerapi`: the Engine API subset, a back end trait, tests with a stand-in client (no docker needed), a static probe line, a fuzz target for the request parser.
- [ ] podvm back end: create, start, exec, logs, wait, inspect, labels, health, kill, remove.
- [ ] Network table: names to loopback addresses; `--add-host`; a refusal with a named reason when a job needs a real network.
- [ ] Machine-tier back end (microVM) for a job with `services:` and for untrusted jobs (A-V-08, T-062).
- [ ] Static `docker` CLI in the golden image; `DOCKER_HOST` points to the socket of the job; no host socket.
- [ ] Live test: a `container:` job, a `services: redis` job, a `docker://` action, a `Dockerfile` action (needs `build`; podvm `build` is `None`: decide to refuse by name or to add a rootless build step; question Q6).
- [ ] G-09 acceptance.

### Phase 4: other forges and CI systems

- [ ] Job-source trait with the fake forge (P9, G-01, G-10).
- [ ] GitLab Runner custom executor scripts (P12).
- [ ] Forgejo and Gitea runner back end (P13); license note for the GPL runner (separate process only).
- [ ] githome: the same `podrunner` source kind for its Actions-compatible API (G-01 to G-12).
- [ ] microVM unit kind with no fallback to a weaker kind (G-02); placement by architecture (G-05).

### Phase 5 (conditional): a native Rust runner

Start only if the measurements of phases 0 to 4 give a reason: the .NET runner cannot run on a required target (armv7 is supported; i686, riscv64, loongarch64, ppc64le, Windows are not), or it is too big for an ephemeral host, or fidelity of the official runner under podvm is not acceptable, or the owner wants a local `run workflow` mode.

- [ ] Study entry with the capture of phase 0 as the spec; write `docs/research/ci/protocol.md` with test vectors.
- [ ] `pod-actions-proto`: wire types, RS256 JWT, session, long poll, acquire, renew, complete, log and result upload, version string. Settings, not environment. Typed failures (rules 6, 9).
- [ ] `pod-actions-expr`: the expression language, the functions (`contains`, `format`, `hashFiles`, `success`, `failure`, `always`, `cancelled`, `toJSON`, `fromJSON`) and the contexts. Fuzz target.
- [ ] `pod-actions-exec`: steps, shells, file commands, masks, composite actions, action download, Node and Docker actions, post steps, summaries, annotations. Container back end is the `pod-dockerapi` trait, called in process, with no docker CLI.
- [ ] `podrunner` backend `native`: a unit with the native runner, static on each podnet target.
- [ ] Acceptance: the same test repository and the same 20-job test as phase 1; a conformance list of GitHub features with a pass or fail table (as `docs/research/podnet/parity-checklist.md` does).
- [ ] Optional: `pod-actions-workflow` (YAML, matrix, `needs`) for a local `podrunner run .github/workflows/x.yml` without GitHub (what `act` does).
- [ ] Alternative if Rust is not approved: fork `github-act-runner` into `vendor/` with its patches (rule C12), built static, with `pod-dockerapi`.

## 7. Related asks of cfb (#35) and githome (#36)

`docs/research/fleet/asks.md` is the routing page.

| Ask | Relation to this plan | Where in the plan |
| --- | --- | --- |
| A-X-01 (P0) `podrunner` | This plan is its design | P8, phases 1 to 3 |
| A-V-04 (P1) Node CI job from an OCI image with no root | Blocks phase 1; acceptance names the `actions/runner` image | P1, phase 0 and 1 |
| A-V-11 (P2) `--secret` | The JIT credential reaches only the runner (G-03) | P2 |
| A-V-06 (P1) `-v` and `:ro` | Workspace and caches | P3 |
| A-V-07 (P1) limits, kill tree, RSS, CPU | Hard timeout and G-12 | P5 |
| A-V-09 (P1) report and labels | `podrunner status`, G-06 | P4 |
| A-V-08 (P1) microVM from an OCI image, KVM | Unit kind `microvm`, G-02 | phase 3, 4; T-062 |
| A-V-01 (P0) confine layer | Hardening of a unit (Landlock, seccomp) | T-108, phase 2 |
| A-V-02 (P0), A-V-03, A-X-05, A-N-13, A-S-11 | Egress hold and policy | P7, T-099 done, T-100 |
| A-X-15 `pod-control`, A-X-13 `pod-release`, A-X-14 acceptance registry, A-X-18 test doubles | Control API, signed release, acceptance, fake GitHub | P10, P9 (T-103, T-104, T-105, T-106) |
| A-S-02, T-102 `pod-unit` | systemd unit of `podrunner` with no token in it | P10 |
| A-X-02 `podnode` | May supervise `podrunner` as a role unit; later | phase 4 |
| A-X-22 CI matrix of restricted hosts (T-119) | The host matrix is the proof set for phase 1 | phase 1 |
| G-01 job-source contract | Source trait, `[[source]]` table | P8, P9, phase 4 |
| G-02 unit kind, no fallback | Backend per label | phase 4 |
| G-03 credential reaches only the runner | `--secret once`; limit stated | P2 |
| G-04 job policy | `pod-broker` | P7 |
| G-05 placement by architecture | Label and host arch | phase 4 |
| G-06 JSON report | Report | P4, P8 |
| G-07 caches for each trust level | Cache directories | phase 2 |
| G-08 capacity signal, drain | `podrunner drain`, `pod-control` | P8, P10 |
| G-09 service containers, container jobs, Docker actions | `pod-dockerapi` | P6, phase 3 |
| G-10 fake forge source | `pod-testkit` stand-in | P9 |
| G-11 warm units by digest | Golden images | P11 |
| G-12 limits and 60 s end | Limits | P5 |

The tunnel set of cfb (A-N-01 to A-N-03, A-C-01, A-C-02) is not part of this path. It waits for the first crates.io release.

## 8. Risks

1. GitHub terms. The documents I read say that self-hosted runners are allowed and that the runner software is free. They say nothing about a third-party implementation of the protocol (unknown). The protocol has no public spec; the session sends a runner `version`, and a version that is too old gets 400. A native runner must send a version string. That can be read as pretending to be the official runner. Ask the owner to read the terms (Q5). The official runner in a unit has no such risk.
2. Self-hosted runners and public repositories. A fork PR runs code of a stranger. Rules: no secrets and a read-only token for a fork PR (githome rule 3), a microVM for it, an ephemeral unit for each job, no shared cache between trust levels. The interposer of the chroot tier is not a security boundary (podvm limits.md). Do not run public-fork jobs in the chroot tier.
3. The runner credential is readable by steps that run as the same user in the same unit, unless `once` and a raw-syscall bypass are handled. A JIT credential is for one job only; it limits the damage to that job.
4. Credentials: `generate-jitconfig` needs admin on a repository or organisation. The minting token stays on the host and never enters a unit (A-X-01). Use a GitHub App with the minimal permission when possible. List test tokens in `.github/secrets-allow.txt`. No token in argv, URL, logs (`--jitconfig` on the command line shows in `/proc/*/cmdline`; use files or the environment of a process that scrubs it; measure).
5. Protocol change: the broker flow and the auth migration changed in 2025. A change breaks the native runner at once. The official runner in a unit follows by an image rebuild. The 30-day update rule needs a monthly image build in CI.
6. Ephemeral JIT with the broker: a reported 5 to 10 % of jobs fail when the runner cancels the long poll after the job (actions/runner issue 4309). Check the state, and retry in `podrunner`.
7. Fidelity: workflows use `sudo`, `apt-get`, the GitHub-hosted tool cache, Docker on `localhost`. A podvm unit has no real root. Provide a golden image with shims; list what fails (`podrunner doctor`).
8. Docker API shim: each unimplemented verb must fail by name, not silently. A "copy that pretends to be a mount" is against the rules of podvm.
9. Size and static rule: a Rust native runner is a large new surface; the .NET runner is not static (Q2).
10. Resource: a unit with Node and a cache can take 20 GB on `target`-like disks; set quotas, clean after each job.
11. Windows and macOS jobs: out of scope. The Windows guest needs an operator image and its licence.
12. The Forgejo runner is GPL-3.0: run it as a separate process only.

## 9. Fit to the owner's rulings and questions for the owner

Rulings that apply:

- "Priority" (2026-11-09): podvm, podnet and podssh first; M8 and the asks of cfb and githome wait; podcrates can build a crate for a new project when a consumer needs it. Therefore:
  - Phase 0 and the podvm items P1 to P5 are podvm work. They fit the priority and can start now (also A-V-04 is a podvm task today).
  - T-099 is done. T-100, T-102 to T-106 are M8 and wait, unless a consumer needs one. `podrunner` (P8) would be that consumer; a ruling is needed.
  - `pod-dockerapi` (P6) is a new crate for a new project. It waits for the ruling in Q1.
- "New projects" (2026-11-09): `podrunner` has its own repository, created by the owner. It stays empty and has no session now.
- C27: podcrates gives libraries; commands live in the product.
- KVM is free (2026-11-09); the cfb tunnel set waits for the first crates.io release; no crates.io release now.
- Rule 1 of `docs/design.md`: each crate links static on each target. `pod-dockerapi` and `pod-actions-*` are pure Rust.

Questions (each with a recommendation; none changes a ruling):

- **Q1.** Lift the wait for `podrunner` and for the crates `pod-dockerapi`, T-100, T-102, T-103, T-104, T-106 so that this path starts after the podvm items P1 to P5? Recommendation: yes for `podrunner` phase 1 and `pod-dockerapi`; the rest in the order of need. If no: do phase 0 and P1 to P5 only (they are podvm tasks).
- **Q2.** Is the official .NET runner, run as a payload inside a podvm unit, accepted under "everything must be statically compiled"? Recommendation: yes. The delivered binaries (`podrunner`, `podvm`) are static; the payload is user content, like Node. If no: go to phase 5 at once (option B or D).
- **Q3.** May `podrunner` serve a Docker Engine API socket for a job (a per-job socket, no host daemon) although podvm has the rule "no daemon"? Recommendation: yes; it is a library of podcrates hosted by the product, lives for one job, and refuses unsupported verbs by name.
- **Q4.** Is a local mode (`podrunner run workflow.yml`, as `act`) in scope? Recommendation: not now. It needs a workflow compiler and the native engine (phase 5).
- **Q5.** Read the GitHub terms for third-party runner implementations and the version field before phase 5. Recommendation: do the official runner first; decide Q5 before any code of phase 5.
- **Q6.** `docker build` and Dockerfile actions: refuse by name, or add a rootless build step? Recommendation: refuse by name at first, then measure the use in the target repositories.
- **Q7.** Name of the new crates: `pod-dockerapi`, `pod-actions-proto|expr|exec`. Recommendation: accept; reserve the names on crates.io with the existing check.

## 10. Unknowns that need a measurement

1. The exact JIT blob format and how the runner reads it from a file or the environment (not argv).
2. The completion, log, step-result, annotation and summary calls of the runner; the result service for cache and artifact; how much of it a native runner must write.
3. Which expressions and conditions the runner evaluates and which GitHub evaluates.
4. Whether the .NET runner runs under podvm tiers on a chroot-denied host (glibc loader, ICU, `statx` of libuv, memfd size, `fexecve`).
5. Whether a musl .NET runner exists and runs.
6. Whether Node actions honour the proxy (`NODE_USE_ENV_PROXY`) or need `podnet pod`.
7. Whether loopback `127.x.y.z` for each slot is allowed on target hosts (service ports).
8. Whether `docker exec` can be served by a fresh chroot re-entry for the steps of `container:` jobs (the runner uses `docker exec` for each step).
9. The health check state of `services:` (`docker inspect` fields) and how long a job waits.
10. The effect of the 30-day update rule on a pinned image.
11. Whether Go `CGO_ENABLED=0` builds of `github-act-runner`, `gitlab-runner` and the docker CLI are static on each target.
12. The cost of an ephemeral unit: start time, disk, memory with a warm image.

## 11. Sources

- Self-hosted runners reference (hosts, ports, OS, ephemeral, JIT, update rule): https://docs.github.com/en/actions/reference/runners/self-hosted-runners
- About self-hosted runners: https://docs.github.com/en/actions/hosting-your-own-runners/managing-self-hosted-runners/about-self-hosted-runners
- REST API: generate-jitconfig and registration-token: https://docs.github.com/en/rest/actions/self-hosted-runners
- Runner authentication design (V1: key pair, OAuth, long poll, encrypted job message): https://github.com/actions/runner/blob/main/docs/design/auth.md
- Runner repository (MIT): https://github.com/actions/runner
- Broker flow (`sessions`, `message`, `acquirejob`, `renewjob`, JIT fields): https://depot.dev/blog/github-actions-runner-architecture-part-1-the-listener
- Ephemeral JIT long-poll report: https://github.com/actions/runner/issues/4309 ; graceful drain proposal: https://github.com/actions/runner/pull/4461
- Official scale set client (MIT, Go, public preview): https://github.com/actions/scaleset
- github-act-runner (MIT, Go): https://github.com/ChristopherHX/github-act-runner
- nektos/act (MIT, Go): https://github.com/nektos/act
- Forgejo runner (GPL-3.0-or-later, Go): https://code.forgejo.org/forgejo/runner
- GitLab Runner (MIT, Go; executors and custom executor stages): https://gitlab.com/gitlab-org/gitlab-runner
- In this repository: `docs/research/fleet/asks.md`, `TODO/fleet.md`, `docs/decisions.md` (rows "Priority", "New projects"), `docs/design.md` (sections 1, 4). In podvm: `PodUtils/podvm:crates/podvm/src/parity.rs`, `PodUtils/podvm:docs/limits.md`, `PodUtils/podvm:TODO/cli.md` (the `--secret` entry). GitHub PodUtils/podcrates#35 (A-X-01, A-V-04, A-V-08, A-V-09, A-V-11) and #36 (G-01 to G-12).

Not read: the GitHub terms of service text, the Gitea `act_runner` and Woodpecker licenses (marked "verify"), the source of `actions/runner`, and the podssh docs beyond the README.
