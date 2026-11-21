#!/bin/sh
# The test of pod-unit that makes a Windows service of its own binary
# (T-1026) skips with no administrator. The Windows runners of GitHub run
# each job as an administrator: this script runs the test there, and fails
# on a skip line, or when the test does not report its steps, so the check
# cannot pass by a skip.
set -eu
cd "$(dirname "$0")/../.." || exit 71
out=$(cargo test --locked -p pod-unit --all-features --test service_live 2>&1) || { printf '%s\n' "$out"; exit 1; }
printf '%s\n' "$out"
if printf '%s\n' "$out" | grep -q '^skipping'; then
    echo "windows-service-tests: the test skipped" >&2
    exit 1
fi
printf '%s\n' "$out" | grep -q '^service_live: 3 steps passed' || {
    echo "windows-service-tests: the test did not run its steps" >&2
    exit 1
}
