#!/bin/sh
# The pairing by a code of pod-wormhole and of pod-relay against
# magic-wormhole's own mailbox server and library, and the watch
# transit-relay against its transit relay, on 127.0.0.1 (T-354, T-548): the
# packages of scripts/ci/wormhole-servers.txt, each checked by its SHA-256,
# in a virtualenv. POD_WORMHOLE_TEST_SERVER=required makes a skip a
# failure. The broker tests are not here: Mosquitto has its own step.
set -eu
cd "$(dirname "$0")/../.." || exit 71
venv="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/mailbox-venv"
python3 -m venv "$venv"
sh scripts/retry.sh "$venv/bin/python" -m pip install --quiet --require-hashes -r scripts/ci/wormhole-servers.txt
export POD_WORMHOLE_TEST_SERVER=required
export POD_WORMHOLE_TEST_PYTHON="$venv/bin/python"
timeout 900 cargo test --locked -p pod-wormhole --all-features --test mailbox --test faults -- --nocapture
timeout 900 cargo test --locked -p pod-relay --all-features --test pair_code -- --nocapture
# The watch transit-relay of pod-services-watch against the transit relay.
timeout 900 cargo test --locked -p pod-services-watch --test watches -- transit:: --nocapture
