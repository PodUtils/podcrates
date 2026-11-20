#!/bin/sh
# The clients of a job through the TLS end of pod-broker (T-973, T-980):
# curl, git, Python with urllib, requests and pip, Node.js, npm, cargo, Go,
# Java, Deno, the Google Cloud CLI and Hex of Elixir, each with the bundle
# of the job (tests/tls_clients.rs). requests and its dependencies come
# from scripts/ci/broker-clients.txt, each wheel checked by its SHA-256, in
# a virtualenv. Node.js 26.13.1 (the release archive for Linux x86_64,
# checked by the SHA-256 that its SHASUMS256.txt names) has --use-env-proxy
# for the leg of fetch. Deno 2.9.8 is the release archive for Linux x86_64,
# checked by the SHA-256 that its release names. Go, Java, npm, gcloud and
# mix with Hex come from the runner and the steps before this one (ci.yml).
# POD_BROKER_TEST_CLIENTS=required makes a skip a failure, and the run must
# say each line below, so the check cannot pass by a skip. The files go to
# RUNNER_TEMP, else TMPDIR, else /tmp.
set -eu
cd "$(dirname "$0")/../.." || exit 71
node_version=26.13.1
node_sha256=5c063e46bab0fbc1c17cc445d7ff129cbdcd0471be6ed83b6ab864f8143bc864
deno_version=2.9.8
deno_sha256=e890975bc2549807bbcc0b584ffafda72cc7c93349dd99c80483dd784f6c4609
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) ;;
    *) echo "broker-clients-tests: the pinned Node.js and Deno archives are for Linux x86_64 alone" >&2; exit 1 ;;
esac
tmp=${RUNNER_TEMP:-${TMPDIR:-/tmp}}
name=node-v$node_version-linux-x64
sh scripts/ci/fetch-tool.sh "https://nodejs.org/dist/v$node_version/$name.tar.xz" \
    "$node_sha256" "$name/bin/node" "$tmp/node-$node_version"
sh scripts/ci/fetch-tool.sh \
    "https://github.com/denoland/deno/releases/download/v$deno_version/deno-x86_64-unknown-linux-gnu.zip" \
    "$deno_sha256" deno "$tmp/deno-$deno_version"
venv=$tmp/broker-clients-venv
python3 -m venv "$venv"
sh scripts/retry.sh "$venv/bin/python" -m pip install --quiet --require-hashes -r scripts/ci/broker-clients.txt
out=$(POD_BROKER_TEST_CLIENTS=required POD_BROKER_TEST_PYTHON="$venv/bin/python" \
    POD_BROKER_TEST_NODE="$tmp/node-$node_version/node" POD_BROKER_TEST_DENO="$tmp/deno-$deno_version/deno" \
    timeout 900 cargo test --locked -p pod-broker --all-features --test tls_clients -- --nocapture 2>&1) || {
    printf '%s\n' "$out"
    exit 1
}
printf '%s\n' "$out"
for client in curl 'python urllib' 'python requests' node 'node fetch' go java deno pip npm gcloud hex; do
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
    'clients: cargo took the bundle, the far end got the value' \
    'clients: python urllib with no SSL_CERT_FILE refused the leaf' \
    'clients: node with no NODE_EXTRA_CA_CERTS refused the leaf' \
    'clients: go with no SSL_CERT_FILE refused the leaf' \
    'clients: java with no JAVA_TOOL_OPTIONS refused the leaf' \
    'clients: deno with no DENO_CERT and NODE_EXTRA_CA_CERTS refused the leaf' \
    'clients: npm with no NODE_EXTRA_CA_CERTS refused the leaf' \
    'clients: gcloud with no CLOUDSDK_CORE_CUSTOM_CA_CERTS_FILE refused the leaf' \
    'clients: hex with no HEX_CACERTS_PATH refused the leaf'; do
    if ! printf '%s\n' "$out" | grep -qF "$line"; then
        echo "broker-clients-tests: the run did not say: $line" >&2
        exit 1
    fi
done
