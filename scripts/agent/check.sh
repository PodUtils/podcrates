#!/bin/sh
# The checks of AGENTS.md before each push: fmt, clippy and the tests, the
# check of the record (cargo todo check), the size of the Rust files
# (scripts/check-size.py, with its self-test), and the static probe of
# x86_64 musl when zig is there (scripts/check-static.sh). Print each exit
# code and the first faults; exit 1 on a fault. With the argument notest,
# the tests are skipped.
#
#   sh scripts/agent/check.sh [notest]
set -u
cd "$(dirname "$0")/../.." || exit 1
log=${TMPDIR:-/tmp}/podcrates-check
mkdir -p "$log"
rc=0
cargo fmt --all --check > "$log/fmt.log" 2>&1; f=$?
echo "fmt $f"; [ "$f" -eq 0 ] || { rc=1; head -20 "$log/fmt.log"; }
cargo clippy --workspace --all-targets --all-features -- -D warnings > "$log/clippy.log" 2>&1; c=$?
echo "clippy $c"; [ "$c" -eq 0 ] || { rc=1; grep -E "^(error|warning)" -A6 "$log/clippy.log" | head -40; }
cargo run -q -p pod-todo -- check > "$log/record.log" 2>&1; r=$?
echo "record $r"; [ "$r" -eq 0 ] || { rc=1; head -20 "$log/record.log"; }
{ python3 -I scripts/check-size.py && python3 -I scripts/check-size.py --self-test; } > "$log/size.log" 2>&1; s=$?
echo "size $s"; [ "$s" -eq 0 ] || { rc=1; grep -v "^ok \|^check-size:" "$log/size.log" | head -20; }
if [ -f examples/static-probe/Cargo.toml ] && command -v zig > /dev/null 2>&1; then
    t=x86_64-unknown-linux-musl
    { cargo zigbuild -q --release --target "$t" -p static-probe &&
        sh scripts/check-static.sh "target/$t/release/static-probe"; } > "$log/static.log" 2>&1; st=$?
    echo "static $st"; [ "$st" -eq 0 ] || { rc=1; tail -20 "$log/static.log"; }
else
    echo "static skipped (no examples/static-probe, or no zig: sh scripts/cloud-setup.sh)"
fi
if [ "${1:-}" != notest ]; then
    timeout 1800 cargo test --workspace --all-features > "$log/test.log" 2>&1; t=$?
    echo "test $t"; [ "$t" -eq 0 ] || { rc=1; grep -E "FAILED|panicked|failed" "$log/test.log" | head -30; }
fi
exit "$rc"
