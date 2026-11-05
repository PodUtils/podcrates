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
**Status:** open

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

# T-005: `pod-core`: the traits, the errors, the events and the helpers that each crate shares

**Source:** `docs/design.md`, sections 3 and 6; `docs/decisions.md`, call
C17.
**Category:** feature
**Milestone:** M0
**Priority:** P1
**Effort:** M
**Status:** open

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

# T-006: `pod-testkit`: the guards of the host and the SSH test server

**Source:** `docs/design.md`, section 4 (M0); `docs/decisions.md`, call C9.
**Category:** chore
**Milestone:** M0
**Priority:** P1
**Effort:** S
**Status:** open

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
2. The generic parts of the SSH harness behind the feature `ssh`.
3. A stand-in takes no type of a pod crate, so that a crate can
   dev-depend on the kit.

## Prove

```sh
cargo test -p pod-testkit --all-features
```

## Start condition

T-001 is done.

# T-007: The workflows of podcrates

**Source:** `docs/design.md`, section 9.
**Category:** chore
**Milestone:** M0
**Priority:** P1
**Effort:** M
**Status:** open

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

# T-008: Adoption trials in CI, and the trial of `pod-todo` in podssh and podnet

**Source:** `docs/design.md`, section 8, step 7; `docs/decisions.md`,
call C15.
**Category:** chore
**Milestone:** M0
**Priority:** P1
**Effort:** M
**Status:** open

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

# T-009: The first release

**Source:** `docs/design.md`, section 10.
**Category:** release
**Milestone:** M0
**Priority:** P1
**Effort:** S
**Status:** open

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
