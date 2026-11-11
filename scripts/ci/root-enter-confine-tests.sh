#!/bin/sh
# The tests of pod-enter's tests/confine.rs on the rungs with a chroot (the
# chroot sequence, and the namespace rung with its tmpfs on /tmp, T-227)
# skip as another user. This script builds the test binary as the user and
# runs it as root: with `sudo -n` on a runner, as it is where the caller is
# root. A skip line fails the run, so the check cannot pass by a skip.
#
#   sh scripts/ci/root-enter-confine-tests.sh
set -eu
cd "$(dirname "$0")/../.." || exit 71
bin=$(cargo test --locked -p pod-enter --all-features --test confine --no-run 2>&1 |
    sed -n 's/.*Executable tests\/confine\.rs (\(.*\))$/\1/p' | tail -1)
[ -n "$bin" ] && [ -x "$bin" ] || { echo "root-enter-confine-tests: no test binary of tests/confine.rs" >&2; exit 1; }
if [ "$(id -u)" = 0 ]; then
    out=$("$bin" 2>&1) || { printf '%s\n' "$out"; exit 1; }
else
    out=$(sudo -n "$bin" 2>&1) || { printf '%s\n' "$out"; exit 1; }
fi
printf '%s\n' "$out"
if printf '%s\n' "$out" | grep -q '^skipping'; then
    echo "root-enter-confine-tests: a test skipped as root" >&2
    exit 1
fi
