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
| 2026-11-05 | The crates | The list agreed by the sessions of podssh and podnet: 26 crates in four waves, and 8 names held with no crate (`pod-wireguard`, `pod-quick`, `pod-udp`, `pod-mailbox`, `pod-reverse`, `podcrates`, `podutils`, `pod-utils`). The WARP and Cloudflare adapters of podnet move here as `pod-warp` and `pod-cloudflare`; `podnet-relay` merges into `pod-relay`. `docs/plan.md`, section 6, has the list; call C17 of the plan adds `pod-core` and `pod-quic`. |
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

## Calls made without a ruling

`docs/plan.md`, section 14, lists the calls C1 to C17 of the plan. They
move here at step M0.

## Questions for the owner

None now. Write each new question here, with the part of the work that
it blocks and a recommendation.

## Superseded

| Date | Ruling | Superseded by |
| --- | --- | --- |
| 2026-11-05 | The agreed design: no C in a library crate; Rust 1.89 (1.91 for the crates on smoltcp 0.14). | 2026-11-05, Static, C, Rust; and Toolchain. |
| 2026-11-05 | The agreed design (podnet "podcrates design", podssh Q101): the crates come from crates.io only; the names are reserved there. | 2026-11-05, Delivery (Q1). |
| 2026-11-05 | The agreed design: podnet and podssh do not edit podcrates. | 2026-11-05, Who edits (Q3). |
