#!/bin/sh
# The checks of AGENTS.md before each push, as ci.yml runs them: fmt, clippy (also
# of the crates with no tokio and of the fuzz workspace), rustdoc and the tests, the
# check of the record (cargo todo check), the size of the Rust files
# (scripts/check-size.py, with its self-test), the tables of docs/design.md
# and README.md (scripts/check-design-paths.py), the order of cargo publish
# (scripts/check-publish-order.py), the rules of the workflows and each
# workflow that a document names (scripts/check-workflows.py, as ci.yml
# runs it), that each planted defect applies (scripts/plant-each.sh
# --apply), and the static probes of
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
# The crates with no tokio and the fuzz workspace, as the lint job of
# ci.yml checks them.
{ cargo clippy -p pod-core -p pod-tls --no-default-features --features pod-tls/pure,pod-tls/rsa -- -D warnings &&
    cargo fmt --manifest-path fuzz/Cargo.toml --check &&
    cargo clippy --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings; } > "$log/lint.log" 2>&1; l=$?
echo "lint $l"; [ "$l" -eq 0 ] || { rc=1; grep -E "^(error|warning|Diff)" -A6 "$log/lint.log" | head -30; }
RUSTDOCFLAGS="-D warnings" cargo doc -q --no-deps --workspace --all-features > "$log/doc.log" 2>&1; d=$?
echo "doc $d"; [ "$d" -eq 0 ] || { rc=1; grep -E "^(error|warning)" -A4 "$log/doc.log" | head -20; }
cargo run -q -p pod-todo -- check > "$log/record.log" 2>&1; r=$?
echo "record $r"; [ "$r" -eq 0 ] || { rc=1; head -20 "$log/record.log"; }
{ python3 -I scripts/check-size.py && python3 -I scripts/check-size.py --self-test; } > "$log/size.log" 2>&1; s=$?
echo "size $s"; [ "$s" -eq 0 ] || { rc=1; grep -v "^ok \|^check-size:" "$log/size.log" | head -20; }
{ python3 -I scripts/check-design-paths.py && python3 -I scripts/check-publish-order.py && python3 -I scripts/check-workflows.py &&
    python3 -I scripts/check-probe.py && sh scripts/plant-each.sh --apply; } > "$log/paths.log" 2>&1; p=$?
echo "paths $p"; [ "$p" -eq 0 ] || { rc=1; head -20 "$log/paths.log"; }
if [ -f examples/static-probe/Cargo.toml ] && command -v zig > /dev/null 2>&1; then
    t=x86_64-unknown-linux-musl
    { cargo zigbuild -q --release --target "$t" -p static-probe &&
        cargo zigbuild -q --release --target "$t" -p static-probe-lean &&
        python3 -I scripts/check-probe.py &&
        sh scripts/check-static.sh "target/$t/release/static-probe" "target/$t/release/static-probe-lean"; } > "$log/static.log" 2>&1; st=$?
    echo "static $st"; [ "$st" -eq 0 ] || { rc=1; tail -20 "$log/static.log"; }
else
    echo "static skipped (no examples/static-probe, or no zig: sh scripts/cloud-setup.sh)"
fi
if [ "${1:-}" != notest ]; then
    timeout 1800 cargo test --workspace --all-features > "$log/test.log" 2>&1; t=$?
    echo "test $t"; [ "$t" -eq 0 ] || { rc=1; grep -E "FAILED|panicked|failed" "$log/test.log" | head -30; }
fi
exit "$rc"
