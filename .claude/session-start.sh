#!/bin/sh
# The SessionStart hook of Claude Code. The start itself is shared by each
# harness and host: scripts/session-start.sh. No step of it fails the
# session.
root=${CLAUDE_PROJECT_DIR:-"$(dirname -- "$0")/.."}
sh "$root/scripts/session-start.sh"
exit 0
