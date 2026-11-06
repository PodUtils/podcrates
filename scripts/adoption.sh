#!/bin/sh
# The adoption trial of CRATE in CONSUMER (docs/design.md, section 8,
# step 7): check out the consumer at adoption/CONSUMER/commit, apply
# adoption/CONSUMER/CRATE.patch, point each git dependency on podcrates to
# the crates of this tree, and run the commands of adoption/CONSUMER/CRATE.run
# in the checkout, each one a step that must exit 0.
#
#   sh scripts/adoption.sh CONSUMER CRATE [WORKDIR]
#
# WORKDIR (default the user's cache, podcrates-adoption) holds the checkout.
# It is outside this tree, whose rustfmt.toml and .cargo/config.toml would
# apply to a consumer with none of its own; outside each directory named
# `target`, as a scan of the consumer passes over each path under one
# (podssh's check-repo.py); and outside the temporary directory, as a
# consumer refuses some commands from a binary there (podnet's `autostart`).
# The patch is the hand-off to the consumer session: it names the git tag of
# podcrates.
set -eu
consumer=${1:-}
crate=${2:-}
[ -n "$consumer" ] && [ -n "$crate" ] || { echo "usage: adoption.sh CONSUMER CRATE [WORKDIR]" >&2; exit 64; }
here=$(cd "$(dirname "$0")/.." && pwd)
dir=$here/adoption/$consumer
for f in repo commit "$crate.patch" "$crate.run"; do
    [ -f "$dir/$f" ] || { echo "adoption: $dir/$f is missing" >&2; exit 66; }
done
repo=$(tr -d '\r\n' < "$dir/repo")
commit=$(tr -d '\r\n' < "$dir/commit")
work=${3:-${XDG_CACHE_HOME:-$HOME/.cache}/podcrates-adoption}/$consumer-$crate
# The steps of a .run name this tree as $PODCRATES: its scripts check the
# consumer (scripts/check-consumer-toolchains.py).
PODCRATES=$here
export PODCRATES
# The consumer builds with the toolchain of podcrates, as adoption.yml sets
# it, not with the rust-toolchain.toml of its checkout.
: "${RUSTUP_TOOLCHAIN:=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$here/rust-toolchain.toml")}"
export RUSTUP_TOOLCHAIN
rm -rf "$work"
mkdir -p "$work"
cd "$work"
git init -q
git remote add origin "https://github.com/$repo"
sh "$here/scripts/retry.sh" git fetch -q --depth 1 origin "$commit"
git -c advice.detachedHead=false checkout -q FETCH_HEAD
git apply "$dir/$crate.patch"
# Each `pod-X = { git = "https://github.com/PodUtils/podcrates", ... }` of
# the patch becomes a path into this tree, and a `version` beside it the
# version of this tree: the patch names the version of its tag, which the
# tree has only from the commit of the release on.
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$here/Cargo.toml" | head -n 1)
for manifest in $(git ls-files -m -o --exclude-standard | grep 'Cargo.toml$' || true) $(git diff --name-only | grep 'Cargo.toml$' || true); do
    [ -f "$manifest" ] || continue
    sed -i -E "s|^(pod-[a-z-]+) = \\{ git = \"https://github.com/PodUtils/podcrates\", tag = \"v[0-9.]+\"|\\1 = { path = \"$here/crates/\\1\"|" "$manifest"
    sed -i -E "/^pod-[a-z-]+ = \\{ path = \"/s|, version = \"[0-9.]+\"|, version = \"$version\"|" "$manifest"
done
echo "adoption: $consumer at ${commit%"${commit#???????}"} with $crate of $(git -C "$here" rev-parse --short HEAD)"
grep -v '^#' "$dir/$crate.run" | while IFS= read -r step; do
    [ -n "$step" ] || continue
    echo "adoption: $ $step"
    sh -c "$step" || { echo "adoption: FAILED: $step" >&2; exit 1; }
done
echo "adoption: $consumer takes $crate: each step passed"
