#!/usr/bin/env python3
"""Check each Rust toolchain that a consumer pins against the rust-version
of podcrates (T-045).

  python3 -I scripts/check-consumer-toolchains.py [--min VERSION] [DIR]

DIR is the checkout of the consumer (default: the current directory). A
raise of rust-version must hold for each job that builds the consumer, not
only for its CI: podnet's release built armv7 with Alpine's packaged Rust
1.96.1, under the 1.99 of podcrates. The check reads:

* each workflow in .github/workflows/ (*.yml, *.yaml): a key whose name holds
  TOOLCHAIN or RUST, or is STABLE, NIGHTLY or MSRV (`RUST_TOOLCHAIN: "1.99.1"`),
  the input `toolchain:` of an action, `rustup toolchain install X`,
  `rustup default X`, `cargo +X`, and each `rust:TAG` image;
* each Dockerfile and Containerfile of the tree: a `rust:TAG` image;
* rust-toolchain.toml and rust-toolchain (`channel`);
* `rust-version` in the Cargo.toml of the root.

A version (`1.99`, `1.99.1`, `rust:1.99.1-alpine`, `1.99-x86_64-...`) and a
dated nightly or beta (its version by the six-week cadence of Rust) are
compared with VERSION, the rust-version of podcrates by default. A pin that
the check cannot read is a note, not a fault: a channel (`stable`), a tag of
a major version (`rust:1-alpine`), a variable (`$RUST_TOOLCHAIN`, read where
it is set), and Alpine's packaged Rust (`apk add rust`), whose version is the
package's.

Each pin is one line: `ok`, `FAULT` or `note`, the file and the line, the
pin and why. Exit 0: no pin below VERSION. Exit 1: a pin below VERSION.
Exit 2: DIR or the rust-version of podcrates cannot be read.
"""

import datetime
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SKIP_DIRS = {".git", "target", "vendor", "node_modules"}

KEY = re.compile(r"^\s*(?:-\s+)?([A-Za-z_][A-Za-z0-9_-]*)\s*:\s*(.+?)\s*$")
RUSTUP = re.compile(r"\brustup\s+(?:toolchain\s+install|install|default|override\s+set)((?:\s+\S+)*)")
CARGO_PLUS = re.compile(r"\bcargo\s+\+([^\s\"']+)")
IMAGE = re.compile(r"(?:^|[\s/\"'=@])rust:([A-Za-z0-9][A-Za-z0-9._-]*)")
APK_RUST = re.compile(r"\bapk\s+add\b[^\n]*\s(?:rust|cargo)(?:\s|$|['\"])")
CHANNEL = re.compile(r"^\s*channel\s*=\s*\"([^\"]+)\"")
RUST_VERSION = re.compile(r"^\s*rust-version\s*=\s*\"([^\"]+)\"")
VERSION = re.compile(r"^(\d+)\.(\d+)(?:\.(\d+))?")
DATED = re.compile(r"^(nightly|beta)-(\d{4})-(\d{2})-(\d{2})")
# Rust 1.0 came out on this day; a new stable comes each six weeks.
FIRST = datetime.date(2015, 5, 15)


def parse(text):
    """The version of a toolchain text, or None, and the words of a note."""
    text = text.strip().strip("\"'")
    m = VERSION.match(text)
    if m:
        return (int(m.group(1)), int(m.group(2)), int(m.group(3) or 0)), None
    m = DATED.match(text)
    if m:
        day = datetime.date(int(m.group(2)), int(m.group(3)), int(m.group(4)))
        stable = (day - FIRST).days // 42
        return (1, stable + (2 if m.group(1) == "nightly" else 1), 0), None
    if text.split("-")[0] in ("stable", "beta", "nightly"):
        return None, f"the channel {text} moves; its version is not read"
    return None, f"{text} is not a version"


def looks_like_toolchain(value):
    value = value.strip().strip("\"'")
    return bool(VERSION.match(value) or DATED.match(value)) or value.split("-")[0] in ("stable", "beta", "nightly")


def pins_of_workflow(text):
    """(line, kind, value) of each pin of a workflow."""
    for n, line in enumerate(text.splitlines(), 1):
        if line.lstrip().startswith("#"):
            continue
        m = KEY.match(line)
        if m:
            key, value = m.group(1), m.group(2).split(" #")[0].strip()
            named = "TOOLCHAIN" in key.upper() or "RUST" in key.upper() or key.upper() in ("STABLE", "NIGHTLY", "MSRV")
            if named and "$" not in value:
                items = value.strip("[]").split(",") if value.startswith("[") else [value]
                for item in items:
                    if looks_like_toolchain(item):
                        yield n, key, item.strip().strip("\"'")
        m = RUSTUP.search(line)
        if m:
            words = [w for w in m.group(1).split() if not w.startswith("-")]
            if words and "$" not in words[0] and looks_like_toolchain(words[0]):
                yield n, "rustup", words[0].strip("\"'")
        for m in CARGO_PLUS.finditer(line):
            if "$" not in m.group(1):
                yield n, "cargo +", m.group(1)
        yield from pins_of_images(n, line)
        if APK_RUST.search(line):
            yield n, "apk", "rust"


def pins_of_images(n, line):
    for m in IMAGE.finditer(line):
        yield n, "image", "rust:" + m.group(1)


def judge(kind, value, least, least_text):
    """ok, FAULT or note, and why."""
    if kind == "apk":
        return "note", "Alpine's packaged Rust: its version is the package's, not read here"
    text = value[len("rust:"):] if kind == "image" else value
    if kind == "image" and re.match(r"^\d+(-|$)", text):
        return "note", f"the tag {text} of a major version moves; its version is not read"
    found, why = parse(text)
    if kind == "image" and found is None:
        return "note", f"the tag {text} names no version"
    if found is None:
        return "note", why
    shown = ".".join(map(str, found))
    if found < least:
        return "FAULT", f"{shown} is below {least_text}"
    return "ok", f"{shown}, {least_text} at least"


def files(root):
    workflows = sorted(p for p in (root / ".github" / "workflows").glob("*") if p.suffix in (".yml", ".yaml"))
    images = []
    for path in sorted(root.rglob("*")):
        if any(part in SKIP_DIRS for part in path.relative_to(root).parts):
            continue
        if path.is_file() and (path.name.startswith("Dockerfile") or path.name.startswith("Containerfile")):
            images.append(path)
    return workflows, images


def pins(root):
    workflows, images = files(root)
    for path in workflows:
        for n, kind, value in pins_of_workflow(path.read_text(encoding="utf-8", errors="replace")):
            yield path, n, kind, value
    for path in images:
        for n, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
            if line.lstrip().startswith("#"):
                continue
            for _, kind, value in pins_of_images(n, line):
                yield path, n, kind, value
            if APK_RUST.search(line):
                yield path, n, "apk", "rust"
    for name in ("rust-toolchain.toml", "rust-toolchain"):
        path = root / name
        if not path.is_file():
            continue
        for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            m = CHANNEL.match(line)
            if m:
                yield path, n, "channel", m.group(1)
            elif name == "rust-toolchain" and line.strip() and "=" not in line and not line.startswith("["):
                yield path, n, "channel", line.strip()
    path = root / "Cargo.toml"
    if path.is_file():
        for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            m = RUST_VERSION.match(line)
            if m:
                yield path, n, "rust-version", m.group(1)


def least_of_podcrates():
    for line in (ROOT / "Cargo.toml").read_text(encoding="utf-8").splitlines():
        m = RUST_VERSION.match(line)
        if m:
            return m.group(1)
    return None


def main(argv):
    least_text, dirs = None, []
    args = iter(argv)
    for arg in args:
        if arg == "--min":
            least_text = next(args, None)
        elif arg.startswith("-"):
            print(f"check-consumer-toolchains: unknown option {arg}", file=sys.stderr)
            return 2
        else:
            dirs.append(arg)
    least_text = least_text or least_of_podcrates()
    least, _ = parse(least_text or "")
    root = Path(dirs[0] if dirs else ".").resolve()
    if least is None or not root.is_dir():
        print(f"check-consumer-toolchains: no version to compare ({least_text}) or no directory {root}", file=sys.stderr)
        return 2
    counts = {"ok": 0, "FAULT": 0, "note": 0}
    seen = set()
    for path, n, kind, value in pins(root):
        if (path, n, value) in seen:
            continue
        seen.add((path, n, value))
        verdict, why = judge(kind, value, least, least_text)
        counts[verdict] += 1
        print(f"{verdict:<5} {path.relative_to(root)}:{n}: {kind} {value}: {why}")
    total = sum(counts.values())
    print(
        f"check-consumer-toolchains: {total} pins of {root.name}, {counts['FAULT']} below {least_text}, "
        f"{counts['note']} notes"
    )
    return 1 if counts["FAULT"] else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
