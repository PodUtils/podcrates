#!/bin/sh
# Check that the workspace and its tests compile for macOS, as the macOS
# jobs of ci.yml and nightly.yml build them: `cargo check --tests` for
# x86_64-apple-darwin, with zig as the C compiler of the build scripts
# (ring). No code is made and nothing runs, so the output stays small. Each
# error and each warning is a fault: a Unix path that uses an item of
# Linux alone (`libc::SOCK_CLOEXEC`, a field of a struct) fails here.
# Exit 1 on a fault, 2 when a tool is missing.
#
# Not checked here: pod-gsr and static-probe. The build script of
# openssl-sys (pod-gsr, and static-probe through it) does not build with
# zig for macOS; the macOS jobs of ci.yml build them.
#
#   sh scripts/agent/check-macos.sh
set -u
cd "$(dirname "$0")/../.." || exit 1
target=x86_64-apple-darwin
command -v zig >/dev/null || { echo "check-macos: zig is missing" >&2; exit 2; }
rustup target list --installed | grep -qx "$target" || rustup target add "$target" || exit 2
tools=${TMPDIR:-/tmp}/podcrates-zig-macos
mkdir -p "$tools"
# zig reads no target triple of the form x86_64-apple-darwin, and no -arch:
# the wrapper drops the flags that the cc crate adds.
cat > "$tools/cc" <<'CC'
#!/bin/sh
for a in "$@"; do shift; case "$a" in --target=*|-arch|x86_64|-mmacosx-version-min=*) ;; *) set -- "$@" "$a";; esac; done
exec zig cc -target x86_64-macos "$@"
CC
printf '#!/bin/sh\nexec zig ar "$@"\n' > "$tools/ar"
chmod +x "$tools/cc" "$tools/ar"
out="$tools/check.log"
CC_x86_64_apple_darwin="$tools/cc" AR_x86_64_apple_darwin="$tools/ar" \
  cargo check --locked --workspace --all-features --tests --target "$target" --keep-going \
  --exclude pod-gsr --exclude static-probe >"$out" 2>&1
rc=$?
faults=$(grep -cE '^(warning|error)' "$out")
if [ "$rc" -ne 0 ] || [ "$faults" -ne 0 ]; then
  grep -E '^(warning|error)' -A4 "$out" | head -60
  echo "check-macos: exit $rc, $faults faults (the log: $out)"
  exit 1
fi
echo "check-macos: 0 faults"
