#!/bin/sh
# Download zig for x86_64 Linux. ziglang.org asks CI not to hard-code it,
# so the file comes from the community mirrors of scripts/zig-mirrors.txt
# in a random order, then from ziglang.org last. Each download is checked
# against the pinned SHA-256; a mirror that serves other bytes is passed
# over. podnet and podssh call this script from podcrates at a pinned commit
# (docs/maintenance.md, "Shared CI scripts").
#
#   sh scripts/fetch-zig.sh VERSION SHA256 OUT [MIRRORS]
#
# ZIG_SOURCE names the project in each request to a mirror (default
# podcrates), as the mirrors ask. ZIG_ORIGIN replaces the download directory
# of ziglang.org (a plant: a name that does not resolve). It needs a POSIX
# shell, curl or wget, sha256sum, awk, sort and cut.
set -u

version=${1:-}
sum=${2:-}
out=${3:-}
mirrors=${4:-$(dirname "$0")/zig-mirrors.txt}
origin=${ZIG_ORIGIN:-https://ziglang.org/download}
source=${ZIG_SOURCE:-podcrates}
if [ -z "$version" ] || [ -z "$sum" ] || [ -z "$out" ] || [ ! -f "$mirrors" ]; then
    echo "usage: fetch-zig.sh VERSION SHA256 OUT [MIRRORS] (got '$*')" >&2
    exit 64
fi
file="zig-x86_64-linux-$version.tar.xz"

# One URL: the file with the pinned bytes, or nothing.
take() {
    rm -f "$out"
    if command -v curl > /dev/null 2>&1; then
        curl -qfsSL --max-time 120 -o "$out" "$1" 2> /dev/null
    else
        wget -q -T 120 -O "$out" "$1" 2> /dev/null
    fi || {
        echo "fetch-zig: $2: no file"
        return 1
    }
    got=$(sha256sum "$out" | cut -d' ' -f1)
    if [ "$got" != "$sum" ]; then
        echo "fetch-zig: $2: other bytes (SHA-256 $got), passed over"
        rm -f "$out"
        return 1
    fi
    echo "fetch-zig: $file from $2, SHA-256 $sum"
}

order=$(awk 'BEGIN { srand() } !/^#/ && NF { print rand() "\t" $1 }' "$mirrors" | sort -n | cut -f2-)
for mirror in $order; do
    take "$mirror/$file?source=$source" "$mirror" && exit 0
done
take "$origin/$version/$file" "$origin" && exit 0
echo "fetch-zig: no mirror and not $origin served $file with SHA-256 $sum" >&2
exit 1
