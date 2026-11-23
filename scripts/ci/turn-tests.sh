#!/bin/sh
# The TURN client of pod-turn (T-380, T-381) and the STUN codec of pod-proxy
# (T-358) against coturn (call C151): the image that images/coturn/Dockerfile
# pins to a digest, run with the host's network on 127.0.0.1 alone. coturn
# serves TURN over UDP, over TCP and over TLS on TCP with a made-up static
# user and realm, a certificate of a root that this script makes, peers on
# the loopback, FINGERPRINT on each answer, and nonces of 2 s. coturn
# enables its TCP relay by default; it has no --tcp-relay option, and
# --no-tcp-relay would close the RFC 6062 road. A second coturn
# with the same options answers each Allocate with 300 and the address of
# the first (--alternate-server, --tls-alternate-server). libjuice's TURN
# server (T-403, call C576), built from its source at a pinned commit with
# scripts/ci/juice-turn.c, offers SHA-256 and announces USERHASH on UDP.
# xturn (T-631, call C587), built with mix from its source at a pinned
# commit and the packages of its mix.lock, signs each answer with
# MESSAGE-INTEGRITY-SHA256 on UDP. POD_TURN_TEST_COTURN=required,
# POD_TURN_TEST_JUICE=required, POD_TURN_TEST_XTURN=required and
# POD_PROXY_TEST_STUN=required make a skip a failure, and the run must say
# each line below, so the check cannot pass by a skip. It needs docker,
# openssl, git, curl, a C compiler, and the Erlang/OTP and Elixir of the
# .tool-versions of xturn (ci.yml installs them). POD_TURN_TEST_COTURN_BIN
# names a turnserver program of the host to run in place of the image.
set -eu
cd "$(dirname "$0")/../.." || exit 71
root=$(pwd)
udp=3478
tls=5349
# The ports of the coturn that redirects; not the alternative ports of the
# first (its port + 1).
redirect_udp=3488
redirect_tls=5358
# The made-up login and realm of tests/coturn.rs of pod-turn.
user=pod-turn-test
password=made-up-coturn-password
realm=coturn.test
# libjuice 1.7.4, its port and the made-up password and realm of
# tests/juice.rs of pod-turn.
juice_commit=b89c792e3612faf2f12cf35bcc56857313a06be3
juice=3498
juice_password=made-up-juice-password
juice_realm=juice.test
# xturn 2.0.0, its port (UDP and TCP), the port of its API, and the made-up
# password of tests/xturn.rs of pod-turn. Its realm is the one of its own
# configuration.
xturn_commit=28662bedbf1acfe25d374c49fbf31daa6ef015fe
xturn=3508
xturn_api=8881
xturn_password=made-up-xturn-password
dir=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/coturn-tests.XXXXXX")
chmod 755 "$dir"

# The source of libjuice at the pinned commit, checked by its id.
git init -q "$dir/libjuice"
sh scripts/retry.sh git -C "$dir/libjuice" fetch -q --depth 1 \
    https://github.com/paullouisageneau/libjuice.git "$juice_commit"
git -C "$dir/libjuice" -c advice.detachedHead=false checkout -q FETCH_HEAD
if [ "$(git -C "$dir/libjuice" rev-parse HEAD)" != "$juice_commit" ]; then
    echo "turn-tests: libjuice is not at $juice_commit" >&2
    exit 1
fi
src=$dir/libjuice
"${CC:-cc}" -O2 -std=gnu11 -DJUICE_STATIC -DUSE_NETTLE=0 -I"$src/include" -I"$src/include/juice" -I"$src/src" \
    "$src"/src/*.c scripts/ci/juice-turn.c -lpthread -o "$dir/juice-turn"
"$dir/juice-turn" "$juice" "$user" "$juice_password" "$juice_realm" > "$dir/juice.log" 2>&1 &
pids=$!
names=
finish() {
    for n in $names; do docker logs "$n" 2>&1 | tail -n 40; done
    for log in coturn redirect juice xturn; do
        if [ -f "$dir/$log.log" ]; then tail -n 40 "$dir/$log.log"; fi
    done
    # shellcheck disable=SC2086  # lists of names and of process ids
    if [ -n "$names" ]; then docker rm -f $names > /dev/null 2>&1 || true; fi
    # shellcheck disable=SC2086
    kill $pids 2> /dev/null || true
    rm -rf "$dir"
}
trap finish EXIT

# The source of xturn at the pinned commit, checked by its id, with the
# Erlang/OTP and the Elixir of its .tool-versions. `mix deps.get
# --check-locked` takes each package of its mix.lock, checked by its hash.
git init -q "$dir/xturn"
sh scripts/retry.sh git -C "$dir/xturn" fetch -q --depth 1 https://github.com/Lazarus404/xturn.git "$xturn_commit"
git -C "$dir/xturn" -c advice.detachedHead=false checkout -q FETCH_HEAD
if [ "$(git -C "$dir/xturn" rev-parse HEAD)" != "$xturn_commit" ]; then
    echo "turn-tests: xturn is not at $xturn_commit" >&2
    exit 1
fi
export LANG=C.UTF-8 MIX_ENV=prod
otp=$(erl -noshell -eval 'R = erlang:system_info(otp_release),
    {ok, V} = file:read_file(filename:join([code:root_dir(), "releases", R, "OTP_VERSION"])),
    io:format("~s", [string:trim(V)]), halt().')
elixir=$(elixir -e 'IO.write(System.version())')
want_otp=$(sed -n 's/^erlang \([^ ]*\).*/\1/p' "$dir/xturn/.tool-versions")
want_elixir=$(sed -n 's/^elixir \([0-9.]*\).*/\1/p' "$dir/xturn/.tool-versions")
if [ "$otp" != "$want_otp" ] || [ "$elixir" != "$want_elixir" ]; then
    echo "turn-tests: xturn takes Erlang/OTP $want_otp and Elixir $want_elixir; here: $otp and $elixir" >&2
    exit 1
fi
(cd "$dir/xturn" && sh "$root/scripts/retry.sh" mix deps.get --check-locked --only prod)
# xsockets 1.0.0 opens each relay socket on Linux with the option recverr,
# which no gen_udp of Erlang/OTP knows (badarg): each Allocate fails. The
# patch removes that option alone (call C588).
git -C "$dir/xturn" apply "$root/scripts/ci/xturn-recverr.patch"
(cd "$dir/xturn" && mix compile) > "$dir/xturn-build.log" 2>&1 || { tail -n 40 "$dir/xturn-build.log"; exit 1; }
(cd "$dir/xturn" && XTURN_SERVER_IP=127.0.0.1 XTURN_STUN_PORT=$xturn XTURN_API_PORT=$xturn_api \
    exec mix run --no-halt --no-compile) > "$dir/xturn.log" 2>&1 &
pids="$pids $!"

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
    --cert="$dir/server.pem" --pkey="$dir/server.key" \
    --lt-cred-mech --realm=$realm --user=$user:$password --allow-loopback-peers --fingerprint \
    --stale-nonce=2 --max-allocate-lifetime=1800
first="--listening-port=$udp --tls-listening-port=$tls"
redirect="--listening-port=$redirect_udp --tls-listening-port=$redirect_tls"
redirect="$redirect --alternate-server=127.0.0.1:$udp --tls-alternate-server=127.0.0.1:$tls"
if [ -n "${POD_TURN_TEST_COTURN_BIN:-}" ]; then
    "$POD_TURN_TEST_COTURN_BIN" --version
    # A turnserver before 4.7 starts its CLI unless told, and refuses the
    # CLI with no password beside --allow-loopback-peers.
    # shellcheck disable=SC2086  # $first and $redirect are lists of options
    "$POD_TURN_TEST_COTURN_BIN" "$@" $first --no-cli > "$dir/coturn.log" 2>&1 &
    pids="$pids $!"
    # shellcheck disable=SC2086
    "$POD_TURN_TEST_COTURN_BIN" "$@" $redirect --no-cli > "$dir/redirect.log" 2>&1 &
    pids="$pids $!"
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
    names=$name
    # shellcheck disable=SC2086  # $first and $redirect are lists of options
    docker run -d --name "$name" --network host -v "$dir:$dir:ro" --entrypoint turnserver "$image" "$@" $first > /dev/null
    names="$name $name-redirect"
    # shellcheck disable=SC2086
    docker run -d --name "$name-redirect" --network host -v "$dir:$dir:ro" --entrypoint turnserver "$image" \
        "$@" $redirect > /dev/null
fi

# Wait for a Binding answer on each UDP port and for each TCP port (0:
# none), at most 60 s: TLS of coturn, the API of xturn.
python3 -I - "$udp" "$tls" "$redirect_udp" "$redirect_tls" "$juice" 0 "$xturn" "$xturn_api" <<'EOF'
import socket, sys, time
ports = [int(p) for p in sys.argv[1:]]
deadline = time.monotonic() + 60
request = bytes.fromhex("000100002112a442") + bytes(range(12))
while True:
    try:
        for udp, tls in zip(ports[::2], ports[1::2]):
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
                s.settimeout(0.5)
                s.sendto(request, ("127.0.0.1", udp))
                s.recvfrom(1500)
            if tls:
                socket.create_connection(("127.0.0.1", tls), 1).close()
        break
    except OSError:
        if time.monotonic() > deadline:
            sys.exit("turn-tests: coturn, libjuice or xturn did not listen within 60 s")
        time.sleep(0.5)
EOF

export POD_TURN_TEST_COTURN=required POD_TURN_TEST_COTURN_UDP=127.0.0.1:$udp \
    POD_TURN_TEST_COTURN_TCP=127.0.0.1:$udp \
    POD_TURN_TEST_COTURN_TLS=127.0.0.1:$tls POD_TURN_TEST_COTURN_CA="$dir/ca.pem" \
    POD_TURN_TEST_COTURN_REDIRECT_UDP=127.0.0.1:$redirect_udp POD_TURN_TEST_COTURN_REDIRECT_TLS=127.0.0.1:$redirect_tls \
    POD_TURN_TEST_JUICE=required POD_TURN_TEST_JUICE_UDP=127.0.0.1:$juice \
    POD_TURN_TEST_XTURN=required POD_TURN_TEST_XTURN_UDP=127.0.0.1:$xturn
set -- --locked -p pod-turn --all-features --test coturn --test coturn_options --test juice --test xturn
cargo test "$@" --no-run
# xturn forgets a user 300 s after it was given, so the build comes first.
curl -qfsS -X POST -H 'content-type: application/json' "http://127.0.0.1:$xturn_api/auth" \
    -d "{\"username\":\"$user\",\"password\":\"$xturn_password\"}" > /dev/null
# Each test binary runs, so that the output names each failed test.
out=$(cargo test "$@" --no-fail-fast -- --nocapture 2>&1) || { printf '%s\n' "$out"; exit 1; }
printf '%s\n' "$out"
# The TCP relay of RFC 6062: the ignored test runs here with the coturn
# that the job started, and its line is below.
tcp=$(cargo test --locked -p pod-turn --all-features --test coturn_tcp -- --ignored --nocapture 2>&1) ||
    { printf '%s\n' "$tcp"; exit 1; }
printf '%s\n' "$tcp"
line='coturn: an allocation over TCP relayed a TCP connection'
if ! printf '%s\n' "$tcp" | grep -qF "$line"; then
    echo "turn-tests: the run did not say: $line" >&2
    exit 1
fi
stun=$(POD_PROXY_TEST_STUN=required POD_PROXY_TEST_STUN_SERVER=127.0.0.1:$udp \
    cargo test --locked -p pod-proxy --all-features --test stun_coturn -- --nocapture 2>&1) ||
    { printf '%s\n' "$stun"; exit 1; }
printf '%s\n' "$stun"
# The tests run in parallel with --nocapture, so a line of a test can
# follow the text of the harness on its line: each line is found as a
# part of a line.
for line in 'coturn: an allocation over UDP relayed a Send and ChannelData' \
    'coturn: an allocation over TLS relayed a Send and padded ChannelData' \
    'coturn: a lifetime past the maximum got the maximum' \
    'coturn: a stale nonce got 438, and the request went again' \
    'coturn: an Allocate of another user on the same 5-tuple got 437' \
    'coturn: a Refresh of another user got 441' \
    'coturn: a certificate of another root was refused' \
    'coturn: an even port kept the next port for its token' \
    'coturn: DONT-FRAGMENT went in the Allocate and a Send' \
    'coturn: IPv6 got 440, and both families got IPv4' \
    'coturn: the ALPN stun.turn was chosen over TLS' \
    'coturn: a 300 of the first Allocate was followed to the alternate server' \
    'coturn: a redirect that the way out refuses failed with its words' \
    'juice: an Allocate with SHA-256 and USERHASH got a success' \
    'juice: a success signed with MD5 alone was refused' \
    'juice: a wrong password with SHA-256 got 401' \
    'xturn: an allocation with SHA-256 relayed a Send and ChannelData' \
    'xturn: each answer to a signed request had MESSAGE-INTEGRITY-SHA256 alone' \
    'xturn: a success of the Allocate with a changed MESSAGE-INTEGRITY-SHA256 was refused' \
    'xturn: a success of a Refresh with a changed MESSAGE-INTEGRITY-SHA256 was refused'; do
    if ! printf '%s\n' "$out" | grep -qF "$line"; then
        echo "turn-tests: the run did not say: $line" >&2
        exit 1
    fi
done
# The pinned image names the family that it could not give; coturn 4.6
# names none.
line='coturn: the ADDRESS-ERROR-CODE of IPv6 named 440'
if [ -z "${POD_TURN_TEST_COTURN_BIN:-}" ] && ! printf '%s\n' "$out" | grep -qF "$line"; then
    echo "turn-tests: the run did not say: $line" >&2
    exit 1
fi
for line in 'coturn: a Binding request mapped the address of its socket' \
    'coturn: a Binding request with an unknown attribute got 420'; do
    if ! printf '%s\n' "$stun" | grep -qF "$line"; then
        echo "turn-tests: the run did not say: $line" >&2
        exit 1
    fi
done
