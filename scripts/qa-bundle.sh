#!/bin/sh
# Give an outside tester a candidate binary, its manual, the licence and
# their checksums: the bundle of a blind QA pass (docs/hosted-sessions.md,
# "Blind QA"). The binary keeps its name (podlab, pod-release); its manual
# comes from the binary itself (`NAME man --no-pager`, pod-manual).
#
#   sh scripts/qa-bundle.sh BINARY OUTPUT_DIR
#
# OUTPUT_DIR must not exist; its parent must. The tester runs
# `sha256sum -c SHA256SUMS` in it. Exit 0 with the path of the bundle, 1
# when the binary gives no manual, 2 for a usage error.
set -eu
umask 022

if [ "$#" -ne 2 ]; then
    echo "usage: qa-bundle.sh BINARY OUTPUT_DIR" >&2
    exit 2
fi
if [ ! -f "$1" ] || [ ! -x "$1" ]; then
    printf 'qa-bundle: %s is not an executable file\n' "$1" >&2
    exit 2
fi
root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
name=$(basename -- "$1")
parent=$(CDPATH='' cd -- "$(dirname -- "$2")" && pwd -P) || {
    printf 'qa-bundle: the parent of %s does not exist\n' "$2" >&2
    exit 2
}
output=$parent/$(basename -- "$2")
if [ -e "$output" ] || [ -L "$output" ]; then
    printf 'qa-bundle: %s already exists\n' "$output" >&2
    exit 2
fi

stage=$(mktemp -d "$parent/.qa-bundle.XXXXXXXX")
trap 'rm -rf -- "$stage"' EXIT
trap 'exit 1' HUP INT TERM
cp -- "$1" "$stage/$name"
chmod 0755 "$stage/$name"
cp -- "$root/LICENSE" "$stage/LICENSE"
# A bound of time: a candidate that hangs gives no manual.
if command -v timeout > /dev/null 2>&1; then
    limit="timeout 30"
else
    limit=
fi
# shellcheck disable=SC2086
if ! TERM=dumb $limit "$stage/$name" man --no-pager < /dev/null > "$stage/$name.man" ||
    [ ! -s "$stage/$name.man" ]; then
    printf 'qa-bundle: %s gave no manual (%s man --no-pager)\n' "$1" "$name" >&2
    exit 1
fi
(cd "$stage" && sha256sum "$name" "$name.man" LICENSE > SHA256SUMS)
chmod 0755 "$stage"
mv -- "$stage" "$output"
trap - EXIT
printf 'qa-bundle: %s\n' "$output"
