#!/bin/sh
# The clients of a job through the TLS end of pod-broker (T-973): curl,
# git, Python with urllib and with requests, Node.js and cargo, each with
# the bundle of the job (tests/tls_clients.rs). requests and its
# dependencies come from scripts/ci/broker-clients.txt, each wheel checked
# by its SHA-256, in a virtualenv. Node.js 26.13.1 (the release archive for
# Linux x86_64, checked by the SHA-256 that its SHASUMS256.txt names) has
# --use-env-proxy for the leg of fetch. POD_BROKER_TEST_CLIENTS=required
# makes a skip a failure, and the run must say each line below, so the
# check cannot pass by a skip. The files go to RUNNER_TEMP, else TMPDIR,
# else /tmp.
set -eu
cd "$(dirname "$0")/../.." || exit 71
node_version=26.13.1
node_sha256=5c063e46bab0fbc1c17cc445d7ff129cbdcd0471be6ed83b6ab864f8143bc864
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) ;;
    *) echo "broker-clients-tests: the pinned Node.js archive is for Linux x86_64 alone" >&2; exit 1 ;;
esac
tmp=${RUNNER_TEMP:-${TMPDIR:-/tmp}}
name=node-v$node_version-linux-x64
sh scripts/ci/fetch-tool.sh "https://nodejs.org/dist/v$node_version/$name.tar.xz" \
    "$node_sha256" "$name/bin/node" "$tmp/node-$node_version"
venv=$tmp/broker-clients-venv
python3 -m venv "$venv"
sh scripts/retry.sh "$venv/bin/python" -m pip install --quiet --require-hashes -r scripts/ci/broker-clients.txt
out=$(POD_BROKER_TEST_CLIENTS=required POD_BROKER_TEST_PYTHON="$venv/bin/python" \
    POD_BROKER_TEST_NODE="$tmp/node-$node_version/node" \
    timeout 900 cargo test --locked -p pod-broker --all-features --test tls_clients -- --nocapture 2>&1) || {
    printf '%s\n' "$out"
    exit 1
}
printf '%s\n' "$out"
for client in curl 'python urllib' 'python requests' node 'node fetch'; do
    for line in "clients: $client with no bundle refused the leaf" \
        "clients: $client took the bundle, the far end got the value, the job read the placeholder" \
        "clients: $client reached a host with no credential by the roots of the bundle, with no swap"; do
        if ! printf '%s\n' "$out" | grep -qF "$line"; then
            echo "broker-clients-tests: the run did not say: $line" >&2
            exit 1
        fi
    done
done
for line in 'clients: git with no bundle refused the leaf' \
    'clients: git with an extra header took the bundle, the far end got the value, the job read the placeholder' \
    'clients: git with a token in the URL took the bundle, the far end got the value, the job read the placeholder' \
    'clients: curl sent a body over the bound of the swap, with Expect: 100-continue' \
    'clients: cargo with no bundle refused the leaf' \
    'clients: cargo took the bundle, the far end got the value'; do
    if ! printf '%s\n' "$out" | grep -qF "$line"; then
        echo "broker-clients-tests: the run did not say: $line" >&2
        exit 1
    fi
done
