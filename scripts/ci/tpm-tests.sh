#!/bin/sh
# The proof of pod-tpm against two TPMs in software that it did not write
# (T-1222, calls C151 and C927): swtpm with libtpms (the packages that
# scripts/ci/swtpm.sh pins), and the simulator of the reference
# implementation, microsoft/ms-tpm-20-ref at the commit of its tag
# v1.83r1, built here (it publishes no binary and no image). The build
# binds the simulator to 127.0.0.1, where its source binds each address.
# Each test starts its own TPM: swtpm on a Unix socket, the simulator on a
# free port of 127.0.0.1. POD_TPM_TEST_SWTPM=required and
# POD_TPM_TEST_MSSIM=required make a skip a failure, and the run must say
# each line of the proof, so the check cannot pass by a skip. The run
# prints the time of TPM2_CreatePrimary on each. It needs Ubuntu 24.04 on
# x86_64, git, a C compiler, autoconf, automake, autoconf-archive,
# pkg-config and the headers of OpenSSL. The build goes to RUNNER_TEMP,
# else TMPDIR, else /tmp.
set -eu
cd "$(dirname "$0")/../.." || exit 71
commit=ee21db0a941decd3cac67925ea3310873af60ab3
sh scripts/ci/swtpm.sh
dir=${RUNNER_TEMP:-${TMPDIR:-/tmp}}/ms-tpm-20-ref-$commit
simulator=$dir/TPMCmd/Simulator/src/tpm2-simulator
if [ ! -x "$simulator" ]; then
    rm -rf "$dir"
    git init -q "$dir"
    sh scripts/retry.sh git -C "$dir" fetch -q --depth 1 https://github.com/microsoft/ms-tpm-20-ref.git "$commit"
    git -C "$dir" checkout -q FETCH_HEAD
    if [ "$(git -C "$dir" rev-parse HEAD)" != "$commit" ]; then
        echo "tpm-tests: the source of the simulator is not the commit $commit" >&2
        exit 1
    fi
    server=$dir/TPMCmd/Simulator/src/TcpServer.c
    sed -i 's/^\(    MyAddress.sin_family = AF_INET;\)$/\1\n    MyAddress.sin_addr.s_addr = htonl(INADDR_LOOPBACK);/' "$server"
    if [ "$(grep -c 'htonl(INADDR_LOOPBACK)' "$server")" != 1 ]; then
        echo "tpm-tests: the bind of the simulator to 127.0.0.1 did not apply to $server" >&2
        exit 1
    fi
    start=$(date +%s)
    (cd "$dir/TPMCmd" && ./bootstrap > /dev/null && ./configure -q > /dev/null && make -s -j4 > /dev/null)
    echo "tpm-tests: ms-tpm-20-ref $commit built in $(($(date +%s) - start)) s"
fi
out=$(POD_TPM_TEST_SWTPM=required POD_TPM_TEST_MSSIM=required POD_TPM_TEST_MSSIM_BIN="$simulator" \
    cargo test --locked -p pod-tpm --features device,soft --test swtpm --test mssim --test soft \
    -- --nocapture 2>&1) || { printf '%s\n' "$out"; exit 1; }
printf '%s\n' "$out"
for tpm in swtpm ms-tpm-20-ref; do
    for line in 'the policy digest of PolicyPCR is the crate'"'"'s' \
        'a key sealed to PCR 7 signed, and p256 verified the signature' \
        'after PCR_Extend of PCR 7 the key was refused' \
        'the policy digest of PolicyAuthorize is the crate'"'"'s' \
        'a signed policy of the new state let the key sign again' \
        'a copy of the key file did not load in a second TPM' \
        'a signature by the password of a key under a policy was refused' \
        'a ticket of nothing was refused outside a trial'; do
        if ! printf '%s\n' "$out" | grep -qxF "$tpm: $line"; then
            echo "tpm-tests: the run did not say: $tpm: $line" >&2
            exit 1
        fi
    done
    if ! printf '%s\n' "$out" | grep -q "^$tpm: TPM2_CreatePrimary of the storage primary took "; then
        echo "tpm-tests: the run did not time TPM2_CreatePrimary on $tpm" >&2
        exit 1
    fi
done
