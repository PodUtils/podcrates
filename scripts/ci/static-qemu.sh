#!/bin/sh
# The pinned static QEMU of podvm's store in DIR (T-1013): bin/ with
# qemu-system-x86_64, qemu-system-aarch64 and qemu-img, and share/qemu with
# the firmware, so that the emulator finds its data with no -L
# (bin/../share/qemu). The pins are the facts of the service
# crates/pod-services/services/hermeticbuild-qemu.toml: each archive is
# checked by its SHA-256 (scripts/ci/fetch-tool.sh), and each binary by its
# own SHA-256 after the unpack.
#
#   sh scripts/ci/static-qemu.sh DIR
#
# DIR/bin goes to GITHUB_PATH when it is set, else to the output. It needs
# a POSIX shell, python3 (3.11 or later), curl, sha256sum and tar.
set -eu
dir=${1:-}
[ -n "$dir" ] || { echo "usage: static-qemu.sh DIR" >&2; exit 64; }
here=$(dirname "$0")
service=$here/../../crates/pod-services/services/hermeticbuild-qemu.toml
# One line for each archive: NAME URL SHA256 MEMBER BIN_SHA256; the data
# archive has "-" as its member and the SHA-256 of its member.
pins=$(python3 -I - "$service" <<'EOF'
import sys
import tomllib

with open(sys.argv[1], "rb") as f:
    s = tomllib.load(f)
facts = {f["name"]: f["value"] for f in s["fact"]}
for e in s["endpoint"]:
    n = e["name"]
    url = f"https://{e['host']}{e['path']}"
    member = facts.get(f"{n}-member", "-")
    print(n, url, facts[f"{n}-sha256"], member, facts.get(f"{n}-bin-sha256", "-"))
EOF
)
mkdir -p "$dir/bin" "$dir/share"
echo "$pins" | while read -r name url sum member bin_sum; do
    if [ "$member" = "-" ]; then
        archive=$dir/${url##*/}
        if [ ! -f "$archive" ] || ! echo "$sum  $archive" | sha256sum -c - > /dev/null 2>&1; then
            rm -f "$archive"
            sh "$here/../retry.sh" curl -qfsSL --max-time 300 -o "$archive" "$url"
        fi
        echo "$sum  $archive" | sha256sum -c - > /dev/null || {
            echo "static-qemu: $name has another SHA-256 than $sum" >&2
            exit 1
        }
        tar -xzf "$archive" -C "$dir" ./share/qemu
        echo "static-qemu: $dir/share/qemu from ${url##*/}, SHA-256 $sum"
        continue
    fi
    sh "$here/fetch-tool.sh" "$url" "$sum" "$member" "$dir/bin"
    echo "$bin_sum  $dir/bin/${member##*/}" | sha256sum -c - > /dev/null || {
        echo "static-qemu: ${member##*/} has another SHA-256 than $bin_sum" >&2
        exit 1
    }
done
"$dir/bin/qemu-system-x86_64" --version
out=${GITHUB_PATH:-/dev/stdout}
echo "$dir/bin" >> "$out"
