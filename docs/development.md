# Development

Rules for the code and the tests of podcrates. `AGENTS.md` has the rules
of each session; `docs/design.md` has the rules of each crate and the
procedure for one crate; `docs/maintenance.md` has the procedures.

## Checks before a push

Run these, and push only when each exits 0:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo todo check
```

CI runs the same on each push, and more (`docs/design.md`, section 9).

## Toolchain

* `rust-toolchain.toml` pins the stable Rust of the day. Raise it with
  `rust-version` of `Cargo.toml` when a new stable comes out.
* Edition 2024. rustfmt with `max_width = 120`.
* A static build: `sh scripts/cloud-setup.sh`, then
  `cargo zigbuild --release --target x86_64-unknown-linux-musl`.
* Set `CARGO_BUILD_JOBS=4`. Run one cargo build at a time.

## File size

A Rust file has 500 lines or fewer. There is no allowlist: split a longer
file. Do not remove comments to make a file shorter.

## Rules for tests

* **The tests of the source come with the code.** Each test keeps its
  name. A test that fails after a move shows a defect of the move: repair
  the code, never the test.
* **A test for each fix.** A fix of a defect comes with a test that fails
  on the code before the fix. Run the new test on the old code one time,
  and write in the commit message that it fails there.
* **A planted defect for each check.** A new check (a test, a CI step, a
  scan) must fail on a planted defect before it is trusted.
* **No sleep as a proof.** A test waits for an event (a channel, a
  socket) with a time limit. For time limits of the code, use
  `#[tokio::test(start_paused = true)]` or make the limit a parameter.
* **Loopback only.** A unit or integration test uses loopback servers
  (`127.0.0.1:0`, a Unix socket in a temporary directory). A test of a
  live service is `#[ignore]` and runs in a live workflow.
* **Skip on a host with no loopback.** A test that binds starts with the
  `need_loopback!` guard of `pod-testkit`; a test that resolves
  `localhost` starts with `need_localhost!`.
* **No shared state.** A test makes its own temporary directory, and
  does not set a process variable.
* **No secret in a test.** A test token or password is a made-up value;
  list it in `.github/secrets-allow.txt` with the reason.

## Random failures

* A test that fails one time in many runs has a defect: in the test, or
  in the code. Do not run CI again until it is green, and do not mark the
  test ignored.
* Reproduce it in a loop
  (`for i in $(seq 50); do cargo test NAME -- --exact || break; done`),
  find the race, fix the cause, and give the loop count in the commit
  message.

## Commits

* The subject is `area: text`, for example `pod-tls: the pins of podnet`.
  The area is the crate, or `record`, `ci`, `docs`, `scripts`.
* The body says what changed and why, and names the tests and their
  counts.
* A commit that does the work of an entry names it (`T-NNN`).
* Write in ASD-STE100 Simplified Technical English.
