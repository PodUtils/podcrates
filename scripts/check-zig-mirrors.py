#!/usr/bin/env python3
"""Compare scripts/zig-mirrors.txt with the list of community mirrors of
ziglang.org (T-046), which scripts/fetch-zig.sh reads before ziglang.org
itself. upstream.yml runs it each day.

  python3 -I scripts/check-zig-mirrors.py [--plant-mirror]

Exit 1 names each mirror that is new or gone, and what to do; exit 0 when
the lists agree; exit 2 when ziglang.org cannot be read. --plant-mirror
adds a mirror that ziglang.org does not list to the copy as read: the check
must then exit 1.
"""

import argparse
import sys
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MIRRORS = "https://ziglang.org/download/community-mirrors.txt"
AGENT = "podcrates-check (https://github.com/PodUtils/podcrates)"


def mirrors_of(text: str) -> set:
    return {line.strip() for line in text.splitlines() if line.strip() and not line.lstrip().startswith("#")}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--plant-mirror", action="store_true")
    args = parser.parse_args()
    copy = mirrors_of((ROOT / "scripts" / "zig-mirrors.txt").read_text(encoding="utf-8"))
    if args.plant_mirror:
        copy.add("https://planted.invalid/zig")
    try:
        request = urllib.request.Request(MIRRORS, headers={"User-Agent": AGENT})
        with urllib.request.urlopen(request, timeout=60) as answer:
            listed = mirrors_of(answer.read().decode("utf-8"))
    except (urllib.error.URLError, TimeoutError) as e:
        print(f"check-zig-mirrors: {MIRRORS}: {e}", file=sys.stderr)
        return 2
    if copy != listed:
        print(
            f"scripts/zig-mirrors.txt is not {MIRRORS}: new {sorted(listed - copy)}, "
            f"gone {sorted(copy - listed)}; copy the list again"
        )
        return 1
    print(f"ok   scripts/zig-mirrors.txt: the {len(copy)} mirrors of {MIRRORS}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
