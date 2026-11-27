#!/bin/sh
# The start of each session, for each harness and host: the git identity
# of this repository, the hooks path, the merge driver of the record, the
# session on main, and the two watchers of the owner's ruling "Watchers"
# (AGENTS.md, section 8; docs/hosted-sessions.md). The Claude Code hook
# (.claude/session-start.sh) runs it; another harness or a person runs it
# from the checkout:
#
#   sh scripts/session-start.sh [--no-watchers]
#   sh scripts/session-start.sh --run-watcher watch|runs
#
# It changes no global setting. It drops no commit: an existing main is
# checked out as it is and only fast-forwarded to origin/main; a new main
# starts at origin/main; a tree with changes, or a branch with commits that
# origin/main does not have, stays where it is. A worktree under
# .claude/worktrees/ keeps its branch and starts no watcher: the lead
# checkout owns the watchers. Exit 0 when both watchers run, 1 when one did
# not start.
#
# A host that stops the background processes of a turn (Gemini /
# Antigravity Cloud) keeps a watcher only as a daemon task of the harness.
# There, --no-watchers does the start with no watcher, and each daemon task
# runs --run-watcher NAME: the watcher in the foreground, under its lock,
# with its log (exit 0 at once when it runs already). With no flag, the
# start skips the watchers on such a host and prints the two commands.
# PODCRATES_NO_WATCHERS=1 skips them on each host; 0 starts them on each
# host.
set -u
usage="usage: sh scripts/session-start.sh [--no-watchers | --run-watcher watch|runs]"
watchers=${PODCRATES_NO_WATCHERS:-auto}
run=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --no-watchers) watchers=1 ;;
        --run-watcher)
            case "${2:-}" in
                watch | runs) run=$2 ;;
                *) echo "$usage" >&2 && exit 64 ;;
            esac
            shift
            ;;
        *) echo "$usage" >&2 && exit 64 ;;
    esac
    shift
done
case "$watchers" in
    auto | 0 | 1) ;;
    *) echo "session-start: PODCRATES_NO_WATCHERS is auto, 0 or 1" >&2 && exit 64 ;;
esac
root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P) || exit 1
cd "$root" || exit 1
git rev-parse --is-inside-work-tree > /dev/null 2>&1 || exit 0

# No timeout(1) on some hosts: then the command runs with no limit.
bounded() {
    if command -v timeout > /dev/null 2>&1; then
        timeout "$@"
    else
        shift
        "$@"
    fi
}

# The command line of a process: /proc on Linux, else ps.
cmdline() {
    if [ -r "/proc/$1/cmdline" ]; then
        tr '\0' ' ' < "/proc/$1/cmdline" | sed 's/ $//'
    else
        ps -o args= -p "$1" 2> /dev/null
    fi
}

# A watcher runs when it holds its lock .work/NAME.lock (flock), or when a
# process has its exact command line and, where /proc tells it, this
# checkout as its directory (a host with no flock, or a watcher that a
# person started by hand with the same command).
running() {
    lock=.work/${1%.py}.lock
    if command -v flock > /dev/null 2>&1 && [ -e "$lock" ] && ! flock -n "$lock" true 2> /dev/null; then
        return 0
    fi
    want="python3 -I scripts/agent/$*"
    pidfile=.work/${1%.py}.pid
    pids=$(cat "$pidfile" 2> /dev/null)
    if command -v pgrep > /dev/null 2>&1; then
        pids="$pids $(pgrep -f -x "$want" 2> /dev/null)"
    fi
    for pid in $pids; do
        [ "$(cmdline "$pid")" = "$want" ] || continue
        if [ -e "/proc/$pid/cwd" ] && [ "$(readlink "/proc/$pid/cwd")" != "$root" ]; then
            continue
        fi
        echo "$pid" > "$pidfile"
        return 0
    done
    return 1
}

git config --local user.name Azathothas
git config --local user.email ajamx101@gmail.com
# The signing key of a cloud session belongs to another identity, so a
# signature of this identity never shows as verified on GitHub. Off, the
# stop hook of the session does not ask each turn to change the identity.
git config --local commit.gpgsign false
# The commit-msg hook refuses agent attribution lines (AGENTS.md, section 3)
# in each harness; the pre-push hook refuses a tree that check.sh did not
# pass (section 11).
git config --local core.hooksPath .githooks
# The merge of the branch of an agent merges the rows of TODO/INDEX.md and
# derives the counts again (.gitattributes, scripts/agent/merge_todo.py).
git config --local merge.podcrates-todo.name "the rows and the counts of the record"
git config --local merge.podcrates-todo.driver "python3 -I scripts/agent/merge_todo.py %O %A %B %P"

git_dir=$(cd "$(git rev-parse --git-dir)" && pwd -P)
common_dir=$(cd "$(git rev-parse --git-common-dir)" && pwd -P)
if [ "$git_dir" != "$common_dir" ]; then
    echo "session-start: a worktree keeps its branch; the lead checkout runs the watchers"
    exit 0
fi

# The arguments of each watcher, the same in each mode: running() finds a
# watcher by this command line.
watcher_args() {
    case "$1" in
        watch) echo "watch.py podcrates --state .work/agent" ;;
        runs) echo "runs.py --repo PodUtils/podcrates" ;;
    esac
}

# --run-watcher NAME: the watcher in the foreground of a daemon task. With
# exec, the task is the watcher (flock -F does not fork), so the task ends
# when the watcher ends and stops it when the host stops the task.
if [ -n "$run" ]; then
    umask 077
    mkdir -p .work || exit 1
    # shellcheck disable=SC2046
    set -- $(watcher_args "$run")
    log=.work/$run.log
    if running "$@"; then
        echo "session-start: $1 runs already; its log is $log"
        exit 0
    fi
    if ! bounded 60 python3 -I "scripts/agent/$1" --self-test > ".work/$run-self-test.log" 2>&1 < /dev/null; then
        echo "session-start: $1 failed its self-test (.work/$run-self-test.log); it was not started" >&2
        exit 1
    fi
    echo "session-start: $1 runs in the foreground; its log is $log"
    echo "$$" > ".work/$run.pid"
    script=scripts/agent/$1
    shift
    if command -v flock > /dev/null 2>&1 && flock --help 2>&1 | grep -q -- --no-fork; then
        exec flock -n -F ".work/$run.lock" python3 -I "$script" "$@" >> "$log" 2>&1 < /dev/null
    elif command -v flock > /dev/null 2>&1; then
        exec flock -n ".work/$run.lock" python3 -I "$script" "$@" >> "$log" 2>&1 < /dev/null
    fi
    exec python3 -I "$script" "$@" >> "$log" 2>&1 < /dev/null
fi

# No prompt for credentials and a limit of time: a host with no network or
# no access to origin still starts. The explicit refspec also updates
# origin/main in a clone of another single branch.
upstream=refs/remotes/origin/main
GIT_TERMINAL_PROMPT=0 bounded 60 git fetch -q origin "+refs/heads/main:$upstream" < /dev/null 2> /dev/null ||
    echo "session-start: git fetch of origin main failed; main stays as it is" >&2
cur=$(git symbolic-ref --short -q HEAD)
if [ "$cur" != main ]; then
    if [ -n "$(git status --porcelain 2> /dev/null)" ]; then
        echo "session-start: the tree has changes on ${cur:-a detached HEAD}; it stays there" >&2
    elif git rev-parse -q --verify "$upstream" > /dev/null &&
        ! git merge-base --is-ancestor HEAD "$upstream" 2> /dev/null; then
        echo "session-start: ${cur:-a detached HEAD} has commits that origin/main does not have; it stays there" >&2
    elif git rev-parse -q --verify refs/heads/main > /dev/null; then
        git checkout -q main 2> /dev/null || echo "session-start: main could not be checked out" >&2
    elif git rev-parse -q --verify "$upstream" > /dev/null; then
        git checkout -q -b main --track origin/main 2> /dev/null ||
            echo "session-start: main could not be made from origin/main" >&2
    fi
fi
# A main that is behind origin/main moves forward; a main with commits of
# its own stays as it is, and the session merges by hand.
if [ "$(git symbolic-ref --short -q HEAD)" = main ] &&
    git rev-parse -q --verify "$upstream" > /dev/null; then
    git merge -q --ff-only "$upstream" 2> /dev/null ||
        echo "session-start: main and origin/main differ; nothing was moved" >&2
fi

# The logs and the state of the watchers are for this user alone.
umask 077
mkdir -p .work || exit 1
# Two starts at the same time start each watcher once.
if command -v flock > /dev/null 2>&1; then
    exec 9> .work/session-start.lock
    flock -w 30 9 || {
        echo "session-start: .work/session-start.lock is held; no watcher was started" >&2
        exit 1
    }
fi

# start NAME ARGS...: scripts/agent/NAME ARGS..., detached from the session
# and with its log .work/NAME.log, unless it runs already. With flock, the
# watcher holds .work/NAME.lock while it runs.
start() {
    name=${1%.py}
    log=.work/$name.log
    if running "$@"; then
        echo "session-start: $1 runs already; its log is $log"
        return 0
    fi
    lock=
    if command -v flock > /dev/null 2>&1; then
        lock="flock -n .work/$name.lock"
    fi
    script=scripts/agent/$1
    shift
    # $lock is empty or three words: it is not quoted on purpose.
    if command -v setsid > /dev/null 2>&1; then
        # shellcheck disable=SC2086
        setsid $lock python3 -I "$script" "$@" >> "$log" 2>&1 < /dev/null 9>&- &
    else
        # shellcheck disable=SC2086
        nohup $lock python3 -I "$script" "$@" >> "$log" 2>&1 < /dev/null 9>&- &
    fi
    echo "$!" > ".work/$name.pid"
    set -- "$name.py" "$@"
    for _ in 1 2 3 4 5; do
        sleep 1
        if running "$@"; then
            echo "session-start: $1 runs; its log is $log"
            return 0
        fi
    done
    echo "session-start: $1 did not start; read $log and docs/hosted-sessions.md, \"The watchers\"" >&2
    return 1
}

if ! bounded 60 python3 -I scripts/agent/watch.py --self-test > .work/watchers-self-test.log 2>&1 < /dev/null ||
    ! bounded 60 python3 -I scripts/agent/runs.py --self-test >> .work/watchers-self-test.log 2>&1 < /dev/null; then
    echo "session-start: a watcher failed its self-test (.work/watchers-self-test.log); no watcher was started" >&2
    exit 1
fi
status=0
# A host of Gemini / Antigravity Cloud sets one of these variables.
if [ "$watchers" = auto ] && [ -n "${ANTIGRAVITY_APP_DATA_DIR:-}${JULES_SESSION_ID:-}" ]; then
    watchers=1
fi
if [ "$watchers" = 1 ]; then
    echo "session-start: no watcher was started; run each one as a daemon task of the harness:"
    echo "session-start:   sh scripts/session-start.sh --run-watcher watch"
    echo "session-start:   sh scripts/session-start.sh --run-watcher runs"
else
    # shellcheck disable=SC2046
    start $(watcher_args watch) || status=1
    # shellcheck disable=SC2046
    start $(watcher_args runs) || status=1
fi
# The host: the free disk (a full check needs 22 GB, AGENTS.md, section 7)
# and the tools of the static builds.
free=$(df -Pk . 2> /dev/null | awk 'NR == 2 { print int($4 / 1048576) }')
if [ -n "$free" ] && [ "$free" -lt 25 ]; then
    echo "session-start: $free GB free on the disk of the checkout; a full check needs 22 GB (docs/hosted-sessions.md, \"Disk and memory\")" >&2
fi
(. ./scripts/cloud-env.sh && command -v cargo > /dev/null && command -v zig > /dev/null) ||
    echo "session-start: cargo or zig is missing; run sh scripts/cloud-setup.sh" >&2
echo "session-start: read .work/watch.log and .work/runs.log after each step; load the toolchain in each shell with . ./scripts/cloud-env.sh; then read docs/HANDOFF.md"
exit "$status"
