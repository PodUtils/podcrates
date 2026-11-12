#!/bin/sh
# Put one program of a release archive in DIR, checked by the SHA-256 of
# the archive (T-355, call C152): a release binary is a download of some
# seconds, where `cargo install` is a build of some minutes. A file
# ARCHIVE that is already in DIR (a cache) is checked again and used.
#
#   sh scripts/ci/fetch-tool.sh URL SHA256 MEMBER DIR
#
# MEMBER is the path of the program in the archive (.tar.xz or .tar.gz).
# The program goes to DIR/NAME, NAME the last part of MEMBER. It needs a
# POSIX shell, curl, sha256sum and tar.
set -eu
url=${1:-}
sum=${2:-}
member=${3:-}
dir=${4:-}
if [ -z "$url" ] || [ -z "$sum" ] || [ -z "$member" ] || [ -z "$dir" ]; then
    echo "usage: fetch-tool.sh URL SHA256 MEMBER DIR (got '$*')" >&2
    exit 64
fi
here=$(dirname "$0")
archive=$dir/${url##*/}
mkdir -p "$dir"
if [ ! -f "$archive" ] || ! echo "$sum  $archive" | sha256sum -c - > /dev/null 2>&1; then
    rm -f "$archive"
    sh "$here/../retry.sh" curl -qfsSL --max-time 300 -o "$archive" "$url"
fi
echo "$sum  $archive" | sha256sum -c - > /dev/null || {
    echo "fetch-tool: ${url##*/} has another SHA-256 than $sum" >&2
    rm -f "$archive"
    exit 1
}
case $archive in
    *.tar.xz) flag=-xJf ;;
    *.tar.gz | *.tgz) flag=-xzf ;;
    *) echo "fetch-tool: not a .tar.xz or a .tar.gz: $archive" >&2; exit 64 ;;
esac
tmp=$dir/.unpack
rm -rf "$tmp"
mkdir -p "$tmp"
tar "$flag" "$archive" -C "$tmp" "$member"
name=${member##*/}
mv "$tmp/$member" "$dir/$name"
rm -rf "$tmp"
chmod 755 "$dir/$name"
echo "fetch-tool: $dir/$name from ${url##*/}, SHA-256 $sum"
