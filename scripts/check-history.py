#!/usr/bin/env python3
"""Check that the documents hold no narrative history.

  python3 -I scripts/check-history.py [--self-test] [--plant]

The rule (`AGENTS.md`, section 5): keep only technical content; git keeps
the history. A document says what is true now: what a crate does, what
waits, the tag that carries a change. It does not tell what a session did,
when a consumer took a commit, or what a text said before.

The check reads the documents that each session reads as the current
state: `README.md`, `AGENTS.md`, `SECURITY.md`, `docs/*.md`, the rules, the
progress and the roadmap of `TODO/`, and the `README.md` of each crate. It
does not read `docs/decisions.md` (each ruling has its date by design), the
entries of the record (a Done section has its date and commit by design),
and the readings at a time (`docs/releases/`, `docs/research/`,
`docs/reports/`, `docs/upstream/`). Words in double quotes are a quote
(of the owner, or of an output) and do not count.

A fault is a line with one of these forms (RULES): a consumer that took a
crate at a commit, a `state:` line, a time stamp with its hour, a sentence
that starts "Before, ", the word "formerly", or the work of one session.
EXCEPTIONS lists a line that must keep its form, with the reason.

--self-test runs the rules on lines with each form and on a control text.
--plant adds one line of each form to the scan, which must then fail.

Exit 0: no fault. Exit 1: a fault, one line for each.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = ["README.md", "AGENTS.md", "SECURITY.md", "docs/*.md", "TODO/RULES.md", "TODO/PROGRESS.md",
        "TODO/ROADMAP.md", "crates/*/README.md"]
SKIP = {"docs/decisions.md"}
MIN_FILES = 50

RULES = [
    ("a take at a commit", re.compile(
        r"\b(?:podssh|podnet|podvm|podrunner)(?: session)? took (?:the crate|these rows|it|part|parts|the tag|the content)\b")),
    ("a state line", re.compile(r"^state:")),
    ("a time stamp", re.compile(r"\b20\d\d-\d\d-\d\d[T ]\d\d:\d\d")),
    ("what a text said before", re.compile(r"(?:^|[.!?;:]\s+)Before, ")),
    ("an old name", re.compile(r"\bformerly\b", re.I)),
    ("the work of one session", re.compile(
        r"\b(?:in|by) (?:this|the last|the previous|an earlier) session\b|\bthe (?:last|previous) session (?:found|did|wrote|ran|made|took)\b", re.I)),
]

# A line that keeps its form, by its file and a part of its text, with the
# reason.
EXCEPTIONS = [
    ("AGENTS.md", "formerly podbox", "the old name of podvm finds its old issues and commits"),
]

QUOTE = re.compile(r'"[^"\n]*"')


def documents(root: Path) -> list[Path]:
    out = []
    for pattern in DOCS:
        out += [p for p in sorted(root.glob(pattern)) if p.relative_to(root).as_posix() not in SKIP]
    return out


def faults_in(rel: str, text: str) -> list[str]:
    out = []
    fence = False
    for n, line in enumerate(text.splitlines(), 1):
        if line.lstrip().startswith("```"):
            fence = not fence
            continue
        if fence:
            continue
        bare = QUOTE.sub('""', line)
        for what, rule in RULES:
            if rule.search(bare):
                if any(rel == f and part in line for f, part, _ in EXCEPTIONS):
                    continue
                out.append(f"{rel}:{n}: {what}: {line.strip()[:140]}")
    return out


PLANTS = [
    "podnet took the crate at `0123abc` (tag `v0.9.0`).",
    "state: open at 2026-11-15T21:50 (UTC).",
    "The run ended at 2026-11-16 07:30 UTC.",
    "Before, the stack dropped the datagram.",
    "The crate was formerly a module of podssh.",
    "The fix was made in this session.",
]
CONTROL = """\
The crate takes the TLS provider from its caller (call C18).
A session of the relay ends at its byte cap; the session resumes on a new link.
The record of `read` gives the date of a value: 2026-11-17.
"The owner, in this session, said it before, formerly" is a quote.
```sh
echo "state: a line in a code block"
```
"""


def self_test() -> int:
    failures = []
    for line in PLANTS:
        if not faults_in("docs/x.md", line + "\n"):
            failures.append(f"no fault for: {line}")
    got = faults_in("docs/x.md", CONTROL)
    if got:
        failures.append(f"faults in the control text: {got}")
    if faults_in("AGENTS.md", "* podvm (`PodUtils/podvm`, formerly podbox): payloads\n"):
        failures.append("the exception of AGENTS.md does not hold")
    for f in failures:
        print(f"check-history: self-test: {f}")
    if failures:
        return 1
    print("check-history: self-test passed")
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return self_test()
    files = documents(ROOT)
    faults = []
    if len(files) < MIN_FILES:
        faults.append(f"the scan read {len(files)} documents, fewer than {MIN_FILES}")
    for path in files:
        rel = path.relative_to(ROOT).as_posix()
        faults += faults_in(rel, path.read_text(encoding="utf-8"))
    if "--plant" in argv:
        faults += faults_in("docs/planted.md", "\n".join(PLANTS) + "\n")
    for f in faults:
        print(f"check-history: {f}")
    if faults:
        return 1
    print(f"check-history: {len(files)} documents, no narrative history")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
