#!/bin/sh
# The test of pod-unit that installs a unit of the system on the systemd of
# the host (T-1027) skips as another user and with no systemd. This script
# builds the test binary as the user and runs it as root: with `sudo -n` on
# a runner, as it is where the caller is root. A skip line fails the run,
# and the test must report one pass, so the check cannot pass by a skip.
set -eu
cd "$(dirname "$0")/../.." || exit 71
bin=$(cargo test --locked -p pod-unit --all-features --test system_live --no-run 2>&1 |
    sed -n 's/.*Executable tests\/system_live\.rs (\(.*\))$/\1/p' | tail -1)
[ -n "$bin" ] && [ -x "$bin" ] || { echo "root-unit-tests: no test binary of tests/system_live.rs" >&2; exit 1; }
root=
[ "$(id -u)" -eq 0 ] || root="sudo -n"
out=$($root "$bin" --nocapture 2>&1) || { printf '%s\n' "$out"; exit 1; }
printf '%s\n' "$out"
if printf '%s\n' "$out" | grep -q '^skipping'; then
    echo "root-unit-tests: the test skipped as root" >&2
    exit 1
fi
printf '%s\n' "$out" | grep -q '^test result: ok\. 1 passed' || { echo "root-unit-tests: the test did not run" >&2; exit 1; }
