#!/bin/sh
# zig and cargo-zigbuild for a job of Linux x86_64 (T-355, call C152).
# DIR holds the archives: a cache step of the workflow keeps it, keyed by
# ZIG_SHA256 and ZIGBUILD_SHA256. Each archive is checked by its SHA-256
# on each run, also from the cache. zig comes from scripts/fetch-zig.sh on
# a miss; cargo-zigbuild is its release binary, not a build by
# `cargo install`.
#
#   sh scripts/ci/zig-tools.sh DIR
#
# The environment gives ZIG_VERSION, ZIG_SHA256, ZIGBUILD_VERSION and
# ZIGBUILD_SHA256. The directories of zig and cargo-zigbuild go to
# GITHUB_PATH when it is set, else to the output.
set -eu
dir=${1:-}
[ -n "$dir" ] || { echo "usage: zig-tools.sh DIR" >&2; exit 64; }
: "${ZIG_VERSION:?}" "${ZIG_SHA256:?}" "${ZIGBUILD_VERSION:?}" "${ZIGBUILD_SHA256:?}"
here=$(dirname "$0")
mkdir -p "$dir"
zip=$dir/zig-x86_64-linux-$ZIG_VERSION.tar.xz
if [ -f "$zip" ] && echo "$ZIG_SHA256  $zip" | sha256sum -c - > /dev/null 2>&1; then
    echo "zig-tools: zig $ZIG_VERSION from the cache, SHA-256 $ZIG_SHA256"
else
    sh "$here/../fetch-zig.sh" "$ZIG_VERSION" "$ZIG_SHA256" "$zip"
fi
rm -rf "$dir/zig"
mkdir -p "$dir/zig"
tar -xJf "$zip" -C "$dir/zig"
name=cargo-zigbuild-x86_64-unknown-linux-musl
sh "$here/fetch-tool.sh" \
    "https://github.com/rust-cross/cargo-zigbuild/releases/download/v$ZIGBUILD_VERSION/$name.tar.xz" \
    "$ZIGBUILD_SHA256" "$name/cargo-zigbuild" "$dir/bin"
out=${GITHUB_PATH:-/dev/stdout}
echo "$dir/zig/zig-x86_64-linux-$ZIG_VERSION" >> "$out"
echo "$dir/bin" >> "$out"
