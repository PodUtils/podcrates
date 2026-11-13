#!/bin/sh
# Check that the tests of the workspace compile for Windows, as the
# Windows jobs of nightly.yml build them: `cargo check --tests` for
# x86_64-pc-windows-gnu, with zig as the C compiler of the build scripts
# (ring, openssl-src). No code is made and nothing runs, so the output
# stays small. Each error and each warning is a fault: a test that uses
# a Unix item needs `#[cfg(unix)]`. Exit 1 on a fault, 2 when a tool is
# missing.
#
#   sh scripts/agent/check-windows.sh
set -u
cd "$(dirname "$0")/../.." || exit 1
target=x86_64-pc-windows-gnu
command -v zig >/dev/null || { echo "check-windows: zig is missing" >&2; exit 2; }
rustup target list --installed | grep -qx "$target" || rustup target add "$target" || exit 2
tools=${TMPDIR:-/tmp}/podcrates-zig-windows
mkdir -p "$tools"
# zig reads no target triple of the form x86_64-pc-windows-gnu: the
# wrapper drops the flag that the cc crate adds, and one that zig lacks.
cat > "$tools/cc" <<'CC'
#!/bin/sh
for a in "$@"; do shift; case "$a" in --target=*|-Wa,-mbig-obj) ;; *) set -- "$@" "$a";; esac; done
exec zig cc -target x86_64-windows-gnu "$@"
CC
printf '#!/bin/sh\nexec zig ar "$@"\n' > "$tools/ar"
chmod +x "$tools/cc" "$tools/ar"
out="$tools/check.log"
CC_x86_64_pc_windows_gnu="$tools/cc" AR_x86_64_pc_windows_gnu="$tools/ar" \
  cargo check --locked --workspace --all-features --tests --target "$target" --keep-going >"$out" 2>&1
rc=$?
faults=$(grep -cE '^(warning|error)' "$out")
if [ "$rc" -ne 0 ] || [ "$faults" -ne 0 ]; then
  grep -E '^(warning|error)' -A4 "$out" | head -60
  echo "check-windows: exit $rc, $faults faults (the log: $out)"
  exit 1
fi
echo "check-windows: 0 faults"
