#!/bin/sh
# The tests of pod-enter's tests/confine.rs on the rungs with a chroot (the
# chroot sequence, and the namespace rung with its tmpfs on /tmp, T-227,
# and its overlay root, T-486) skip as another user. podvm's acceptance
# test of the overlay root (tests/overlay_root.rs) skips as another user
# and with no static busybox (busybox-static gives /bin/busybox), and so
# does the test of the host name on the namespace rung (tests/hostname.rs,
# T-669), and so does the test of a refused exec on the namespace rung
# (tests/exec_refused.rs, T-1080), and so does the path of host root of the
# locks of the mounts of the namespace rung (tests/locked.rs, T-1560). This
# script builds the test binaries as the user and runs them as root: with
# `sudo -n` on a runner, as it is where the caller is root. Each binary runs
# from a directory of mode 755, so that a test that starts its binary again
# as uid 65534 reaches it. A skip line fails the run, so the check cannot
# pass by a skip.
#
#   sh scripts/ci/root-enter-confine-tests.sh
set -eu
cd "$(dirname "$0")/../.." || exit 71
as_root() {
    if [ "$(id -u)" = 0 ]; then "$@"; else sudo -n "$@"; fi
}
bins=$(mktemp -d)
trap 'rm -rf "$bins"' EXIT
chmod 755 "$bins"
for t in confine overlay_root hostname exec_refused locked; do
    bin=$(cargo test --locked -p pod-enter --all-features --test "$t" --no-run 2>&1 |
        sed -n "s/.*Executable tests\/$t\.rs (\(.*\))\$/\1/p" | tail -1)
    [ -n "$bin" ] && [ -x "$bin" ] || { echo "root-enter-confine-tests: no test binary of tests/$t.rs" >&2; exit 1; }
    cp "$bin" "$bins/$t"
    chmod 755 "$bins/$t"
    out=$(as_root "$bins/$t" --nocapture 2>&1) || { printf '%s\n' "$out"; exit 1; }
    printf '%s\n' "$out"
    if printf '%s\n' "$out" | grep -q '^skipping\|^SKIP:'; then
        echo "root-enter-confine-tests: a test of tests/$t.rs skipped as root" >&2
        exit 1
    fi
done
