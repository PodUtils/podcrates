#!/bin/sh
# SPDX-License-Identifier: 0BSD
# The test of the installer core (docs/install.md) with the installer of
# podlab, as a release stamps it (scripts/install/pins.py), against a
# mirror on the loopback (scripts/install/mirror.py). No network.
#
#   sh scripts/install/test.sh [--strict] [--case NAME]...
#
# The cases: pins, first, second, dryrun, upgrade, broken, corrupt, cut,
# offline, space, downloaders, proxy, addpath, shells, exe, sig, labs,
# lint. With no --case, each one runs. A tool that is not on this host (a
# shell, a downloader, podlab, shellcheck) skips its part; with --strict a
# skip fails. A part that the host OS does not have (the list of na) is
# "n/a" and does not fail. Each run of the installer has a time limit of
# LIMIT seconds (INSTALL_TEST_LIMIT, default 30). PODLAB is the podlab
# binary of the assets and of the labs (default: target/release/podlab).
# shellcheck disable=SC2016 # the scripts that the test writes expand at their own run.
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
here=$root/scripts/install
strict=0 cases=''
while [ $# -gt 0 ]; do
    case $1 in
        --strict) strict=1 ;;
        --case) shift; cases="$cases ${1:-}" ;;
        *) echo "usage: sh scripts/install/test.sh [--strict] [--case NAME]..." >&2; exit 64 ;;
    esac
    shift
done
[ -n "$cases" ] || cases='pins first second dryrun upgrade broken corrupt cut offline space downloaders proxy addpath shells exe sig labs lint'
limit=${INSTALL_TEST_LIMIT:-30}
# Not under /tmp: the labs of small-tmp and noexec-tmp mount a tmpfs there.
work=$root/target/install-test.$$
pids=''
cleanup() {
    for p in $pids; do kill "$p" 2> /dev/null; done
    rm -rf "$work"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir -p "$work/home" "$work/m/good" "$work/m/bad" "$work/m/broken" || exit 71
n=0 skips=0 nas=0
ok() { n=$((n + 1)); echo "ok $n $*"; }
fail() {
    echo "FAIL: $*" >&2
    [ ! -f "$work/out" ] || sed 's/^/  | /' "$work/out" >&2
    exit 1
}
# The parts that an OS does not have: no busybox for macOS; no dash,
# busybox or wget in Git Bash; no labs off Linux; no POSIX signal from
# Git Bash to a native downloader. shellcheck gives the same result on
# each host: the job on Linux runs it. Linux has no such part.
os=$(uname -s)
case $os in Linux) os=linux ;; Darwin) os=macos ;; MINGW* | MSYS* | CYGWIN*) os=windows ;; *) os=other ;; esac
na() { case $os in macos) echo busybox labs shellcheck ;; windows) echo dash busybox wget labs cut shellcheck ;; esac; }
# Git Bash: a native program (python, curl) needs SYSTEMROOT for a socket.
osenv=''
[ "$os" != windows ] || osenv="SYSTEMROOT=${SYSTEMROOT:-C:\\Windows}"
skip() { # PART TEXT
    case " $(na) " in
        *" $1 "*) echo "n/a: $2 ($1: not on $os)"; nas=$((nas + 1)); return 0 ;;
    esac
    echo "skip: $2"
    skips=$((skips + 1))
    [ "$strict" = 0 ] || fail "a skip with --strict: $2"
}
expect() { grep -E -q -e "$1" "$work/out" || fail "$2: no line matches '$1'"; }
gets() { if [ -f "$work/log" ]; then grep -c -e "$1" "$work/log"; else echo 0; fi; }
# sha256sum, else shasum of macOS.
shacmd='sha256sum'
command -v sha256sum > /dev/null 2>&1 || shacmd='shasum -a 256'
# shellcheck disable=SC2086 # shacmd is a command and its options.
sum() { $shacmd < "$1" | cut -d ' ' -f 1; }
# The count of the temporary files in DIR, and the names that are not hidden.
temps() {
    c=0
    for f in "$1"/.podlab-install.*; do [ ! -e "$f" ] || c=$((c + 1)); done
    echo "$c"
}
visible() { for f in "$1"/*; do [ ! -e "$f" ] || printf '%s' "${f##*/}"; done; }

# The text rules of a command line: printable ASCII, tab and newline only,
# and no id of a work entry.
text_rules() {
    if LC_ALL=C grep -n "$(printf '[^\t -~]')" "$1" > /dev/null; then fail "a byte that is not printable ASCII in the output"; fi
    if grep -E -q '(^|[^A-Za-z])(T-[0-9]{3}|C[0-9]{3})([^0-9]|$)' "$1"; then fail "an id of a work entry in the output"; fi
}

# A directory of PATH with a cosign of the host becomes a directory of
# links with no cosign: only the stand-in of the case sig is a cosign.
hostpath='' i=0
ifs=$IFS IFS=:
for d in $PATH; do
    if [ -x "$d/cosign" ]; then
        i=$((i + 1))
        mkdir -p "$work/path$i"
        for f in "$d"/*; do [ "${f##*/}" = cosign ] || ln -s "$f" "$work/path$i/"; done
        d=$work/path$i
    fi
    hostpath=${hostpath:+$hostpath:}$d
done
IFS=$ifs
PATH=$hostpath
export PATH

# The target and the .exe of this host, by the core itself.
# shellcheck source=scripts/install/core.sh
target=$(INST_NAME=x INST_ENV=X INST_TARGETS=any; . "$here/core.sh"; inst_detect_target; printf '%s %s' "$inst_target" "$inst_exe")
exe=${target#* } target=${target% *}
[ -n "$target" ] || fail "the core finds no target for this host"
case $target in x86_64-*) other=aarch64-unknown-linux-musl ;; *) other=x86_64-unknown-linux-musl ;; esac
asset=podlab-$target$exe bin=podlab$exe
# The file of Windows, for the case exe on each host.
wasset='podlab-x86_64-pc-windows-msvc.exe'

# The assets: the real podlab when it is built, else a program of sh.
podlab=${PODLAB:-$root/target/release/podlab}
version=9.9.9-test
fake() { # FILE VERSION
    printf '#!/bin/sh\ncase ${1:-} in\n--version) echo "podlab %s" ;;\nlist) echo profiles ;;\n*) exit 2 ;;\nesac\n' "$2" > "$1"
    # Padding, so that a slow download takes some seconds.
    head -c 1048576 /dev/zero | tr '\0' '#' >> "$1"
    chmod 0755 "$1"
}
if [ -x "$podlab" ] && v=$("$podlab" --version 2> /dev/null) && [ "${v%% *}" = podlab ]; then
    version=${v#podlab }
    cp "$podlab" "$work/m/good/$asset"
else
    podlab=''
    fake "$work/m/good/$asset" "$version"
fi
fake "$work/m/good/podlab-$other" "$version"
# The assets of the release: each name once (on Windows, asset is wasset).
assets="$asset podlab-$other"
[ "$asset" = "$wasset" ] || { fake "$work/m/good/$wasset" "$version" && assets="$assets $wasset"; }
# shellcheck disable=SC2086 # assets is a list of names.
python3 -I "$here/pins.py" sums "$work/m/good" $assets > /dev/null || fail "pins.py sums"
installer=$work/podlab-install.sh
python3 -I "$here/pins.py" stamp "$here/podlab.sh" --version "$version" --assets "$work/m/good" --out "$installer" > /dev/null ||
    fail "pins.py stamp"
cp "$work/m/good/$asset" "$work/m/bad/$asset"
printf 'x' >> "$work/m/bad/$asset"
# An asset that agrees with its pin and does not run.
printf '#!/bin/sh\nexit 1\n' > "$work/m/broken/$asset"
cp "$work/m/good/podlab-$other" "$work/m/broken/"
python3 -I "$here/pins.py" sums "$work/m/broken" "$asset" "podlab-$other" > /dev/null || fail "pins.py sums"
python3 -I "$here/pins.py" stamp "$here/podlab.sh" --version "$version" --assets "$work/m/broken" --out "$work/broken.sh" > /dev/null ||
    fail "pins.py stamp"

serve() { # PORTVAR [RATE]
    # No output to the pipe of the job: a process that holds it holds the job.
    python3 -I "$here/mirror.py" "$work/m" "$work/port.$1" "$work/log" ${2:+"$2"} > /dev/null 2> "$work/serve.$1" &
    pids="$pids $!"
    i=0
    while [ ! -s "$work/port.$1" ]; do
        i=$((i + 1))
        [ "$i" -lt 100 ] || fail "the mirror does not start"
        sleep 0.1
    done
}
serve port
port=$(cat "$work/port.port")
serve slow 300000
slow=$(cat "$work/port.slow")
mirror=http://127.0.0.1:$port/good

# run SHELL PREFIX [ARG...]: the installer with a clean environment, its
# output in $work/out. The variables env, xpath and inst give more
# environment, more PATH and another installer; lab gives a profile of
# podlab; mirror gives the mirror. reset sets each one back.
good=$mirror
reset() { env='' xpath='' inst='' lab='' mirror=$good; }
reset
run() {
    sh_=$1 rp=$2
    shift 2
    t0=$(date +%s)
    # shellcheck disable=SC2086 # osenv, env and lab are lists of words; sh_ is a command.
    env -i $osenv PATH="${xpath:+$xpath:}$PATH" HOME="$work/home" PODLAB_MIRROR="$mirror" $env \
        ${lab:+"$podlab" run "$lab" --} $sh_ "${inst:-$installer}" ${rp:+--prefix "$rp"} "$@" > "$work/out" 2>&1
    rc=$?
    t=$(($(date +%s) - t0))
    [ "$t" -le "$limit" ] || fail "a run took $t s, over the limit of $limit s"
    text_rules "$work/out"
    return "$rc"
}

c_pins() {
    python3 -I "$here/pins.py" check "$here/podlab.sh" > "$work/out" 2>&1 || fail "pins.py check of the tree copy"
    p=$work/p-pins
    # shellcheck disable=SC2086 # osenv is a list of words.
    env -i $osenv PATH="$PATH" HOME="$work/home" sh "$here/podlab.sh" --prefix "$p" > "$work/out" 2>&1 && fail "the tree copy installs"
    expect 'has no pins of a release' "the tree copy refuses"
    [ ! -e "$p" ] || fail "the tree copy made $p"
    mkdir -p "$work/planted"
    for f in SHA256SUMS $assets; do cp "$work/m/good/$f" "$work/planted/"; done
    cp "$work/m/bad/$asset" "$work/planted/"
    python3 -I "$here/pins.py" stamp "$here/podlab.sh" --version 1.0.0 --assets "$work/planted" --out "$work/x.sh" > "$work/out" 2>&1 &&
        fail "pins.py stamps a file that does not agree with SHA256SUMS"
    expect "$asset: the SHA-256 is" "pins.py names the file"
    run sh '' --version 0.0.1 && fail "--version of another release installs"
    expect 'holds the pins of' "--version of another release"
    ok "pins: the tree copy has no pins and refuses; a wrong asset fails the stamp; another version refuses"
}

first() { # SHELL PREFIX
    g=$(gets "get /good/$asset")
    run "$1" "$2" || fail "the first run ($1)"
    [ "$(sum "$2/$bin")" = "$(sum "$work/m/good/$asset")" ] || fail "$2/$bin is not the asset ($1)"
    expect "installed $2/$bin " "the first run says what it installed"
    expect 'self-check: podlab list: ok' "the self-check"
    expect "export PATH=\"$2:" "the PATH line"
    [ "$(gets "get /good/$asset")" = $((g + 1)) ] || fail "the first run did not download the asset once ($1)"
    [ "$(temps "$2")" = 0 ] || fail "the first run left a temporary file ($1)"
}

second() { # SHELL PREFIX
    g=$(gets "/good/$asset")
    run "$1" "$2" || fail "the second run ($1)"
    expect 'is already installed .*nothing was downloaded' "the second run says so"
    [ "$(gets "/good/$asset")" = "$g" ] || fail "the second run downloaded again ($1)"
}

c_first() { first sh "$work/p1" && ok "first: a fresh install, its self-check and its PATH line"; }
c_second() {
    [ -f "$work/p1/$bin" ] || first sh "$work/p1"
    second sh "$work/p1" && ok "second: a second run downloads nothing and says so"
}

c_dryrun() {
    g=$(gets /good/)
    run sh "$work/p-dry" --dry-run || fail "the dry run"
    expect 'dry run: no file was changed' "the dry run says so"
    expect "dry run: $mirror/$asset to $work/p-dry/$bin" "the dry run names the file"
    [ ! -e "$work/p-dry" ] || fail "the dry run made the prefix"
    [ "$(gets /good/)" = "$g" ] || fail "the dry run downloaded"
    # The two variables of one proxy give one line (PodUtils/podnet#650).
    env="http_proxy=http://127.0.0.1:9 https_proxy=http://127.0.0.1:9"
    run sh "$work/p-dry" --dry-run || fail "the dry run with a proxy"
    reset
    [ "$(grep -c '^.*proxy: http://127.0.0.1:9$' "$work/out")" = 1 ] || fail "the proxy line is not there once"
    ok "dryrun: the plan, no file and no download; one line for one proxy"
}

c_upgrade() {
    p=$work/p-up
    mkdir -p "$p"
    printf '#!/bin/sh\necho podlab 0.0.1\n' > "$p/$bin"
    first sh "$p"
    ok "upgrade: a file that does not agree with its pin is replaced"
}

c_broken() {
    p=$work/p-broken
    first sh "$p"
    inst=$work/broken.sh mirror=http://127.0.0.1:$port/broken
    run sh "$p" && fail "an asset that does not run is installed"
    reset
    expect "$asset does not run on this host" "the refusal names the file"
    [ "$(sum "$p/$bin")" = "$(sum "$work/m/good/$asset")" ] || fail "the old podlab changed"
    [ "$(temps "$p")" = 0 ] || fail "a temporary file stayed"
    ok "broken: the new file runs once before it replaces the old one"
}

c_corrupt() {
    p=$work/p-bad
    mirror=http://127.0.0.1:$port/bad
    run sh "$p" && fail "a corrupt file installs"
    reset
    expect "error: $asset: the SHA-256 is [0-9a-f]{64}, not the pin" "the error names the file"
    [ ! -e "$p/$bin" ] || fail "a corrupt file is in place"
    [ "$(temps "$p")" = 0 ] || fail "a temporary file stayed"
    ok "corrupt: a file that does not agree with its pin fails by name"
}

dfl='import os, signal, sys
for s in (signal.SIGINT, signal.SIGQUIT):
    signal.signal(s, signal.SIG_DFL)
os.execvp(sys.argv[1], sys.argv[1:])'

# Wait for the first bytes of the download in PREFIX, or of the file in
# place when no temporary file comes.
wait_temp() {
    i=0
    while :; do
        for f in "$1"/.podlab-install.*."$bin"; do [ ! -s "$f" ] || return 0; done
        # A run that writes to the name in place.
        [ "$(sum "$1/$bin")" = "$old" ] || return 0
        i=$((i + 1))
        [ "$i" -lt 200 ] || fail "no download started in $1"
        sleep 0.05
    done
}

c_cut() {
    [ "$os" != windows ] || { skip cut "cut: a signal of Git Bash does not stop a native downloader"; return 0; }
    p=$work/p-cut
    mkdir -p "$p"
    printf '#!/bin/sh\necho podlab 0.0.1\n' > "$p/$bin"
    old=$(sum "$p/$bin")
    slowurl=http://127.0.0.1:$slow/good
    # KILL of the run and its children: no trap runs. A new session (no
    # setsid on macOS: python3) makes them one process group.
    # shellcheck disable=SC2086 # osenv is a list of words.
    env -i $osenv PATH="$PATH" HOME="$work/home" PODLAB_MIRROR="$slowurl" python3 -I -c 'import os, sys
os.setsid()
os.execvp(sys.argv[1], sys.argv[1:])' sh "$installer" --prefix "$p" > "$work/out" 2>&1 &
    pid=$!
    wait_temp "$p"
    kill -s KILL -- "-$pid" 2> /dev/null || kill -s KILL "$pid"
    wait "$pid" 2> /dev/null
    [ "$(sum "$p/$bin")" = "$old" ] || fail "a run cut by KILL changed $p/$bin"
    [ "$(visible "$p")" = "$bin" ] || fail "a run cut by KILL left a file that is not hidden: $(visible "$p")"
    run sh "$p" || fail "the run after a cut"
    expect 'removed .* of a run that stopped' "the next run removes the files of the cut run"
    [ "$(temps "$p")" = 0 ] || fail "a temporary file stayed after the next run"
    [ "$(sum "$p/$bin")" = "$(sum "$work/m/good/$asset")" ] || fail "the run after a cut did not install"
    printf '#!/bin/sh\necho podlab 0.0.1\n' > "$p/$bin"
    # TERM, INT and HUP: the trap stops the download and removes its file.
    for sig in TERM INT HUP; do
        : > "$work/log.cut"
        # An asynchronous command of sh ignores INT: dfl gives it back.
        # shellcheck disable=SC2086 # osenv is a list of words.
        env -i $osenv PATH="$PATH" HOME="$work/home" PODLAB_MIRROR="$slowurl" \
            python3 -I -c "$dfl" sh "$installer" --prefix "$p" > "$work/out" 2>&1 &
        pid=$!
        wait_temp "$p"
        c=$(gets "cut /good/$asset")
        kill -s "$sig" "$pid"
        wait "$pid"
        rc=$?
        case $sig in TERM) want=143 ;; INT) want=130 ;; HUP) want=129 ;; esac
        [ "$rc" = "$want" ] || fail "a run stopped by $sig exits $rc, not $want"
        [ "$(temps "$p")" = 0 ] || fail "a run stopped by $sig left a temporary file"
        [ "$(visible "$p")" = "$bin" ] || fail "a run stopped by $sig left a file"
        i=0
        while [ "$(gets "cut /good/$asset")" = "$c" ]; do
            i=$((i + 1))
            [ "$i" -lt 50 ] || fail "a run stopped by $sig did not stop its download"
            sleep 0.1
        done
    done
    [ "$(sum "$p/$bin")" = "$old" ] || fail "a stopped run changed $p/$bin"
    ok "cut: a run cut by KILL, TERM, INT or HUP leaves the old file and no half file"
}

c_offline() {
    b=$work/podlab-$version.tar.gz
    python3 -I "$here/pins.py" bundle "$b" "podlab-$version" "$installer" "$work/m/good/$asset" \
        "$work/m/good/podlab-$other" "$work/m/good/SHA256SUMS" > /dev/null || fail "pins.py bundle"
    # No network and no downloader: a probe of one would fail the run.
    for src in "$b" dir; do
        p=$work/p-off-$src
        [ "$src" = "$b" ] || { mkdir -p "$work/unpacked" && (cd "$work/unpacked" && gzip -dc < "$b" | tar -xf -) && src=$work/unpacked; }
        mirror=http://127.0.0.1:1/none env='PODLAB_DOWNLOADER=none'
        run sh "$p" --offline "$src" || fail "the offline install from $src"
        reset
        [ "$(sum "$p/$bin")" = "$(sum "$work/m/good/$asset")" ] || fail "the offline install from $src"
        [ "$(temps "$p")" = 0 ] || fail "the offline install left a temporary file"
    done
    python3 -I "$here/pins.py" bundle "$work/bad.tar.gz" "podlab-$version" "$work/m/bad/$asset" > /dev/null || fail "pins.py bundle"
    run sh "$work/p-off-bad" --offline "$work/bad.tar.gz" && fail "a corrupt bundle installs"
    expect "$asset: the SHA-256 is" "the error names the file"
    ok "offline: a bundle (.tar.gz or a directory) with no network; a corrupt bundle fails"
}

c_space() {
    mkdir -p "$work/shim"
    printf '#!/bin/sh\necho "Filesystem 1024-blocks Used Available Capacity Mounted on"\necho "fake 100 99 1 99%% /"\n' > "$work/shim/df"
    chmod 0755 "$work/shim/df"
    p=$work/p-space
    xpath=$work/shim
    run sh "$p" && fail "an install with no free space"
    reset
    expect "has 1 KiB free; the install needs [0-9]+ KiB.*--prefix DIR" "the error names the free space and the knob"
    [ ! -e "$p/$bin" ] || fail "an install with no free space wrote a file"
    ok "space: the free space of the prefix before a download, with the knob that fixes it"
}

c_addpath() {
    p=$work/p-path
    run sh "$p" --add-path "$work/home/.profile" || fail "--add-path"
    run sh "$p" --add-path "$work/home/.profile" || fail "--add-path again"
    [ "$(grep -c -F "export PATH=\"$p:" "$work/home/.profile")" = 1 ] || fail "--add-path wrote the line not once"
    xpath=$p
    run sh "$p" || fail "a run with the prefix in PATH"
    reset
    expect "$p is in PATH" "the run sees the prefix in PATH"
    ok "addpath: the PATH line goes to a profile only when asked, once"
}

c_shells() {
    for s in dash 'bash --posix' 'busybox sh' sh; do
        # shellcheck disable=SC2086 # s is a command and its options.
        set -- $s
        # shellcheck disable=SC2086 # s is a command and its options.
        if ! command -v "$1" > /dev/null 2>&1 || ! $s -c 'exit 0' 2> /dev/null; then
            skip "$1" "shells: $s is not on this host"
            continue
        fi
        p=$work/p-sh-$1
        first "$s" "$p"
        second "$s" "$p"
        ok "shells: $s: a first and a second run"
    done
}

c_labs() {
    if [ -z "$podlab" ]; then
        skip labs "labs: no podlab binary (build it, or give PODLAB)"
        return 0
    fi
    for prof in small-tmp noexec-tmp noexec-home inherited-fds no-proc no-passwd-entry ro-etc-ssl; do
        p=$work/p-lab-$prof
        # The default prefix first: under HOME, which noexec-home holds.
        lab=$prof
        run sh ''
        rc=$?
        if [ "$rc" = 125 ]; then
            reset
            skip labs "labs: this host cannot hold $prof"
            continue
        fi
        if [ "$prof" = noexec-home ]; then
            [ "$rc" != 0 ] || fail "an install into a home with noexec"
            expect 'cannot run programs: its file system is mounted with noexec' "the reason of the refusal"
        else
            [ "$rc" = 0 ] || fail "the install into the default prefix in $prof"
        fi
        first sh "$p"
        second sh "$p"
        reset
        ok "labs: $prof: a first and a second run"
    done
    rm -rf "$work/home/.local"
    # A default prefix on a noexec mount gives way to the next default, with
    # a line (PodUtils/podnet#650).
    lab=noexec-tmp env="XDG_BIN_HOME=/tmp/podlab-bin"
    run sh ''
    rc=$?
    reset
    if [ "$rc" = 125 ]; then
        skip labs "labs: this host cannot hold noexec-tmp"
    else
        [ "$rc" = 0 ] || fail "the install with a default prefix on noexec"
        expect 'cannot run programs \(its file system is mounted with noexec\): the install goes to .*/home/\.local/bin' \
            "the line of the next default"
        [ -x "$work/home/.local/bin/$bin" ] || fail "the install into the next default"
        ok "labs: noexec-tmp: a default prefix on noexec gives way to ~/.local/bin"
    fi
    rm -rf "$work/home/.local"
}

c_lint() {
    for s in dash 'bash --posix' 'busybox sh'; do
        # shellcheck disable=SC2086 # s is a command and its options.
        set -- $s
        command -v "$1" > /dev/null 2>&1 || continue
        # shellcheck disable=SC2086 # s is a command and its options.
        $s -n "$installer" || fail "$s -n: a syntax error in the installer"
    done
    if command -v shellcheck > /dev/null 2>&1; then
        # From the root: the source directives name paths from there.
        (cd "$root" && shellcheck -x --shell=sh scripts/install/core.sh scripts/install/core-net.sh \
            scripts/install/podlab.sh scripts/install/test.sh scripts/install/test-cases.sh "$installer") || fail "shellcheck"
        ok "lint: shellcheck --shell=sh"
    else
        skip shellcheck "lint: no shellcheck"
    fi
}

# The cases downloaders, proxy, exe and sig.
# shellcheck source=scripts/install/test-cases.sh
. "$here/test-cases.sh"

for c in $cases; do
    command -v "c_$c" > /dev/null 2>&1 || fail "no case $c"
    "c_$c"
done
echo "install-test: $n passed, $skips skipped, $nas n/a ($os)"
