#!/bin/sh
# The SessionStart hook of Claude Code. The start itself is shared by each
# harness and host: scripts/session-start.sh. No step of it fails the
# session: a failure prints how to repair it.
root=${CLAUDE_PROJECT_DIR:-"$(dirname -- "$0")/.."}
root=$(CDPATH='' cd -- "$root" && pwd -P) || exit 0
if ! sh "$root/scripts/session-start.sh"; then
    echo "session-start: run sh scripts/session-start.sh in the checkout; see docs/hosted-sessions.md" >&2
fi
# Each shell command of the session gets the toolchain and the cargo limits
# of scripts/cloud-env.sh.
if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
    quoted=$(printf '%s' "$root" | sed "s/'/'\\\\''/g")
    printf ". '%s/scripts/cloud-env.sh'\n" "$quoted" >> "$CLAUDE_ENV_FILE"
fi
exit 0
