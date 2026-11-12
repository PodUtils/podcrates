#!/bin/sh
# The TURN client of pod-turn (T-380) and the STUN codec of pod-proxy
# (T-358) against coturn (call C151): the image that images/coturn/Dockerfile
# pins to a digest, run with the host's network on 127.0.0.1 alone. coturn
# serves TURN over UDP and over TLS on TCP with a made-up static user and
# realm, a certificate of a root that this script makes, peers on the
# loopback, FINGERPRINT on each answer, and nonces of 2 s.
# POD_TURN_TEST_COTURN=required and POD_PROXY_TEST_STUN=required make a skip
# a failure, and the run must say each line below, so the check cannot pass
# by a skip. It needs docker and openssl. POD_TURN_TEST_COTURN_BIN names a
# turnserver program of the host to run in place of the image.
set -eu
cd "$(dirname "$0")/../.." || exit 71
udp=3478
tls=5349
# The made-up login and realm of tests/coturn.rs of pod-turn.
user=pod-turn-test
password=made-up-coturn-password
realm=coturn.test
dir=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/coturn-tests.XXXXXX")
chmod 755 "$dir"

# A root and a certificate of coturn.test and 127.0.0.1 from it, P-256.
printf '[req]\ndistinguished_name=dn\n[dn]\n' > "$dir/req.cnf"
openssl req -x509 -new -nodes -config "$dir/req.cnf" -newkey ec -pkeyopt ec_paramgen_curve:P-256 \
    -keyout "$dir/ca.key" -out "$dir/ca.pem" -days 2 -subj "/CN=pod-turn test root" \
    -addext basicConstraints=critical,CA:TRUE -addext keyUsage=critical,keyCertSign 2> /dev/null
openssl req -new -nodes -config "$dir/req.cnf" -newkey ec -pkeyopt ec_paramgen_curve:P-256 \
    -keyout "$dir/server.key" -out "$dir/server.csr" -subj "/CN=coturn.test" 2> /dev/null
printf '%s\n' 'subjectAltName=DNS:coturn.test,IP:127.0.0.1' 'basicConstraints=critical,CA:FALSE' \
    'keyUsage=critical,digitalSignature' 'extendedKeyUsage=serverAuth' > "$dir/server.ext"
openssl x509 -req -in "$dir/server.csr" -CA "$dir/ca.pem" -CAkey "$dir/ca.key" -CAcreateserial \
    -out "$dir/server.pem" -days 2 -extfile "$dir/server.ext" 2> /dev/null
# coturn in the image runs as nobody.
chmod 644 "$dir/server.key" "$dir/server.pem" "$dir/ca.pem"

set -- -n --log-file=stdout --listening-ip=127.0.0.1 --relay-ip=127.0.0.1 \
    --listening-port=$udp --tls-listening-port=$tls --cert="$dir/server.pem" --pkey="$dir/server.key" \
    --lt-cred-mech --realm=$realm --user=$user:$password --allow-loopback-peers --fingerprint \
    --stale-nonce=2 --max-allocate-lifetime=1800
if [ -n "${POD_TURN_TEST_COTURN_BIN:-}" ]; then
    "$POD_TURN_TEST_COTURN_BIN" --version
    # A turnserver before 4.7 starts its CLI unless told, and refuses the
    # CLI with no password beside --allow-loopback-peers.
    "$POD_TURN_TEST_COTURN_BIN" "$@" --no-cli > "$dir/coturn.log" 2>&1 &
    pid=$!
    trap 'tail -n 40 "$dir/coturn.log"; kill "$pid" 2> /dev/null || true; rm -rf "$dir"' EXIT
else
    ref=$(tr -d '\r' < images/coturn/Dockerfile | sed -n 's/^FROM[[:space:]]\{1,\}\([^[:space:]]\{1,\}\).*/\1/p')
    case "$ref" in
        *@sha256:*) ;;
        *) echo "turn-tests: images/coturn/Dockerfile names no image pinned to a digest" >&2; exit 1 ;;
    esac
    image=$(sh scripts/pull-image.sh "$ref")
    name=pod-turn-coturn-$$
    # The entrypoint of the image passes each argument through `eval echo`,
    # which takes `-n` as its own option: turnserver itself is the program.
    docker run --rm --entrypoint turnserver "$image" --version
    docker run -d --name "$name" --network host -v "$dir:$dir:ro" --entrypoint turnserver "$image" "$@" > /dev/null
    trap 'docker logs "$name" 2>&1 | tail -n 40; docker rm -f "$name" > /dev/null 2>&1 || true; rm -rf "$dir"' EXIT
fi

# Wait for a Binding answer on UDP and for the TLS port, at most 60 s.
python3 -I - "$udp" "$tls" <<'EOF'
import socket, sys, time
udp, tls = int(sys.argv[1]), int(sys.argv[2])
deadline = time.monotonic() + 60
request = bytes.fromhex("000100002112a442") + bytes(range(12))
while True:
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
            s.settimeout(0.5)
            s.sendto(request, ("127.0.0.1", udp))
            s.recvfrom(1500)
        socket.create_connection(("127.0.0.1", tls), 1).close()
        break
    except OSError:
        if time.monotonic() > deadline:
            sys.exit("turn-tests: coturn did not listen within 60 s")
        time.sleep(0.5)
EOF

out=$(POD_TURN_TEST_COTURN=required POD_TURN_TEST_COTURN_UDP=127.0.0.1:$udp \
    POD_TURN_TEST_COTURN_TLS=127.0.0.1:$tls POD_TURN_TEST_COTURN_CA="$dir/ca.pem" \
    cargo test --locked -p pod-turn --all-features --test coturn -- --nocapture 2>&1) ||
    { printf '%s\n' "$out"; exit 1; }
printf '%s\n' "$out"
stun=$(POD_PROXY_TEST_STUN=required POD_PROXY_TEST_STUN_SERVER=127.0.0.1:$udp \
    cargo test --locked -p pod-proxy --all-features --test stun_coturn -- --nocapture 2>&1) ||
    { printf '%s\n' "$stun"; exit 1; }
printf '%s\n' "$stun"
for line in 'coturn: an allocation over UDP relayed a Send and ChannelData' \
    'coturn: an allocation over TLS relayed a Send and padded ChannelData' \
    'coturn: a lifetime past the maximum got the maximum' \
    'coturn: a stale nonce got 438, and the request went again' \
    'coturn: an Allocate of another user on the same 5-tuple got 437' \
    'coturn: a Refresh of another user got 441' \
    'coturn: a certificate of another root was refused'; do
    if ! printf '%s\n' "$out" | grep -qxF "$line"; then
        echo "turn-tests: the run did not say: $line" >&2
        exit 1
    fi
done
for line in 'coturn: a Binding request mapped the address of its socket' \
    'coturn: a Binding request with an unknown attribute got 420'; do
    if ! printf '%s\n' "$stun" | grep -qxF "$line"; then
        echo "turn-tests: the run did not say: $line" >&2
        exit 1
    fi
done
