#!/bin/sh
# Watch each repository of PodUtils for the messages to podcrates, without
# end: a new issue of PodUtils/podcrates, and an issue of another repository
# whose title holds "(to podcrates)" or that has the label "podcrates", new
# or amended (docs/maintenance.md, "Talk between sessions"). One line for
# each, from scripts/agent/org-issues.py. Run it under the Monitor tool at
# the start of each session, and re-arm it when the monitor expires.
#
#   sh scripts/agent/watch-org.sh [SECONDS] [STATE_DIR]
#
# SECONDS between two polls (default 300). STATE_DIR keeps org-seen.txt and
# a copy of each message (default: .work/agent of this tree, which git does
# not track). The first run records what exists and prints nothing for it.
# The reads go through forge.web.pkgforge.dev (the read-only mirror of the
# GitHub API of AGENTS.md); the n= parameter defeats its cache.
set -u
pause=${1:-300}
here=$(cd "$(dirname "$0")" && pwd)
state=${2:-$here/../../.work/agent}
mkdir -p "$state"
seen=$state/org-seen.txt
api=https://forge.web.pkgforge.dev/github
announce=0
[ -s "$seen" ] && announce=1
n=0
while true; do
    n=$((n + 1))
    repos=$(curl -qfsS --max-time 30 "$api/orgs/PodUtils/repos?per_page=100&n=$n$$" 2> /dev/null |
        python3 -I -c 'import json, sys; [print(r["full_name"]) for r in json.load(sys.stdin)]' 2> /dev/null)
    [ -n "$repos" ] || repos="PodUtils/podcrates PodUtils/podssh PodUtils/podnet PodUtils/podvm"
    for repo in $repos; do
        body=$(curl -qfsS --max-time 30 "$api/repos/$repo/issues?state=all&sort=updated&direction=desc&per_page=50&n=$n$$" 2> /dev/null) || continue
        printf '%s\t%s\n' "$repo" "$(printf '%s' "$body" | tr -d '\n')"
    done | python3 -I "$here/org-issues.py" "$seen" "$announce"
    announce=1
    sleep "$pause"
done
