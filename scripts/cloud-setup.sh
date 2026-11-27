#!/bin/sh
# Make a Linux host or a cloud session ready for the static builds: rustup
# and the toolchain of rust-toolchain.toml, the musl targets, zig and
# cargo-zigbuild. Each step is skipped when its tool is there. The versions
# of zig and cargo-zigbuild are the ones of the CI workflows.
#
#   sh scripts/cloud-setup.sh [--dev-tools] [TARGET...]
#
# With no TARGET, it adds x86_64-unknown-linux-musl. A new shell finds
# cargo, rustup, zig and cargo-zigbuild with `. ./scripts/cloud-env.sh`.
# --dev-tools also installs the tools of the optional checks that the host
# does not have (shellcheck for check.sh, groff and mandoc for
# check-man.sh) with apt-get, as root or with sudo that asks for no
# password (also a sudo for apt-get alone); else it names them.
set -eu
dev_tools=0
targets=
for arg in "$@"; do
    case "$arg" in
        --dev-tools) dev_tools=1 ;;
        -h | --help) awk 'NR > 1 && !/^#/ { exit } NR > 1' "$0" | sed 's/^# \{0,1\}//' && exit 0 ;;
        -*) echo "usage: sh scripts/cloud-setup.sh [--dev-tools] [TARGET...]" >&2 && exit 64 ;;
        *) targets="$targets $arg" ;;
    esac
done
# A target name has no space: the split is safe.
# shellcheck disable=SC2086
set -- $targets
here=$(cd "$(dirname "$0")" && pwd)
# rustup reads rust-toolchain.toml in the directory of the command.
cd "$here/.."
ZIG_VERSION=${ZIG_VERSION:-0.16.0}
# zig for the machine of the host: the pins of index.json of ziglang.org.
case "$(uname -m)" in
    aarch64 | arm64)
        ZIG_ARCH=aarch64
        zig_sum=ea4b09bfb22ec6f6c6ceac57ab63efb6b46e17ab08d21f69f3a48b38e1534f17
        ;;
    *)
        ZIG_ARCH=x86_64
        zig_sum=70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00
        ;;
esac
export ZIG_ARCH
ZIG_SHA256=${ZIG_SHA256:-$zig_sum}
ZIGBUILD_VERSION=${ZIGBUILD_VERSION:-0.23.4}
ZIGBUILD_SHA256=${ZIGBUILD_SHA256:-9e3cf73485edbd45905c8aadbc0fdf869c7ddc3848f0c898229f2680db52e44b}
RUSTUP_VERSION=${RUSTUP_VERSION:-1.29.3}
tools=${PODCRATES_TOOLS:-$HOME/.local/podcrates}
cargo_bin=${CARGO_HOME:-$HOME/.cargo}/bin
mkdir -p "$HOME/.local/bin" "$tools"
# Two setups at the same time install each tool once.
if command -v flock > /dev/null 2>&1; then
    exec 9> "$tools/setup.lock"
    flock -w 900 9 || { echo "cloud-setup: $tools/setup.lock is held" >&2; exit 1; }
fi
# A limit of time for each long step; a host with no timeout(1) runs it
# with no limit.
bounded() {
    if command -v timeout > /dev/null 2>&1; then
        timeout "$@"
    else
        shift
        "$@"
    fi
}

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
    bounded 300 "$tools/rustup/rustup-init" -y --no-modify-path --default-toolchain none < /dev/null
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
    bounded 900 rustup toolchain install --no-self-update < /dev/null
fi

[ "$#" -gt 0 ] || set -- x86_64-unknown-linux-musl
bounded 600 rustup target add "$@" < /dev/null

if ! command -v zig > /dev/null 2>&1; then
    mkdir -p "$tools"
    sh "$here/fetch-zig.sh" "$ZIG_VERSION" "$ZIG_SHA256" "$tools/zig.tar.xz"
    tar -xJf "$tools/zig.tar.xz" -C "$tools"
    rm -f "$tools/zig.tar.xz"
    ln -sf "$tools/zig-$ZIG_ARCH-linux-$ZIG_VERSION/zig" "$HOME/.local/bin/zig"
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
if [ "$dev_tools" = 1 ]; then
    missing=
    for tool in shellcheck groff mandoc; do
        command -v "$tool" > /dev/null 2>&1 || missing="$missing $tool"
    done
    # sudo -E keeps DEBIAN_FRONTEND and the proxy variables that apt-get
    # needs; a host that allows only "sudo apt-get" gets plain sudo -n.
    sudo=none
    if [ "$(id -u)" = 0 ]; then
        sudo=
    elif command -v sudo > /dev/null 2>&1 && sudo -n -E true 2> /dev/null; then
        sudo="sudo -n -E"
    elif command -v sudo > /dev/null 2>&1 && sudo -n apt-get --version > /dev/null 2>&1; then
        sudo="sudo -n"
    fi
    if [ -z "$missing" ]; then
        :
    elif command -v apt-get > /dev/null 2>&1 && [ "$sudo" != none ]; then
        DEBIAN_FRONTEND=noninteractive
        export DEBIAN_FRONTEND
        # A failed source fails the update: the install then uses the lists
        # that apt has.
        # $sudo and $missing are lists of words: not quoted on purpose.
        # shellcheck disable=SC2086
        bounded 300 $sudo apt-get update -qq < /dev/null ||
            echo "cloud-setup: apt-get update failed; the install uses the lists that apt has" >&2
        # shellcheck disable=SC2086
        bounded 300 $sudo apt-get install -y -qq --no-install-recommends $missing < /dev/null ||
            echo "cloud-setup: apt-get did not install$missing" >&2
    else
        echo "cloud-setup: install$missing by hand (no apt-get, or no root and no sudo with no password)" >&2
    fi
fi
echo "cloud-setup: ready ($(zig version 2> /dev/null || echo "zig not on PATH: add \$HOME/.local/bin"))"
