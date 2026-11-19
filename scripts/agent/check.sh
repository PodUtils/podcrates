#!/bin/sh
# The checks of AGENTS.md before each push, as ci.yml runs them: fmt, clippy (also
# of the crates with no tokio and of the fuzz workspace), rustdoc and the tests, the
# check of the record (cargo todo check), the size of the Rust files
# (scripts/check-size.py, with its self-test), the tables of docs/design.md
# and README.md (scripts/check-design-paths.py), the order of cargo publish
# (scripts/check-publish-order.py), the rules of the workflows and each
# workflow that a document names (scripts/check-workflows.py, as ci.yml
# runs it), the wire vectors against docs/wire.md
# (scripts/check-wire-vectors.py, with its plant), no narrative history in
# the documents (scripts/check-history.py), a row of schemas/CHANGES.md for
# each change of a schema (scripts/check-schemas.py), the writer of the
# release index (scripts/release-index.py --self-test), no new repeated work
# (scripts/codegraph.py dups), that each planted defect applies (scripts/plant-each.sh
# --apply), shellcheck of the installer when it is there, and the static probes and
# the binary pod-release of x86_64 musl when zig is there (scripts/check-static.sh,
# scripts/check-strings.py). Print each exit
# code and the first faults; exit 1 on a fault. It also prints what the
# change against origin/main looks like (scripts/codegraph.py new), as a
# hint. With the argument notest, the tests are skipped.
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
    python3 -I scripts/check-probe.py && python3 -I scripts/check-loopback.py &&
    python3 -I scripts/check-private-replace.py && python3 -I scripts/check-results.py &&
    python3 -I scripts/check-wire-vectors.py && python3 -I scripts/check-wire-vectors.py --plant &&
    python3 -I scripts/check-history.py && python3 -I scripts/check-history.py --self-test &&
    python3 -I scripts/check-schemas.py && python3 -I scripts/release-index.py --self-test &&
    python3 -I scripts/codegraph.py dups && python3 -I scripts/codegraph.py --self-test &&
    sh scripts/plant-each.sh --apply; } > "$log/paths.log" 2>&1; p=$?
echo "paths $p"; [ "$p" -eq 0 ] || { rc=1; head -20 "$log/paths.log"; }
# What the change looks like: each new item with the existing items that
# look like it, and each clone of an existing body. A hint, not a fault.
python3 -I scripts/codegraph.py new --base origin/main 2>&1 | grep -v "^codegraph: 0 new items" | head -20
if command -v shellcheck > /dev/null 2>&1; then
    shellcheck -x --shell=sh scripts/install/core.sh scripts/install/core-net.sh scripts/install/podlab.sh \
        scripts/install/test.sh scripts/install/test-cases.sh > "$log/shellcheck.log" 2>&1; sc=$?
    echo "shellcheck $sc"; [ "$sc" -eq 0 ] || { rc=1; head -20 "$log/shellcheck.log"; }
fi
if [ -f examples/static-probe/Cargo.toml ] && command -v zig > /dev/null 2>&1; then
    t=x86_64-unknown-linux-musl
    # The binaries are in CARGO_TARGET_DIR when a worktree builds in its own.
    out=${CARGO_TARGET_DIR:-target}/$t/release
    { cargo zigbuild -q --release --target "$t" -p static-probe &&
        cargo zigbuild -q --release --target "$t" -p static-probe-lean &&
        cargo zigbuild -q --release --target "$t" -p pod-release --features cli --bin pod-release &&
        python3 -I scripts/check-probe.py &&
        sh scripts/check-static.sh "$out/static-probe" "$out/static-probe-lean" "$out/pod-release" &&
        python3 -I scripts/check-strings.py "$out/pod-release"; } > "$log/static.log" 2>&1; st=$?
    echo "static $st"; [ "$st" -eq 0 ] || { rc=1; tail -20 "$log/static.log"; }
else
    echo "static skipped (no examples/static-probe, or no zig: sh scripts/cloud-setup.sh)"
fi
if [ "${1:-}" != notest ]; then
    timeout 1800 cargo test --workspace --all-features > "$log/test.log" 2>&1; t=$?
    echo "test $t"; [ "$t" -eq 0 ] || { rc=1; grep -E "FAILED|panicked|failed" "$log/test.log" | head -30; }
fi
exit "$rc"
