#!/bin/sh
# Run each fuzz target of fuzz/ (T-017), or the TARGETs named, for SECONDS
# each (60 by default), with cargo-fuzz and libFuzzer on a pinned nightly
# Rust and the address sanitizer. libFuzzer reads the seeds of
# fuzz/seeds/TARGET/ and keeps the inputs that it finds in
# fuzz/corpus/TARGET/.
#
#   sh scripts/fuzz.sh [SECONDS [TARGET...]]
#
# fuzz.yml runs it each week and by hand. scripts/agent/check.sh does not
# run it: the checks use stable Rust. ci.yml builds the targets on stable
# Rust and runs their stable test (fuzz/README.md).
#
# Linux with glibc only: libFuzzer on musl dies of a signal at once, so the
# script refuses musl with 69. It needs a C++ compiler. cargo-fuzz builds
# with the stable Rust of rust-toolchain.toml.
#
# Exit 0 when each target ran its time with no crash, else 1: the input of
# each crash is printed in hex and stays in fuzz/artifacts/TARGET/.

set -u

NIGHTLY=nightly-2026-11-01
CARGO_FUZZ_VERSION=0.13.2
# The SHA-256 of cargo-fuzz-$CARGO_FUZZ_VERSION-x86_64-unknown-linux-musl.tar.gz,
# the release archive (T-355): GitHub's digest of the asset, measured again
# at its first download.
CARGO_FUZZ_SHA256=b5b704018b63e0f151c17a057ac53b5111e1db545d1b9f72fee79f08a545931c

RUN_FOR=${1:-60}
[ $# -gt 0 ] && shift
case $RUN_FOR in
'' | *[!0-9]*)
    echo "fuzz.sh: SECONDS must be a whole number, not '$RUN_FOR'" >&2
    exit 64
    ;;
esac

cd "$(dirname "$0")/.." || exit 1

if ldd --version 2>&1 | grep -qi musl; then
    echo "fuzz.sh: libFuzzer on musl dies of a signal at once; run it with glibc" >&2
    exit 69
fi
command -v c++ > /dev/null 2>&1 || { echo "fuzz.sh: libFuzzer needs a C++ compiler (c++)" >&2; exit 69; }

sh scripts/retry.sh rustup toolchain install "$NIGHTLY" --profile minimal > /dev/null || exit 1
# On Linux x86_64 the release binary (static, musl) is a download of some
# seconds; elsewhere `cargo install` builds it.
if ! cargo fuzz --version 2> /dev/null | grep -q " $CARGO_FUZZ_VERSION\$"; then
    if [ "$(uname -s)-$(uname -m)" = Linux-x86_64 ]; then
        tools=${RUNNER_TEMP:-$PWD/target}/tools/bin
        sh scripts/ci/fetch-tool.sh \
            "https://github.com/rust-fuzz/cargo-fuzz/releases/download/$CARGO_FUZZ_VERSION/cargo-fuzz-$CARGO_FUZZ_VERSION-x86_64-unknown-linux-musl.tar.gz" \
            "$CARGO_FUZZ_SHA256" cargo-fuzz "$tools" || exit 1
        PATH=$tools:$PATH
        export PATH
    else
        sh scripts/retry.sh cargo install cargo-fuzz --version "$CARGO_FUZZ_VERSION" --locked || exit 1
    fi
fi
# The host's own target: cargo-fuzz builds for the platform that it was
# built for, and on musl the sanitizer does not work.
host=$(rustc +"$NIGHTLY" -vV | sed -n 's/^host: //p')
[ -n "$host" ] || exit 1

if [ $# -eq 0 ]; then
    # shellcheck disable=SC2046  # one word for each target
    set -- $(cargo +"$NIGHTLY" fuzz list --fuzz-dir fuzz) || exit 1
fi
echo "fuzz.sh: $# targets, $RUN_FOR s each, $NIGHTLY, cargo-fuzz $CARGO_FUZZ_VERSION, $host"

failed=0
for target in "$@"; do
    echo
    echo "== $target"
    mkdir -p "fuzz/corpus/$target"
    log=fuzz/$target.log
    # One allocation of 64 MiB from an input of some KiB is a defect; an
    # input that runs for 10 s is a hang.
    timeout $((RUN_FOR + 900)) cargo +"$NIGHTLY" fuzz run --fuzz-dir fuzz -s address --target "$host" "$target" \
        "fuzz/corpus/$target" "fuzz/seeds/$target" -- \
        -max_total_time="$RUN_FOR" -malloc_limit_mb=64 -rss_limit_mb=2048 -timeout=10 -print_final_stats=1 \
        > "$log" 2>&1
    rc=$?
    grep -E '^(Done|stat::number_of_executed_units|stat::new_units_added)' "$log"
    if [ "$rc" -ne 0 ]; then
        failed=1
        echo "FAIL  $target: exit $rc"
        tail -40 "$log"
        for input in fuzz/artifacts/"$target"/*; do
            [ -f "$input" ] || continue
            echo "input $input:"
            od -An -tx1 "$input" | head -20
        done
    else
        echo "ok    $target"
    fi
    rm -f "$log"
done
exit "$failed"
