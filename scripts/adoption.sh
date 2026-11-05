#!/bin/sh
# The adoption trial of CRATE in CONSUMER (docs/design.md, section 8,
# step 7): check out the consumer at adoption/CONSUMER/commit, apply
# adoption/CONSUMER/CRATE.patch, point each git dependency on podcrates to
# the crates of this tree, and run the commands of adoption/CONSUMER/CRATE.run
# in the checkout, each one a step that must exit 0.
#
#   sh scripts/adoption.sh CONSUMER CRATE [WORKDIR]
#
# WORKDIR (default ${TMPDIR:-/tmp}/podcrates-adoption) holds the checkout,
# outside each directory named `target`: a scan of the consumer passes over
# each path under one (podssh's check-repo.py). The patch is the hand-off to
# the consumer session: it names the git tag of podcrates.
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
work=${3:-${TMPDIR:-/tmp}/podcrates-adoption}/$consumer-$crate
# The consumer builds with the toolchain of podcrates, as adoption.yml sets
# it: the checkout is outside this tree and its rust-toolchain.toml.
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
# the patch becomes a path into this tree.
for manifest in $(git ls-files -m -o --exclude-standard | grep 'Cargo.toml$' || true) $(git diff --name-only | grep 'Cargo.toml$' || true); do
    [ -f "$manifest" ] || continue
    sed -i -E "s|^(pod-[a-z-]+) = \\{ git = \"https://github.com/PodUtils/podcrates\", tag = \"v[0-9.]+\"|\\1 = { path = \"$here/crates/\\1\"|" "$manifest"
done
echo "adoption: $consumer at ${commit%"${commit#???????}"} with $crate of $(git -C "$here" rev-parse --short HEAD)"
grep -v '^#' "$dir/$crate.run" | while IFS= read -r step; do
    [ -n "$step" ] || continue
    echo "adoption: $ $step"
    sh -c "$step" || { echo "adoption: FAILED: $step" >&2; exit 1; }
done
echo "adoption: $consumer takes $crate: each step passed"
