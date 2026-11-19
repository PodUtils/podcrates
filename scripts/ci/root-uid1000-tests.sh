#!/bin/sh
# The tests of pod-extract and pod-rootfs that start a worker of uid 1000
# (the set-id bits of a file, the S_ISGID that the kernel drops from a
# directory, a closed tree, a closed etc, the extended attributes: T-581,
# T-605, T-606, T-598, T-957) run the worker only as root, and skip the
# proof as another user. This script
# builds the test binaries as the user and runs them as root: with `sudo
# -n` on a runner, as they are where the caller is root. As root, each
# test passes only when its worker passed. A skip line fails the run, and
# each test must report one pass, so the check cannot pass by a skip.
set -eu
cd "$(dirname "$0")/../.." || exit 71
root=
[ "$(id -u)" -eq 0 ] || root="sudo -n"
# The test binary of the library of a crate, built as this user.
lib_bin() {
    cargo test --locked -p "$1" --all-features --lib --no-run 2>&1 |
        sed -n 's/.*Executable unittests src\/lib\.rs (\(.*\))$/\1/p' | tail -1
}
rc=0
for spec in \
    "pod-extract drive::setid::set_id_bits_survive_the_write" \
    "pod-extract drive::dirmodes::a_second_extraction_over_a_closed_tree_passes" \
    "pod-extract drive::dirmodes::an_extraction_over_a_tree_that_its_owner_cannot_read_passes" \
    "pod-extract drive::xattrs::extended_attributes_are_set_or_named" \
    "pod-rootfs closed::a_closed_etc_is_completed_and_keeps_its_mode"; do
    crate=${spec%% *}
    test=${spec#* }
    bin=$(lib_bin "$crate")
    [ -n "$bin" ] && [ -x "$bin" ] || { echo "root-uid1000-tests: no test binary of $crate" >&2; exit 1; }
    if ! out=$($root "$bin" --exact "$test" --nocapture 2>&1); then
        printf '%s\n' "$out"
        echo "root-uid1000-tests: $crate $test failed" >&2
        rc=1
        continue
    fi
    printf '%s\n' "$out"
    if printf '%s\n' "$out" | grep -q -e '^skipping' -e 'SKIP:'; then
        echo "root-uid1000-tests: $crate $test skipped as root" >&2
        rc=1
    elif ! printf '%s\n' "$out" | grep -q '^test result: ok\. 1 passed'; then
        echo "root-uid1000-tests: $crate $test did not run" >&2
        rc=1
    fi
done
exit $rc
