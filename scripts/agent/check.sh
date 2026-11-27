#!/bin/sh
# The checks of AGENTS.md before each push, as ci.yml runs them: fmt, clippy (also
# of the crates with no tokio and of the fuzz workspace), rustdoc and the tests, the
# check of the record (cargo todo check), the size of the Rust files
# (scripts/check-size.py, with its self-test), the tables of docs/design.md
# and README.md (scripts/check-design-paths.py), the order of cargo publish
# (scripts/check-publish-order.py), each fork of vendor/ against its upstream
# crate plus its patches (scripts/check-vendor.py, with its self-test), the
# rules of the workflows and each
# workflow that a document names (scripts/check-workflows.py, as ci.yml
# runs it), the wire vectors against docs/wire.md
# (scripts/check-wire-vectors.py, with its plant), no narrative history in
# the documents (scripts/check-history.py), a row of schemas/CHANGES.md for
# each change of a schema (scripts/check-schemas.py), the writer of the
# release index (scripts/release-index.py --self-test), the self-tests of the
# watchers (scripts/agent/watch.py, runs.py), no new repeated work
# (scripts/codegraph.py dups), that each planted defect applies
# (scripts/plant-each.sh --apply), shellcheck of the installer and of the
# start scripts when it is there, and the static probes and the binary
# pod-release of x86_64 musl when zig is there (scripts/check-static.sh,
# scripts/check-strings.py). Print each exit code and the first faults; exit 1 on a fault. It also prints what the
# change against origin/main looks like (scripts/codegraph.py new), as a
# hint. With the argument notest, the tests are skipped.
#
# First it makes the cargo target fit this checkout
# (scripts/agent/stale_target.py, T-1384): a worktree of an agent can
# leave artifacts in a shared target that cargo takes as fresh. It also
# checks the merge driver of the record (scripts/agent/merge_todo.py), the
# commit messages since origin/main (.githooks/commit-msg), the session
# start and the setup (sh -n, the refusal of a bad flag, and the JSON of
# .agents/environment.json). It loads scripts/cloud-env.sh itself. A run
# that passes writes the tree that it checked into
# .work/agent/checked-trees (a full run) or .work/agent/linted-trees
# (notest): .githooks/pre-push refuses a push of another tree.
#
#   sh scripts/agent/check.sh [notest]
set -u
cd "$(dirname "$0")/../.." || exit 1
# The toolchain and the cargo limits in a harness that does not load
# cloud-env.sh in each shell (no CLAUDE_ENV_FILE). A value that is set
# already stays.
# shellcheck source=scripts/cloud-env.sh
. ./scripts/cloud-env.sh
# check-workflows.py needs PyYAML: the python3 of the system has it on
# some hosts where the python3 of PATH does not.
yaml_py=python3
if ! python3 -I -c 'import yaml' > /dev/null 2>&1 && [ -x /usr/bin/python3 ] &&
    /usr/bin/python3 -I -c 'import yaml' > /dev/null 2>&1; then
    yaml_py=/usr/bin/python3
fi
# The file creation mask of CI: a test of the mode of a file it makes
# gives the same result here.
umask 022
log=${TMPDIR:-/tmp}/podcrates-check
mkdir -p "$log"
rc=0
# The tree of the working copy (tracked and new files), with no change to
# the index.
tree_now() {
    idx=$(mktemp) || return 1
    cp "$(git rev-parse --git-path index)" "$idx" &&
        GIT_INDEX_FILE=$idx git add -A > /dev/null 2>&1 &&
        GIT_INDEX_FILE=$idx git write-tree
    e=$?
    rm -f "$idx"
    return "$e"
}
tree=$(tree_now)
python3 -I scripts/agent/stale_target.py > "$log/target.log" 2>&1; g=$?
echo "target $g"; cat "$log/target.log"; [ "$g" -eq 0 ] || rc=1
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
{ python3 -I scripts/check-design-paths.py && python3 -I scripts/check-publish-order.py && "$yaml_py" -I scripts/check-workflows.py &&
    python3 -I scripts/check-probe.py && python3 -I scripts/check-loopback.py &&
    python3 -I scripts/check-vendor.py && python3 -I scripts/check-vendor.py --self-test &&
    python3 -I scripts/check-private-replace.py && python3 -I scripts/check-results.py &&
    python3 -I scripts/check-wire-vectors.py && python3 -I scripts/check-wire-vectors.py --plant &&
    python3 -I scripts/check-history.py && python3 -I scripts/check-history.py --self-test &&
    python3 -I scripts/check-schemas.py && python3 -I scripts/release-index.py --self-test &&
    python3 -I scripts/agent/watch.py --self-test && python3 -I scripts/agent/runs.py --self-test &&
    python3 -I scripts/agent/stale_target.py --self-test && python3 -I scripts/agent/merge_todo.py --self-test &&
    sh .githooks/commit-msg --self-test &&
    { ! git rev-parse -q --verify origin/main > /dev/null || sh .githooks/commit-msg --range origin/main..HEAD; } &&
    sh -n scripts/session-start.sh && sh -n .claude/session-start.sh && sh -n scripts/cloud-env.sh &&
    sh -n scripts/cloud-setup.sh && python3 -I -m json.tool .agents/environment.json > /dev/null &&
    { sh scripts/session-start.sh --bad-flag 2> /dev/null; [ "$?" -eq 64 ]; } &&
    sh -n scripts/qa-bundle.sh && sh -n .githooks/pre-push &&
    python3 -I scripts/codegraph.py dups && python3 -I scripts/codegraph.py --self-test &&
    sh scripts/plant-each.sh --apply; } > "$log/paths.log" 2>&1; p=$?
echo "paths $p"; [ "$p" -eq 0 ] || { rc=1; head -20 "$log/paths.log"; }
# What the change looks like: each new item with the existing items that
# look like it, and each clone of an existing body. A hint, not a fault.
python3 -I scripts/codegraph.py new --base origin/main 2>&1 | grep -v "^codegraph: 0 new items" | head -20
if command -v shellcheck > /dev/null 2>&1; then
    shellcheck -x --shell=sh scripts/install/core.sh scripts/install/core-net.sh scripts/install/podlab.sh \
        scripts/install/test.sh scripts/install/test-cases.sh scripts/session-start.sh scripts/cloud-setup.sh \
        scripts/cloud-env.sh scripts/agent/check.sh .claude/session-start.sh > "$log/shellcheck.log" 2>&1; sc=$?
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
    timeout 1800 cargo test --workspace --all-features --no-fail-fast > "$log/test.log" 2>&1; t=$?
    # Not "failed": each summary line has "0 failed" and would hide the cause.
    echo "test $t"; [ "$t" -eq 0 ] || { rc=1; grep -E "FAILED|panicked|^error|^failures:|^---- " "$log/test.log" | head -30; }
fi
if [ "$rc" -eq 0 ]; then
    stamps=.work/agent/checked-trees
    [ "${1:-}" = notest ] && stamps=.work/agent/linted-trees
    if [ -z "$tree" ] || [ "$(tree_now)" != "$tree" ]; then
        echo "the tree changed during the checks: not recorded"
    else
        mkdir -p .work/agent && echo "$tree" >> "$stamps" && echo "tree $tree in $stamps"
    fi
fi
exit "$rc"
