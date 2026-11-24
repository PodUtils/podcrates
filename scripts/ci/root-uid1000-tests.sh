#!/bin/sh
# The tests of pod-extract and pod-rootfs that start a worker of uid 1000
# (the set-id bits of a file, the S_ISGID that the kernel drops from a
# directory, a closed tree, a closed etc, the extended attributes: T-581,
# T-605, T-606, T-598, T-957) run the worker only as root, and skip the
# proof as another user. GNU tar with the arguments of incusd, which runs
# it as root, over a directory that a newer layer closed (T-1295) skips as
# another user too. This script
# builds the test binaries as the user and runs them as root: with `sudo
# -n` on a runner, as they are where the caller is root. As root, each
# test passes only when its worker passed. A skip line fails the run, and
# each test must report one pass, so the check cannot pass by a skip.
set -eu
cd "$(dirname "$0")/../.." || exit 71
root=
[ "$(id -u)" -eq 0 ] || root="sudo -n"
# The test binary of a crate, built as this user: of its library for
# "lib", else of the test target of that name.
test_bin() {
    if [ "$2" = lib ]; then
        cargo test --locked -p "$1" --all-features --lib --no-run 2>&1 |
            sed -n 's/.*Executable unittests src\/lib\.rs (\(.*\))$/\1/p' | tail -1
    else
        cargo test --locked -p "$1" --all-features --test "$2" --no-run 2>&1 |
            sed -n "s/.*Executable tests\/$2\.rs (\(.*\))\$/\1/p" | tail -1
    fi
}
rc=0
for spec in \
    "pod-extract lib drive::setid::set_id_bits_survive_the_write" \
    "pod-extract lib drive::dirmodes::a_second_extraction_over_a_closed_tree_passes" \
    "pod-extract lib drive::dirmodes::an_extraction_over_a_tree_that_its_owner_cannot_read_passes" \
    "pod-extract lib drive::xattrs::extended_attributes_are_set_or_named" \
    "pod-extract flatten gnu_tar_as_root_takes_a_directory_that_a_newer_layer_closed" \
    "pod-rootfs lib closed::a_closed_etc_is_completed_and_keeps_its_mode"; do
    crate=${spec%% *}
    rest=${spec#* }
    target=${rest%% *}
    test=${rest#* }
    bin=$(test_bin "$crate" "$target")
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
