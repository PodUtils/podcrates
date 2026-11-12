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
ZIGBUILD_SHA256=${ZIGBUILD_SHA256:-9e3cf73485edbd45905c8aadbc0fdf869c7ddc3848f0c898229f2680db52e44b}
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
    # The release binary on Linux x86_64; elsewhere a build.
    if [ "$(uname -s)-$(uname -m)" = Linux-x86_64 ]; then
        name=cargo-zigbuild-x86_64-unknown-linux-musl
        sh "$here/ci/fetch-tool.sh" \
            "https://github.com/rust-cross/cargo-zigbuild/releases/download/v$ZIGBUILD_VERSION/$name.tar.xz" \
            "$ZIGBUILD_SHA256" "$name/cargo-zigbuild" "$tools/bin"
        mkdir -p "$HOME/.local/bin"
        ln -sf "$tools/bin/cargo-zigbuild" "$HOME/.local/bin/cargo-zigbuild"
    else
        cargo install --locked --quiet cargo-zigbuild --version "$ZIGBUILD_VERSION"
    fi
fi
echo "cloud-setup: ready ($(zig version 2> /dev/null || echo 'zig not on PATH: add $HOME/.local/bin'))"
