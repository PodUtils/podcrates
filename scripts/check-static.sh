#!/bin/sh
# Check that each BINARY is static (docs/design.md, section 7):
#
# * ELF (Linux): no NEEDED entry and no program interpreter (readelf);
# * Mach-O (macOS): only the libraries of the system (otool -L): Apple
#   allows no static libSystem;
# * PE (Windows): only the DLLs of the system, and no DLL of the C runtime
#   (objdump -p, OBJDUMP=llvm-objdump, or dumpbin /dependents).
#
#   sh scripts/check-static.sh BINARY...
#
# Exit 1 at the first binary that is not static, 2 when a tool is missing.
set -u
[ "$#" -gt 0 ] || { echo "usage: check-static.sh BINARY..." >&2; exit 2; }

# The libraries of macOS that a binary may name.
MACOS_OK='^(/usr/lib/libSystem\.B\.dylib|/usr/lib/libiconv\.2\.dylib|/usr/lib/libresolv\.9\.dylib|/usr/lib/libc\+\+\.1\.dylib|/usr/lib/libobjc\.A\.dylib|/System/Library/Frameworks/(CoreFoundation|Security|SystemConfiguration|CoreServices|IOKit)\.framework/.*)$'
# The DLLs of the C runtime that a static Windows binary must not name.
WINDOWS_CRT='^(vcruntime[0-9]*\.dll|msvcp[0-9]*\.dll|ucrtbase\.dll|api-ms-win-crt-.*\.dll|libgcc_s.*\.dll|libwinpthread.*\.dll)$'

kind() {
    magic=$(od -An -tx1 -N4 "$1" | tr -d ' \n')
    case "$magic" in
        7f454c46) echo elf ;;
        cffaedfe|cefaedfe|cafebabe) echo macho ;;
        4d5a*) echo pe ;;
        *) echo unknown ;;
    esac
}

for b in "$@"; do
    [ -f "$b" ] || { echo "$b: no such file" >&2; exit 2; }
    case "$(kind "$b")" in
        elf)
            command -v readelf > /dev/null || { echo "check-static: no readelf" >&2; exit 2; }
            if readelf -d "$b" 2> /dev/null | grep -q NEEDED; then
                echo "$b: dynamic dependencies:" >&2
                readelf -d "$b" | grep NEEDED >&2
                exit 1
            fi
            if readelf -l "$b" 2> /dev/null | grep -q INTERP; then
                echo "$b: asks for a program interpreter" >&2
                exit 1
            fi
            ;;
        macho)
            command -v otool > /dev/null || { echo "check-static: no otool" >&2; exit 2; }
            bad=$(otool -L "$b" | tail -n +2 | awk '{print $1}' | grep -Ev "$MACOS_OK" || true)
            if [ -n "$bad" ]; then
                echo "$b: libraries that are not of the system:" >&2
                echo "$bad" >&2
                exit 1
            fi
            ;;
        pe)
            od=${OBJDUMP:-objdump}
            if command -v "$od" > /dev/null && "$od" -p "$b" > /dev/null 2>&1; then
                dlls=$("$od" -p "$b" | awk '/DLL Name:/ {print $3}')
            elif command -v dumpbin > /dev/null; then
                dlls=$(dumpbin /dependents "$b" | awk '/\.dll/ {print $1}')
            else
                echo "check-static: no objdump and no dumpbin" >&2
                exit 2
            fi
            bad=$(printf '%s\n' "$dlls" | tr 'A-Z' 'a-z' | grep -E "$WINDOWS_CRT" || true)
            if [ -n "$bad" ]; then
                echo "$b: dynamic dependencies of the C runtime:" >&2
                echo "$bad" >&2
                exit 1
            fi
            ;;
        *)
            echo "$b: not an ELF, Mach-O or PE file" >&2
            exit 2
            ;;
    esac
    echo "static: $b ($(wc -c < "$b") bytes)"
done
