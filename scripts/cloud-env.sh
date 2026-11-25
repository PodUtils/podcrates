# The environment of a shell of a session (docs/hosted-sessions.md). Load
# it in each new shell, from any directory:
#
#   . ./scripts/cloud-env.sh
#
# It puts the tools of scripts/cloud-setup.sh on PATH ($HOME/.local/bin and
# the bin of CARGO_HOME), and gives cargo the limits of AGENTS.md, section
# 7, and docs/maintenance.md, "Rules of the work": 4 jobs, and no
# incremental data (it fills the disk while agents build). A value that is
# set already stays. The Claude Code hook (.claude/session-start.sh) loads
# it for each shell command of the session through CLAUDE_ENV_FILE.
# shellcheck shell=sh
case ":$PATH:" in
    *":${CARGO_HOME:-$HOME/.cargo}/bin:"*) ;;
    *) PATH=${CARGO_HOME:-$HOME/.cargo}/bin:$PATH ;;
esac
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) PATH=$HOME/.local/bin:$PATH ;;
esac
: "${CARGO_BUILD_JOBS:=4}"
: "${CARGO_INCREMENTAL:=0}"
export PATH CARGO_BUILD_JOBS CARGO_INCREMENTAL
