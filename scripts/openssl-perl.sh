#!/bin/sh
# Give the build of OpenSSL (openssl-src, the TLS with SRP of pod-gsr) a
# perl of Windows on a Windows runner. The Configure of OpenSSL for MSVC
# refuses the perl of MSYS that Git Bash puts first on the PATH. The script
# writes OPENSSL_SRC_PERL to $GITHUB_ENV: Strawberry Perl, or a perl of
# Windows on the PATH, else Strawberry Perl from Chocolatey.
#
#   sh scripts/openssl-perl.sh
#
# Exit 0 when the variable is written, 1 when no perl of Windows is found.
set -u
: "${GITHUB_ENV:?the script writes to the environment file of a GitHub job}"

windows_perl() {
    for p in /c/Strawberry/perl/bin/perl.exe "/c/Program Files/Strawberry/perl/bin/perl.exe"; do
        [ -x "$p" ] && { echo "$p"; return 0; }
    done
    # `where` gives the Windows path of each perl on the PATH; a perl of
    # MSYS or of Git is not one of Windows.
    where perl 2> /dev/null | tr -d '\r' | grep -viE '\\(git|msys64)\\' | head -1
}

perl=$(windows_perl)
if [ -z "$perl" ] && command -v choco > /dev/null 2>&1; then
    sh scripts/retry.sh choco install strawberryperl -y --no-progress > /dev/null || exit 1
    perl=$(windows_perl)
fi
[ -n "$perl" ] || { echo "openssl-perl: no perl of Windows" >&2; exit 1; }
echo "openssl-perl: $perl"
echo "OPENSSL_SRC_PERL=$perl" >> "$GITHUB_ENV"
