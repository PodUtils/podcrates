#!/bin/sh
# Watch each repository of PodUtils for the messages to ME, without end: a
# new or amended issue of PodUtils/ME, and an issue of another repository
# whose title holds "(to ME)" or "(to all)", or that has the label ME, new or
# amended (docs/maintenance.md, "Talk between sessions"). One line for each,
# from org-issues.py beside this script. Run it under the Monitor tool at
# the start of each session, and re-arm it when the monitor expires. Each
# session of the organization runs it: a consumer takes both files from
# podcrates at a pinned commit (docs/maintenance.md, "Shared CI scripts").
#
#   sh scripts/agent/watch-org.sh [SECONDS] [STATE_DIR] [ME]
#
# SECONDS between two polls (default 90). STATE_DIR keeps org-seen.txt and
# a copy of each message (default: .work/agent of the tree of the script,
# which git does not track). ME is the name of the reader's repository
# (default podcrates). The first run records what exists and prints nothing
# for it.
# The reads go through forge.web.pkgforge.dev (the read-only mirror of the
# GitHub API of AGENTS.md); the n= parameter defeats its cache.
set -u
pause=${1:-90}
here=$(cd "$(dirname "$0")" && pwd)
state=${2:-$here/../../.work/agent}
me=${3:-podcrates}
mkdir -p "$state"
seen=$state/org-seen.txt
api=https://forge.web.pkgforge.dev/github
announce=0
[ -s "$seen" ] && announce=1
n=0
while true; do
    n=$((n + 1))
    repos=$(curl -qfsS --max-time 30 "$api/orgs/PodUtils/repos?per_page=100&n=$n$$" 2> /dev/null |
        python3 -I -c 'import json, sys; [print(r["full_name"]) for r in json.load(sys.stdin)]' 2> /dev/null |
        tr -d '\r')
    # A Python of Windows ends each line with CRLF: a name with a carriage
    # return gives a wrong URL, which curl refuses, so tr takes them out.
    [ -n "$repos" ] || repos="PodUtils/podcrates PodUtils/podssh PodUtils/podnet PodUtils/podvm"
    for repo in $repos; do
        body=$(curl -qfsS --max-time 30 "$api/repos/$repo/issues?state=all&sort=updated&direction=desc&per_page=50&n=$n$$" 2> /dev/null) || continue
        printf '%s\t%s\n' "$repo" "$(printf '%s' "$body" | tr -d '\n')"
    done | python3 -I "$here/org-issues.py" "$seen" "$announce" "$me"
    announce=1
    sleep "$pause"
done
