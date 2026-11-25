#!/bin/sh
# Make a Linux host or a cloud session ready for the static builds: rustup
# and the toolchain of rust-toolchain.toml, the musl targets, zig and
# cargo-zigbuild. Each step is skipped when its tool is there. The versions
# of zig and cargo-zigbuild are the ones of the CI workflows.
#
#   sh scripts/cloud-setup.sh [TARGET...]
#
# With no TARGET, it adds x86_64-unknown-linux-musl. A new shell finds
# cargo, rustup, zig and cargo-zigbuild with
# `export PATH="$HOME/.local/bin:$PATH"`.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
# rustup reads rust-toolchain.toml in the directory of the command.
cd "$here/.."
ZIG_VERSION=${ZIG_VERSION:-0.16.0}
ZIG_SHA256=${ZIG_SHA256:-70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00}
ZIGBUILD_VERSION=${ZIGBUILD_VERSION:-0.23.4}
ZIGBUILD_SHA256=${ZIGBUILD_SHA256:-9e3cf73485edbd45905c8aadbc0fdf869c7ddc3848f0c898229f2680db52e44b}
RUSTUP_VERSION=${RUSTUP_VERSION:-1.29.3}
tools=${PODCRATES_TOOLS:-$HOME/.local/podcrates}
cargo_bin=${CARGO_HOME:-$HOME/.cargo}/bin
mkdir -p "$HOME/.local/bin"

# A fresh host or a local machine with no rustup: the installer of the Rust
# project, checked by the SHA-256 of its .sha256 file, with no toolchain
# and no change of the profile of the shell.
if ! command -v rustup > /dev/null 2>&1 && [ ! -x "$cargo_bin/rustup" ]; then
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64)
            arch=x86_64
            sum=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55
            ;;
        Linux-aarch64 | Linux-arm64)
            arch=aarch64
            sum=9f608c142e0e38a78957d0661ba1dfaf0b6fb37184cfaa4b9a0aa57af6184282
            ;;
        *)
            echo "cloud-setup: no pinned rustup-init for $(uname -s) $(uname -m): install rustup" >&2
            exit 1
            ;;
    esac
    sh "$here/ci/fetch-tool.sh" \
        "https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/$arch-unknown-linux-gnu/rustup-init" \
        "$sum" rustup-init "$tools/rustup"
    "$tools/rustup/rustup-init" -y --no-modify-path --default-toolchain none < /dev/null
fi
if [ -x "$cargo_bin/rustup" ]; then
    case ":$PATH:" in *":$cargo_bin:"*) ;; *) PATH=$cargo_bin:$PATH && export PATH ;; esac
    for tool in cargo rustc rustup; do
        if [ ! -e "$HOME/.local/bin/$tool" ] && [ ! -L "$HOME/.local/bin/$tool" ]; then
            ln -s "$cargo_bin/$tool" "$HOME/.local/bin/$tool"
        fi
    done
fi
# The toolchain of rust-toolchain.toml, when rustup does not have it yet.
if ! RUSTUP_AUTO_INSTALL=0 rustup which cargo > /dev/null 2>&1; then
    rustup toolchain install --no-self-update < /dev/null
fi

[ "$#" -gt 0 ] || set -- x86_64-unknown-linux-musl
rustup target add "$@"

if ! command -v zig > /dev/null 2>&1; then
    mkdir -p "$tools"
    sh "$here/fetch-zig.sh" "$ZIG_VERSION" "$ZIG_SHA256" "$tools/zig.tar.xz"
    tar -xJf "$tools/zig.tar.xz" -C "$tools"
    rm -f "$tools/zig.tar.xz"
    ln -sf "$tools/zig-x86_64-linux-$ZIG_VERSION/zig" "$HOME/.local/bin/zig"
fi
# `cargo zigbuild --version` is refused as an argument of the build; the
# binary itself prints its version.
if ! cargo-zigbuild --version > /dev/null 2>&1; then
    # The release binary on Linux x86_64; elsewhere a build.
    if [ "$(uname -s)-$(uname -m)" = Linux-x86_64 ]; then
        name=cargo-zigbuild-x86_64-unknown-linux-musl
        sh "$here/ci/fetch-tool.sh" \
            "https://github.com/rust-cross/cargo-zigbuild/releases/download/v$ZIGBUILD_VERSION/$name.tar.xz" \
            "$ZIGBUILD_SHA256" "$name/cargo-zigbuild" "$tools/bin"
        ln -sf "$tools/bin/cargo-zigbuild" "$HOME/.local/bin/cargo-zigbuild"
    else
        cargo install --locked --quiet cargo-zigbuild --version "$ZIGBUILD_VERSION"
    fi
fi
echo "cloud-setup: ready ($(zig version 2> /dev/null || echo 'zig not on PATH: add $HOME/.local/bin'))"
