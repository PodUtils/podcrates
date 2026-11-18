#!/usr/bin/env python3
"""The code graph of podcrates: what exists, where, and what repeats.

  python3 -I scripts/codegraph.py similar WORDS...   # search before you write
  python3 -I scripts/codegraph.py find REGEX [--all]
  python3 -I scripts/codegraph.py crate NAME
  python3 -I scripts/codegraph.py graph [--dot]
  python3 -I scripts/codegraph.py build [--out FILE]
  python3 -I scripts/codegraph.py dups [--update-baseline] [--plant-clone]
  python3 -I scripts/codegraph.py new [--base REV] [--strict]
  python3 -I scripts/codegraph.py --self-test

The ruling "Shared code": before a session writes a capability, it searches
podcrates for it. `similar` ranks each public item of each crate, each entry
of the record, each crate and each script by the words of the query (the
words of an identifier count: `FileLock` is "file lock"). `find` lists the
items whose name matches. `crate` gives the dependencies of a crate, the
crates that take it, and its public items by kind. `graph` gives the
dependencies between the crates. `build` writes the whole graph as JSON
(by default `target/codegraph.json`).

`dups` is the check of repeated work. It fails on each finding that
`scripts/dups-baseline.txt` does not hold:

- clone: one function body (Rust `fn` or Python `def`, at least 6 lines)
  in two files, token for token. Comments, layout and the text of the
  literals do not count, so a copy with new messages is still a clone.
  A test-clone is a clone with each copy in test code: its one place is
  `pod-testkit` or a shared module of the tests.
- entry: two entries of the record with nearly the same title (most of the
  words), one of them not done.
- doc: one paragraph (at least 25 words) in two documents.
- deps: a crate whose "Pod deps" in `docs/design.md`, section 4, differ
  from the pod dependencies of its `Cargo.toml`.

Each line of the baseline is a key, two spaces and the reason (an entry id
that removes the finding). A key that no longer occurs is a fault too, so
the baseline only shrinks. `--update-baseline` writes the keys of today
with the reasons that the old baseline gave.

`new` reads the change against REV (default `origin/main`, with the files
that git does not track yet). For each new public item, each new `def` and
each new entry, it names the existing items or entries that look like it,
and each body that is a clone of an existing one. It exits 0, or 1 with
`--strict` when a new body is a clone or a new entry has a twin.

Exit 0: no fault. Exit 1: a fault, one line for each. Exit 64: a usage
error.
"""

from __future__ import annotations

import contextlib
import io
import json
import math
import re
import subprocess
import sys
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import codegraph_scan as scan  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / "scripts" / "dups-baseline.txt"
ENTRY_TWIN = 0.75
NEW_LOOK = 0.55


def usage() -> int:
    print(__doc__.split("\n\n")[1], file=sys.stderr)
    return 64


# ---- search ----------------------------------------------------------------


def documents(root: Path):
    """Each thing a search can find: (kind, label, where, words)."""
    out = []
    for it in scan.items(root):
        if it.test or (it.kind != "def" and not it.public) or it.kind == "mod":
            continue
        name = scan.words(it.name)
        out.append((it.kind, f"{it.crate}::{it.module}::{it.name}".replace("::::", "::"), f"{it.file}:{it.line}",
                    name * 3 + scan.words(it.module) + scan.words(it.doc)))
    for e in scan.entries(root):
        out.append(("entry", f"{e.id} [{e.status}] {e.title}", f"TODO/{e.area}.md", scan.words(e.title) * 2))
    for c in scan.crates(root):
        out.append(("crate", c.name, f"{c.path}/README.md", scan.words(c.name) * 3 + scan.words(c.summary)))
    for path in sorted((root / "scripts").rglob("*")):
        if path.suffix in (".py", ".sh") and path.is_file():
            head = path.read_text(encoding="utf-8", errors="replace")[:600]
            rel = path.relative_to(root).as_posix()
            out.append(("script", rel, rel, scan.words(path.stem) * 3 + scan.words(head)))
    return out


def rank(docs, query: list[str], top: int):
    """The best `top` documents for the query words: first by the share of
    the weight of the query words that a document holds (1.0: each word),
    then by BM25, so a short name that holds the words comes before a long
    text that holds them once."""
    df = defaultdict(int)
    for d in docs:
        for w in set(d[3]):
            df[w] += 1
    n = len(docs) or 1
    idf = {w: math.log(1 + n / df[w]) for w in df}
    q = set(query)
    best = sum(idf.get(w, 0.0) for w in q) or 1.0
    avg = sum(len(d[3]) for d in docs) / n or 1.0
    k, b = 1.2, 0.75
    scored = []
    for d in docs:
        tf = Counter(d[3])
        share = sum(idf.get(w, 0.0) for w in q if w in tf) / best
        if not share:
            continue
        norm = k * (1 - b + b * len(d[3]) / avg)
        bm = sum(idf[w] * tf[w] * (k + 1) / (tf[w] + norm) for w in q if w in tf)
        scored.append((share, bm, d))
    scored.sort(key=lambda x: (-x[0], -x[1], x[2][1]))
    return [(share, d) for share, _, d in scored[:top]]


def cmd_similar(args: list[str]) -> int:
    top = 25
    if "--top" in args:
        i = args.index("--top")
        top, args = int(args[i + 1]), args[:i] + args[i + 2 :]
    query = scan.words(" ".join(args))
    if not query:
        return usage()
    for score, (kind, label, where, _) in rank(documents(ROOT), query, top):
        print(f"{score:4.2f}  {kind:6} {label}  ({where})")
    return 0


def cmd_find(args: list[str]) -> int:
    every = "--all" in args
    args = [a for a in args if a != "--all"]
    if len(args) != 1:
        return usage()
    rx = re.compile(args[0], re.I)
    for it in scan.items(ROOT):
        if rx.search(it.name) and (every or (it.public and not it.test) or it.kind == "def"):
            mark = " test" if it.test else ""
            print(f"{it.kind:6} {it.crate}::{it.module}::{it.name}{mark}  ({it.file}:{it.line})".replace("::::", "::"))
    return 0


def cmd_crate(args: list[str]) -> int:
    if len(args) != 1:
        return usage()
    cs = {c.name: c for c in scan.crates(ROOT)}
    c = cs.get(args[0])
    if c is None:
        print(f"codegraph: no crate {args[0]}", file=sys.stderr)
        return 1
    users = sorted(o.name for o in cs.values() if c.name in o.deps.get("normal", []))
    dev_users = sorted(o.name for o in cs.values() if c.name in o.deps.get("dev", []))
    print(f"{c.name} ({c.path})\n  {c.summary[:300]}")
    print(f"  takes: {', '.join(c.deps.get('normal', [])) or 'no pod crate'}")
    print(f"  dev: {', '.join(c.deps.get('dev', [])) or 'none'}")
    print(f"  taken by: {', '.join(users) or 'no crate'}; in tests of: {', '.join(dev_users) or 'none'}")
    print(f"  features: {', '.join(c.features) or 'none'}")
    by_kind = defaultdict(list)
    for it in scan.items(ROOT):
        if it.crate == c.name and it.public and not it.test and it.kind not in ("mod",):
            by_kind[it.kind].append(f"{it.module}::{it.name}".lstrip(":"))
    for kind in sorted(by_kind):
        names = sorted(set(by_kind[kind]))
        print(f"  pub {kind} ({len(names)}): {', '.join(names[:60])}{' ...' if len(names) > 60 else ''}")
    return 0


def cmd_graph(args: list[str]) -> int:
    cs = scan.crates(ROOT)
    if "--dot" in args:
        print("digraph podcrates {")
        for c in cs:
            for d in c.deps.get("normal", []):
                print(f'  "{c.name}" -> "{d}";')
        print("}")
        return 0
    for c in cs:
        print(f"{c.name}: {' '.join(c.deps.get('normal', [])) or '-'}")
    return 0


def cmd_build(args: list[str]) -> int:
    out = ROOT / "target" / "codegraph.json"
    if "--out" in args:
        out = Path(args[args.index("--out") + 1])
    graph = {
        "crates": [c.__dict__ for c in scan.crates(ROOT)],
        "items": [it.__dict__ for it in scan.items(ROOT)],
        "entries": [e.__dict__ for e in scan.entries(ROOT)],
    }
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(graph, indent=1), encoding="utf-8")
    print(f"codegraph: {len(graph['crates'])} crates, {len(graph['items'])} items, "
          f"{len(graph['entries'])} entries: {out}")
    return 0


# ---- the check of repeated work --------------------------------------------


def title_words(title: str) -> set[str]:
    t = re.sub(r"\((?:GitHub #|PodUtils/)[^)]*\)", "", title)
    t = re.sub(r"`[0-9a-f]{7,40}`", "", t)
    return set(scan.words(t))


def jaccard(a: set[str], b: set[str]) -> float:
    return len(a & b) / len(a | b) if a and b else 0.0


def findings(root: Path, items=None, plant: bool = False) -> dict[str, str]:
    """Each finding of repeated work, by its key, with its words."""
    items = scan.items(root) if items is None else items
    out = {}
    bodies = defaultdict(list)
    for it in items:
        if it.body:
            bodies[it.body].append(it)
    if plant:
        big = max((it for it in items if it.body and len(bodies[it.body]) == 1), key=lambda i: i.lines)
        bodies[big.body].append(scan.Item("planted", "fn", big.name, "copy", "planted/copy.rs", 1, True, False,
                                          body=big.body, lines=big.lines))
    for h, group in bodies.items():
        files = sorted({it.file for it in group})
        if len(files) < 2:
            continue
        where = ", ".join(f"{it.file}:{it.line} {it.name}" for it in sorted(group, key=lambda i: (i.file, i.line)))
        kind = "test-clone" if all(it.test for it in group) else "clone"
        out[f"{kind}:{h}"] = f"one body of {group[0].lines} lines in {len(files)} files: {where}"
    ents = scan.entries(root)
    tw = [(e, title_words(e.title)) for e in ents]
    for i, (a, wa) in enumerate(tw):
        if len(wa) < 4:
            continue
        for b, wb in tw[i + 1 :]:
            if len(wb) < 4 or (a.status == "done" and b.status == "done"):
                continue
            if jaccard(wa, wb) >= ENTRY_TWIN:
                out[f"entry:{a.id},{b.id}"] = f"{a.id} [{a.status}] and {b.id} [{b.status}] have nearly one title: {a.title}"
    seen = defaultdict(list)
    for rel, line, text in scan.doc_paragraphs(root):
        seen[scan.digest(text)].append((rel, line, text))
    for h, group in seen.items():
        if len({g[0] for g in group}) > 1:
            where = ", ".join(f"{r}:{n}" for r, n, _ in group)
            out[f"doc:{h}"] = f"one paragraph in {where}: {' '.join(group[0][2].split()[:12])} ..."
    real = {c.name: set(c.deps.get("normal", [])) for c in scan.crates(root)}
    for name, said in scan.design_deps(root).items():
        if name in real and said != real[name]:
            miss = ", ".join(sorted(real[name] - said)) or "none"
            extra = ", ".join(sorted(said - real[name])) or "none"
            out[f"deps:{name}"] = (f"docs/design.md, section 4: the Pod deps of {name} lack {miss} and name "
                                   f"{extra} that its Cargo.toml does not take")
    return out


def read_baseline(path: Path) -> dict[str, str]:
    out = {}
    if path.exists():
        for line in path.read_text(encoding="utf-8").splitlines():
            if not line.strip() or line.startswith("#"):
                continue
            key, _, why = line.partition("  ")
            out[key.strip()] = why.strip()
    return out


HEAD = """# The known findings of `scripts/codegraph.py dups`: one key, two spaces and
# the reason. Each reason names the entry that removes the finding. A new
# finding fails the check; a key that no longer occurs fails it too, so this
# file only shrinks. `python3 -I scripts/codegraph.py dups --update-baseline`
# writes the keys of today with the reasons of this file.
"""


def cmd_dups(args: list[str], root: Path = ROOT, baseline: Path = BASELINE) -> int:
    found = findings(root, plant="--plant-clone" in args)
    known = read_baseline(baseline)
    if "--update-baseline" in args:
        lines = [f"{k}  {known.get(k, 'NEW: name the entry that removes it')}" for k in sorted(found)]
        baseline.write_text(HEAD + "\n".join(lines) + "\n", encoding="utf-8")
        print(f"codegraph: {len(found)} keys in {baseline.relative_to(root)}")
        return 0
    faults = [f"new {k}: {v}" for k, v in sorted(found.items()) if k not in known]
    faults += [f"gone {k}: remove its line from {baseline.name} ({known[k]})" for k in sorted(known) if k not in found]
    faults += [f"no reason {k}: name the entry that removes it" for k, why in sorted(known.items())
               if k in found and (not why or why.startswith("NEW"))]
    for f in faults:
        print(f"codegraph: {f}")
    if faults:
        return 1
    print(f"codegraph: {len(found)} known findings, no new one")
    return 0


# ---- the check of a change -------------------------------------------------

NEW_RUST = re.compile(r"^\+\s*pub\s+(?:(?:const|async|unsafe)\s+)*(fn|struct|enum|trait)\s+([A-Za-z_]\w*)")
NEW_DEF = re.compile(r"^\+\s*def\s+([A-Za-z_]\w*)")
NEW_ENTRY = re.compile(r"^\+# (T-\d+): (.+)$")


def changed_lines(root: Path, base: str) -> list[tuple[str, str]]:
    def git(*a):
        return subprocess.run(["git", "-C", str(root), *a], capture_output=True, text=True, check=False)

    diff = git("diff", "--unified=0", "--no-color", base, "--", "crates", "scripts", "TODO")
    if diff.returncode != 0:
        raise SystemExit(f"codegraph: git diff {base}: {diff.stderr.strip()}")
    out, path = [], ""
    for line in diff.stdout.splitlines():
        if line.startswith("+++ "):
            path = line[6:] if line.startswith("+++ b/") else ""
        elif line.startswith("+") and path:
            out.append((path, line))
    for rel in git("ls-files", "--others", "--exclude-standard", "crates", "scripts", "TODO").stdout.split():
        p = root / rel
        if p.suffix in (".rs", ".py", ".md") and p.is_file():
            out += [(rel, "+" + line) for line in p.read_text(encoding="utf-8", errors="replace").splitlines()]
    return out


def cmd_new(args: list[str], root: Path = ROOT) -> int:
    base = "origin/main"
    if "--base" in args:
        base = args[args.index("--base") + 1]
    strict = "--strict" in args
    lines = changed_lines(root, base)
    items = scan.items(root)
    docs = [d for d in documents(root)]
    by_body = defaultdict(list)
    for it in items:
        if it.body:
            by_body[it.body].append(it)
    hits = 0
    news = []
    for path, line in lines:
        if m := NEW_RUST.match(line):
            news.append((path, m.group(2)))
        elif path.endswith(".py") and (m := NEW_DEF.match(line)):
            news.append((path, m.group(1)))
    for path, name in news:
        mine = [it for it in items if it.file == path and it.name == name]
        for it in mine:
            twins = [o for o in by_body.get(it.body, []) if o.file != path] if it.body else []
            for o in twins:
                hits += 1
                print(f"codegraph: clone: {path}:{it.line} {name} has the body of {o.file}:{o.line} {o.name}")
        looks = [(s, d) for s, d in rank(docs, scan.words(name) + scan.words(mine[0].doc if mine else ""), 6)
                 if s >= NEW_LOOK and not d[2].startswith(path + ":")]
        for s, (kind, label, where, _) in looks[:3]:
            print(f"codegraph: look: {path} {name} is like {kind} {label} ({where}, {s:.2f})")
    ents = scan.entries(root)
    for path, line in lines:
        if m := NEW_ENTRY.match(line):
            mine = title_words(m.group(2))
            for e in ents:
                if e.id == m.group(1):
                    continue
                j = jaccard(mine, title_words(e.title))
                if j >= ENTRY_TWIN:
                    hits += 1
                    print(f"codegraph: twin: {m.group(1)} has nearly the title of {e.id} [{e.status}] ({j:.2f}): {e.title}")
                elif j >= NEW_LOOK:
                    print(f"codegraph: look: {m.group(1)} is like {e.id} [{e.status}] ({j:.2f}): {e.title}")
    print(f"codegraph: {len(news)} new items against {base}; {hits} clones or twins")
    return 1 if strict and hits else 0


# ---- the self-test ---------------------------------------------------------

BODY = """    let mut total = 0u64;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            return Err(Error::Empty(format!("part {i} is empty")));
        }
        total += part.len() as u64;
    }
    Ok(total)
"""


def tree(tmp: Path, files: dict[str, str]) -> Path:
    for rel, text in files.items():
        p = tmp / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8")
    return tmp


def base_files() -> dict[str, str]:
    row = "| [T-{n}](area.md) | P2 | S | M0 | defect | {s} | {t} |"
    return {
        "crates/pod-a/Cargo.toml": '[package]\nname = "pod-a"\n[dependencies]\npod-b = { path = "../pod-b" }\n',
        "crates/pod-b/Cargo.toml": '[package]\nname = "pod-b"\n',
        "crates/pod-a/src/lib.rs": "/// Sums the parts.\npub fn total(parts: &[&[u8]]) -> Result<u64, Error> {\n" + BODY + "}\n",
        "crates/pod-b/src/lib.rs": "pub struct FileLock;\n",
        "docs/design.md": "## 4. The crates\n\n| Crate | Source | Lines | Pod deps |\n| --- | --- | --- | --- |\n"
                          "| `pod-a` | new | 1 | pod-b |\n| `pod-b` | new | 1 | none |\n\n## 5. End\n",
        "TODO/INDEX.md": "\n".join([row.format(n="001", s="open", t="the reader of the zone files of the system"),
                                    row.format(n="002", s="done", t="a lock of a file for each system")]) + "\n",
        "scripts/x.py": "def one():\n    return 1\n",
    }


def self_test() -> int:
    failures = []

    def expect(name, files, want_keys, plant=False):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(Path(tmp), files)
            got = findings(root, plant=plant)
            for prefix in want_keys:
                if not any(k.startswith(prefix) for k in got):
                    failures.append(f"{name}: no finding {prefix} in {sorted(got)}")
            if not want_keys and got:
                failures.append(f"{name}: findings in the control tree: {sorted(got)}")

    good = base_files()
    expect("control", good, [])
    copy = dict(good, **{"crates/pod-b/src/copy.rs": "pub fn sum_all(parts: &[&[u8]]) -> Result<u64, Error> {\n"
                         + BODY.replace("is empty", "has no byte") + "}\n"})
    expect("a renamed copy with a new message", copy, ["clone:"])
    entry = dict(good, **{"TODO/INDEX.md": good["TODO/INDEX.md"]
                          + "| [T-003](area.md) | P2 | S | M0 | defect | open | the reader of the zone files of each system |\n"})
    expect("an entry with nearly one title", entry, ["entry:T-001,T-003"])
    para = " ".join(["each session reads the record before it writes code for a new capability"] * 3)
    doc = dict(good, **{"README.md": para + "\n", "AGENTS.md": para + "\n"})
    expect("one paragraph in two documents", doc, ["doc:"])
    deps = dict(good, **{"crates/pod-b/Cargo.toml": '[package]\nname = "pod-b"\n[dependencies]\npod-c = "1"\n'})
    expect("a dependency that design.md does not name", deps, ["deps:pod-b"])
    expect("the planted clone", good, ["clone:"], plant=True)
    with tempfile.TemporaryDirectory() as tmp:
        root = tree(Path(tmp), good)
        docs = documents(root)
        top = rank(docs, scan.words("file lock"), 1)
        if not top or top[0][1][1] != "pod-b::FileLock":
            failures.append(f"similar: 'file lock' does not find pod-b::FileLock first: {top}")
        top = rank(docs, scan.words("zone files"), 1)
        if not top or not top[0][1][1].startswith("T-001"):
            failures.append(f"similar: 'zone files' does not find T-001 first: {top}")
        base = root / "base.txt"
        base.write_text("clone:0  T-999\n", encoding="utf-8")
        with contextlib.redirect_stdout(io.StringIO()):
            stale = cmd_dups([], root, base)
        if stale != 1:
            failures.append("dups: a baseline key that no longer occurs passes")
    if scan.words("FileLock::try_acquire") != ["file", "lock", "try", "acquire"]:
        failures.append(f"words: {scan.words('FileLock::try_acquire')}")
    for f in failures:
        print(f"codegraph: self-test: {f}")
    if failures:
        return 1
    print("codegraph: self-test passed")
    return 0


def main(argv: list[str]) -> int:
    if not argv:
        return usage()
    if argv[0] == "--self-test":
        return self_test()
    cmds = {"similar": cmd_similar, "find": cmd_find, "crate": cmd_crate, "graph": cmd_graph,
            "build": cmd_build, "dups": cmd_dups, "new": cmd_new}
    if argv[0] not in cmds:
        return usage()
    return cmds[argv[0]](argv[1:])


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
