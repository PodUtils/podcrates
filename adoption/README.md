# Adoption trials

Each directory is a consumer of podcrates: `repo` (the GitHub repository),
`commit` (the commit of the consumer that the trials start from), and for
each crate that the consumer takes:

* `CRATE.patch`: the change of the consumer that takes the crate in place
  of its own module (`git apply` at `commit`). It names the git tag of
  podcrates; the trial points it to the crates of this tree. It is the
  hand-off to the consumer session. `Cargo.lock` is not in it: cargo
  writes it again, and the consumer runs `cargo todo remap Cargo.lock`.
* `CRATE.run`: the commands of the trial, one on each line, run in the
  checkout of the consumer. Each must exit 0.

Each trial starts from `commit` alone, with its one patch. A consumer
session applies the patches of its crates in the order of the releases;
only the first patch of a consumer adds podcrates to `allow-git` of its
`deny.toml`.

`sh scripts/adoption.sh CONSUMER CRATE` runs one trial;
`.github/workflows/adoption.yml` runs each one in CI. When the consumer
moves on, a session raises `commit` and makes the patch again.
