#!/bin/sh
# The tests of pod-confine that need root (the drop of the capabilities of
# a root payload, T-221) skip as another user. This script builds the test
# binary as the user and runs it as root: with `sudo -n` on a runner, as it
# is where the caller is root. A skip line fails the run, so the check
# cannot pass by a skip.
set -eu
cd "$(dirname "$0")/../.." || exit 71
bin=$(cargo test --locked -p pod-confine --all-features --test apply --no-run 2>&1 |
    sed -n 's/.*Executable tests\/apply\.rs (\(.*\))$/\1/p' | tail -1)
[ -n "$bin" ] && [ -x "$bin" ] || { echo "root-confine-tests: no test binary of tests/apply.rs" >&2; exit 1; }
filter=${1:-a_confined_root}
if [ "$(id -u)" = 0 ]; then
    out=$("$bin" "$filter" --nocapture 2>&1) || { printf '%s\n' "$out"; exit 1; }
else
    out=$(sudo -n "$bin" "$filter" --nocapture 2>&1) || { printf '%s\n' "$out"; exit 1; }
fi
printf '%s\n' "$out"
if printf '%s\n' "$out" | grep -q '^skipping'; then
    echo "root-confine-tests: a test skipped as root" >&2
    exit 1
fi
