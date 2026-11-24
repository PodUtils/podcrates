#!/bin/sh
# Run the tests of the workspace with a limit of real time for each test
# binary and for the doc tests, and go on after a binary that hangs. cargo
# calls scripts/ci/limit.sh as the runner of each test binary, so the
# filters, the environment and the working directory of each test stay as
# cargo sets them. With --no-fail-fast a binary that hangs or fails does not
# hide the later ones, and the exit status is not 0 when one failed.
#
#   sh scripts/ci/test-limited.sh BINARY_SECONDS DOC_SECONDS [CARGO TEST ARGUMENT...]
#
# The arguments go to `cargo test` twice: with --tests (each binary, under
# its own limit) and with --doc (one limit for the whole run, because
# rustdoc runs the doc tests).
set -u
[ "$#" -ge 2 ] || { echo "usage: test-limited.sh BINARY_SECONDS DOC_SECONDS [ARGUMENT...]" >&2; exit 2; }
binary=$1
doc=$2
shift 2

here=$(cd "$(dirname "$0")" && pwd)
# cargo runs the runner with the package as the working directory, and on
# Windows as a native program: give it a path in the form of that system.
if command -v cygpath > /dev/null 2>&1; then
    here=$(cygpath -m "$here")
fi
host=$(rustc -vV | sed -n 's/^host: //p')
[ -n "$host" ] || { echo "test-limited.sh: no host triple from rustc" >&2; exit 1; }
var=CARGO_TARGET_$(printf %s "$host" | tr 'a-z.-' 'A-Z__')_RUNNER

failed=0
echo "== test binaries, limit $binary s each"
env "$var=sh $here/limit.sh $binary" cargo test "$@" --tests || failed=$((failed + 1))
echo "== doc tests, limit $doc s"
sh "$here/limit.sh" "$doc" cargo test "$@" --doc || failed=$((failed + 1))
echo "test runs that failed: $failed"
test "$failed" -eq 0
