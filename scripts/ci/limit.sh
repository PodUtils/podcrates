#!/bin/sh
# Run COMMAND with a limit of real time and print its seconds. At the limit
# the command and its process group get TERM, and KILL 30 s later. The exit
# status is 124 at the limit, else the status of the command. A job step
# uses it for one test binary: a binary that does not end fails with its
# name, and the next binary runs. cargo calls it as the runner of the tests
# (scripts/ci/test-limited.sh).
#
#   sh scripts/ci/limit.sh SECONDS COMMAND [ARGUMENT...]
#
# The tool is the GNU `timeout` (Linux, Git Bash), else `gtimeout`, else
# perl (the base system of macOS has no `timeout`). LIMIT_TOOL=perl forces
# perl.
set -u
[ "$#" -ge 2 ] || { echo "usage: limit.sh SECONDS COMMAND [ARGUMENT...]" >&2; exit 2; }
limit=$1
shift

tool=perl
if [ "${LIMIT_TOOL:-}" != perl ]; then
    for name in timeout gtimeout; do
        if "$name" --version 2> /dev/null | grep -q GNU; then
            tool=$name
            break
        fi
    done
fi

start=$(date +%s)
if [ "$tool" = perl ]; then
    # The child leads its own group, so that the kill reaches its children.
    perl -e '
        my $limit = shift @ARGV;
        my $pid = fork();
        defined $pid or die "limit.sh: fork: $!\n";
        if (!$pid) { setpgrp(0, 0); exec { $ARGV[0] } @ARGV; print STDERR "limit.sh: $ARGV[0]: $!\n"; exit 127; }
        my $late = 0;
        $SIG{ALRM} = sub {
            $late++;
            kill($late == 1 ? "TERM" : "KILL", -$pid);
            alarm 30 if $late == 1;
        };
        alarm $limit;
        while (waitpid($pid, 0) < 0) { last unless $!{EINTR}; }
        my $st = $?;
        exit 124 if $late;
        exit($st & 127 ? 128 + ($st & 127) : $st >> 8);
    ' "$limit" "$@"
else
    "$tool" -k 30 "$limit" "$@"
fi
rc=$?
secs=$(($(date +%s) - start))
case $rc in
    124 | 137) echo "limit.sh: no end after $limit s, ended: $*" >&2 ;;
esac
echo "== $secs s, exit $rc: $*" >&2
exit "$rc"
