# Study note: the archive `ghactions-local` (idea only), for podrunner local mode (Q11)

Input: `ghactions-local-idea-only.zip`, 2250 members, 22.4 MB unpacked. No member is absolute, has `..` or is a symlink.
Nothing in it was built or run. Compared with `docs/research/ci/podrunner-plan.md`, PodUtils/podrunner#1 and its
issues, T-162 (`TODO/podvm.md`) and the row "Q11" of `docs/decisions.md`. Paths below are relative to `ghactions-local/`.

Tags: [O] Observed (read in the archive, or in the upstream git tree where stated). [I] Inferred. [NR] Not read.

## 1. The idea

- Goal: a standalone local GitHub Actions engine. It runs a workflow file on one host with no GitHub. [O] README.md
- The base is `ChristopherHX/runner.server` v3.14.0, commit 069646146c90 (verified upstream), in `vendor/runner.server/`. [O]
- The base is .NET 8: Runner.Client (CLI, act-like `-W wf.yml -P label=image`), Runner.Server (ASP.NET Core emulation of the Actions service), Runner.Listener, Runner.Worker, PluginHost (the official runner code that runs steps). [O] csproj files, README.md
- The base is not act. `vendor/act/` (nektos/act, commit 2540ba6c49df of 2026-11-04, verified upstream) is a "read-only reference for the services port; never built". [O] README.md
- Planned parts are only named: `src/` packages Results, Services, Debugger, Doctor, Pack; `cmd/`; `poc/`. None is in the archive. [O]
- The plan `../FORK-PLAN.md` is not in the archive. [NR]
- Method: the vendored tree is edited in place; each change gets a `PATCHES.md` entry with a reproduction and a retire condition; fixtures are pinned by sha256; three gates `make pins`, `make conformance`, `make reproduce`. [O] AGENTS.md, Makefile
- `docs/diff-from-base.md` says: the only change from the base is the deletion of `copilot-instructions.md`, the CI workflows and `dependabot.yml`; each later hunk maps to a `PATCHES.md` entry. [O]
- The real diff (git blob hashes against the upstream trees): runner.server 1409 of 1414 files identical, 2 files differ only by CRLF to LF, 363 upstream files absent (docs/, images/, testworkflows/, .vscode/, Agents.db, copilot-instructions.md). [O]
- act: 411 of 413 files identical; `canary-link` (an upstream symlink) is an empty file; 52 files absent (.github/, docs/img, AGENTS.md, lint configs, two testdata secrets files). [O]
- Neither tree has a code patch. `PATCHES.md` has one entry: "Results Service / artifacts v4 (planned, no code yet)". [O]

Experiments:

- `10-probe-host.sh`: checks git, docker, curl, a dotnet 8 SDK, the images `catthehacker/ubuntu:act-latest`, `node:20-alpine`, `redis:7-alpine`; prints the `max file size` limit; checks the build output and `.base-commit`. It measures nothing. [O]
- `20-build-base.sh`: times `make build-base` and checks four output files. [O]
- `experiments/README.md` records (Debian 13 sandbox, dotnet 8.0.427): warm build 16 to 33 s; conformance w1 to w5 PASS, w6 FAIL (403 on the action fetch through an allowlist proxy), w7 FAIL (put on the missing Results Service). [O]
- It also records: the Listener needs Worker, PluginHost and `Misc/layoutbin/*.sh.template` beside it, else jobs hang at "Running job". [O]
- No log of these runs is in the archive. [O]

Reproductions (`testdata/reproductions/`, exit 0 = the defect is present):

- `01-dotnet-sigxfsz.sh`: with W^X on (the .NET default), the runtime's double mapper makes a memfd and calls `ftruncate` to 2 TiB. Under an `RLIMIT_FSIZE` below that (20 GiB on the test host) the kernel sends SIGXFSZ; dotnet dies at start with exit 153. [O]
- Recorded fix: `DOTNET_EnableWriteXorExecute=0` (the Makefile and `env.sh` export it). [O] An equal fix: no fsize limit, or a limit of 2 TiB or more. [I]
- `05-results-service-econnreset.sh`: runs w7 (`upload-artifact@v4`, then `download-artifact@v4`) in host mode (`-P ubuntu-latest=-self-hosted`) against a server on :5000. [O]
- Its evidence: the survey log `##[error]Unable to make request: ECONNRESET`; the in-tree gate saw `403 (Forbidden)`. Recorded cause: Runner.Server lacks the Results Service routes. Recorded fix: patch 1 (planned). [O] The cause is wrong; see finding 2.
- `07-localci-rerun-eacces.sh`: a third-party tool (redwoodjs/local-ci) fails on its second run. The runner container wrote files as uid 1001; the host process (uid 1000) cannot remove them (`unlinkat ... EACCES`). [O]
- Recorded fix: none in this tree. The record keeps the class (container uid against host uid at clean-up) as a guard for a planned "Runner.Services". [O]

## 2. The conformance suite

- Seven pinned workflows (`vendor/pinned/conformance/.github/`): w1 `run:` and exit codes; w2 env, `contains`, `startsWith`, `fromJSON('[1,2,3]')[1]`, job `outputs` and `needs`; w3 matrix 2x2 with one `exclude` and one `include` (4 legs), `max-parallel: 2`. [O]
- w4 a `redis:7-alpine` service with a health check, reached on `127.0.0.1:6379` or the name `redis`; w5 a job container `node:20-alpine`, and a job container with a service; w6 `actions/checkout@v5`, `actions/setup-node@v4`, a local composite action with an output; w7 artifacts v4 upload and download. [O]
- Runner contract: `<bin> -W <wf> -P ubuntu-latest=catthehacker/ubuntu:act-latest [args]`, in a fresh git repository made from the fixture. Each workflow has a 500 s limit. [O] `testdata/conformance/conformance.sh`
- Pass rule: runner exit 0 and the marker of the workflow in the log give PASS; deeper greps can lower it to PARTIAL; else FAIL. The gate exits 0 only at 7/7 PASS. [O]
- `testdata/pins.sh` checks 9 fixture files by sha256 (CR removed) against `PINS.md`; it never fetches. It does not pin images, actions, the vendored trees or the SDK. [O]
- Versions: runner.server v3.14.0 (but `releaseVersion` 3.9.2, `src/runnerversion` 3.11.16); act 2540ba6c49df (`VERSION` 0.2.91, not bumped upstream); Go 1.26.10; .NET SDK 8.0.x `rollForward: latestMinor`, measured 8.0.427. [O]
- Images by moving tags (`act-latest`, `20-alpine`, `7-alpine`); actions by major tags (`checkout@v5`, `setup-node@v4`, `upload-artifact@v4`, `download-artifact@v4`). [O]

## 3. Fit to podrunner and the PodUtils crates

Local mode (Q11; PodUtils/podrunner#10 still lists it as a v1 non-goal with the trigger "an ask names it"):

- The archive's shape (a local stand-in of the Actions service, plus the real Runner.Worker) is the shape of PodUtils/podrunner#38 (canned fake broker, level 2). #38 already names this seven-workflow contract. [O]
- Therefore local mode can be: a `local` job source (seam of #16) that compiles a workflow file into job messages, served by the #38 broker to the official runner in a podvm unit (M-B, #26). [I]
- The compiler is `pod-actions-workflow` plus `pod-actions-expr` (plan section 6, phase 5). This gives local mode on x64 and arm64 with no `pod-actions-exec`. [I]
- `pod-actions-exec` and `pod-actions-proto` stay the path for targets with no official runner (#10 "alternate payloads"; plan phase 5). [I]
- Local mode and #38 need a results service (artifacts v4, cache). Behaviour references: runner.server `ArtifactControllerV2.cs` (twirp ArtifactService, 7 methods) and `CacheControllerV2.cs`; act `pkg/artifacts/artifacts_v4.go` and `pkg/artifactcache`. [O]
- Home of that service: a new podcrates crate (name to decide, e.g. `pod-actions-results`) or a module of `pod-actions-exec`; the GitHub fakes stay in podrunner (#38). [I]
- w2 maps to `pod-actions-expr`; w3 to `pod-actions-workflow`; w6 to action download and an offline mirror (#10 "action mirror"; runner.server reads `ActionDownloadUrls` from `appsettings.json`). [I]
- The seven workflows become the first rows of the pass/fail table of phase 5 and of local mode, after the fixes of section 5. [I]

The official-runner path (M-A to M-D):

- SIGXFSZ agrees with #9 U2 ("settled: yes, with default W^X"), #23 (`start.env` sets W^X off, never an fsize limit), #26 (M-B, U13 cost of W^X off) and PodUtils/podvm#231 (N3). [O]
- `01-dotnet-sigxfsz.sh` is a ready reproduction for #26 and for test 4 of T-162 ("no `RLIMIT_FSIZE` by default"). [I]
- T-162: any fsize limit below 2 TiB also kills each .NET program that a job step starts (`dotnet build`), unless the W^X variable reaches the step. A disk bound must use another means (a store size check), not `RLIMIT_FSIZE`. [I]
- The ECONNRESET and 403 of the results calls: for M-D (#33) the egress allowlist must hold `results-receiver.actions.githubusercontent.com` and `*.blob.core.windows.net` (#24, T-164). A reset by the allowlist proxy is a likely cause of the survey's ECONNRESET. [I]
- uid clean-up (07): fits #23 (per-unit store, destroy), #21 (destroy and clean), the principal axis of #1, PodUtils/podvm#234 (N6) and #240 (N12). [I]
- On podvm's userland rung all files have the host uid; the class comes back with a container kind (#10 `docker`) or a microVM with shared directories (#37). [I]
- The archive's planned "Doctor" is `podrunner doctor` (#22) with `pod-hostprobe` (T-107): probe `RLIMIT_FSIZE`, W^X need, images, egress. [I]

podvm and podnet:

- podvm: the job container of w5, the service of w4 (health state), and `catthehacker/ubuntu` as a rootfs (`pod-oci`, `pod-rootfs`); one rootfs for each container (PodUtils/podvm#234). With no Docker daemon, w4 and w5 need `pod-dockerapi` (Q10, phase 3) or podvm units. [I]
- podnet: egress for the action fetch and the results upload; Node ignores `HTTPS_PROXY`, so `podnet pod` or `pod-hold` (PodUtils/podvm#228, sections 2.8 and 2.9). [I]

## 4. Licence, static build, risks

- act: MIT ("Copyright (c) 2019", no holder named). Inside it: `pkg/container/DOCKER_LICENSE` Apache-2.0; `pkg/lookpath/LICENSE` BSD-3-Clause (Go Authors); Gitea Authors MIT headers in `pkg/artifacts/*.go` and `pkg/common/auth.go`. [O]
- runner.server: MIT (Christopher Homberger 2021-2025, GitHub 2019). Bundled web and VS Code extension assets carry their own licences; one npm dependency is "MIT OR GPL-3.0-or-later". [O]
- A port of code into a 0BSD crate must keep the MIT and BSD notices, and for Apache-2.0 also NOTICE and change marks. A new implementation from behaviour and the wire needs no notice. [I]
- The archive's own files (README, scripts, fixture) carry no licence. Record the owner's grant (0BSD) before a copy into a PodUtils repository. [O]
- Static: runner.server is published framework-dependent (no `-r`, not self-contained); it needs a .NET 8 runtime, ICU or invariant mode, OpenSSL, and native libraries of EF Core Sqlite and Mono.Posix. It is not static. [O]
- NativeAOT of runner.server is unlikely (reflection, ASP.NET MVC, EF Core). It fits only as a payload in a unit (plan Q2). [I]
- act: Go, `CGO_ENABLED=0` in `.goreleaser.yml`, no `import "C"`: a static build is likely. It needs a Docker Engine API for containers; its host mode (`-self-hosted`) has no isolation. [O]/[I]
- Risk: .NET 8 support ended on 2026-11-10 (Microsoft lifecycle; from memory, verify). runner.server targets net8.0. [I]
- Risk: with no OIDC settings, `DevModeOrAuthenticatedUser` lets each request pass, and `AllowedHosts` is `*`. A server on :5000 that the network can reach runs any job it gets. Bind to loopback only. [O]/[I]
- Risk: one upstream maintainer; a large tree (25 MB of source; ASP.NET, EF Core, Quartz). [O]
- Risk: not offline. The action fetch uses `api.github.com` tarballs (`appsettings.json`). [O]

## 5. Findings

1. README.md, AGENTS.md: `../FORK-PLAN.md`, `src/`, `cmd/`, `poc/` are absent. The idea has no design text past the README. Ask the owner for FORK-PLAN.md if it exists.
2. `vendor/runner.server/PATCHES.md` entry 1: its premise is false for this base. `src/Runner.Server/Controllers/ArtifactControllerV2.cs` serves all 7 twirp ArtifactService routes; `MessageController.cs` sets `ResultsServiceUrl`; `Runner.Server.Tests/ArtfactsTests.cs` tests them; upstream `testworkflows/actions_artifacts_v4/` (deleted in the lean cut) exercises them. [O]
3. Same: w7 uses two remote actions, so the w6 cause (403 on the action fetch) also stops w7. The route is not the cause. [I]
4. `testdata/reproductions/05-results-service-econnreset.sh`: any nonzero exit and the INCONCLUSIVE branch give exit 0 ("reproduced"). A missing server or a blocked fetch counts as the defect; the retire signal is unsound.
5. `docs/diff-from-base.md`, `VENDORED.md`: the listed deletions do not match the tree (`.github/workflows/` and `dependabot.yml` are present; docs/, testworkflows/, images/, .vscode/ are absent; 2 files changed CRLF to LF). A byte diff shows more than "those deletions".
6. `vendor/pinned/PINS.md` calls act "vendored-and-patched, edited in place"; README.md calls it "read-only reference; never built". Its base is the branch `main`, not a tag.
7. `vendor/runner.server`: the version files disagree (tag v3.14.0, `releaseVersion` 3.9.2, `src/runnerversion` 3.11.16).
8. w6 (`vendor/pinned/conformance/.github/workflows/w6-actions.yml`): `test -f app.js` and `node app.js` (marker `app-ok`) need `app.js`; the fixture has none. w6 cannot pass even with network.
9. `conformance.sh`, `check.sh`: markers can match the echoed script, not the output. w7 `artifact-data` is in the text of step 1; w5 `alpine` is in the image name of the pull log; the w5 combo step always exits 0 and has `combo-ok` in its text; w4 has `localhost-ok` and `redis-name-ok` in its text.
10. Fix for 9: print markers that only the run computes (for example `echo "m-$((6*7))"`) and match output lines only.
11. `conformance.sh:64`: `docker rm -f $(docker ps -aq)` removes each container of the host (#38 says so too).
12. Conformance gaps: no check of the composite output `greeting`, the `fromJSON` value, `github.actor`, env `GLOBAL`, matrix `extra`, `max-parallel`.
13. Absent features: reusable workflows, `actions/cache`, secrets and masks, `GITHUB_ENV`, `GITHUB_PATH`, `GITHUB_STEP_SUMMARY`, post steps, Docker and Dockerfile actions, `continue-on-error`, `timeout-minutes`, `concurrency`, `defaults`, `hashFiles`, `workflow_dispatch` inputs.
14. Conformance pins: images by moving tags, actions by major tags. A result can change with no change of the tree. Pin images by digest and actions by commit.
15. `conformance.sh:48` counts `'| matrix os='`, a log prefix of one runner; `check.sh` counts `matrix os=`, which also counts the echoed command. The count needs an adapter for each runner.
16. README.md says "a phase is done when all three gates pass"; `experiments/README.md` accepts Phase 0 with w6 and w7 FAIL, so `make conformance` exits 1 at acceptance.
17. `Makefile` target `conformance`: it does not start the server on :5000 that it needs (its comment says it does); it calls `dotnet` from PATH, while `build-base` uses `$(DOTNET)`.
18. `experiments/10-probe-host.sh`: `fail=1` for a missing image overwrites `fail=2` for a missing tool. With no docker the exit is 1 ("ran and failed"), not 2.
19. `testdata/reproductions/run-all.sh`: exit 1 means "retire the patch", but 01 and 07 have no `PATCHES.md` entry, and 07 is a third-party defect. Numbers 02 to 04 and 06 are absent. [NR] why.
20. `01-dotnet-sigxfsz.sh`: the grep for "File size limit exceeded" reads the child's log; bash writes that text to its own stderr, so only the exit-153 test works. [I]
21. README.md "no absolute paths": `experiments/README.md:46` and the pinned `check.sh:5` hold absolute paths of the author's machine; `10-probe-host.sh` names `/tmp/di.sh`; `env.sh` names the absent `poc/`.
22. `07-localci-rerun-eacces.sh`: builds from `ghcr.io/actions/actions-runner:latest` (not pinned) and sets `safe.directory "*"` (turns off git's owner check).
23. Open (for PodUtils/podrunner#26 and #23): does `DOTNET_EnableWriteXorExecute=0` in `start.env` reach each job step? If a limit is ever set, .NET steps die. Keep T-162 with no fsize limit.
24. Open (for PodUtils/podrunner#10): Q11 is now the ask that the row "a local mode" waits for. Move the row into the plan.
25. Open (for the owner or podrunner): the engine of local mode. Recommendation: #38 broker plus the official runner first (high fidelity, no new executor), with `pod-actions-workflow` and `pod-actions-expr`; `pod-actions-exec` later. The archive measures neither act nor a native engine.
