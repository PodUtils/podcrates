#!/bin/sh
# Poll the open issues of the consumers for a COORD issue to podcrates
# (docs/maintenance.md, "The channel of a consumer session with no
# messages"), and exit when one is new or changed since the last poll.
#
#   sh scripts/agent/watch-coord.sh [SECONDS]
#
# The last state is in .work/coord-seen (not committed). Each poll reads
# the read-only mirror; a poll that fails is tried again at the next turn.
set -u
pause=${1:-300}
here=$(cd "$(dirname "$0")/../.." && pwd)
seen="$here/.work/coord-seen"
mkdir -p "$here/.work"
touch "$seen"
while true; do
    now=""
    for repo in PodUtils/podnet PodUtils/podssh; do
        body=$(curl -qfsS --max-time 30 "https://forge.web.pkgforge.dev/github/repos/$repo/issues?state=open&per_page=50&n=$$$(date +%s)" 2> /dev/null) || continue
        now="$now$(printf '%s' "$body" | python3 -I -c '
import json, sys
repo = sys.argv[1]
for i in json.load(sys.stdin):
    if "pull_request" in i:
        continue
    t = i["title"]
    if t.startswith("COORD-") and "(to podcrates)" in t:
        print(repo + "#" + str(i["number"]) + " " + i["updated_at"] + " " + t)
' "$repo")
"
    done
    new=$(printf '%s' "$now" | grep -v '^$' | grep -vxF -f "$seen" || true)
    if [ -n "$new" ]; then
        printf '%s\n' "$new"
        printf '%s' "$now" | grep -v '^$' > "$seen"
        exit 0
    fi
    sleep "$pause"
done
