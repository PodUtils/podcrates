#!/bin/sh
# Make a Linux host or a cloud session ready for the static builds: the
# musl targets of rustup, zig and cargo-zigbuild. Each step is skipped when
# its tool is there. The versions are the ones of the CI workflows.
#
#   sh scripts/cloud-setup.sh [TARGET...]
#
# With no TARGET, it adds x86_64-unknown-linux-musl.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
ZIG_VERSION=${ZIG_VERSION:-0.16.0}
ZIG_SHA256=${ZIG_SHA256:-70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00}
ZIGBUILD_VERSION=${ZIGBUILD_VERSION:-0.23.4}
tools=${PODCRATES_TOOLS:-$HOME/.local/podcrates}

[ "$#" -gt 0 ] || set -- x86_64-unknown-linux-musl
rustup target add "$@"

if ! command -v zig > /dev/null 2>&1; then
    mkdir -p "$tools"
    sh "$here/fetch-zig.sh" "$ZIG_VERSION" "$ZIG_SHA256" "$tools/zig.tar.xz"
    tar -xJf "$tools/zig.tar.xz" -C "$tools"
    rm -f "$tools/zig.tar.xz"
    mkdir -p "$HOME/.local/bin"
    ln -sf "$tools/zig-x86_64-linux-$ZIG_VERSION/zig" "$HOME/.local/bin/zig"
fi
if ! cargo zigbuild --version > /dev/null 2>&1; then
    cargo install --locked --quiet cargo-zigbuild --version "$ZIGBUILD_VERSION"
fi
echo "cloud-setup: ready ($(zig version 2> /dev/null || echo 'zig not on PATH: add $HOME/.local/bin'))"
