#!/usr/bin/env python3
"""Check that each test target that a consumer's scripts name exists.

  python3 -I scripts/check-consumer-tests.py [DIR] [SCRIPT]...

DIR is the checkout of the consumer (default: the current directory). Each
SCRIPT (default: scripts/gate.sh) is read for `--test NAME`. Each NAME must
be a test target of the workspace (`cargo metadata --no-deps`). A patch of
an adoption trial that moves a test target to a crate of podcrates must not
leave a gate that names it: the trial runs the tests of the patch, not the
gate of the consumer. Exit 1 with each missing name; exit 0 with the count.
"""

import json
import re
import subprocess
import sys
from pathlib import Path


def main(argv: list[str]) -> int:
    root = Path(argv[0]) if argv else Path(".")
    scripts = argv[1:] or ["scripts/gate.sh"]
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=root, capture_output=True, text=True, check=True,
    ).stdout
    targets = {t["name"] for p in json.loads(out)["packages"] for t in p["targets"] if "test" in t["kind"]}
    named: dict[str, str] = {}
    for script in scripts:
        path = root / script
        if not path.is_file():
            continue
        for name in re.findall(r"--test[ =]([A-Za-z0-9_-]+)", path.read_text()):
            named.setdefault(name, script)
    missing = sorted(n for n in named if n not in targets)
    for name in missing:
        print(f"check-consumer-tests: {named[name]} names the test target {name}, which no crate has", file=sys.stderr)
    if missing:
        return 1
    print(f"check-consumer-tests: {len(named)} test targets named, each exists")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
