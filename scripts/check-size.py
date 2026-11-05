#!/usr/bin/env python3
"""Check the size of each Rust file of the repository.

  python3 -I scripts/check-size.py [--self-test]

No `.rs` file has more than LIMIT lines (docs/decisions.md, File size).
There is no allowlist: a longer file is split.

The scan reads each `.rs` file under the root of the repository, but not in
`vendor/` (the forks keep their own files), in a directory named `target`,
or in a directory whose name starts with `.`. A scan that reads fewer than
MIN_FILES files is a fault: a scan of nothing proves nothing.

--self-test runs the check on temporary trees with planted faults. It exits
0 only when the check finds each fault and passes the control tree.

Exit 0: no fault. Exit 1: a fault, one line for each.
"""

import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LIMIT = 500
MIN_FILES = 10
SKIP_TOP = {"vendor"}


def count_lines(path: Path) -> int:
    return len(path.read_text(encoding="utf-8", errors="replace").splitlines())


def rust_files(root: Path) -> list[str]:
    """Each `.rs` file under `root` that the rule covers, relative to it."""
    out = []
    stack = [root]
    while stack:
        here = stack.pop()
        for entry in here.iterdir():
            if entry.is_dir() and not entry.is_symlink():
                skip = entry.name == "target" or entry.name.startswith(".")
                if here == root and entry.name in SKIP_TOP:
                    skip = True
                if not skip:
                    stack.append(entry)
            elif entry.suffix == ".rs" and entry.is_file():
                out.append(entry.relative_to(root).as_posix())
    return sorted(out)


def check(root: Path, min_files: int) -> list[str]:
    files = rust_files(root)
    faults = []
    if len(files) < min_files:
        faults.append(f"{len(files)} Rust files read, fewer than {min_files}: the scan read too little to pass")
    for rel in files:
        lines = count_lines(root / rel)
        if lines > LIMIT:
            faults.append(f"{rel}: {lines} lines, over {LIMIT}: split it")
    return faults


def plant(root: Path, rel: str, lines: int) -> None:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(f"// {i}\n" for i in range(lines)), encoding="utf-8")


def self_test() -> int:
    """Each planted fault must be found, and the control must pass."""

    def control(root):
        for i in range(3):
            plant(root, f"crates/a/src/f{i}.rs", LIMIT)
        plant(root, "target/debug/build/out.rs", 2000)
        plant(root, ".claude/worktrees/x/src/big.rs", 2000)
        plant(root, "vendor/fork/src/big.rs", 2000)

    def long_file(root):
        control(root)
        plant(root, "crates/a/src/long.rs", LIMIT + 1)

    def long_test(root):
        control(root)
        plant(root, "crates/a/tests/long.rs", LIMIT + 1)

    cases = [
        ("control", control, None),
        ("a source file over the limit", long_file, f"crates/a/src/long.rs: {LIMIT + 1} lines, over {LIMIT}"),
        ("a test file over the limit", long_test, f"crates/a/tests/long.rs: {LIMIT + 1} lines, over {LIMIT}"),
        ("an empty tree", lambda root: None, "0 Rust files read, fewer than 3"),
    ]
    failed = 0
    for name, setup, expect in cases:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            setup(root)
            faults = check(root, 3)
        ok = not faults if expect is None else any(expect in f for f in faults)
        print(f"{'ok  ' if ok else 'FAIL'}  {name}: {faults if faults else 'no fault'}")
        failed += not ok
    return 1 if failed else 0


def main(argv: list[str]) -> int:
    if argv == ["--self-test"]:
        return self_test()
    if argv:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    faults = check(ROOT, MIN_FILES)
    for f in faults:
        print(f)
    print(f"check-size: {len(rust_files(ROOT))} Rust files, {len(faults)} faults (limit {LIMIT} lines)")
    return 1 if faults else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
