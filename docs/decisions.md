# Decisions

The rulings of the owner for podcrates, with the date of each, the calls
made without a ruling, and the questions for the owner. A ruling stays in
force until a later ruling supersedes it. Then move it to the table
"Superseded", with the ruling that replaced it. `AGENTS.md` has the short
form of the rulings that apply to each session.

## In force

| Date | Area | Ruling |
| --- | --- | --- |
| 2026-11-05 | Repository | `PodUtils/podcrates` holds the crates that the PodUtils projects share (prefix `pod-`). podssh, podnet and podvm (formerly podbox) take their shared code from it. The code of podssh comes to podnet through these crates, not as copies. (Rulings "Shared crates" of podnet and Q101 of podssh.) |
| 2026-11-05 | Who edits | The session of podcrates builds the crates and does not edit podssh, podnet or podvm. From the import of a module until its consumer takes the crate, the consumer changes that module only for fixes, and podcrates ports each fix. The consumer takes each crate on the day of its release. After that, a consumer session can change the crate in podcrates (on `main`, under `AGENTS.md`, with the checks green), or open an issue in podcrates. (Q3.) |
| 2026-11-05 | Delivery | The consumers take the crates by a git tag of podcrates (`tag = "vX.Y.Z"`), not from crates.io. One version for the workspace. The forks that a crate needs are in `vendor/`. podcrates publishes nothing to crates.io now; each `pod-*` dependency keeps `path` and `version` for a later publish. (Q1.) |
| 2026-11-05 | File size | A source file has at most 500 lines. A longer file is split at import. No allowlist. (Q2.) |
| 2026-11-05 | The crates | The list agreed by the sessions of podssh and podnet: 26 crates in four waves, and 8 names held with no crate (`pod-wireguard`, `pod-quick`, `pod-udp`, `pod-mailbox`, `pod-reverse`, `podcrates`, `podutils`, `pod-utils`). The WARP and Cloudflare adapters of podnet move here as `pod-warp` and `pod-cloudflare`; `podnet-relay` merges into `pod-relay`. `docs/design.md`, section 4, has the list; call C17 of the plan adds `pod-core` and `pod-quic`. |
| 2026-11-05 | Static, C, Rust | "We use whatever is necessary, including C, zig, modern Rust, no backwards compat. Our users and clients do not care about building the tool, they care about downloading and using it. The only rule is: everything must be statically compiled." It supersedes the rules "no C" and "MSRV 1.89" of the agreed design. |
| 2026-11-05 | Toolchain | Modern and stable Rust, "even nightly/bleeding edge if its necessary to maintain performance, security whatever we need". The `rust-version` is the stable of the day. A consumer moves its toolchain to match; podcrates does not hold back for the toolchain of a consumer. |
| 2026-11-05 | Compatibility | No compatibility with released binaries for now. A name or a format of the wire (AppIDs, variables, the words of the relay contract) can change. No test against a released binary. (podssh, 2026-11-05.) |
| 2026-11-05 | Capabilities | No feature or capability of the source code may be lost. Compatibility for users is not required. (podnet, "Regressions".) |
| 2026-11-05 | License | 0BSD. Code from podssh, podnet (cfrs), `talaria0101/cfrs` and `nemo-010/cfrs` is 0BSD and needs no credit. |
| 2026-11-05 | Git | Work on `main` only. Push no other branch. Parallel agents use local worktree branches that are not pushed. Identity `Azathothas <ajamx101@gmail.com>`. No agent attribution in commits, pull requests, comments or files. |
| 2026-11-05 | Writing | ASD-STE100 for each text. Technical content only, no history. |
| 2026-11-05 | Crypto crates | `snow` 0.10, `spake2` 0.4 and `crypto_secretbox` 0.1 are accepted. (podnet, "Crypto crates".) |
| 2026-11-05 | Issues | GitHub issues are an inbox. Each issue becomes record entries. Close each issue with evidence (commits, CI runs, test output). |
| 2026-11-05 | Dependabot | Apply each judged update as an own commit on `main` (checks green). Close the pull request with a link to the commit. Do not merge the Dependabot branch. |
| 2026-11-05 | Publish workflow | "publish.yml everywhere: a dedicated publish workflow, separate from the existing release.yml. The agents must create publish.yml." The owner set by the API: the environment `crates-io` (only a tag `v*` deploys to it), the ruleset "release tags" (a tag `v*` cannot be deleted, moved or forced), and the names of the crates on crates.io (0.0.0 placeholders). |
| 2026-11-05 | Publish trigger | `publish.yml` runs on a tag push `v*` and by dispatch; `release.yml` dispatches it on its tag, as a push with `GITHUB_TOKEN` starts no workflow. The publish job has `environment: crates-io` and only `id-token: write` and `contents: read`; the top level has `permissions: {}`; the token comes from `rust-lang/crates-io-auth-action`, pinned by its commit. |
| 2026-11-05 | Publish gate | While Q1 stands, each run does the dry run only. The switch `crates-io` of `[workspace.metadata.podcrates]` in `Cargo.toml` turns the publish on; an agent sets it only when the owner rules so. |
| 2026-11-05 | crates.io names | The owner reserves each name with a 0.0.0 placeholder before its first publish; trusted publishing does not work for another name. `pod-*` is of podcrates; podnet, podssh and podvm use `podnet-*`, `podssh-*`, `podvm-*`. After the first publish of a crate from CI, the owner turns on "Require trusted publishing" for it: keep `publish.yml` working. |
| 2026-11-05 | Shared CI scripts | "A CI script that podnet and podssh share (the zig fetch with its mirrors) lives in `PodUtils/podcrates`. podnet calls it from there; it does not copy it." (podnet's record, `PodUtils/podnet:docs/decisions.md`.) T-046. |
| 2026-11-05 | Relay contract watch | "The weekly watch of the relay contract belongs to `pod-relay` in `PodUtils/podcrates`, not to podnet." (podnet's record.) T-047. |
| 2026-11-05 | Services crate | "they hardcode/measure many relays/third party services, it risks drift and staleness, so we move that code & related script here as a new crate". The crate is `pod-services` (the owner chose the name). T-048, T-049. |

## Calls made without a ruling

Each call stays in force until the owner rules otherwise. `docs/design.md`
has the detail.

| Date | Call | What |
| --- | --- | --- |
| 2026-11-05 | C1 | Edition 2024; `rust-version` = the stable of the day; a pinned nightly when an entry needs it (`docs/design.md`). |
| 2026-11-05 | C2 | rustfmt of podssh (`max_width = 120`, `Max`). |
| 2026-11-05 | C3 | `tracing` for the diagnostics of each crate. |
| 2026-11-05 | C4 | One version for the workspace and one tag for each release (`docs/design.md`). |
| 2026-11-05 | C5 | `pod-todo` keeps no external dependency. It reads the facts of a repository from `TODO/todo.toml`: the milestones, the areas, the categories, the top directories and the root files, the documents that it reads and the ones that it does not read (podnet `READINGS`), the roadmap file, the first GitHub number (now in podnet's `check-github-numbers.py`), and the manifests (a virtual workspace has no root package). A consumer keeps a small member crate (`podnet-todo`, `podssh-todo`) whose `main` calls `pod_todo::main()`, so that `cargo todo` stays the same; podssh's `interop` command stays in its member crate through an extension hook. A foreign id is written `PodUtils/podssh@T-684`, and the checker does not look it up; a foreign file is `owner/repo:path`. Its tests run on the real `TODO/` of podssh and podnet (in `adoption.yml`). |
| 2026-11-05 | C6 | The record of podcrates starts at T-001. Milestones: M0 (foundation), M1 to M4 (the waves), M5 (maintenance: ports, the reports of the consumers). |
| 2026-11-05 | C7 | The full test matrix runs each night and before a release; the static probe runs on each target on each push (`docs/design.md`). |
| 2026-11-05 | C8 | `pod-wg` keeps podnet's code on ring. |
| 2026-11-05 | C9 | `pod-testkit` has russh 0.65 from crates.io, on ring, behind its feature `ssh`, a dev-dependency only. |
| 2026-11-05 | C10 | One QUIC stack (quinn or noq), chosen by measurement in the first entry of M2, before `pod-quic` and the `quic_road/` of `pod-relay`. |
| 2026-11-05 | C11 | A C dependency is allowed when it links static on each target; the static probe proves it. |
| 2026-11-05 | C12 | The forks go into `vendor/` with their patches, their upstream commit and a weekly check (podssh `check-vendored.py`). |
| 2026-11-05 | C13 | snow 0.10, spake2 0.4 and crypto_secretbox 0.1 stay (podnet, "Crypto crates"). |
| 2026-11-05 | C14 | podnet `src/proxy.rs`, `src/config.rs` and `src/race.rs` go into `pod-cloudflare`. |
| 2026-11-05 | C15 | An adoption trial for each crate and each consumer, in CI (`docs/design.md`). |
| 2026-11-05 | C16 | podvm: `pod-relay` (feature `blocking`) and `pod-term` build for each target of podssh. |
| 2026-11-05 | C17 | podcrates adds `pod-core` (`docs/design.md`) and `pod-quic` (the QUIC code that `pod-relay`, `pod-warp` and `pod-cloudflare` share). Both names were free on crates.io on 2026-11-05. Without them, the agreed crates depend on each other in loops. |
| 2026-11-05 | C18 | `pod-tls`: one `Context` that the binary owns holds the kept TLS configurations, the groups and the state of ECH; `ech` turns on `pure`; RUSTSEC-2023-0071 is ignored, as `pod-tls` verifies with RSA public keys only (T-012, Decision). |
| 2026-11-05 | C19 | The library crates are publishable (no `publish = false`); `static-probe` is not. The publish job does not build again (`--no-verify`): the dry run of the same commit built each package, and the token lasts 30 minutes. |

## Questions for the owner

- **Q4.** When does `publish.yml` publish real versions of the `pod-*`
  crates to crates.io? It blocks nothing: the consumers take the git tag
  (Q1), and the dry run of each release keeps the packaging ready. The
  answer sets the switch `crates-io` of `Cargo.toml`. Recommendation: not
  yet, as the ruling of podnet of 2026-11-05 ("Publish no real version
  yet"); publish from the release that the owner names.

## Superseded

| Date | Ruling | Superseded by |
| --- | --- | --- |
| 2026-11-05 | The agreed design: no C in a library crate; Rust 1.89 (1.91 for the crates on smoltcp 0.14). | 2026-11-05, Static, C, Rust; and Toolchain. |
| 2026-11-05 | The agreed design (podnet "podcrates design", podssh Q101): the crates come from crates.io only; the names are reserved there. | 2026-11-05, Delivery (Q1). |
| 2026-11-05 | The agreed design: podnet and podssh do not edit podcrates. | 2026-11-05, Who edits (Q3). |
