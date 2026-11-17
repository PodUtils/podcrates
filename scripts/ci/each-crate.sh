#!/bin/sh
# Each crate of the workspace builds alone with its default features, as a
# consumer takes it. The steps that use --all-features turn on each
# feature, so a `cfg` of a feature on the wrong item passes them.
#
#   sh scripts/ci/each-crate.sh
set -eu
cd "$(dirname "$0")/../.." || exit 71
crates=$(cargo metadata --locked --no-deps --format-version 1 |
    python3 -I -c 'import json, sys; print("\n".join(sorted(p["name"] for p in json.load(sys.stdin)["packages"])))')
rc=0
for crate in $crates; do
    if ! cargo check --locked --quiet -p "$crate"; then
        echo "each-crate: $crate does not build with its default features"
        rc=1
    fi
done
exit $rc
