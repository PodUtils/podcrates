#!/bin/sh
# The planted defects of the checks: each check that the record trusts must
# fail on a defect planted for it (docs/development.md).
#
#   sh scripts/plant-each.sh --list
#   sh scripts/plant-each.sh [--group G] [ID...]
#
# Each plant is scripts/plants/ID.plant, a file of shell assignments:
#
#   ENTRY=T-NNN       the entry whose check it proves
#   GROUP=name        a group of plants (plants.yml runs each group)
#   EXPECT='regex'    what the failed run must print (grep -E), so that a run
#                     that fails for another reason is not a catch
#   RUN='command'     the check; it must exit nonzero on the plant
#   LIMIT=seconds     optional: the time limit of RUN (default 1800)
#   SWITCH=1          optional: RUN plants the defect itself (a change in
#                     the worktree of the plant, or a --plant-*
#                     option of the check); there is no patch
#
# and, unless SWITCH is set, scripts/plants/ID.patch, a diff that `git apply`
# takes. Each plant runs in a git worktree of HEAD under target/plants/,
# never in the tree itself, so a planted defect cannot reach a commit. The
# builds share the target directory target/plant-build. A plant is caught
# when RUN exits nonzero and printed EXPECT; it is MISSED when RUN passed, or
# failed without EXPECT, and STALE when its patch does not apply: each is a
# failure of this script. Commit before a run: a worktree holds HEAD only.

set -u
cd "$(dirname "$0")/.." || exit 71
root=$(pwd)
dir=scripts/plants
group=
list=0
while [ $# -gt 0 ]; do
    case "$1" in
        --group) shift; group=${1:-} ;;
        --list) list=1 ;;
        -*) echo "plant-each: unknown option $1" >&2; exit 64 ;;
        *) break ;;
    esac
    shift
done
[ -d "$dir" ] || { echo "plant-each: $dir is missing" >&2; exit 71; }
: "${CARGO_BUILD_JOBS:=4}"
export CARGO_BUILD_JOBS
CARGO_TARGET_DIR=$root/target/plant-build
export CARGO_TARGET_DIR

read_plant() {
    ENTRY='' GROUP='' EXPECT='' RUN='' LIMIT=1800 SWITCH=''
    # shellcheck disable=SC1090  # each plant file is the repository's own
    . "$root/$dir/$1.plant"
}

ids=
if [ $# -gt 0 ]; then
    ids=$*
else
    for f in "$dir"/*.plant; do
        [ -f "$f" ] || continue
        id=$(basename "$f" .plant)
        if [ -n "$group" ]; then
            g=$(read_plant "$id"; printf '%s' "$GROUP")
            [ "$g" = "$group" ] || continue
        fi
        ids="$ids $id"
    done
fi
[ -n "$ids" ] || { echo "plant-each: no plant to run" >&2; exit 71; }

if [ "$list" -eq 1 ]; then
    for id in $ids; do
        (read_plant "$id"; printf '%-28s %-10s %s\n' "$id" "$GROUP" "$ENTRY")
    done
    exit 0
fi

failed=0
caught=0
mkdir -p target/plants
for id in $ids; do
    [ -f "$dir/$id.plant" ] || { echo "MISSED $id: no $dir/$id.plant"; failed=$((failed + 1)); continue; }
    read_plant "$id"
    wt=$root/target/plants/$id
    git worktree remove --force "$wt" > /dev/null 2>&1
    rm -rf "$wt"
    if ! git worktree add --quiet --detach "$wt" HEAD > /dev/null 2>&1; then
        echo "STALE $id: no worktree of HEAD"
        failed=$((failed + 1))
        continue
    fi
    if [ -z "$SWITCH" ] && ! git -C "$wt" apply "$root/$dir/$id.patch" 2> "$wt.apply.log"; then
        echo "STALE $id: the patch does not apply: $(head -1 "$wt.apply.log")"
        failed=$((failed + 1))
        git worktree remove --force "$wt" > /dev/null 2>&1
        continue
    fi
    log=$root/target/plants/$id.log
    (cd "$wt" && timeout "$LIMIT" sh -c "$RUN") > "$log" 2>&1
    rc=$?
    if [ "$rc" -ne 0 ] && grep -Eq "$EXPECT" "$log"; then
        echo "caught $id ($ENTRY): exit $rc"
        caught=$((caught + 1))
    else
        echo "MISSED $id ($ENTRY): exit $rc, and the output has no match of: $EXPECT"
        tail -15 "$log" | sed 's/^/    /'
        failed=$((failed + 1))
    fi
    git worktree remove --force "$wt" > /dev/null 2>&1
done
git worktree prune
echo "plant-each: $caught caught, $failed missed or stale"
[ "$failed" -eq 0 ]
