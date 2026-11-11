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
**Status:** done

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

## Done

2026-11-06. `python3 -I scripts/check-crates-io-names.py`: 51 names, 0
faults; each of the 14 names is a `0.0.0` placeholder with the repository of
podcrates.

# T-064: `watch-org.sh` reads no repository but the last on Windows (PodUtils/podvm#195)

**Source:** PodUtils/podvm#195 (COORD-002 of podvm, to podcrates).
**Category:** defect
**Milestone:** M0
**Priority:** P1
**Effort:** S
**Status:** done

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

## Done

2026-11-06, commit `54427f6`. With a stand-in of `python3` that ends each
line with CRLF, the watcher recorded 0 messages at `367db93` and the
messages of podcrates and podvm after the fix. `docs/session-protocol.md`
pins the commit of the fix.

# T-081: `pod-core`: a `Failure` gives its I/O cause, and "not found" is not "unavailable"

**Source:** the trial of `pod-hostprobe` in podvm (T-055, `c39f20f`).
**Category:** feature
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podvm keeps an I/O error of the space and exec probes as its own
`Error::Io` with its path and kind. `pod_core::Failure` cannot give up its
cause, and `Class::of_io(NotFound)` is `Unavailable`, the class of "no
space": podvm's glue downcasts the source and strips the cause from the
text to tell them apart.

## Premise

- Read: `crates/pod-core/src/failure.rs`.

## Approach

`Failure::io_cause(&self) -> Option<&std::io::Error>` (the cause when it is
one), and a class `NotFound` that `Class::of_io` gives for
`ErrorKind::NotFound`; the words and each consumer's `match` follow (a new
variant: a `match` with no `_` arm adds it). Tests of both.

## Prove

```sh
cargo test -p pod-core
```

## Done

2026-11-06. `Failure::io_cause` gives the error of I/O of the chain of
causes; `Class::NotFound` ("notfound") is the class of
`ErrorKind::NotFound` in `Class::of_io`. Each `match` on `Class` in podssh,
podnet and the crates has a `_` arm. The test
`the_io_cause_comes_back_and_not_found_has_its_class` passes, and
`scripts/agent/check.sh` passes. podvm's glue of the space and exec checks
can read `io_cause()` and `Class::NotFound` from the next release.

# T-084: The record of `SOURCES` names a git blob id (GitHub #22)

**Source:** GitHub #22
**Category:** docs
**Milestone:** M0
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

Issue #22: `docs/design.md` and `docs/maintenance.md` say that a
`SOURCES` line holds a SHA-256. `scripts/check-sources.py` and the files
`crates/*/SOURCES` hold the git blob id (SHA-1, 40 hex digits).

## Premise

- Read: the three places and `crates/pod-hostprobe/SOURCES`.
- The record format stays. The fix is in text and needs no ruling.

## Approach

Change the two documents and the header text that `record()` writes in
`scripts/check-sources.py`. Name `git ls-tree COMMIT -- PATH` as the
command that checks a value by hand.

## Prove

```sh
grep -n "SHA-256" docs/design.md docs/maintenance.md scripts/check-sources.py
python3 -I scripts/check-sources.py --plant-change pod-todo
cargo todo check
```

The `grep` gives no line about `SOURCES`. The second command exits 1.

## Done

2026-11-09. Commit `d7a9091`. `docs/design.md`, `docs/maintenance.md` and
the header that `record()` of `scripts/check-sources.py` writes name the git
blob id (SHA-1, 40 hex digits) and `git ls-tree COMMIT -- PATH` as the
check. `grep -n "SHA-256" docs/design.md docs/maintenance.md
scripts/check-sources.py` gives no line about `SOURCES`.
`python3 -I scripts/check-sources.py --plant-change pod-term` exits 1 (the
mirror answered 404 and 403 for `pod-todo` and `pod-hostprobe`, so the plant
ran on `pod-term`). `cargo todo check`: the record agrees.

# T-085: `check-sources.py`: the default branch of a source repository (GitHub #23)

**Source:** GitHub #23
**Category:** defect
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Issue #23: `Mirror.rev` returns `"main"` and `Clone.rev` returns
`"origin/main"` in `scripts/check-sources.py`. A repository with another
default branch gives `removed` for each file. This blocks the import of
`hackerschoice/gsocket` (T-086): its default branch is `master`. Issue
#23 names four files of the import at one commit, and issue #24 names
another commit for the relay facts (Q7 asks the owner).

## Premise

- Read: `scripts/check-sources.py`. Each source repository uses `main`
  today, so the red run of the `upstream` job has another cause.
- Not measured: the API answer for a repository with another branch.

## Approach

Read the default branch (`GET /repos/{repo}`, cached; for a clone,
`git symbolic-ref refs/remotes/origin/HEAD`). Make `rev` a required
argument of `tree` and `commits`. A 404 at a resolved ref is `removed`.
An unresolved ref is `Unreadable` (exit 2). Update the docstring and the
header that `record()` writes.

## Prove

```sh
python3 -I scripts/check-sources.py --plant-change pod-todo
sh scripts/plant-each.sh
```

A new plant uses a block whose repository has a default branch that is
not `main`.

## Done

2026-11-09, `86dfd6f`. `Mirror` reads `default_branch` at
`GET /repos/REPO` once for each repository; `Clone` reads
`refs/remotes/origin/HEAD`, else `git ls-remote --symref origin HEAD`, and
checks that the branch exists. `rev` is a required argument of `tree` and
`commits`. `tree` gives no file for a 404 (mirror) or an empty `ls-tree`
(clone) at a resolved ref, so the file is `removed`; a ref that does not
resolve raises `Unreadable` (exit 2), and `compare` no longer reads a
failed read as a removal. A commit with an empty message no longer stops
the list of commits (gsocket has one: `2e01cd1`).

- `sh scripts/plant-each.sh sources-default-branch`: caught, exit 1. The
  script of the commit before prints `removed
  hackerschoice/gsocket:include/gsocket/gsocket.h` for the same plant: a
  miss.
- A block of `hackerschoice/gsocket` at `5950647f` through the mirror and
  through a clone: 0 changes for an equal blob, `changed` for a different
  blob, `removed` for a path that `master` does not hold. A repository that
  does not exist: exit 2. A clone with no `origin/HEAD`: the remote's HEAD
  (`master`), 0 changes; a clone whose remote cannot be read: exit 2.
- `python3 -I scripts/check-sources.py --plant-change pod-todo` with a
  clone of `Azathothas/podssh`: `pod-todo: changed`. Through the mirror,
  the run exits 2 at the `taken` commit `9198a38`: the mirror answers 404
  for `Azathothas/podssh/commits/9198a38` (the repository moved to
  `PodUtils/podssh`). The script of the commit before exits 2 there too;
  CI reads clones (`upstream.yml`).

# T-092: The live workflow `live.yml` (GitHub #30)

**Source:** GitHub #30
**Category:** chore
**Milestone:** M0
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

Issue #30: `AGENTS.md`, `docs/development.md`, `docs/design.md`,
`crates/pod-dns/README.md` and `crates/pod-tls/tests/live_handshake.rs`
name a live workflow. `.github/workflows/` has no `live.yml`. 10 tests
with `#[ignore]` never run in CI: 5 in
`crates/pod-tls/tests/live_handshake.rs`, 4 in
`crates/pod-dns/tests/live_doh.rs`, 1 in `crates/pod-proxy/tests/mptcp.rs`.
The `--ignored` steps of `.github/workflows/nightly.yml` run
`system_store` only. The fourth `live_doh` test needs a proxy of names;
scripts/box/proxy.py is in podssh, not here.

## Premise

- Read: the files above and `scripts/check-workflows.py`.
- Call C24 (`docs/decisions.md`): `live.yml` runs each day at an
  off-peak hour and by `workflow_dispatch`, with egress to the relay and
  to public DoH resolvers.
- `crates/pod-services/README.md` names a `live.yml` that is a step of
  podnet: do not change it. T-050 owns the data watches.

## Approach

Add the file .github/workflows/live.yml (schedule and `workflow_dispatch`,
`permissions: contents: read`, `timeout-minutes`) with three steps:
`live_handshake`, `live_doh`, `mptcp`. Port the proxy of names from
podssh or write a stand-in in `pod-testkit`. Update the workflow list in
`docs/design.md`. Add a rule to `scripts/check-workflows.py`: each
workflow that `AGENTS.md` and `docs/` name exists. Add a plant for it.

## Prove

```sh
python3 -I scripts/check-workflows.py
sh scripts/plant-each.sh
```

A dispatched run of `live.yml` on `main` is green. Write its run id in
`## Done`.

## Done

2026-11-09. Commits `b966f30` (the workflow, the proxy of names and the rule),
`31fa6e3` (the plant). `.github/workflows/live.yml` runs each day at 01:53
UTC and by dispatch, with `contents: read` and `timeout-minutes`, the
actions pinned: `live_handshake` (5 tests), `live_doh` (4 tests, the fourth
through `scripts/names-proxy.py`, a port of the mode `--take names` of the
proxy of podssh) and `mptcp`. `scripts/check-workflows.py` checks that each
workflow that `AGENTS.md` and `docs/*.md` name is a file; plant
`workflows-missing`: caught. `docs/design.md` has the row. Dispatch on
`main` at `48de06f`: run 42699700554, success (the three steps).


# T-093: README: the state and the table of crates (GitHub #31)

**Source:** GitHub #31
**Category:** docs
**Milestone:** M0
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

Issue #31: `README.md` says that milestone M0 runs now and that the first
release follows M0. `Cargo.toml` has version 0.7.0 and `docs/releases/`
holds the notes of v0.1.0 to v0.7.0. The crate table lacks `pod-services`
and `pod-hostprobe`, and the waves M6 and M7. `crates/` holds 10 crates.

## Premise

- Read: `README.md`, `Cargo.toml`, `docs/design.md` (section 4),
  `TODO/PROGRESS.md`.
- Not measured: none.

## Approach

Rewrite the State from `TODO/PROGRESS.md` (the version, the releases).
Rewrite the table from section 4 of `docs/design.md`. Option: a check
that each name in `crates/` is in the table.

## Prove

```sh
cargo todo check
for c in $(ls crates); do grep -q "\`$c\`" README.md || echo "missing $c"; done
```

The loop prints nothing.

## Done

2026-11-09. Commits `0fb38ee` (the work) and the record commit that holds
this section. The State of `README.md` names version 0.8.0, the releases
and the current wave. The table lists each crate of `crates/` (15) and each
wave (M0 to M4, M6, M7, M8). `scripts/check-design-paths.py` checks that
each directory of `crates/` is in the table; the plant
`scripts/plants/readme-crate-missing.plant` catches it. The loop of the Prove
prints nothing. `cargo todo check`: the record agrees.

# T-094: `docs/design.md`: the repository table matches the tree (GitHub #32)

**Source:** GitHub #32
**Category:** docs
**Milestone:** M0
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

Issue #32: the repository table of `docs/design.md` names five paths of
`scripts/` that do not exist: `check-vendored.py`, `plant.sh`,
`release.sh`, `fake-*.py` and `check-relay-spec.py`. It does not name
twelve scripts that exist: in `scripts/` `adoption.sh`,
`check-consumer-toolchains.py`, `check-crates-io-names.py`,
`check-zig-mirrors.py`, `pull-image.sh`, `secrets-report.py`; in
`scripts/agent/` `new-issues.py`, `org-issues.py`, `runs.py`,
`secrets_like.py`, `togit.sh`, `watch-org.sh`. It omits `HANDOFF.md`,
`session-protocol.md` and `research/` of `docs/`.

## Premise

- Read: the table and `ls scripts scripts/agent docs`.
- Not measured: none.

## Approach

Rewrite the rows as issue #32 gives, with a "planned" row for a path that
is not built. Add a checker that reads the table and reports each absent
path (report only for the planned row). Run it from
`scripts/agent/check.sh`. Add a plant for it.

## Prove

```sh
python3 -I scripts/check-design-paths.py
sh scripts/plant-each.sh
```

The checker is part of the entry. The first command exits 0.

## Done

2026-11-09. Commit `0fb38ee`. The repository table of `docs/design.md`
names the scripts of `scripts/` and `scripts/agent/`, the files of `docs/`
and `schemas/`; a path that is not built is a planned row; the stale names are gone.
`scripts/check-design-paths.py` checks the table (a path that is absent, a
file that the row does not name); `scripts/agent/check.sh` and `ci.yml` run
it. `python3 -I scripts/check-design-paths.py` exits 0. `sh
scripts/plant-each.sh design-path-missing design-script-unlisted
readme-crate-missing`: 3 caught, 0 missed. `cargo todo check`: the record
agrees.

# T-095: The static probe turns on `pod-proxy/blocking` and the builds with no tokio (GitHub #33)

**Source:** GitHub #33
**Category:** chore
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Issue #33: `examples/static-probe/Cargo.toml` turns on the feature `ech`
of `pod-proxy` only. `crates/pod-proxy/Cargo.toml` has `blocking`, `ech`
and `fuzzing`. The blocking face is the one that podvm takes.
`docs/design.md` says "each crate with each feature". The probe also
builds `pod-core` and `pod-tls` with default features; the builds with
`default-features = false` (call C20) are not probed. `docs/development.md`
gives the build command with no `-p static-probe`.

## Premise

- Read: the probe, the manifest of `pod-proxy`, `docs/development.md`.
- Not measured: the size of the probe with `blocking`.

## Approach

Add `blocking` and one call (`blocking::Dialer::new` and `proxy_at`) to
the probe. Keep `fuzzing` out, with a comment. Add the builds with no
tokio of `pod-core` and `pod-tls`. Add a checker: each feature of a
library crate is in the probe or in an allowance list; add a plant. Fix
the command in `docs/development.md`.

## Prove

```sh
cargo build --release -p static-probe
sh scripts/check-static.sh
sh scripts/plant-each.sh
```

The `static` job of `ci.yml` is green on the 12 targets.

## Done

2026-11-09. Commits `42fb7e2` (the probe and the checker), `a63b14b` (the
plants). `examples/static-probe` turns on `pod-proxy/blocking` and calls
`blocking::Dialer::new` and `proxy_at`. The new package
`examples/static-probe-lean` builds `pod-core` and `pod-tls` (`pure`,
`rsa`) with no default feature: its tree has no tokio.
`scripts/check-probe.py` checks that each feature of each library crate is
in a probe or in its allowance list (`fuzzing` of `pod-proxy` and
`pod-gsr`, `screen` of `pod-testkit`), and that tokio is not in the tree of
the lean probe. Plants `probe-feature-missing` and `probe-lean-tokio`:
caught. `ci.yml` builds each probe in an own command (one build of both
turns tokio on) and checks both. `docs/development.md` has the `-p` option.
The `static` job on the 12 targets runs on the push of this commit.


# T-101: `pod-core`: the classes Revoked, Policy and Unsupported, one exit code for each class, one JSON log schema (GitHub #35)

**Source:** GitHub #35 (A-X-17, A-C-07)
**Category:** feature
**Milestone:** M1
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

A-X-17 and A-C-07 ask for one table of exit codes, with "credential revoked", "policy refusal" and "not
supported". `pod_core::Class` has Usage, Config, Unavailable, NotFound, TempFail, Timeout, Protocol and
Refused.

## Premise

- Read: `crates/pod-core/src/` (the classes) and the codes of podssh and podnet in the asks.
- Not measured: a change.

## Approach

Add Revoked, Policy and Unsupported. Add `Class::exit_code()` on the sysexits (64 usage, 69 unavailable,
75 tempfail, 77 refused, 78 config and revoked; the entry chooses a code for policy and for unsupported).
Write the table and a JSON log schema (time, level, event, fields) in a document. The schema goes to
`schemas/` (ruling of 2026-11-09).

## Prove

```sh
cargo test -p pod-core
cargo todo check
```

## Done

2026-11-09. `pod_core::Class` has `Revoked`, `Policy` and `Unsupported`,
`Class::ALL` and `Class::exit_code()` on sysexits(3): 64 usage, 66
notfound, 69 unavailable, 75 tempfail and timeout, 76 protocol, 77 refused,
78 config and revoked (one `RestartPreventExitStatus=78` for both), 79
policy, 80 unsupported. The README of `pod-core` has the table and the log
line; `schemas/log.v1.schema.json` is the JSON log schema (time, level,
event, fields; stage and class of a failure). The test `exit_codes` holds
the table and the schema to the code; a planted code (policy 81) fails it.
`cargo test -p pod-core` passes.


# T-105: `pod-todo`: an acceptance registry of ask ids and their tests (GitHub #35)

**Source:** GitHub #35 (A-X-14)
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

Each release must say which ask ids it passes. No tool maps an ask id to a test.

## Premise

- Read: `crates/pod-todo/src/` and the ask of A-X-14.
- Not measured: a registry.

## Approach

A registry file in `TODO/` maps an ask id to a test name. `cargo todo check` fails on a test that does
not exist. `cargo todo acceptance --json` writes `acceptance.json` for the release index of T-104. Each
repository already runs `cargo todo check`.

## Prove

```sh
cargo test -p pod-todo
cargo todo check
```

# T-118: Supply chain: an SBOM for each release and a planted advisory that fails `deny.yml` (GitHub #35)

**Source:** GitHub #35 (A-X-21)
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

No repository publishes an SBOM. A-X-21 asks for one, and for a check that a known-bad advisory fails CI.

## Premise

- Read: `.github/workflows/deny.yml`, `.github/workflows/fuzz.yml` and the ask.
- Not measured: an SBOM.

## Approach

A CycloneDX or SPDX SBOM of each tag in `release.yml`. A plant that adds a known-bad advisory and must
fail `deny.yml`.

## Prove

```sh
sh scripts/plant-each.sh
```

# T-143: `plants.yml`: four plants are stale and two miss

**Source:** `plants.yml` runs 42702242541 to 42745399953 (red since 2026-11-09).
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`plants.yml` failed on each push: `probe-feature-missing`,
`proxy-pins-not-given`, `proxy-source-not-handed` and `record-milestone`
were stale (the patch did not apply after the code moved), and
`sources-change` missed, as the read-only mirror answered 404 or an empty
tree for `Azathothas/podssh`. The record did not know the red run.

## Premise

- Read: the logs of run 42745399953; `scripts/plant-each.sh`;
  `scripts/check-sources.py` (`compare`, `--plant-change`).
- Measured: the mirror answered 200, then 403 and 404, for the same reads
  within minutes.

## Approach

- Make each stale patch again on the current code; set the `EXPECT` of
  `record-milestone` to the milestones of `TODO/todo.toml` (M0 to M8).
- `sources-change` reads a clone of podssh (`--clone`), not the mirror.

## Prove

```sh
sh scripts/plant-each.sh probe-feature-missing proxy-pins-not-given proxy-source-not-handed record-milestone sources-change
```

## Done

2026-11-10. Commit: the commit that names T-143. Each of the five plants
is caught (`plant-each: 5 caught, 0 missed or stale`, in two runs: three,
then two). The run of `plants.yml` after the push is the proof of the
other plants.

# T-154: Timed tests under an emulator: one bound of the test kit and a sweep

**Source:** the nightly runs 42771421361, 42790696301 and 42793991844 (v0.13.0).
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

A test with a bound of time made for a kernel fails, one at a time, under
qemu-user in the nightly cross jobs (riscv64, loongarch64, powerpc64le):
the eight sessions of `pod-ws` (`tests/h2_road.rs`) and the idle limit of
`pod-proxy` (`tests/blocking_streamed.rs`). Each fix copied the same
detection (`pod_hostprobe::interp::detect`) into the test. The next bound
fails in the next run.

## Premise

- `pod_hostprobe::interp::detect(..).is_emulated()` sees qemu-user by its
  variables and by `binfmt_misc`.
- `pod-hostprobe` is Linux only; the tests of the other targets need no
  scale.

## Approach

- `pod_testkit::host::slow()` (or a `scaled(Duration)`): the factor of an
  emulator (5) or 1 on a kernel, with the detection in one place, Linux
  only, and a skip line that names it.
- Use it in `pod-ws` and `pod-proxy` (drop the copies), then sweep the
  bounds of each test (grep `Duration::from_` in `tests/` and in each
  `#[cfg(test)]` module with an `elapsed()` or a `timeout(`) and scale each
  bound that measures speed, not correctness.
- Run the cross tests of riscv64 and loongarch64 under qemu-user here
  with the variable of an emulator set.

## Prove

```sh
cargo test --workspace --all-features
```

Then a nightly with each cross job green.

## Done

2026-11-10. Commit: the commit that names T-154. `pod_testkit::host::emulated()`
(one detection: `pod_hostprobe::interp::detect`, cached, Linux only) and
`pod_testkit::host::scaled(d)` (5 times `d` under an emulator). `pod-testkit`
takes `pod-hostprobe` as a Linux dependency; `pod-hostprobe` has `pod-testkit`
as a dev-dependency only, which cargo accepts. The copies in `pod-ws`
(`h2_road.rs`) and `pod-proxy` (`blocking_streamed.rs`) and their
dev-dependency on `pod-hostprobe` are gone. The sweep scales each outer bound
that guards a hang (`timeout(`, read and receive limits, `.timeout(WAIT)`) and
each `elapsed() <` bound of a test in `pod-broker`, `pod-dns`, `pod-gsr`,
`pod-oci`, `pod-proxy`, `pod-tls`, `pod-ws`, `pod-term`, `pod-guest`,
`pod-hostprobe` and `pod-services-watch`. A timer that a test checks (an idle
limit, a liveness period, a body idle limit, a lower bound) stays as it was.
All test binaries of these crates (96, with `--all-features`) pass under
`qemu-riscv64` and `qemu-loongarch64` with `QEMU_LD_PREFIX=/` set. The test
`the_bound_scales_under_an_emulator` proves the detection there.

# T-192: `pod-core`: one replace of a private file with a durable commit and three outcomes (PodUtils/podnet#437)

**Source:** PodUtils/podnet#437 (COORD-019 of podnet), the owner's donor
review: tailsplit 1. The donor: idy/tailsplit at
`0bd3a74f667c286b3c212004c289a501ee4eb947` (GPL-3.0-or-later): rules.go,
rules_replace_other.go, rules_replace_windows.go.
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

Nine copies of the replace of a private file (a temporary file, then a
rename) are in podnet, podssh and `pod-core`. Four of the nine never sync
the file, and none syncs the directory. An operator runs
`podnet git serve` on a small VPS. The first start writes the SSH host key
through `state::replace_file` of podnet, and the power goes off a few
seconds later. On a file system that does not write the data before the
rename, the key file can come back empty. The next start cannot parse it,
and each client then sees a changed host key. The same gap applies to the
relay pair file, the `/etc/resolv.conf` of `warp tun` and the instance
record. After a power cut the file has the correct name and mode, and zero
length or old content.

The donor writes a temporary file in the same directory (mode 0600),
syncs and closes it, renames it, then opens the directory and syncs it.
Its result has two parts: installed or not, and an error. A caller treats
"installed, error" as a good install with a warning ("rules installed but
directory durability could not be confirmed"), never as a failure. On
Windows, one replace on the same volume with write-through gives the same
contract.

## Premise

- Read (this tree): `crates/pod-core/src/files.rs:165-181`.
  `write_private` syncs the file and renames it, and does not sync the
  directory; its temporary name is `.NAME.PID.tmp`. It is private to the
  crate: `store_noting` (`:57-69`) and `store_file_in_first` call it, and
  through them `pod-tls` (`crates/pod-tls/src/groups/dirs.rs:25`) and
  `pod-identity` (`crates/pod-identity/src/pins.rs:134`). The review
  agrees.
- Read: `crates/pod-core/src/files/own.rs:67-92`. `create_private` syncs
  the file, links it into place with a hard link (never over a file), and
  does not sync the directory. The review agrees. It is a create, not a
  replace.
- Read: `crates/pod-core/src/files/closed_win.rs` (the private files of
  Windows) has no `MoveFileExW`.
- Read: `scripts/check-size.py` runs in `.github/workflows/ci.yml:73` and
  in `scripts/agent/check.sh:35`. A new check of the repository needs its
  plant in `scripts/plants` (T-003).
- Found while reading: T-195 (two threads that store one name at once
  share the temporary name of `write_private`).
- Not read: the eight copies in podnet (at `649c8e3`) and podssh (at
  `4489df2`), and the writers that the review did not compare (the store
  commit of podvm; `net/store.rs`, `acme/store.rs`, `known_hosts/edit.rs`
  and the key generation of podssh).
- Not measured: the loss of data after a power cut (Inferred by the
  review). A power cut cannot be injected in CI; the injected faults test
  the control flow only.

## Approach

Licence: tailsplit is GPL-3.0-or-later. Write the implementation from this
description, not from the donor source. PodUtils is 0BSD.

- `replace_private(path, bytes, mode, in_place_fallback)` of
  `pod_core::files` gives `io::Result<Commit>`, with the variants
  `Commit::Durable` and `Commit::InstalledUnconfirmed(io::Error)`. `Err`
  means: nothing changed at `path`, and no temporary file stays (except
  after the fallback in place).
- The steps: a temporary name beside `path`, `.NAME.PID.N.tmp` (the PID
  and a counter of the process, never a fixed name); a create with
  `O_EXCL | O_NOFOLLOW` and the final mode (the umask removes bits; set the
  mode again after the create); write; `sync_all`; rename; then open the
  directory and `sync_all` it. A failed directory sync gives
  `InstalledUnconfirmed`. No reader sees a wider mode, also not for a
  moment.
- A rename that fails over a bind-mounted file (`/etc/resolv.conf` in a
  container), with `in_place_fallback`: write the file in place and sync
  it. The result is then always `InstalledUnconfirmed`, because a write in
  place is not atomic.
- A symlink at `path` is replaced, not followed (keep the test
  `replace_file_does_not_follow_a_symlink` of podnet as a test here).
- Optional, in 20 lines at most: the next call for a name removes a stale
  `.NAME.PID.N.tmp` of a PID that is not alive.
- Windows: `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` and
  `MOVEFILE_WRITE_THROUGH`, beside `closed_win.rs`.
- `write_private` goes onto the helper, and `create_private` syncs the
  directory. A caller matches on `Commit` and writes `InstalledUnconfirmed`
  as one `tracing::warn!` line with the path and the error, never as an
  exit code.
- A check, a script beside `scripts/check-size.py` with its plant, refuses
  `fs::rename` outside the helper on a path that a private writer made.
- Keep each file under 500 lines (`crates/pod-core/src/files.rs` has 329):
  the helper goes into a module of its own.
- The consumers move their copies in their own repositories after the
  release, one repository at a time: podnet six copies (with
  PodUtils/podnet#431 and PodUtils/podnet#433), podssh two copies and the
  `sync_dir` of its spent-grant record. The hand-off of the release names
  the helper. No consumer changes here. The helper does not make a change
  of two files atomic: a pair file and its record keep a rule of order of
  their own.

## Prove

```sh
cargo test -p pod-core --all-features
python3 -I scripts/check-private-replace.py
python3 -I scripts/check-private-replace.py --self-test
```

The tests:

- A file layer that injects faults fails, in turn, the create, the write,
  the file sync, the rename and the directory sync. The first four give
  `Err` and leave no temporary file; the last gives
  `Ok(InstalledUnconfirmed)`.
- Under umask 0 and umask 077, the mode is 0600.
- A symlink at `path` is replaced, and its target does not change.
- A `path` with no file name (`/`) gives `Err` and makes nothing (the
  negative case).
- The check refuses its plant.

# T-193: `pod-core`: a bounded tail of a log that starts at a line (PodUtils/podnet#437)

**Source:** PodUtils/podnet#437 (COORD-019 of podnet), the owner's donor
review: tailsplit 2. The donor: idy/tailsplit at
`0bd3a74f667c286b3c212004c289a501ee4eb947` (GPL-3.0-or-later):
`desktopLogTail` in cmd/tailsplit-tray/desktop_logs.go.
**Category:** feature
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

Three paths of PodUtils read a whole log file into memory to show its last
lines: `instances_logs` and `bug_report::tail` of podnet, and `logs` and
`follow_tail` of `pod-supervise` (thus `podbox logs --tail 50` of podvm).
A detached podnet instance (a tunnel, a WARP proxy) runs for weeks, and
its log has no cap and no rotation. `podnet bug-report` then uses memory
that grows with the log. One byte that is not UTF-8 anywhere in the file
makes `read_to_string` fail, and the report holds "(the log ... cannot be
read)" in place of the last 100 lines.

The donor reads a window of 128 KiB from the end. It seeks to the size
minus the window and reads one window at most. It removes each byte up to
and including the first newline, or all of the window when it has no
newline. It converts to UTF-8 with replacement characters, redacts, and
cuts to the window again at the start of a character. The cut line goes
first because a cut can land inside a secret, past the prefix that the
redactor knows (the `scrub_word` of podnet knows tokens by a prefix, for
example `eyJ`).

## Premise

- Read (this tree): `crates/pod-supervise/src/logs.rs:13-24`. `logs`
  reads the whole file with `std::fs::read`. `follow_tail` (`:95-105`)
  calls it, then `tail_offset` (`:72`), then `follow_from` at the length
  of that read. The review agrees.
- Read: `crates/pod-core/src/files.rs:158` (`read_trusted`) and
  `crates/pod-core/src/files/own.rs:28` (`read_own`), the neighbours of
  the new function.
- Found, not in the review: `logs` gives bytes (`Vec<u8>`), the raw output
  of a payload. A lossy `String` would change the bytes that `follow_tail`
  writes. Thus `pod-core` gives the window as bytes, and the lossy text is
  a second step, for a view of a person.
- Not read: `instances.rs` and `bug_report.rs` of podnet (at `649c8e3`);
  `podvm_supervise::logs` of podvm (at `782449a`).
- Not measured: the cost in memory and in time against the size of the
  log (Inferred by the review). Not run here: the failure of
  `read_to_string` on one byte `0xFF` that the review observed.

## Approach

Licence: tailsplit is GPL-3.0-or-later. Write the implementation from this
description, not from the donor source.

- `read_tail(path, max_bytes) -> io::Result<Tail>`, with the window as
  bytes, a lossy text form, and `cut`. Open the file and read its length.
  Seek to the absolute offset `len - max_bytes` (not `End(-n)`, which fails
  when the file became shorter). Read through `take(max_bytes)`, so the
  bound holds when the file grows. When cut, remove each byte up to and
  including the first newline, or all of the window when it has no
  newline.
- `bound_text(text, max_bytes)`: a cut from the front at the start of a
  character, for a caller that redacts after the read (a replacement can
  make the text longer).
- A file that became shorter after the stat gives a short or empty tail
  and no error.
- `pod-supervise`: `logs` with a bound, and `follow_tail` on the window;
  `follow_from` starts at the end of the window. When `cut` is true and the
  window holds fewer lines than asked, the caller gets a note ("older
  lines are in the log file").
- The windows: 128 KiB for a view, 256 KiB for a bug report. These are
  choices, not measurements.
- The consumers: podnet replaces its two reads after the release; podvm
  takes the change of `pod-supervise` with the tag. The rotation of the
  instance log stays with podnet, the owner of its writer.

## Prove

```sh
cargo test -p pod-core --all-features
cargo test -p pod-supervise --all-features
```

The tests:

- A log of 64 MiB with 100 known last lines gives them, and a reader that
  counts shows that at most `max_bytes` were read.
- A token (`eyJ` and 40 characters) with the cut in its middle: no part of
  it is in the output, and `cut` is true.
- One byte `0xFF` in the middle: the last lines, and no error.
- One line longer than the window: an empty tail, `cut` true, and the
  note.
- A file truncated to zero between the stat and the read: an empty tail,
  and no error.
- A directory as the path: the error of the system, not an empty tail
  (the negative case).

# T-195: `pod-core`: two threads that store one private file at once share one temporary name

**Source:** the reading of `crates/pod-core/src/files.rs` for T-192
(PodUtils/podnet#437, tailsplit 1)
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`write_private` names its temporary file `.NAME.PID.tmp`
(`crates/pod-core/src/files.rs:168`) and first removes a file of that name
(`:169`). Two threads of one process that store the same name at once
share that file. Thread B removes the file of thread A and makes its own.
The rename of A then puts the file of B in place, possibly before B wrote
all of it. The rename of B then fails, so `store_noting` tries the next
directory. A reader can see half a file, and the file can be in two
directories. `create_private` makes a unique name with a counter of the
process (`crates/pod-core/src/files/own.rs:68-74`).

The callers: `pod-tls` keeps its file of TLS groups
(`crates/pod-tls/src/groups/dirs.rs:25`), and `pod-identity` keeps the pins
of node keys (`crates/pod-identity/src/pins.rs:134`). A torn pins file
refuses each connect with `PinError::Unreadable` until a person repairs
it.

## Premise

- Read: `crates/pod-core/src/files.rs:162-181`,
  `crates/pod-core/src/files/own.rs:61-92`,
  `crates/pod-tls/src/groups/dirs.rs`, `crates/pod-identity/src/pins.rs`.
- Read: `pin` (`crates/pod-identity/src/pins.rs:114-138`) reads the pins,
  adds a line and stores the whole file, with no lock. Two pins at once
  can thus lose one line, also with a unique temporary name. A lost pin
  makes the next connect of that node a first sight again (trust on first
  use).
- Not measured: whether a consumer stores one name from two threads at
  once, or pins two nodes at once. Not reproduced.

## Approach

- `write_private` takes the counter of the process, as `create_private`
  does: `.NAME.PID.N.tmp`. The remove before the create goes: a unique name
  needs none. T-192 then builds on this.
- `pin`: a lock of the process around the read, the add and the store. A
  lock of the file between two processes is a call of the work; record it
  in `docs/decisions.md`.
- Tests: threads that store one name at once each get `Ok`, and each read
  gives one whole body; two pins at once in one directory keep both lines.

## Prove

```sh
cargo test -p pod-core --all-features
cargo test -p pod-identity --all-features
```

## Done

2026-11-11. `write_private` names its temporary file `.NAME.PID.N.tmp`
with a counter of the process and removes no file first. `pins::check_split`
holds a lock of the process around the read, the add and the store; two
processes are not held (call C54). Tests:
`threads_that_store_one_name_at_once_leave_one_whole_body` (8 threads, 16
stores each of 64 KiB, one whole body and no temporary file stays) and
`threads_that_pin_at_once_keep_each_line` (16 threads, 16 lines). The
plants `core-store-shares-one-temp-name` and `identity-pin-with-no-lock`
fail them in each of 3 runs. `cargo test -p pod-core -p pod-identity
--all-features` passes.
