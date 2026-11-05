#!/usr/bin/env python3
"""Check that each name of docs/design.md, section 4, is on crates.io, with
the repository of podcrates (GitHub #1, T-043).

  python3 -I scripts/check-crates-io-names.py

The names: the first cell of each row of the tables of crates, and the
names held with no crate. It prints one line for each name: its newest
version and its repository, or `missing`. Exit 1 when a name is missing or
names another repository; exit 2 when crates.io cannot be read.
"""

import json
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

API = "https://crates.io/api/v1/crates/"
AGENT = "podcrates-check (https://github.com/PodUtils/podcrates)"
OURS = ("https://github.com/PodUtils/podcrates", "https://github.com/PodUtils")


def names(design: str) -> list[str]:
    part = design.split("## 4. The crates", 1)[1].split("\n## 5.", 1)[0]
    found = re.findall(r"^\| `([a-z0-9-]+)` \|", part, re.M)
    held = re.search(r"^Names held, with no crate:(.*?)\.\n", part, re.M | re.S)
    if held:
        found += re.findall(r"`([a-z0-9-]+)`", held.group(1))
    return list(dict.fromkeys(found))


def crate(name: str):
    request = urllib.request.Request(API + name, headers={"User-Agent": AGENT})
    try:
        with urllib.request.urlopen(request, timeout=30) as answer:
            return json.load(answer)["crate"]
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return None
        raise


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    wanted = names((root / "docs/design.md").read_text(encoding="utf-8"))
    if not wanted:
        print("check-crates-io-names: no name in docs/design.md, section 4", file=sys.stderr)
        return 2
    faults = 0
    for name in wanted:
        try:
            found = crate(name)
        except (urllib.error.URLError, TimeoutError, ValueError, KeyError) as e:
            print(f"check-crates-io-names: {name}: {type(e).__name__}: {e}", file=sys.stderr)
            return 2
        if found is None:
            print(f"{name:16} missing")
            faults += 1
        elif (found.get("repository") or "").rstrip("/") not in OURS:
            print(f"{name:16} {found['max_version']} names another repository: {found.get('repository')}")
            faults += 1
        else:
            print(f"{name:16} {found['max_version']} {found['repository']}")
    print(f"check-crates-io-names: {len(wanted)} names, {faults} faults")
    return 1 if faults else 0


if __name__ == "__main__":
    sys.exit(main())
