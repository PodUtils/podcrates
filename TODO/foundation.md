This file holds the entries of milestone M0: the workspace, the record, the
checks, the workflows, `pod-core`, `pod-testkit` and the first release.
`docs/design.md` gives the rules of each crate and the procedure for one
crate.

# T-001: `pod-todo`: one checker for the records of podssh, podnet and podcrates

**Source:** `docs/design.md`, section 4 (M0); `docs/decisions.md`, call C5.
**Category:** chore
**Milestone:** M0
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podssh and podnet each keep a copy of the checker of the record
(`PodUtils/podssh:crates/podssh-todo`, `PodUtils/podnet:crates/podnet-todo`).
The copies differ only in the facts of their repository. podcrates needs
the same checker for its own record.

## Premise

- Read: `PodUtils/podssh:crates/podssh-todo` at `d4e186d` (3,612 lines of
  source, 1,416 of tests, no dependency) is the base; podnet's copy differs
  in the milestones, the roadmap path, the top directories, the root
  files, the documents, the readings at a time, the heading of the
  questions and the discovery of the packages of a Prove.
- Read: the `interop` command of podssh reads podssh's interop harnesses.

## Approach

1. Import the crate with no change, without `src/interop/` and
   `tests/interop.rs`.
2. Edition 2024 in its own commit.
3. Read the facts of a repository from `TODO/todo.toml`; ids of another
   record as `REPO@T-NNN`; `pod_todo::main()` for the member crate of a
   consumer.

## Prove

```sh
cargo test -p pod-todo
cargo clippy -p pod-todo --all-targets -- -D warnings
cargo todo check
```

## Done

2026-11-05. Commits `012ae9f` (import), `360cf7f` (edition 2024),
`9ff04cf` (the configuration). `cargo test -p pod-todo`: 111 passed before
this record existed; with it, the three tests that read the record of this
repository pass too (114). `tests/config.rs` (9 tests) covers each key of
`TODO/todo.toml`, a wrong file, a foreign id and a reading at a time.
`cargo clippy -p pod-todo --all-targets -- -D warnings`: clean.

# T-002: The record, the design and the procedures of podcrates

**Source:** `docs/decisions.md` (the rulings of 2026-11-05).
**Category:** docs
**Milestone:** M0
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The plan of podcrates was one document. A session needs the record of the
work, the rules of the crates, the procedures and the rulings in their
own files, as podssh and podnet keep them.

## Premise

- Read: the record model of podnet (`PodUtils/podnet:TODO/RULES.md`) and of
  podssh; the plan of 2026-11-05, reviewed by three studies against the
  code.

## Approach

1. `TODO/`: `RULES.md`, `ROADMAP.md`, `INDEX.md`, `PROGRESS.md`,
   `issues.md`, `github.txt`, `todo.toml`, and one area file for each
   milestone, with an entry for each step of the plan.
3. Remove the plan: its content is in `docs/design.md`,
   `docs/decisions.md` and the record.
2. `docs/design.md` (the rules, the crates, the traits, the targets, the
   procedure, the checks, the releases), `docs/maintenance.md`,
   `docs/development.md`; the calls in `docs/decisions.md`.

## Prove

```sh
cargo todo check
cargo test -p pod-todo --test repository --test github
```

## Done

2026-11-05, in the commit of this record. `cargo todo check`: the record
agrees. `cargo test -p pod-todo`: 114 passed.

# T-003: The checks of each push, each with its plant

**Source:** `docs/design.md`, section 9.
**Category:** chore
**Milestone:** M0
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

The rules of `docs/design.md` (the size of a file, the record, the
sources of each crate, the pins of the workflows) need checks that run on
each push. A check is trusted only after it fails on a planted defect.

## Premise

- Read: podnet `PodUtils/podnet:scripts/check-size.py` (with an allowlist),
  `PodUtils/podnet:scripts/check-workflows.py`,
  `PodUtils/podnet:scripts/agent/check.sh`; podssh
  `PodUtils/podssh:scripts/plant.sh` and `PodUtils/podssh:scripts/plant-each.sh`
  with `PodUtils/podssh:scripts/plants/`.
- The size limit is 500 lines with no allowlist (`docs/decisions.md`,
  File size).

## Approach

1. `scripts/agent/check.sh`: fmt, clippy, tests, `cargo todo check`, the
   size check, the static probe of x86_64 musl (after T-004).
2. `scripts/check-size.py`: each `.rs` file of the tree but vendor/
   has 500 lines or fewer; a floor of files read.
3. `scripts/check-sources.py`: each `SOURCES` file against the `main` of
   its repository: changed, new, removed files, with their commits.
4. `scripts/check-workflows.py`: each action pinned by its commit, the
   permissions of each job.
5. `scripts/plant-each.sh`, and one plant for each check in
   `scripts/plants/`.

## Prove

```sh
sh scripts/agent/check.sh
sh scripts/plant-each.sh
```

Each plant makes its check fail; with no plant, each check passes.

## Start condition

T-001 is done.

## Done

2026-11-05, commit of this entry. `sh scripts/agent/check.sh`: fmt 0,
clippy 0, record 0, size 0, test 0 (the static probe waits for T-004).
`sh scripts/plant-each.sh`: 6 caught, 0 missed or stale
(`clippy-len-zero`, `fmt-spaces`, `record-milestone`, `size-long-file`,
`size-self-test`, `sources-change`). `scripts/check-sources.py` reads
the mirror and a clone; `crates/pod-todo/SOURCES` has 22 files and no
change. The plant of `scripts/check-workflows.py` comes with the first
workflow (T-007), and the plant of the numbers of GitHub with the first
issue: each needs a file that does not exist now.

# T-004: The static probe on each target

**Source:** `docs/design.md`, section 7; the owner, 2026-11-05: "the only
rule is: everything must be statically compiled".
**Category:** chore
**Milestone:** M0
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

Each crate must link into a static binary on each target of the
consumers. Only a link on each target shows it.

## Premise

- Read: podnet `PodUtils/podnet:scripts/check-static.sh` (readelf: no
  `NEEDED`, no interpreter); the zig build of podnet and podssh
  (zig 0.16.0, cargo-zigbuild 0.23.4).
- Read: podssh measured `libSystem`, `libiconv` and `CoreFoundation` in
  its macOS binaries.

## Approach

1. A binary in examples/ that depends on each crate with each feature,
   and calls one function of each.
2. `scripts/check-static.sh BINARY...`: readelf on Linux, `otool -L` on
   macOS, `dumpbin /dependents` on Windows, each against its list of the
   libraries of the system.
3. A plant: a probe that links a library dynamically fails the check.

## Prove

```sh
cargo zigbuild --release --target x86_64-unknown-linux-musl -p static-probe
sh scripts/check-static.sh target/x86_64-unknown-linux-musl/release/static-probe
```

## Start condition

T-005 is done.

## Done

2026-11-05, commit of this entry. `examples/static-probe` links
`pod-core` (each new crate adds itself there). `scripts/check-static.sh`
reads ELF (readelf), Mach-O (otool, with the libraries of the system) and
PE (objdump or dumpbin, with the DLLs of the C runtime refused);
`.cargo/config.toml` links the C runtime into each Windows binary. The
probe for `x86_64-unknown-linux-musl` from cargo-zigbuild (zig 0.16.0):
`static: target/x86_64-unknown-linux-musl/release/static-probe (389824
bytes)`. The plant `static-dynamic` (the probe for the glibc target of the
host) is caught: `dynamic dependencies: libgcc_s.so.1, libc.so.6`. The
build on each target in CI is the work of T-007.

# T-005: `pod-core`: the traits, the errors, the events and the helpers that each crate shares

**Source:** `docs/design.md`, sections 3 and 6; `docs/decisions.md`, call
C17.
**Category:** feature
**Milestone:** M0
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

The agreed crates depend on each other in loops without a bottom crate
(`docs/design.md`, section 3). Each crate needs one `Dial`, one
`Resolve`, one `Datagrams`, the shared errors, the events and the
observer.

## Premise

- Read: podssh-ws `PodUtils/podssh:crates/podssh-ws/src/text.rs`,
  `clock.rs`, `wire.rs`, `names.rs`; the `backoff` of
  `PodUtils/podssh:crates/podssh-relay/src/open.rs`; the private files of
  `PodUtils/podssh:crates/podssh-relay/src/cache.rs`.
- Read: podnet `PodUtils/podnet:src/exit.rs` (`Fault`), `src/hop.rs`
  (`Hop`), `src/decisions.rs`, the `Metered` of `src/edge_h2.rs`, the
  `RelayBudget` of `src/ws_relay.rs`.
- The design of the traits is in podnet
  `PodUtils/podnet:docs/research/podnet/architecture.md`, section 4.3.

## Approach

1. Import the helper files of the sources with no change.
2. Write the traits and the types of `docs/design.md`, section 6, and the
   failure type with its stage and its class.
3. Tests: the tests of the imported helpers; a test of each trait with a
   loopback implementation.

## Prove

```sh
cargo test -p pod-core
sh scripts/agent/check.sh
```

## Start condition

T-001 is done.

## Done

2026-11-05, commit of this entry. `crates/pod-core`: `dial` (`Target`,
`Dial`, `Resolve` with its sources, `Connected`, `BoxedIo`), `datagrams`
(`Datagrams` in the shape of the UDP socket of quinn and noq,
`SystemDatagrams`), `failure` (`Failure`, `Stage`, `Class`), `event`
(`Event`, `Events`, `Observer`, `Report`), `metered`, `backoff`, and the
imported `clock` (with `utc_minutes`), `text`, `names`.
`cargo test -p pod-core`: 24 unit tests and 4 tests of the traits with
loopback implementations pass. The design now says which parts come with
which later crate (`docs/design.md`, section 3), and that `wire.rs` goes
to `pod-ws`.

# T-006: `pod-testkit`: the guards of a test host

**Source:** `docs/design.md`, section 4 (M0); `docs/decisions.md`, call C9.
**Category:** chore
**Milestone:** M0
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The tests that move to podcrates bind loopback sockets. A host can refuse
`bind(AF_INET)` or have no loopback; the tests then skip with a reason
(podnet `need_loopback!`). podssh's tests of SSH need a test server.

## Premise

- Read: podnet `PodUtils/podnet:tests/test_host/mod.rs`; podssh
  `PodUtils/podssh:crates/podssh-cli/tests/ssh_harness/` (russh 0.65, 20
  files, 2,054 lines).

## Approach

1. `need_loopback!` and `need_localhost!` as exported macros.
2. A stand-in takes no type of a pod crate, so that a crate can
   dev-depend on the kit.

## Decision

The SSH test server of podssh is T-041. Most of its 2,054 lines answer
podssh's own commands (sudo, containers, health, X11, the podssh binary);
no crate of M1 to M4 needs it. It moves when a crate or a consumer needs
an SSH server in its tests.

## Prove

```sh
cargo test -p pod-testkit --all-features
```

## Start condition

T-001 is done.

## Done

2026-11-05, commit `3ff514f`. `crates/pod-testkit` has
`need_loopback!` and `need_localhost!` (exported macros) and the probes
of `host`; `cargo test -p pod-testkit`: 3 passed. `pod-core` uses the
guards in its tests.

# T-007: The workflows of podcrates

**Source:** `docs/design.md`, section 9.
**Category:** chore
**Milestone:** M0
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

The checks must run on GitHub on each push, each night and each day, as
`docs/design.md` gives them.

## Premise

- Read: podnet `PodUtils/podnet:.github/workflows/ci.yml`, `deny.yml`,
  `secrets.yml`, `record-issues.yml`, `upstream.yml`, `release.yml`;
  podssh `PodUtils/podssh:.github/workflows/plants.yml`.

## Approach

1. `ci.yml`: fmt, clippy, tests on Linux x86_64, `cargo todo check`, the
   size check, the static probe on each target.
2. `nightly.yml`: the tests on macOS, Windows and the other Linux targets
   (QEMU).
3. `deny.yml`, `secrets.yml`, `plants.yml`, `record-issues.yml`,
   `upstream.yml`, `release.yml` (a dry run with no input).
4. .github/dependabot.yml.

## Prove

```sh
python3 -I scripts/check-workflows.py
```

The runs of the push are green on GitHub (`scripts/agent/watch-runs.sh`).

## Start condition

T-003 and T-004 are done.

## Done

2026-11-05, commit `9c337f4` (the workflows), with the repairs `371b447`
(the static check of Windows reads `NAME.exe` first), `6633ddf`
(`retry.sh` tries 5 times) and `345c227` (the QEMU handler of arm64 is
`qemu-aarch64`). `python3 -I scripts/check-workflows.py` passes. The runs
at `345c227`: ci 42026381365 (lint, test, record, the static probe on 12
targets), deny 42026381387, secrets 42026381328, plants 42026381369,
nightly 42026387870 (the tests on macOS, Windows and 7 Linux targets
under QEMU). The runs at `1ec8c15` and `6633ddf`: upstream 42025312191,
record-issues 42025317405, the dry run of release 42026164788.

# T-008: Adoption trials in CI, and the trial of `pod-todo` in podssh and podnet

**Source:** `docs/design.md`, section 8, step 7; `docs/decisions.md`,
call C15.
**Category:** chore
**Milestone:** M0
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

A crate is ready for a consumer only when the consumer's own tests pass
with the crate in the place of its module. A checkout of a consumer and
its build do not fit the disk of a session.

## Premise

- Read: podssh and podnet keep their checker as a member crate, with the
  alias `cargo todo` (`.cargo/config.toml` of each).

## Approach

1. `adoption.yml`: check out the consumer at the commit of
   adoption/CONSUMER/commit, apply adoption/CONSUMER/pod-X.patch,
   point the git source of podcrates to the checkout of the push, run the
   tests that the patch names.
2. The first trial: a member crate of each consumer whose `main` calls
   `pod_todo::main()`, its `TODO/todo.toml`, and `cargo todo check` on
   the record of the consumer.

## Prove

```sh
gh workflow run adoption.yml -f crate=pod-todo
```

The run checks the records of podssh and podnet with `pod-todo`, with no
problem.

## Start condition

T-007 is done.

## Done

2026-11-05, commit `dc31b4a` (adoption.yml, scripts/adoption.sh, the
trials of `pod-todo`), with the trials of `pod-term` (`2ad40ad`) and
`pod-identity` (`371b447`). In podnet at `b7682c5`, `cargo todo check`
with `pod-todo` agrees with the record (137 entries, no problem). In
podssh at `d4e186d`, the record agrees (640 entries) and the 11 tests of
its interop pass. The runs of `adoption.yml` on a push: 42022006523
(`2ad40ad`) and 42023409486 (`1ec8c15`), each trial green.

# T-009: The first release

**Source:** `docs/design.md`, section 10.
**Category:** release
**Milestone:** M0
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The consumers take the crates by a git tag of podcrates. M0 ends with the
first tag.

## Premise

- The release is unattended when the checks are green
  (`docs/decisions.md`, Delivery).

## Approach

1. docs/releases/v0.1.0.md: the crates and how a consumer takes them.
2. `release.yml` with the version as its input: it checks that `main` is
   green and tags.

## Prove

```sh
gh workflow run release.yml -f version=0.1.0
```

## Start condition

T-005, T-006, T-007 and T-008 are done.

## Done

2026-11-05, the tag `v0.1.0` at `57a731e`, from the run 42027912474 of
`release.yml` (the dry run 42026164788 before it), with the ci run
42027429221 green at that commit. The release
<https://github.com/PodUtils/podcrates/releases/tag/v0.1.0> has the notes
of `docs/releases/v0.1.0.md`: `pod-todo`, `pod-core`, `pod-testkit`,
`pod-term`, `pod-identity`, and the trials of `adoption/`.

# T-042: `publish.yml`: the publish to crates.io, apart from the release

**Source:** the owner, 2026-11-05 (`docs/decisions.md`, Publish workflow); GitHub #1.
**Category:** chore
**Milestone:** M0
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The owner chose "publish.yml everywhere": "A dedicated publish workflow,
separate from the existing release.yml. The agents must create
publish.yml." The owner set by the API the environment `crates-io` (only
a tag `v*` deploys to it), the ruleset "release tags" and the names of
the crates on crates.io (0.0.0 placeholders, repository
`PodUtils/podcrates`).

## Premise

- Read: the environments and the rulesets of podcrates, podnet and
  podssh through the API mirror: each has the environment `crates-io`
  with the branch policy `v*` (a tag); none has a `publish.yml` yet.
- Read: crates.io has 0.0.0 of `pod-todo`, `pod-core`, `pod-testkit`,
  `pod-term`, `pod-identity`, `pod-tls`, `pod-proxy`, `pod-dns`, `pod-ws`
  and the names of M2 to M4, except `pod-quic`, `pod-piping`,
  `pod-opentunnel`, `pod-egress`, `pod-iroh`, `pod-tor`, `pod-ts`, which
  are free.
- Not read: the trusted publishers on crates.io (that needs the owner's
  login).

## Approach

1. `.github/workflows/publish.yml`, as the spec of the owner's setup
   gives it: a tag push `v*` and a dispatch start it; `permissions: {}`
   at the top; the job `dry-run` (`cargo publish --workspace --dry-run`,
   the switch, the version of the tag, the green `ci` run); the job
   `publish` (`environment: crates-io`, `id-token: write`, `contents:
   read`, `rust-lang/crates-io-auth-action` pinned by its commit).
2. The switch `crates-io` of `[workspace.metadata.podcrates]` in
   `Cargo.toml`, false while Q1 stands.
3. `release.yml` dispatches it on each new tag. The library crates lose
   `publish = false`.

## Decision

The owner chose the trigger (a tag push and a dispatch) and the gate (the
switch in `Cargo.toml`). A real version waits for the ruling of Q4; the
dry run of each release keeps the packaging ready (`docs/decisions.md`,
Publish trigger, Publish gate, C19).

## Prove

```sh
python3 -I scripts/check-workflows.py
gh workflow run publish.yml --ref main
```

The dry run passes on `main`.

## Done

2026-11-05, commits `9540df9` and `256c09e` (`publish.yml` as the spec of
the owner's setup gives it, the switch `crates-io = false`, the library
crates publishable). `python3 -I scripts/check-workflows.py`: 10 files, 0
faults. The dry run on `main`, run 42032844195 at `256c09e`: the job
`dry-run` packed and built the 6 crates, and the job `publish` was
skipped (the switch is false). The ci run 42033640639 at `18eb667` is
green.

# T-043: The names of the crates on crates.io and their trusted publishers (GitHub #1)

**Source:** GitHub #1.
**Category:** chore
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

GitHub #1 (the owner, 2026-11-05), "crates.io: reserved pod-* names and
trusted publishing": "Every `pod-*` name of `docs/design.md` section 4 is
reserved on crates.io as a `0.0.0` placeholder owned by the owner. This
issue tracks the reservations; it is updated as each name is published
(crates.io allows about one new crate every 10 minutes). **Trusted
publishing** (set on each crate once it exists): repository
`PodUtils/podcrates`, workflow `.github/workflows/publish.yml`, environment
`crates-io` (deployable from `v*` tags only). Token publishing stays
allowed until each crate's first release from CI; then the owner turns on
"Require trusted publishing". This does not change the Q1 ruling:
podcrates publishes nothing to crates.io until the owner says so. The
placeholders only hold the names." The table of the issue: 22 of 34
reserved, 12 queued.

## Premise

- Measured: `python3 -I scripts/check-crates-io-names.py` reads the 28
  crates and the 8 held names of `docs/design.md`, section 4, and finds
  24 of 36 on crates.io (the 22 of the issue, `podcrates` and
  `podutils`); 12 are missing, as the issue says.
- Not measured: the trusted publisher of each crate (it needs the owner's
  login on crates.io).

## Approach

The owner reserves each name and sets its trusted publisher. This
repository keeps `publish.yml` as the trusted publisher wants it (T-042)
and checks the names with `scripts/check-crates-io-names.py`. A new crate
of `docs/design.md` gets its name from the owner before its first publish.

## Prove

```sh
python3 -I scripts/check-crates-io-names.py
```

Each name is on crates.io with the repository of podcrates.

## Done

2026-11-06. `python3 -I scripts/check-crates-io-names.py`: 37 names, 0
faults: each crate and held name of `docs/design.md`, section 4,
`pod-services` among them, is on crates.io (0.0.0) with the repository
of podcrates (`podutils`: PodUtils). The trusted publisher of each crate
is the owner's to set and is not read here (it needs the owner's login).

# T-046: The shared CI scripts: the zig fetch, its mirrors and their weekly check

**Source:** the owner, 2026-11-05 (`PodUtils/podnet:docs/decisions.md`, Shared CI scripts; PodUtils/podnet@T-133).
**Category:** chore
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The owner's ruling: "A CI script that podnet and podssh share (the zig
fetch with its mirrors, PodUtils/podnet@T-133) lives in `PodUtils/podcrates`. podnet calls
it from there; it does not copy it." podssh has the same script and a
weekly check of its mirror list (`PodUtils/podssh:scripts/check-release-tools.py`).

## Premise

- Read: `scripts/fetch-zig.sh` and `scripts/zig-mirrors.txt` of podcrates;
  podssh's at `6ece2ff` differ in comments, in wget for curl, and in the
  `source` of the request; the mirror lists are the same.
- Read: `PodUtils/podssh:scripts/check-release-tools.py` (the mirrors part) and its
  weekly job in `upstream.yml`.
- podnet's release builds in its image `PodUtils/podnet:images/build` (Alpine), which has
  wget and maybe no curl.

## Approach

1. `fetch-zig.sh`: curl, else wget; the `source` of the request from
   `ZIG_SOURCE` (default `podcrates`).
2. A new script, check-zig-mirrors.py: `zig-mirrors.txt` against ziglang.org's
   list, with a planted mirror that must fail; a job of `upstream.yml`.
3. `docs/maintenance.md`: how a consumer calls the script from podcrates
   at a pinned commit.

## Prove

```sh
python3 -I scripts/check-zig-mirrors.py
sh scripts/fetch-zig.sh 0.16.0 70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00 /tmp/zig.tar.xz
```

The job of `upstream.yml` passes, and fails on the planted mirror.

## Done

2026-11-05, commits `4ed7792` (`fetch-zig.sh` with curl or wget and
`ZIG_SOURCE`; `check-zig-mirrors.py`; the job `mirrors` of `upstream.yml`;
`docs/maintenance.md`, "Shared CI scripts") and `dabb073` (the check of the
sources reads a taken block before its adoption). Here: the check found
the 17 mirrors of ziglang.org, the planted mirror gave exit 1, and
`fetch-zig.sh` fetched zig 0.16.0 with its SHA-256 through curl and
through wget alone. The run 42066432619 of `upstream.yml` at `dabb073`:
the jobs `sources` and `mirrors` green.

# T-047: The weekly watch of the relay contract, in `pod-relay`

**Source:** the owner, 2026-11-05 (`PodUtils/podnet:docs/decisions.md`, Relay contract watch; PodUtils/podnet@T-134).
**Category:** chore
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

The owner's ruling: "The weekly watch of the relay contract belongs to
`pod-relay` in `PodUtils/podcrates` (PodUtils/podnet@T-134), not to podnet." The contract
is at <https://tcp.ssh.relay.ajam.dev/llms-full.txt>.

## Premise

- Not read yet: podnet's watch (PodUtils/podnet@T-134) and podssh's checks
  of the contract; read them when `pod-relay` starts.

## Approach

A weekly workflow that reads the contract and compares it with the
words, the routes and the limits that `pod-relay` holds; a planted change
of the contract must fail it.

## Prove

```sh
gh workflow run relay-contract.yml
```

## Start condition

T-025 is done.

# T-051: The fallback channel between the sessions, tested (GitHub #2)

**Source:** GitHub #2; the owner, 2026-11-05: "please exercise the backup issue way of communication in case it doesn't work, suggest/use something else".
**Category:** chore
**Milestone:** M0
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The owner, before a night away: "in case your or they aren't able to msg
each other, please use a github issue with unique id each & amend it,
poll it for changes", then "please exercise the backup issue way of
communication in case it doesn't work, suggest/use something else". GitHub
#2 (COORD-001) is the test: each session amends its own section of the
body; the podcrates session polls it.

## Premise

- The sessions of podssh and podnet have GitHub tools for their own
  repository; writing to PodUtils/podcrates is not measured yet.

## Approach

1. Each session amends its section of GitHub #2, or, when it cannot write
   to podcrates, its own `COORD-001` issue, and says which way worked.
2. `docs/maintenance.md`, "Talk between sessions", gives the way that
   worked.

The test, 2026-11-05: podnet's session cannot write to PodUtils/podcrates
(its access covers PodUtils/podnet only) and wrote PodUtils/podnet#317,
which this session read through the mirror ("state: received at
2026-11-05T17:02Z"). podssh's session asked the owner first (its rules allow a public post only
on its own user's word), then wrote PodUtils/podssh#151. So the fallback
is one mailbox for each writer (`docs/maintenance.md`, "Talk between
sessions").

## Prove

```sh
curl -qfsS "https://forge.web.pkgforge.dev/github/repos/PodUtils/podcrates/issues/2?n=1"
```

Each section of the body was amended by its session.

## Done

2026-11-05. GitHub #2 (COORD-001): neither consumer session can write to
PodUtils/podcrates (podssh: "Access denied: repository
"podutils/podcrates" is not configured for this session"); each wrote its
side in its own repository, PodUtils/podnet#317 (17:02 UTC) and
PodUtils/podssh#151 (17:05 UTC, with the owner's word in that session),
and this session read both through the mirror. `docs/maintenance.md`,
"Talk between sessions" (`d2bab56`), gives the procedure: one mailbox for
each writer. Q5 stays open for a standing permission of the posts.
# T-053: The names of the crates of podvm on crates.io (GitHub #1)

**Source:** GitHub #1 (reopened 2026-11-06); the owner, 2026-11-06 (`docs/decisions.md`, The crates of podvm).
**Category:** chore
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** blocked

## Problem

GitHub #1, reopened for podvm: "reserve now each name that podvm needs
now or later, because a reservation takes time". The 13 `pod-*` crates of
M6 and M7 and the held name `pod-probe` are in `docs/design.md`, section
4; the 7 `podvm-*` names are podvm's (repository PodUtils/podvm).

## Premise

- Measured: `python3 -I scripts/check-crates-io-names.py` reads the new
  rows and reports each new name as missing until it is reserved.

## Approach

The owner reserves the 14 names with the repository of podcrates and the
trusted publisher of `publish.yml`; the script finds each one.

## Prove

```sh
python3 -I scripts/check-crates-io-names.py
```

## Blocker

The owner reserves `pod-hostprobe`, `pod-oci`, `pod-extract`, `pod-rootfs`,
`pod-enter`, `pod-supervise`, `pod-guest`, `pod-microvm`, `pod-sud`,
`pod-confine`, `pod-netlab`, `pod-pcap`, `pod-sandprobe` and `pod-probe`.


# T-064: `watch-org.sh` reads no repository but the last on Windows (PodUtils/podvm#195)

**Source:** PodUtils/podvm#195 (COORD-002 of podvm, to podcrates).
**Category:** defect
**Milestone:** M0
**Priority:** P1
**Effort:** S
**Status:** open

## Problem

podvm, 2026-11-06: "On Windows (Git Bash with the native Python),
`python3` prints each repository name with a CRLF line ending. The names
in `repos` then end in `\r`, and every issue URL but the last one is
wrong. `curl -f` fails for each, `|| continue` passes over it, and
`org-issues.py` gets only `PodUtils/.github`. The watcher prints nothing
and records an empty `org-seen.txt`, with no error."

## Premise

- Measured: with a stand-in of `python3` that ends each line with CRLF,
  the watcher at `367db93` recorded 0 messages.

## Approach

`tr -d '\r'` after the Python that lists the repositories.

## Prove

```sh
sh -n scripts/agent/watch-org.sh
```

With the CRLF stand-in, the watcher records the messages of podcrates and
podvm.
