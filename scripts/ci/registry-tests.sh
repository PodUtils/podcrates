#!/bin/sh
# The client of pod-oci against the CNCF distribution registry (T-353,
# call C151): the release archive of distribution 3.1.2 for Linux x86_64,
# checked by its SHA-256 (scripts/ci/fetch-tool.sh). The test target
# distribution starts the registry on 127.0.0.1 alone, with a
# configuration, certificates and a made-up login that the test makes, and
# pushes its own index of two platforms. POD_OCI_TEST_REGISTRY=required
# makes a skip a failure, and the run must say each line below, so the
# check cannot pass by a skip. The archive goes to RUNNER_TEMP, else
# TMPDIR, else /tmp.
set -eu
cd "$(dirname "$0")/../.." || exit 71
version=3.1.2
sha256=40df2224d410f72ae425c3371873b078bbdbda3b8b612be9571f0e6751f3acc8
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) ;;
    *) echo "registry-tests: the pinned archive is for Linux x86_64 alone" >&2; exit 1 ;;
esac
dir=${RUNNER_TEMP:-${TMPDIR:-/tmp}}/distribution-$version
sh scripts/ci/fetch-tool.sh \
    "https://github.com/distribution/distribution/releases/download/v$version/registry_${version}_linux_amd64.tar.gz" \
    "$sha256" registry "$dir"
"$dir/registry" --version
out=$(POD_OCI_TEST_REGISTRY=required POD_OCI_TEST_REGISTRY_BIN="$dir/registry" \
    cargo test --locked -p pod-oci --test distribution -- --nocapture 2>&1) || { printf '%s\n' "$out"; exit 1; }
printf '%s\n' "$out"
for line in 'distribution: an index of two platforms resolved through htpasswd' \
    'distribution: a wrong password got 401' \
    'distribution: a request with no login got 401' \
    'distribution: an index of two platforms resolved through a bearer token' \
    'distribution: a token ask with no login got 401' \
    'distribution: a blob and a manifest that do not match their digests were refused' \
    'distribution: no login went over plain HTTP'; do
    if ! printf '%s\n' "$out" | grep -qxF "$line"; then
        echo "registry-tests: the run did not say: $line" >&2
        exit 1
    fi
done
