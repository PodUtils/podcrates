#!/bin/sh
# The DNS over HTTPS of pod-dns against CoreDNS (T-350, call C151): the
# release archive of CoreDNS 1.15.0 for Linux x86_64, checked by its
# SHA-256. The test target doh_coredns starts CoreDNS on 127.0.0.1 alone,
# with a zone and a certificate that the test makes.
# POD_DNS_TEST_COREDNS=required makes a skip a failure, and the run must
# say that CoreDNS answered over each protocol, so the check cannot pass by
# a skip. The archive goes to RUNNER_TEMP, else TMPDIR, else /tmp.
set -eu
cd "$(dirname "$0")/../.." || exit 71
version=1.15.0
sha256=2495c5b8f3fab6d0a336c397f1227b75406d86e6ec7125744a29df9e936ab8cd
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) ;;
    *) echo "coredns-tests: the pinned archive is for Linux x86_64 alone" >&2; exit 1 ;;
esac
dir=${RUNNER_TEMP:-${TMPDIR:-/tmp}}/coredns-$version
mkdir -p "$dir"
if [ ! -f "$dir/coredns.tgz" ]; then
    sh scripts/retry.sh curl -qfsSL -o "$dir/coredns.tgz" \
        "https://github.com/coredns/coredns/releases/download/v$version/coredns_${version}_linux_amd64.tgz"
fi
echo "$sha256  $dir/coredns.tgz" | sha256sum -c -
tar -xzf "$dir/coredns.tgz" -C "$dir" coredns
"$dir/coredns" -version
out=$(POD_DNS_TEST_COREDNS=required POD_DNS_TEST_COREDNS_BIN="$dir/coredns" \
    cargo test --locked -p pod-dns --test doh_coredns -- --nocapture 2>&1) || { printf '%s\n' "$out"; exit 1; }
printf '%s\n' "$out"
for line in 'CoreDNS: the wire form answered over HTTP/1.1' 'CoreDNS: the wire form answered over HTTP/2' \
    'CoreDNS: NXDOMAIN ended the lookup' 'CoreDNS: a certificate of another root was refused'; do
    if ! printf '%s\n' "$out" | grep -qxF "$line"; then
        echo "coredns-tests: the run did not say: $line" >&2
        exit 1
    fi
done
