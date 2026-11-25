#!/bin/sh
# Install swtpm and libtpms of Ubuntu 24.04 (noble), each pinned by its
# version and its SHA-256 (T-1222): the programs of the tests of pod-tpm
# against a TPM in software. swtpm publishes a source tarball and no
# binary, and no image of its project holds it; the versions of the noble
# release stay in the archive, where a version of noble-updates can go.
# A consumer (podssh, for its check of a TPM signature with OpenSSH) can
# call this script at a pinned commit of podcrates. It needs apt-get,
# sha256sum and root or sudo, and prints the version of swtpm.
set -eu
swtpm_version=0.7.3-0ubuntu5
swtpm_sha256=00f017ff30b3c1be5fd25a6b67f9844ca3de32dab928a70779ee3e25c1aa865f
libtpms_version=0.9.3-0ubuntu4
libtpms_sha256=8095fd5f6a63166650010367efc07247e17ad0632c75d45d8360ae4c98915773
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) ;;
    *) echo "swtpm: the pinned packages are for Linux x86_64 alone" >&2; exit 1 ;;
esac
case "$(. /etc/os-release && echo "$ID-$VERSION_ID")" in
    ubuntu-24.04) ;;
    *) echo "swtpm: the pinned packages are of Ubuntu 24.04" >&2; exit 1 ;;
esac
root() {
    if [ "$(id -u)" = 0 ]; then "$@"; else sudo "$@"; fi
}
installed() {
    dpkg-query -W -f '${Version}' "$1" 2>/dev/null || true
}
here=$(cd "$(dirname "$0")/../.." && pwd)
if [ "$(installed swtpm)" != "$swtpm_version" ] || [ "$(installed libtpms0)" != "$libtpms_version" ]; then
    dir=$(mktemp -d)
    trap 'rm -rf "$dir"' EXIT
    root sh "$here/scripts/retry.sh" apt-get update -q
    # apt-get download writes as the user _apt when it can: a directory of
    # mode 0755 that root owns takes no file of it.
    chmod 0777 "$dir"
    (cd "$dir" && sh "$here/scripts/retry.sh" apt-get download -q \
        "swtpm=$swtpm_version" "libtpms0=$libtpms_version")
    printf '%s  %s\n%s  %s\n' \
        "$swtpm_sha256" "$dir/swtpm_${swtpm_version}_amd64.deb" \
        "$libtpms_sha256" "$dir/libtpms0_${libtpms_version}_amd64.deb" | sha256sum -c -
    root apt-get install -y -q --no-install-recommends --allow-downgrades \
        "$dir/libtpms0_${libtpms_version}_amd64.deb" "$dir/swtpm_${swtpm_version}_amd64.deb"
fi
swtpm --version
