#!/bin/sh
# The namespace rung for a caller with no capabilities (T-670): the entry
# of pod-enter in a user namespace (tests/userns.rs) and its refused exec
# (tests/exec_refused.rs, T-1080), the leg of
# pod-hostprobe (tests/userns_leg.rs), the rows of the host classes
# (tests/host_rows.rs: as root again as uid 65534), the labs of pod-lab
# (tests/lab.rs: the tmpfs of small-tmp in a user namespace, T-692), and the
# end of a tree of pod-supervise with a PID namespace in a user namespace
# (tests/tree.rs, T-1036). Each test runs as a user with no
# capabilities: as this user, or as uid 65534 where this user is root.
#
# Ubuntu 24.04 gives an unconfined process a user namespace with no
# capabilities (kernel.apparmor_restrict_unprivileged_userns=1): the
# script sets the value to 0, as root, before the run. uid 65534 must
# reach each test binary: the script copies it into a directory of mode
# 755. A skip line fails the run, so the check cannot pass by a skip.
#
# A bind keeps the flags of its source mount: where the host's /dev has
# nosuid or noexec, the check of the remount of the devices of pod-enter
# passes with no remount (T-736). As root, that test runs over a /dev
# with no such flag in a mount namespace of its own. As another user, the
# script prints the options of the host's /dev and, where `sudo -n` works,
# runs that test again with root.
#
#   sh scripts/ci/userns-tests.sh
set -eu
cd "$(dirname "$0")/../.." || exit 71
as_root() {
    if [ "$(id -u)" = 0 ]; then "$@"; else sudo -n "$@"; fi
}
restrict=/proc/sys/kernel/apparmor_restrict_unprivileged_userns
if [ -f "$restrict" ] && [ "$(cat "$restrict")" != 0 ]; then
    as_root sh -c "echo 0 > $restrict"
fi
dev=$(awk '$5 == "/dev" { o = $6 } END { print o }' /proc/self/mountinfo)
echo "userns-tests: the options of the host's /dev: ${dev:-no mount}"
bins=$(mktemp -d)
trap 'rm -rf "$bins"' EXIT
chmod 755 "$bins"
rc=0
for spec in "pod-enter userns" "pod-enter exec_refused" "pod-hostprobe userns_leg" "pod-hostprobe host_rows" "pod-lab lab" "pod-supervise tree"; do
    crate=${spec%% *}
    t=${spec#* }
    bin=$(cargo test --locked -p "$crate" --all-features --test "$t" --no-run 2>&1 |
        sed -n "s/.*Executable tests\/$t\.rs (\(.*\))\$/\1/p" | tail -1)
    [ -n "$bin" ] && [ -x "$bin" ] || { echo "userns-tests: no test binary of $crate tests/$t.rs" >&2; exit 1; }
    cp "$bin" "$bins/$t"
    chmod 755 "$bins/$t"
    if ! out=$("$bins/$t" 2>&1); then
        printf '%s\n' "$out"
        echo "userns-tests: $crate tests/$t.rs failed" >&2
        rc=1
        continue
    fi
    printf '%s\n' "$out"
    if printf '%s\n' "$out" | grep -q -e '^skipping' -e '^SKIP:'; then
        echo "userns-tests: $crate tests/$t.rs skipped" >&2
        rc=1
    fi
done
devices=a_caller_with_no_capabilities_gets_the_devices_of_the_host
if [ "$(id -u)" != 0 ] && sudo -n true 2> /dev/null; then
    # sudo clears the environment: the busybox of the image goes through env.
    set -- env
    [ -z "${PODVM_T486_BUSYBOX:-}" ] || set -- env "PODVM_T486_BUSYBOX=$PODVM_T486_BUSYBOX"
    if ! out=$(sudo -n "$@" "$bins/userns" "$devices" --exact 2>&1); then
        printf '%s\n' "$out"
        echo "userns-tests: pod-enter tests/userns.rs $devices failed with root" >&2
        rc=1
    else
        printf '%s\n' "$out"
        if printf '%s\n' "$out" | grep -q -e '^skipping' -e '^SKIP:' -e '^not seen here'; then
            echo "userns-tests: pod-enter tests/userns.rs $devices with root did not see its check" >&2
            rc=1
        fi
    fi
elif [ "$(id -u)" != 0 ]; then
    echo "userns-tests: no sudo: $devices does not run again with root"
fi
exit $rc
