#!/bin/sh
# Run, as root, the netns tests of pod-hold that skip for a user on a
# GitHub runner (no user namespace there): the namespace with its TUN
# (tests/hold.rs) and the veth wire with a stack on it (tests/veth.rs).
# Root needs no user namespace. The script fails when a test fails or
# prints a line `SKIP:`, so each run proves the wire again.
#
#   sh scripts/root-netns-tests.sh
set -eu
cd "$(dirname "$0")/.."
log=${TMPDIR:-/tmp}/root-netns-tests
mkdir -p "$log"
# Build as this user; run as root. jq reads the paths of the test binaries.
bins=$(cargo test --locked --no-run --message-format=json -p pod-hold --test hold --test veth 2> "$log/build.err" |
    jq -r 'select(.reason == "compiler-artifact" and .profile.test == true) | .executable // empty')
[ -n "$bins" ] || { cat "$log/build.err" >&2; echo "root-netns-tests: no test binary" >&2; exit 1; }
# Root runs the binaries as they are; another user runs them with sudo.
root=
[ "$(id -u)" -eq 0 ] || root=sudo
rc=0
for bin in $bins; do
    name=$(basename "$bin")
    out="$log/$name.out"
    if ! $root "$bin" > "$out" 2>&1; then
        echo "root-netns-tests: $name failed:" >&2
        tail -40 "$out" >&2
        rc=1
        continue
    fi
    cat "$out"
    if grep -iE "^skip:" "$out" > "$log/skips"; then
        echo "root-netns-tests: $name skipped as root:" >&2
        cat "$log/skips" >&2
        rc=1
    fi
done
exit "$rc"
