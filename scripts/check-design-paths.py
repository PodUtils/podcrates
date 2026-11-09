#!/usr/bin/env python3
"""Check the repository table of docs/design.md (section 2) against the tree,
and the crate table of README.md against crates/ (T-093, T-094).

  python3 -I scripts/check-design-paths.py [--plant-path | --plant-unlisted | --plant-crate]

Rules:

* Each path in the first column of a row exists. An uppercase word in a path
  (adoption/CONSUMER/) and `*` match any name. A row whose second column
  starts with "planned" is only reported.
* The rows `docs/`, `scripts/` and `scripts/agent/` name each file and
  directory that the tree holds there, and each name that the row lists
  exists in that directory. A name in parentheses is a note and is not read.
* README.md names each directory of crates/ in a code span.

--plant-path adds a path that does not exist to the table, --plant-unlisted
removes one name from the row `scripts/`, --plant-crate removes one crate
from README.md: the run must then exit 1.

Exit 0 when the table agrees, 1 at a fault (one line for each), 2 when a
file cannot be read.
"""

import glob
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LISTED = ("docs/", "scripts/", "scripts/agent/")
IGNORED = {"__pycache__", "target"}
SPAN = re.compile(r"`([^`]+)`")


def read(rel):
    try:
        with open(os.path.join(ROOT, rel), encoding="utf-8") as f:
            return f.read()
    except OSError as e:
        print(f"check-design-paths: {rel}: {e}", file=sys.stderr)
        sys.exit(2)


def rows(text):
    """(first column, second column) of each row of section 2."""
    out, on = [], False
    for line in text.split("\n"):
        if line.startswith("## "):
            on = line.startswith("## 2. ")
        elif on and line.startswith("| `"):
            cells = [c.strip() for c in line.strip().strip("|").split(" | ", 1)]
            if len(cells) == 2:
                out.append((cells[0], cells[1]))
    return out


def pattern(path):
    return re.sub(r"(^|/)[A-Z]+(?=/|$)", r"\1*", path)


def exists(pat):
    return bool(glob.glob(os.path.join(ROOT, pat), recursive=False)) or os.path.exists(os.path.join(ROOT, pat))


def names(cell):
    cell = re.sub(r"\([^()]*\)", "", cell)
    return [n for n in SPAN.findall(cell) if re.fullmatch(r"[A-Za-z0-9_.*-]+/?", n)]


def check_table(text, plant_path, plant_unlisted):
    faults = []
    table = rows(text)
    if plant_path:
        table.append(("`scripts/no-such-script.py`", "planted"))
    if not table:
        faults.append("docs/design.md: no row in section 2")
    for first, second in table:
        planned = second.lower().startswith("planned")
        for path in SPAN.findall(first):
            if not exists(pattern(path)) and not planned:
                faults.append(f"docs/design.md: `{path}` is in the repository table and not in the tree")
        if first.strip("`") not in LISTED:
            continue
        d = first.strip("`")
        listed = names(second)
        if plant_unlisted and d == "scripts/":
            listed = [n for n in listed if n != "retry.sh"]
        for n in listed:
            if not exists(pattern(d + n)):
                faults.append(f"docs/design.md: `{d}{n}` is in the row `{d}` and not in the tree")
        have = {n.rstrip("/") for n in listed}
        for entry in sorted(os.listdir(os.path.join(ROOT, d))):
            if entry not in IGNORED and not entry.startswith(".") and entry not in have:
                faults.append(f"docs/design.md: `{d}{entry}` is in the tree and not in the row `{d}`")
    return faults


def check_readme(text, plant_crate):
    faults = []
    for c in sorted(os.listdir(os.path.join(ROOT, "crates"))):
        if not os.path.isdir(os.path.join(ROOT, "crates", c)):
            continue
        if plant_crate and c == "pod-core":
            continue
        if f"`{c}`" not in text:
            faults.append(f"README.md: `{c}` is in crates/ and not in the table of crates")
    return faults


def main(argv):
    flags = set(argv)
    if flags - {"--plant-path", "--plant-unlisted", "--plant-crate"}:
        print(__doc__, file=sys.stderr)
        return 2
    faults = check_table(read("docs/design.md"), "--plant-path" in flags, "--plant-unlisted" in flags)
    readme = read("README.md")
    if "--plant-crate" in flags:
        readme = readme.replace("`pod-core`", "")
    faults += check_readme(readme, False)
    for f in faults:
        print(f)
    if not faults:
        print("check-design-paths: ok")
    return 1 if faults else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
