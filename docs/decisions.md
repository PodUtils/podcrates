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
| 2026-11-05 | Who edits | One session in `PodUtils/podcrates` builds the crates. The sessions of podssh and podnet do not edit podcrates; the session of podcrates does not edit podssh, podnet or podvm. |
| 2026-11-05 | The crates | The list agreed by the sessions of podssh and podnet: 26 crates in four waves, and 8 names held with no crate (`pod-wireguard`, `pod-quick`, `pod-udp`, `pod-mailbox`, `pod-reverse`, `podcrates`, `podutils`, `pod-utils`). The WARP and Cloudflare adapters of podnet move here as `pod-warp` and `pod-cloudflare`; `podnet-relay` merges into `pod-relay`. `docs/plan.md`, section 5, has the list. |
| 2026-11-05 | Static, C, Rust | "We use whatever is necessary, including C, zig, modern Rust, no backwards compat. Our users and clients do not care about building the tool, they care about downloading and using it. The only rule is: everything must be statically compiled." It supersedes the rules "no C" and "MSRV 1.89" of the agreed design. |
| 2026-11-05 | Compatibility | No compatibility with released binaries for now. A name or a format of the wire (AppIDs, variables, the words of the relay contract) can change. No test against a released binary. (podssh, 2026-11-05.) |
| 2026-11-05 | Capabilities | No feature or capability of the source code may be lost. Compatibility for users is not required. (podnet, "Regressions".) |
| 2026-11-05 | License | 0BSD. Code from podssh, podnet (cfrs), `talaria0101/cfrs` and `nemo-010/cfrs` is 0BSD and needs no credit. |
| 2026-11-05 | Git | Work on `main` only. Push no other branch. Parallel agents use local worktree branches that are not pushed. Identity `Azathothas <ajamx101@gmail.com>`. No agent attribution in commits, pull requests, comments or files. |
| 2026-11-05 | Writing | ASD-STE100 for each text. Technical content only, no history. |
| 2026-11-05 | Crypto crates | `snow` 0.10, `spake2` 0.4 and `crypto_secretbox` 0.1 are accepted. (podnet, "Crypto crates".) |
| 2026-11-05 | Issues | GitHub issues are an inbox. Each issue becomes record entries. Close each issue with evidence (commits, CI runs, test output). |
| 2026-11-05 | Dependabot | Apply each judged update as an own commit on `main` (checks green). Close the pull request with a link to the commit. Do not merge the Dependabot branch. |

## Calls made without a ruling

`docs/plan.md`, section 13, lists the calls C1 to C15 of the plan. They
move here at step M0.

## Questions for the owner

`docs/plan.md`, section 12: Q1 (git tag or crates.io) and Q2 (500 or 800
lines for each source file). Both block step M0.

## Superseded

| Date | Ruling | Superseded by |
| --- | --- | --- |
| 2026-11-05 | The agreed design: no C in a library crate; Rust 1.89 (1.91 for the crates on smoltcp 0.14). | 2026-11-05, Static, C, Rust. |
