# Adoption trials

Each directory is a consumer of podcrates: `repo` (the GitHub repository),
`commit` (the commit of the consumer that the trials start from), and for
each crate that the consumer takes:

* `CRATE.patch`: the change of the consumer that takes the crate in place
  of its own module (`git apply` at `commit`). It names the git tag of
  podcrates (and its version, for a publishable crate of the consumer);
  the trial points it to the crates of this tree, with the version of the
  tree. It is the
  hand-off to the consumer session. `Cargo.lock` is not in it: cargo
  writes it again, and the consumer runs `cargo todo remap Cargo.lock`.
* `CRATE.run`: the commands of the trial, one on each line, run in the
  checkout of the consumer. Each must exit 0.

Each trial starts from `commit` alone, with its one patch. A consumer
session applies the patches of its crates in the order of the releases;
only the first patch of a consumer adds podcrates to `allow-git` of its
`deny.toml`.

One patch can take more than one crate, when the crates change the same
files: `pod-dns.patch` takes `pod-dns` and `pod-services` (v0.4.0), as
`pod-dns` reads its default resolvers from `pod-services`, and both take the
place of constants in the same files of each consumer.

`sh scripts/adoption.sh CONSUMER CRATE` runs one trial;
`.github/workflows/adoption.yml` runs each one in CI. When the consumer
moves on, a session raises `commit` and makes the patch again.

When the consumer takes a crate, its trial leaves this directory: the
consumer's own CI runs the crate from then on, and git keeps the patch.
The block of the consumer in `crates/CRATE/SOURCES` gets the line
`taken COMMIT` (or `taken COMMIT PATH...` when the adoption replaced only
some files): `scripts/check-sources.py` then compares those files with
the parent of the adoption, so that a fix that came before it is still
found, and the files that the adoption removed are no change.
Taken: podssh `pod-todo`, `pod-term`, `pod-identity` (podssh `04f4551`)
and `pod-tls` (podssh `6ece2ff`) and `pod-proxy` (podssh `a791553`) and `pod-dns`
with `pod-services` (podssh `72fb94d`); podnet `pod-todo` (podnet `6c210ed`)
and `pod-tls` (podnet `a5894fe`, `a2af228`) and `pod-proxy` (podnet
`380d9a4`) and `pod-dns` with `pod-services` (podnet `bf0c7ca`); podvm `pod-term` (podvm `75ed306`) and `pod-tls` (podvm
`b917f97`) and `pod-proxy` with `blocking` (podvm `b1138c2`) and its streamed
client for the registry (podvm `06f2427`).

A `.run` of podssh that moves a value of a service names the test
`cargo test -p podssh-relay --test default_relay` too: the take of v0.4.0
broke it, and the trial did not run it (PodUtils/podssh#154).

A `.run` file runs the checks of the consumer's repository too (its
record, its file rules, rustfmt of the crates that the patch changes), so
that the patch is what the consumer session can commit as it is.

A `.run` file also builds the binary of the consumer for
`x86_64-unknown-linux-musl` with `cargo zigbuild`, with the features of the
consumer's release (podssh: `RELEASE_FEATURES` of its `release.yml`; the
other features are checked with `cargo check`), and checks it with the
consumer's own `scripts/check-static.sh`. A debug build is enough: the
check reads the ELF headers, and the release profile takes LTO.
`adoption.yml` gives the trials the musl target, zig and cargo-zigbuild; in
a session, `sh scripts/cloud-setup.sh` gives them.

A `.run` file checks each Rust toolchain that the consumer pins in its
workflows and build images against the `rust-version` of podcrates:
`python3 -I "$PODCRATES/scripts/check-consumer-toolchains.py"` (adoption.sh
sets `PODCRATES` to this tree). A pin below it is a fault; a pin that the
script cannot read (a channel, `rust:1-alpine`, Alpine's packaged Rust) is
a note.

What the consumers asked of the next patches:

* podssh: each name of a module that leaves goes too (scripts/gate.sh,
  AGENTS.md, docs/, scripts/), and the citations of TODO/ follow
  (`cargo run -q -p podssh-todo -- check`, `python3 scripts/check-repo.py`).
* podnet: each `pod-*` dependency of a publishable crate (podnet,
  podnet-netns) has `version` beside `git` and `tag`, as cargo-deny counts
  a git dependency with no version as a wildcard; `fuzz/Cargo.lock`
  follows (`cargo update -w` in fuzz/); the `.run` has
  `cargo deny --locked check bans sources`.
* Both: a raise of `rust-version` must be checked against each toolchain
  of the consumer's release workflow, not only against its CI (podnet's
  armv7 job built with Alpine's packaged Rust 1.96.1, below the 1.99 of
  podcrates; podnet `5535ffb` moved it into the pinned image).
