#!/bin/sh
# The MQTT meeting point of pod-wormhole against Eclipse Mosquitto (T-261):
# the image that images/mosquitto/Dockerfile pins to a digest, with the
# listener of images/mosquitto/mosquitto.conf (MQTT over a WebSocket),
# published on 127.0.0.1 alone. POD_WORMHOLE_TEST_MQTT=required makes a
# skip a failure, and the run must say that Mosquitto paired two ends, so
# the check cannot pass by a skip. It needs docker.
set -eu
cd "$(dirname "$0")/../.." || exit 71
port=9001
ref=$(tr -d '\r' < images/mosquitto/Dockerfile | sed -n 's/^FROM[[:space:]]\{1,\}\([^[:space:]]\{1,\}\).*/\1/p')
case "$ref" in
    *@sha256:*) ;;
    *) echo "mosquitto-tests: images/mosquitto/Dockerfile names no image pinned to a digest" >&2; exit 1 ;;
esac
image=$(sh scripts/pull-image.sh "$ref")
name=pod-wormhole-mosquitto-$$
docker run -d --name "$name" -p "127.0.0.1:$port:$port" \
    -v "$(pwd)/images/mosquitto/mosquitto.conf:/mosquitto/config/mosquitto.conf:ro" "$image" > /dev/null
trap 'docker logs "$name" 2>&1 | tail -n 40; docker rm -f "$name" > /dev/null 2>&1 || true' EXIT
# Wait for the listener, at most 60 s.
python3 -I - "$port" <<'EOF'
import socket, sys, time
deadline = time.monotonic() + 60
while True:
    try:
        socket.create_connection(("127.0.0.1", int(sys.argv[1])), 1).close()
        break
    except OSError:
        if time.monotonic() > deadline:
            sys.exit("mosquitto-tests: Mosquitto did not listen within 60 s")
        time.sleep(0.5)
EOF
out=$(POD_WORMHOLE_TEST_MQTT=required POD_WORMHOLE_TEST_BROKER="ws://127.0.0.1:$port/mqtt" \
    cargo test --locked -p pod-wormhole --test broker -- --nocapture 2>&1) || { printf '%s\n' "$out"; exit 1; }
printf '%s\n' "$out"
if ! printf '%s\n' "$out" | grep -q '^Eclipse Mosquitto: two ends paired'; then
    echo "mosquitto-tests: no test paired two ends through Mosquitto" >&2
    exit 1
fi
