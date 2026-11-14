#!/bin/sh
# The tests of pod-enter's tests/confine.rs on the rungs with a chroot (the
# chroot sequence, and the namespace rung with its tmpfs on /tmp, T-227,
# and its overlay root, T-486) skip as another user. podvm's acceptance
# test of the overlay root (tests/overlay_root.rs) skips as another user
# and with no static busybox (busybox-static gives /bin/busybox). This
# script builds the test binaries as the user and runs them as root: with
# `sudo -n` on a runner, as it is where the caller is root. A skip line
# fails the run, so the check cannot pass by a skip.
#
#   sh scripts/ci/root-enter-confine-tests.sh
set -eu
cd "$(dirname "$0")/../.." || exit 71
as_root() {
    if [ "$(id -u)" = 0 ]; then "$@"; else sudo -n "$@"; fi
}
for t in confine overlay_root; do
    bin=$(cargo test --locked -p pod-enter --all-features --test "$t" --no-run 2>&1 |
        sed -n "s/.*Executable tests\/$t\.rs (\(.*\))\$/\1/p" | tail -1)
    [ -n "$bin" ] && [ -x "$bin" ] || { echo "root-enter-confine-tests: no test binary of tests/$t.rs" >&2; exit 1; }
    out=$(as_root "$bin" --nocapture 2>&1) || { printf '%s\n' "$out"; exit 1; }
    printf '%s\n' "$out"
    if printf '%s\n' "$out" | grep -q '^skipping\|^SKIP:'; then
        echo "root-enter-confine-tests: a test of tests/$t.rs skipped as root" >&2
        exit 1
    fi
done
